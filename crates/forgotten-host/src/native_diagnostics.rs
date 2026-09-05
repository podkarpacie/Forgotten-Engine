//! Bounded native-session diagnostics: stderr trace lines and per-action diagnostic summaries.

use std::net::SocketAddr;

use super::NativeOtClientGameAction;
pub(crate) fn native_diagnostic_record(
    enabled: bool,
    peer: SocketAddr,
    event: &str,
) -> Option<String> {
    enabled.then(|| format!("> Native OTCv8 trace peer={peer} {event}"))
}

pub(crate) fn native_diagnostic(enabled: bool, peer: SocketAddr, event: &str) {
    if let Some(record) = native_diagnostic_record(enabled, peer, event) {
        eprintln!("{record}");
    }
}

/// Guild chat channel id on classic profiles: FE reserves 0x00F1 so it never collides with
/// configured public channel ids (which start at 1 in TFS-style catalogs).
pub(crate) const NATIVE_GUILD_CHAT_CHANNEL_ID: u16 = 0x00F1;

pub(crate) fn native_action_diagnostic_summary(action: &NativeOtClientGameAction) -> String {
    match action {
        NativeOtClientGameAction::Ping => "action=ping".into(),
        NativeOtClientGameAction::PingBack => "action=ping-back".into(),
        NativeOtClientGameAction::EnterGame => "action=enter-game".into(),
        NativeOtClientGameAction::LeaveGame => "action=leave-game".into(),
        NativeOtClientGameAction::Stop => "action=stop".into(),
        NativeOtClientGameAction::RequestTrade { .. } => "action=request-trade".into(),
        NativeOtClientGameAction::AcceptTrade => "action=accept-trade".into(),
        NativeOtClientGameAction::RejectTrade => "action=reject-trade".into(),
        NativeOtClientGameAction::NpcBuy { .. } => "action=npc-buy".into(),
        NativeOtClientGameAction::NpcSell { .. } => "action=npc-sell".into(),
        NativeOtClientGameAction::NpcTradeClose => "action=npc-trade-close".into(),
        NativeOtClientGameAction::Turn(direction) => format!("action=turn direction={direction:?}"),
        NativeOtClientGameAction::CardinalMove(direction) => {
            format!("action=cardinal-move direction={direction:?}")
        }
        NativeOtClientGameAction::DiagonalMove(direction) => {
            format!("action=diagonal-move direction={direction:?}")
        }
        NativeOtClientGameAction::AutoWalk(path) => format!(
            "action=auto-walk path-directions={} expanded-steps={}",
            path.len(),
            path.iter()
                .map(|direction| direction.cardinal_steps().len())
                .sum::<usize>()
        ),
        NativeOtClientGameAction::Talk(request) => {
            format!(
                "action=talk mode={} channel-id={} text-bytes={}",
                request.mode,
                request.channel_id.map_or(0, u16::from),
                request.message.len()
            )
        }
        NativeOtClientGameAction::AddVip(name) => {
            format!("action=vip-add target-name-bytes={}", name.len())
        }
        NativeOtClientGameAction::RemoveVip(target_player_id) => {
            format!("action=vip-remove target-player-id={target_player_id}")
        }
        NativeOtClientGameAction::EditVip {
            target_player_id,
            description,
            icon,
            notify,
        } => format!(
            "action=vip-edit target-player-id={target_player_id} description-bytes={} icon={icon} notify={notify}",
            description.len()
        ),
        NativeOtClientGameAction::ThrowItem {
            source_position,
            source_client_thing_id,
            source_stack_position,
            target_position,
            count,
        } => format!(
            "action=throw-item source={},{},{} source-client-thing-id={} source-stack-position={} target={},{},{} count={}",
            source_position.x,
            source_position.y,
            source_position.z,
            source_client_thing_id,
            source_stack_position,
            target_position.x,
            target_position.y,
            target_position.z,
            count
        ),
        NativeOtClientGameAction::ChangeFightModes(request) => format!(
            "action=change-fight-modes mode={:?} chase={} secure={}",
            request.mode, request.chase, request.secure
        ),
        NativeOtClientGameAction::CloseContainer(container_id) => {
            format!("action=close-container container-id={container_id}")
        }
        NativeOtClientGameAction::UpArrowContainer(container_id) => {
            format!("action=up-arrow-container container-id={container_id}")
        }
        NativeOtClientGameAction::UpdateContainer(container_id) => {
            format!("action=update-container container-id={container_id}")
        }
        NativeOtClientGameAction::UseItem {
            position,
            client_thing_id,
            stack_position,
            index,
        } => format!(
            "action=use-item position={},{},{} client-thing-id={} stack-position={} index={}",
            position.x, position.y, position.z, client_thing_id, stack_position, index
        ),
        NativeOtClientGameAction::UseItemEx {
            source_position,
            source_client_thing_id,
            source_stack_position,
            target_position,
            target_client_thing_id,
            target_stack_position,
        } => format!(
            "action=use-item-ex source={},{},{} source-client-thing-id={} source-stack-position={} target={},{},{} target-client-thing-id={} target-stack-position={}",
            source_position.x,
            source_position.y,
            source_position.z,
            source_client_thing_id,
            source_stack_position,
            target_position.x,
            target_position.y,
            target_position.z,
            target_client_thing_id,
            target_stack_position,
        ),
        NativeOtClientGameAction::UseItemOnCreature {
            source_position,
            source_client_thing_id,
            source_stack_position,
            target_creature_id,
        } => format!(
            "action=use-item-on-creature source={},{},{} source-client-thing-id={} source-stack-position={} target-creature-id={}",
            source_position.x,
            source_position.y,
            source_position.z,
            source_client_thing_id,
            source_stack_position,
            target_creature_id,
        ),
        NativeOtClientGameAction::RotateItem {
            position,
            client_thing_id,
            stack_position,
        } => format!(
            "action=rotate-item position={},{},{} client-thing-id={} stack-position={}",
            position.x, position.y, position.z, client_thing_id, stack_position
        ),
        NativeOtClientGameAction::LookMap {
            position,
            thing_id,
            stack_position,
        } => format!(
            "action=look-map position={},{},{} thing-id={} stack-position={}",
            position.x, position.y, position.z, thing_id, stack_position
        ),
        NativeOtClientGameAction::LookCreature { creature_id } => {
            format!("action=look-creature creature-id={creature_id}")
        }
        NativeOtClientGameAction::RequestOutfit => "action=request-outfit".into(),
        NativeOtClientGameAction::RequestQuestLog => "action=request-quest-log".into(),
        NativeOtClientGameAction::RequestQuestLine { quest_id } => {
            format!("action=request-quest-line quest-id={quest_id}")
        }
        NativeOtClientGameAction::RequestChannels => "action=request-channels".into(),
        NativeOtClientGameAction::JoinChannel(channel_id) => {
            format!("action=join-channel channel-id={channel_id}")
        }
        NativeOtClientGameAction::LeaveChannel(channel_id) => {
            format!("action=leave-channel channel-id={channel_id}")
        }
        NativeOtClientGameAction::ChangeOutfit(outfit) => format!(
            "action=change-outfit look-type={} colors={},{},{},{}",
            outfit.look_type, outfit.head, outfit.body, outfit.legs, outfit.feet
        ),
        NativeOtClientGameAction::IgnoredInteraction(opcode) => {
            format!("action=ignored-interaction opcode=0x{opcode:02x}")
        }
        NativeOtClientGameAction::SelectTarget(native_id) => {
            format!("action=select-target native-id={native_id}")
        }
        NativeOtClientGameAction::SelectFollow(native_id) => {
            format!("action=select-follow native-id={native_id}")
        }
        NativeOtClientGameAction::PartyInvite(native_id) => {
            format!("action=party-invite native-id={native_id}")
        }
        NativeOtClientGameAction::PartyJoin(native_id) => {
            format!("action=party-join native-id={native_id}")
        }
        NativeOtClientGameAction::PartyRevokeInvitation(native_id) => {
            format!("action=party-revoke-invitation native-id={native_id}")
        }
        NativeOtClientGameAction::PartyPassLeadership(native_id) => {
            format!("action=party-pass-leadership native-id={native_id}")
        }
        NativeOtClientGameAction::PartyLeave => "action=party-leave".into(),
        NativeOtClientGameAction::PartySharedExperience(active) => {
            format!("action=party-shared-experience active={active}")
        }
        NativeOtClientGameAction::CancelAttackAndFollow => {
            "action=cancel-attack-and-follow".into()
        }
    }
}
