//! Session shared-state drain helpers: public-chat event delivery, VIP presence fan-out,
//! and party-shield refresh, each bounded to the per-session epoch baselines.

use super::*;

pub(crate) fn drain_shared_public_chat(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedPublicChatEvent>,
    open_public_channel_ids: &BTreeSet<u16>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                let Some((record, mode, event_kind)) = (if event.private {
                    Some((
                        encode_native_otclient_private_message_from(
                            profile,
                            &event.speaker_name,
                            &event.text,
                        )
                        .map_err(HostError::Protocol)?,
                        4,
                        "private-chat",
                    ))
                } else {
                    match event.channel_id {
                        Some(channel_id) if !open_public_channel_ids.contains(&channel_id) => None,
                        Some(channel_id) => Some((
                            encode_native_otclient_public_channel_say(
                                profile,
                                &event.speaker_name,
                                channel_id,
                                &event.text,
                            )
                            .map_err(HostError::Protocol)?,
                            7,
                            "public-channel-chat",
                        )),
                        None => match event.talk_mode {
                            NATIVE_OTCLIENT_MESSAGE_WHISPER => Some((
                                encode_native_otclient_whisper(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                2,
                                "whisper-chat",
                            )),
                            NATIVE_OTCLIENT_MESSAGE_YELL => Some((
                                encode_native_otclient_yell(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                3,
                                "yell-chat",
                            )),
                            NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST => Some((
                                // Console broadcasts ride the 0xB4/0x13 game-announcement
                                // class: white center-screen text mirrored into the Server
                                // Log tab. The mode-9 GM talk record renders console-red only.
                                encode_native_otclient_game_announcement(
                                    profile,
                                    format!("{}: {}", event.speaker_name, event.text).as_str(),
                                )
                                .map_err(HostError::Protocol)?,
                                9,
                                "console-broadcast",
                            )),
                            _ => Some((
                                encode_native_otclient_public_say(
                                    profile,
                                    &event.speaker_name,
                                    event.speaker_position,
                                    &event.text,
                                )
                                .map_err(HostError::Protocol)?,
                                1,
                                "public-chat",
                            )),
                        },
                    }
                }) else {
                    continue;
                };
                write_frame(stream, &record)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound={} opcode=0xaa mode={} text-bytes={}",
                        event_kind,
                        mode,
                        event.text.len(),
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Drains only queued presence changes for the current active session's persisted VIP targets.
/// Queue disconnects are harmless because the enclosing native session owns both endpoints.
pub(crate) fn drain_shared_vip_presence(
    stream: &mut TcpStream,
    profile: &NativeOtClientProfile,
    events: &mpsc::Receiver<SharedVipPresenceEvent>,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    loop {
        match events.try_recv() {
            Ok(event) => {
                let record = encode_native_otclient_classic_vip_presence(
                    profile,
                    event.target_player_id,
                    event.online,
                )
                .map_err(HostError::Protocol)?;
                write_frame(stream, &record)?;
                native_diagnostic(
                    extended_diagnostics,
                    peer,
                    &format!(
                        "outbound=vip-presence opcode=0x{:02x} online={}",
                        record.0[0], event.online
                    ),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// Sends one current, bounded party-shield snapshot only after an authoritative party epoch
/// change. The shared-world lock is released before every socket write, and no visibility or UI
/// policy beyond the current all-active native render set is claimed here.
pub(crate) fn refresh_native_party_shields(
    stream: &mut TcpStream,
    shared_world: &SharedNativeWorld,
    profile: &NativeOtClientProfile,
    observer_id: u64,
    observed_party_epoch: &mut u64,
    extended_diagnostics: bool,
    peer: SocketAddr,
) -> Result<(), HostError> {
    let party_epoch = shared_world.party_epoch();
    if party_epoch == *observed_party_epoch {
        return Ok(());
    }
    let frames = shared_world.party_display_frames(profile, observer_id)?;
    for frame in &frames {
        write_frame(stream, frame)?;
    }
    *observed_party_epoch = party_epoch;
    native_diagnostic(
        extended_diagnostics,
        peer,
        &format!(
            "outbound=party-shield-refresh records={} epoch={party_epoch}",
            frames.len()
        ),
    );
    Ok(())
}
