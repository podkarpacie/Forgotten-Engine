//! Staged native-render preparation foundation: bounded worker fan-out for detached viewport
//! encoding. Production listener integration is deferred until lifecycle and benchmark gates
//! pass (see docs/benchmarks/native-render-preparation-v7.4.44.md).

use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::{
    encode_native_otclient_map_viewport_with_static_spawns_and_players, HostError,
    NativeOtClientEmptyWorldSnapshot, NativeOtClientProfile, NativeWorldRenderSnapshot, WorldMap,
};
use forgotten_protocol::{Frame, ProtocolError};
// Staged foundation: production listener integration is deferred until lifecycle and benchmark gates pass.
#[allow(dead_code)]
pub(crate) const NATIVE_RENDER_PREPARATION_QUEUE_CAPACITY: usize = 32;
#[allow(dead_code)]
const MAX_NATIVE_RENDER_PREPARATION_WORKERS: usize = 8;
#[allow(dead_code)]
pub(crate) const MAX_NATIVE_RENDER_PUBLICATION_BATCH: usize =
    NATIVE_RENDER_PREPARATION_QUEUE_CAPACITY * MAX_NATIVE_RENDER_PREPARATION_WORKERS;

#[allow(dead_code)]
pub(crate) struct NativeRenderPreparationRequest {
    pub(crate) profile: NativeOtClientProfile,
    pub(crate) snapshot: NativeOtClientEmptyWorldSnapshot,
    pub(crate) world_map: Arc<WorldMap>,
    pub(crate) render_snapshot: NativeWorldRenderSnapshot,
    pub(crate) response: mpsc::SyncSender<Result<Frame, ProtocolError>>,
}

#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct NativeRenderPreparationWorker {
    pub(crate) requests: mpsc::SyncSender<NativeRenderPreparationRequest>,
    pub(crate) response_timeout: Duration,
}

#[allow(dead_code)]
impl NativeRenderPreparationWorker {
    pub(crate) fn start(response_timeout: Duration) -> (Self, JoinHandle<()>) {
        let (requests, receiver) = mpsc::sync_channel::<NativeRenderPreparationRequest>(
            NATIVE_RENDER_PREPARATION_QUEUE_CAPACITY,
        );
        let thread = thread::spawn(move || {
            while let Ok(request) = receiver.recv() {
                let frame = encode_native_otclient_map_viewport_with_static_spawns_and_players(
                    &request.profile,
                    &request.snapshot,
                    request.world_map.as_ref(),
                    Some(&request.render_snapshot.static_spawns),
                    Some(&request.render_snapshot.visible_players),
                );
                let _ = request.response.send(frame);
            }
        });
        (
            Self {
                requests,
                response_timeout,
            },
            thread,
        )
    }

    pub(crate) fn prepare(
        &self,
        profile: NativeOtClientProfile,
        snapshot: NativeOtClientEmptyWorldSnapshot,
        world_map: Arc<WorldMap>,
        render_snapshot: NativeWorldRenderSnapshot,
    ) -> Result<Frame, HostError> {
        self.schedule(profile, snapshot, world_map, render_snapshot)?
            .recv_timeout(self.response_timeout)
            .map_err(|_| HostError::RenderPreparationUnavailable)?
            .map_err(HostError::Protocol)
    }

    pub(crate) fn schedule(
        &self,
        profile: NativeOtClientProfile,
        snapshot: NativeOtClientEmptyWorldSnapshot,
        world_map: Arc<WorldMap>,
        render_snapshot: NativeWorldRenderSnapshot,
    ) -> Result<mpsc::Receiver<Result<Frame, ProtocolError>>, HostError> {
        let (response, receiver) = mpsc::sync_channel(1);
        self.requests
            .try_send(NativeRenderPreparationRequest {
                profile,
                snapshot,
                world_map,
                render_snapshot,
                response,
            })
            .map_err(|_| HostError::RenderPreparationUnavailable)?;
        Ok(receiver)
    }
}

/// One ordered immutable viewport-publication request. The sequence belongs to the caller's
/// established publication order; workers only encode its detached inputs.
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct NativeRenderPublication {
    pub(crate) sequence: u64,
    pub(crate) profile: NativeOtClientProfile,
    pub(crate) snapshot: NativeOtClientEmptyWorldSnapshot,
    pub(crate) world_map: Arc<WorldMap>,
    pub(crate) render_snapshot: NativeWorldRenderSnapshot,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeRenderPublicationError {
    InvalidWorkerCount(usize),
    PublicationLimitExceeded { limit: usize },
    DuplicateSequence(u64),
    PreparationUnavailable,
    Protocol,
}

/// Bounded worker fan-out for detached native viewport snapshots. It deliberately has no shared
/// world, database, socket, action queue, or mutation callback. `prepare_batch` sorts by the
/// caller-provided publication sequence before scheduling and returns frames in that same order,
/// independently of which worker completes first.
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct NativeRenderPreparationPool {
    pub(crate) workers: Vec<NativeRenderPreparationWorker>,
}

#[allow(dead_code)]
impl NativeRenderPreparationPool {
    pub(crate) fn start(
        worker_count: usize,
        response_timeout: Duration,
    ) -> Result<(Self, Vec<JoinHandle<()>>), NativeRenderPublicationError> {
        if worker_count == 0 || worker_count > MAX_NATIVE_RENDER_PREPARATION_WORKERS {
            return Err(NativeRenderPublicationError::InvalidWorkerCount(
                worker_count,
            ));
        }
        let mut workers = Vec::with_capacity(worker_count);
        let mut worker_threads = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let (worker, worker_thread) = NativeRenderPreparationWorker::start(response_timeout);
            workers.push(worker);
            worker_threads.push(worker_thread);
        }
        Ok((Self { workers }, worker_threads))
    }

    pub(crate) fn prepare_batch(
        &self,
        mut publications: Vec<NativeRenderPublication>,
    ) -> Result<Vec<Frame>, NativeRenderPublicationError> {
        if publications.len() > MAX_NATIVE_RENDER_PUBLICATION_BATCH {
            return Err(NativeRenderPublicationError::PublicationLimitExceeded {
                limit: MAX_NATIVE_RENDER_PUBLICATION_BATCH,
            });
        }
        publications.sort_by_key(|publication| publication.sequence);
        let mut seen_sequences = BTreeSet::new();
        for publication in &publications {
            if !seen_sequences.insert(publication.sequence) {
                return Err(NativeRenderPublicationError::DuplicateSequence(
                    publication.sequence,
                ));
            }
        }
        let mut responses = Vec::with_capacity(publications.len());
        for (index, publication) in publications.into_iter().enumerate() {
            let worker = &self.workers[index % self.workers.len()];
            let response = worker
                .schedule(
                    publication.profile,
                    publication.snapshot,
                    publication.world_map,
                    publication.render_snapshot,
                )
                .map_err(|_| NativeRenderPublicationError::PreparationUnavailable)?;
            responses.push((response, worker.response_timeout));
        }
        responses
            .into_iter()
            .map(|(response, timeout)| {
                response
                    .recv_timeout(timeout)
                    .map_err(|_| NativeRenderPublicationError::PreparationUnavailable)?
                    .map_err(|_| NativeRenderPublicationError::Protocol)
            })
            .collect()
    }
}
