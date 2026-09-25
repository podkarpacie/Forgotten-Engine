//! Bounded pre-routing gates for native talk records: fixed-window flood control and
//! account-mute enforcement. Both checks are narrow and context-free (four or fewer
//! parameters, no session context): the session loop owns frame emission and control flow,
//! so these stay independently unit-testable while the Talk arm shrinks.

use std::collections::VecDeque;
use std::time::Instant;

use super::{EngineDatabase, HostError, CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW, CHAT_FLOOD_WINDOW};

/// Fixed-window flood check over one session's sent-timestamps. Evicts expired entries,
/// records `now` on admission, and reports whether the record is suppressed. Suppressed
/// messages emit no client feedback and never reach the shared chat queue. Deterministic
/// under injected timestamps.
pub(crate) fn check_native_talk_flood(talk_windows: &mut VecDeque<Instant>, now: Instant) -> bool {
    while talk_windows
        .front()
        .is_some_and(|sent| now.saturating_duration_since(*sent) >= CHAT_FLOOD_WINDOW)
    {
        talk_windows.pop_front();
    }
    if talk_windows.len() >= CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW {
        return true;
    }
    talk_windows.push_back(now);
    false
}

/// Mute check for one speaker: persisted gamemaster tiers stay reachable so moderation
/// keeps working while muted; a muted non-GM account reports its remaining seconds.
/// Returns `None` when the speaker may talk. Read-only against the database.
pub(crate) fn check_native_account_mute(
    database: &EngineDatabase,
    character_id: u64,
    account_id: u64,
) -> Result<Option<u64>, HostError> {
    let speaker_gm_level = database
        .player_gm_level(character_id)
        .map_err(HostError::Persistence)?;
    if speaker_gm_level != 0 {
        return Ok(None);
    }
    database
        .account_mute_remaining_seconds(account_id)
        .map_err(HostError::Persistence)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use forgotten_core::{Player, Position};
    use std::time::Duration;

    #[test]
    fn flood_gate_admits_a_windowful_then_suppresses() {
        let start = Instant::now();
        let mut windows = VecDeque::new();
        for _ in 0..CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW {
            assert!(!check_native_talk_flood(&mut windows, start));
        }
        assert_eq!(windows.len(), CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW);
        assert!(check_native_talk_flood(&mut windows, start));
        // Suppression records nothing: the window stays full.
        assert_eq!(windows.len(), CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW);
    }

    #[test]
    fn flood_gate_evicts_expired_entries_before_counting() {
        let start = Instant::now();
        let mut windows = VecDeque::new();
        for _ in 0..CHAT_FLOOD_MAX_MESSAGES_PER_WINDOW {
            assert!(!check_native_talk_flood(&mut windows, start));
        }
        assert!(check_native_talk_flood(&mut windows, start));
        // One window past the edge, everything expired: admission resumes.
        let later = start + CHAT_FLOOD_WINDOW + Duration::from_millis(1);
        assert!(!check_native_talk_flood(&mut windows, later));
        assert_eq!(windows.len(), 1);
    }

    #[test]
    fn mute_gate_passes_unmuted_speakers_and_exempts_gms() {
        let path = std::env::temp_dir().join(format!(
            "forgotten-engine-talk-gates-{}-mute.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let database = EngineDatabase::open(&path).unwrap();
        let account_id = database
            .create_account_with_password("gatekeeper", "correct horse battery staple")
            .unwrap();
        database
            .save_player(&Player {
                id: 1,
                account_id: account_id as u64,
                name: "Gatekeeper".into(),
                position: Position {
                    x: 100,
                    y: 100,
                    z: 7,
                },
                level: 8,
                experience: 4_900,
                skill_points: 3,
            })
            .unwrap();
        // Unmuted mortal passes.
        assert_eq!(
            check_native_account_mute(&database, 1, account_id as u64).unwrap(),
            None
        );
        // Muted mortal reports remaining seconds.
        database.record_account_mute(account_id as u32, 60).unwrap();
        let remaining = check_native_account_mute(&database, 1, account_id as u64).unwrap();
        assert!(remaining.is_some_and(|seconds| seconds <= 60));
        // A promoted GM talks through the same mute.
        database.update_player_gm_level(1, 2).unwrap();
        assert_eq!(
            check_native_account_mute(&database, 1, account_id as u64).unwrap(),
            None
        );
        // Unknown speakers fail closed, never open.
        assert!(check_native_account_mute(&database, 999, account_id as u64).is_err());
        drop(database);
        let _ = std::fs::remove_file(&path);
    }
}
