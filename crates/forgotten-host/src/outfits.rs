//! Session player-appearance windows: the outfit chooser and accepted outfit changes with
//! persisted storage, shared-state propagation, and visibility refresh. Rejected or
//! unencodable changes degrade to client-visible failure text instead of ending the session.

use super::*;
use forgotten_protocol::NativeOtClientClassicOutfit;

/// Delivers the outfit chooser window, or a client-visible rejection when the chooser range is
/// missing or misconfigured. Never fails the session.
pub(crate) fn apply_native_request_outfit_action(
    player_outfit: NativeOtClientClassicOutfit,
    empty_world: &NativeOtClientEmptyWorldConfig,
    profile: &NativeOtClientProfile,
    stream: &mut TcpStream,
    peer: SocketAddr,
    extended_diagnostics: bool,
) -> Result<(), HostError> {
    // A missing or misconfigured chooser range must degrade to a client-visible
    // rejection instead of tearing down the session.
    match encode_native_otclient_choose_outfit(
        profile,
        player_outfit,
        empty_world.outfit_first_look_type,
        empty_world.outfit_last_look_type,
    ) {
        Ok(outfit_window) => {
            write_frame(stream, &outfit_window)?;
            native_diagnostic(
                extended_diagnostics,
                peer,
                &format!(
                    "outbound=choose-outfit opcode=0xc8 bytes={} look-type={}",
                    outfit_window.0.len(),
                    player_outfit.look_type
                ),
            );
        }
        Err(error) => {
            let rejection = encode_native_otclient_failure_message(
                profile,
                "The outfit window is not configured on this server.",
            )
            .map_err(HostError::Protocol)?;
            write_frame(stream, &rejection)?;
            native_diagnostic(
                extended_diagnostics,
                peer,
                &format!("action=request-outfit outcome=rejected reason={error}"),
            );
        }
    }
    Ok(())
}

/// Applies one accepted outfit change: validates the look type against the configured range,
/// persists it, propagates to shared state with a visibility refresh, and echoes the applied
/// outfit (or a failure text when unencodable). Never fails the session.
pub(crate) fn apply_native_change_outfit_action(
    ctx: &mut SessionContext<'_>,
    player_outfit: &mut NativeOtClientClassicOutfit,
    empty_world: &NativeOtClientEmptyWorldConfig,
    requested_outfit: NativeOtClientClassicOutfit,
) -> Result<(), HostError> {
    let accepted = native_classic_outfit_is_allowed(
        requested_outfit,
        empty_world.outfit_first_look_type,
        empty_world.outfit_last_look_type,
    );
    if accepted {
        ctx.database.update_player_outfit(
            ctx.character_id,
            PlayerOutfit {
                look_type: requested_outfit.look_type,
                head: requested_outfit.head,
                body: requested_outfit.body,
                legs: requested_outfit.legs,
                feet: requested_outfit.feet,
            },
        )?;
        *player_outfit = requested_outfit;
        ctx.shared_world
            .update_player_outfit(ctx.character_id, *player_outfit)?;
        *ctx.observed_visibility_epoch = ctx.shared_world.visibility_epoch();
    }
    // The applied-outfit echo must never fail the session; a rejected or
    // unencodable change degrades to a client-visible failure text.
    match encode_native_otclient_creature_outfit(
        &ctx.config.client_profile,
        ctx.snapshot.player_id,
        *player_outfit,
    ) {
        Ok(applied_outfit) => {
            write_frame(&mut *ctx.stream, &applied_outfit)?;
        }
        Err(error) => {
            let rejection = encode_native_otclient_failure_message(
                &ctx.config.client_profile,
                "The selected outfit could not be applied on this server.",
            )
            .map_err(HostError::Protocol)?;
            write_frame(&mut *ctx.stream, &rejection)?;
            native_diagnostic(
                ctx.config.extended_diagnostics,
                ctx.peer,
                &format!("action=change-outfit outcome=echo-rejected reason={error}"),
            );
        }
    }
    native_diagnostic(
        ctx.config.extended_diagnostics,
        ctx.peer,
        &format!(
            "outbound=creature-outfit opcode=0x8e accepted={} look-type={}",
            accepted, player_outfit.look_type
        ),
    );
    Ok(())
}
