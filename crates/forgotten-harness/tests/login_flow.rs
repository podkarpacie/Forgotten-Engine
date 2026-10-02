//! Milestone 1's actual scenario. This is the whole of Task A's "minimum
//! viable deliverable" that this milestone claims: provision a real world,
//! log in over the real wire protocol, select the provisioned character,
//! and confirm the game port answers with something other than an error.
//!
//! What this test does NOT prove, stated plainly per the task's own
//! reporting requirement: it does not decode viewport tiles or local-player
//! identity (that's the rest of step 5, left for the next milestone), and
//! it proves nothing about how a real, unmodified client would render any
//! of this. See the crate-level doc comment in `lib.rs` for the full scope
//! note.

use forgotten_harness::process::provision_and_run;
use forgotten_harness::{run_login_to_world_init, GameOutcome, LoginRequest};

#[test]
fn login_selects_character_and_reaches_world_init() {
    let world = provision_and_run(
        "fe-7.4",
        740,
        "harness-account",
        "harness-password",
        "HarnessCharacter",
    )
    .expect(
        "world should provision and start - see stderr above for which \
             CLI step failed if this panics",
    );

    let outcome = run_login_to_world_init(
        world.login_addr,
        world.game_addr,
        LoginRequest {
            protocol_version: world.protocol_version,
            account_id: world.account_id,
            password: "harness-password".to_string(),
        },
        "HarnessCharacter".to_string(),
    )
    .expect(
        "login -> character select -> world init should succeed against \
             a freshly provisioned world with no other sessions competing \
             for the character",
    );

    match outcome {
        GameOutcome::WorldInitStarted { login_state } => {
            // Assert on decoded frame contents, not just "it didn't error".
            // Both fields are required to be nonzero by the encoder's own
            // validation, so they cannot pass by accident.
            assert_ne!(
                login_state.player_id, 0,
                "game login state carried player_id 0, which no real session \
                 should ever produce"
            );
            assert_ne!(
                login_state.server_beat, 0,
                "game login state carried a zero server beat"
            );
        }
        GameOutcome::Error(message) => {
            panic!(
                "game port rejected a freshly selected, freshly \
                    provisioned character: {message}"
            );
        }
    }
}
