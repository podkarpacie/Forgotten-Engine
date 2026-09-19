//! Session-local container window views: close, up-arrow (deferred), and refresh of one
//! open top-level container window. Window tracking sets stay session-owned; handlers mutate
//! them explicitly rather than through shared state.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Closes one container view: records the id as closed, drops any corpse/content window state
/// for it, and emits the close record. Never fails the session.
pub(crate) fn apply_native_close_container_action(
    ctx: &mut SessionContext<'_>,
    container_id: u8,
    closed_container_ids: &mut BTreeSet<u8>,
    open_corpse_windows: &mut BTreeMap<u8, (Position, usize)>,
    open_content_windows: &mut BTreeMap<u8, (u8, usize)>,
) -> Result<(), HostError> {
    closed_container_ids.insert(container_id);
    open_corpse_windows.remove(&container_id);
    open_content_windows.remove(&container_id);
    let close = encode_native_otclient_close_container(&ctx.config.client_profile, container_id)
        .map_err(HostError::Protocol)?;
    write_frame(&mut *ctx.stream, &close)?;
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!("action=close-container outcome=session-view-closed container-id={container_id}"),
    );
    Ok(())
}

/// Acknowledges one container up-arrow as deferred (no supported parent). Narrow enough for
/// direct parameters.
pub(crate) fn apply_native_up_arrow_container_action(
    extended_diagnostics: bool,
    peer: SocketAddr,
    container_id: u8,
) -> Result<(), HostError> {
    native_diagnostic(
        extended_diagnostics,
        peer,
        &format!(
            "action=up-arrow-container outcome=deferred-no-supported-parent container-id={container_id}"
        ),
    );
    Ok(())
}

/// Refreshes one open top-level container view, re-encoding its current authoritative items.
/// A closed, missing, or unmapped window emits a diagnostic without effect.
pub(crate) fn apply_native_update_container_action(
    ctx: &mut SessionContext<'_>,
    container_id: u8,
    closed_container_ids: &mut BTreeSet<u8>,
) -> Result<(), HostError> {
    let containers = ctx.shared_world.player_containers(ctx.character_id)?;
    let frame = containers
        .container(container_id)
        .map(|container| {
            native_classic_container_frame(
                &ctx.config.client_profile,
                ctx.config.item_presentation_catalog.as_deref(),
                container,
            )
        })
        .transpose()
        .map_err(HostError::Protocol)?
        .flatten();
    let refreshed = frame.is_some();
    if let Some(frame) = frame {
        closed_container_ids.remove(&container_id);
        write_frame(&mut *ctx.stream, &frame)?;
    }
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "action=update-container outcome={} container-id={container_id}",
            if refreshed {
                "session-view-refreshed"
            } else {
                "deferred-unavailable-or-unmapped"
            }
        ),
    );
    Ok(())
}
