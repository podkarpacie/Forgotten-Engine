//! End-to-end login against the *real* Tibia 7.4 world, not a synthetic fixture.
//!
//! The milestone-1 scenario provisions its own tiny world because that is fast and
//! hermetic. This one instead points at an already-running test base carrying the
//! original 7.4 map, so it proves the same wire path works against genuine content:
//! 7,296,174 tiles, 18,552 static spawn entities, and the real 7.4 items.otb.
//!
//! Configure with two environment variables so the test never hardcodes a path:
//!
//!   FE_TESTBASE_LOGIN   e.g. 127.0.0.1:19174
//!   FE_TESTBASE_GAME    e.g. 127.0.0.1:19175
//!
//! Both are required. Without them the test skips rather than fails, so a normal
//! `cargo test --workspace` run on a machine without a test base stays green.

use std::net::SocketAddr;
use std::time::Duration;

use forgotten_harness::{run_login_to_world_init_with_timeout, GameOutcome, LoginRequest};

fn address(variable: &str) -> Option<SocketAddr> {
    std::env::var(variable).ok()?.parse().ok()
}

#[test]
fn logs_into_the_real_7_4_world_and_reaches_world_init() {
    let (Some(login_addr), Some(game_addr)) =
        (address("FE_TESTBASE_LOGIN"), address("FE_TESTBASE_GAME"))
    else {
        eprintln!(
            "skipping: set FE_TESTBASE_LOGIN and FE_TESTBASE_GAME to run against a test base"
        );
        return;
    };

    let outcome = run_login_to_world_init_with_timeout(
        login_addr,
        game_addr,
        LoginRequest {
            // 760, matching the test base config. A 740 client discards server text,
            // so 760 is the profile whose behaviour is actually observable.
            protocol_version: 760,
            account_id: 1,
            password: "testerpass".to_string(),
        },
        "TestKnight".to_string(),
        // The 7.4 world takes well over the fixture-friendly 5s to assemble its
        // world-initialization stream. Without a generous timeout this reports as
        // a connection failure even though login and character selection both
        // succeeded - the server log shows the game session starting and then
        // ending with UnexpectedEof when the harness gives up and closes.
        Duration::from_secs(120),
    )
    .expect("real 7.4 world should accept a fresh login and character selection");

    match outcome {
        GameOutcome::WorldInitStarted { login_state } => {
            assert_ne!(
                login_state.player_id, 0,
                "game login state carried player_id 0 for the real world"
            );
            assert_ne!(
                login_state.server_beat, 0,
                "game login state carried a zero server beat for the real world"
            );
            eprintln!(
                "> real-world login ok: player_id={} server_beat={}",
                login_state.player_id, login_state.server_beat
            );
        }
        GameOutcome::Error(message) => {
            panic!("real 7.4 world rejected character selection: {message}")
        }
    }
}
