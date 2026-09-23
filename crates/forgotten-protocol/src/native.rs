//! Native OTClient 7.4 client codecs: login/game request decode, action decoding,
//! and all classic 740 game/status/container/trade/chat/effect encoders.

use super::*;

pub fn decode_native_otclient_login_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientLoginRequest, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != NATIVE_OTCLIENT_ENTER_ACCOUNT {
        return Err(ProtocolError::InvalidNativeLoginRequest);
    }
    let operating_system = reader.u16()?;
    let protocol_version = reader.u16()?;
    let dat_signature = reader.u32()?;
    let spr_signature = reader.u32()?;
    let pic_signature = reader.u32()?;
    let account_id = reader.u32()?;
    let password = reader.string(MAX_LOGIN_STRING_BYTES)?;
    let (client_tag, client_build) = decode_optional_client_tag_build(&mut reader)?;
    let request = NativeOtClientLoginRequest {
        operating_system,
        protocol_version,
        dat_signature,
        spr_signature,
        pic_signature,
        account_id,
        password,
        client_tag,
        client_build,
    };
    if !classic_protocol_version_is_accepted(request.protocol_version)
        || reader.remaining() > profile.max_padding_bytes
    {
        return Err(ProtocolError::InvalidNativeLoginRequest);
    }
    Ok(request)
}

pub fn decode_native_otclient_game_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientGameRequest, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != NATIVE_OTCLIENT_PENDING_GAME {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    let operating_system = reader.u16()?;
    let protocol_version = reader.u16()?;
    if reader.byte()? != 0 {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    let account_id = reader.u32()?;
    let character_name = reader.string(MAX_LOGIN_STRING_BYTES)?;
    let password = reader.string(MAX_LOGIN_STRING_BYTES)?;
    let (client_tag, client_build) = decode_optional_client_tag_build(&mut reader)?;
    let request = NativeOtClientGameRequest {
        operating_system,
        protocol_version,
        account_id,
        character_name,
        password,
        client_tag,
        client_build,
    };
    if !classic_protocol_version_is_accepted(request.protocol_version)
        || reader.remaining() > profile.max_padding_bytes
    {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    Ok(request)
}

/// Classic 740 and 760 login packets are byte-identical apart from the advertised version, and
/// OTCv8 clients may select either in their version dropdown. A classic-profile server therefore
/// accepts both so a client configured for a different-but-compatible protocol still connects;
/// visible-text behavior keeps following the server's own profile.
fn classic_protocol_version_is_accepted(version: u16) -> bool {
    version == 740 || version == 760
}

/// Reads the trailing FE client tag/build pair when present. Stock 7.4 logins end
/// after the password, so fully-absent fields default to empty/zero; a present tag
/// with a truncated build (or any truncated field) still rejects as malformed.
fn decode_optional_client_tag_build(
    reader: &mut Reader<'_>,
) -> Result<(String, u16), ProtocolError> {
    if reader.remaining() == 0 {
        return Ok((String::new(), 0));
    }
    let client_tag = reader.string(MAX_LOGIN_STRING_BYTES)?;
    let client_build = if reader.remaining() == 0 {
        0
    } else {
        reader.u16()?
    };
    Ok((client_tag, client_build))
}

pub fn encode_native_otclient_character_list(
    entries: &[CharacterListEntry],
) -> Result<Frame, ProtocolError> {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST);
    writer.byte(u8::try_from(entries.len()).map_err(|_| ProtocolError::TooManyCharacters)?);
    for entry in entries {
        writer.string(&entry.name);
        writer.string(&entry.world_name);
        let IpAddr::V4(address) = entry.address else {
            return Err(ProtocolError::UnsupportedAddressFamily);
        };
        writer.bytes(&address.octets());
        writer.u16(entry.port);
    }
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic 740 world-light (ambient) record the client renders as global
/// map brightness. Verified against the client's `parseWorldLight` (intensity + color
/// bytes under the ambient opcode). The engine currently sends permanent daylight;
/// a day/night cycle is a deferred slice.
pub fn encode_native_otclient_world_light(intensity: u8, color: u8) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_WORLD_LIGHT);
    writer.byte(intensity);
    writer.byte(color);
    Frame(writer.finish())
}

pub fn encode_native_otclient_login_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_LOGIN_ERROR);
    writer.string(message);
    Frame(writer.finish())
}

pub fn encode_native_otclient_game_login_error(message: &str) -> Frame {
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_LOGIN_ERROR);
    writer.string(message);
    Frame(writer.finish())
}

/// Encodes the parser-verified classic 740 status-message record. The message class is the
/// independently verified `MSG_STATUS_DEFAULT` value, which clients render in the default status
/// area and console. Other protocol profiles require their own verified record layout.
pub fn encode_native_otclient_status_message(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if message.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
        return Err(ProtocolError::StringTooLong(message.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TEXT_MESSAGE);
    writer.byte(NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT);
    writer.string(message);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic 740 server-talk record for one visible public `Say` message. The local
/// OTCv8 parser maps server mode `1` to `MessageSay` for protocol 740, then reads the speaker
/// position before the text. Channels, private messages, yell/whisper variants, levels, and later
/// statement fields require separate profile-backed layouts.
pub fn encode_native_otclient_public_say(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 8usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(NATIVE_OTCLIENT_MESSAGE_SAY);
    writer.u16(speaker_position.x);
    writer.u16(speaker_position.y);
    writer.byte(speaker_position.z);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 whispered Talk record. The local OTCv8 parser maps server mode `2`
/// to `MessageWhisper` for protocol 740 and reads the speaker position before the text, exactly
/// like the ordinary Say layout.
pub fn encode_native_otclient_whisper(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_positioned_talk(
        profile,
        NATIVE_OTCLIENT_MESSAGE_WHISPER,
        speaker_name,
        speaker_position,
        text,
    )
}

/// Encodes one classic 740 yelled Talk record. The local OTCv8 parser maps server mode `3`
/// to `MessageYell` for protocol 740 and reads the speaker position before the text, exactly
/// like the ordinary Say layout.
pub fn encode_native_otclient_yell(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_positioned_talk(
        profile,
        NATIVE_OTCLIENT_MESSAGE_YELL,
        speaker_name,
        speaker_position,
        text,
    )
}

fn encode_native_otclient_positioned_talk(
    profile: &NativeOtClientProfile,
    mode: u8,
    speaker_name: &str,
    speaker_position: NativeOtClientPosition,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 8usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(mode);
    writer.u16(speaker_position.x);
    writer.u16(speaker_position.y);
    writer.byte(speaker_position.z);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 normal channel Talk record. The selected OTCv8 profile maps server
/// mode `7` to a normal channel message and reads a 16-bit channel ID before the text.
pub fn encode_native_otclient_public_channel_say(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    channel_id: u16,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || channel_id == 0
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 7usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(7);
    writer.u16(channel_id);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 private message received from another player. The local OTCv8 parser
/// maps server mode `4` to `MessagePrivateFrom` and reads no recipient or channel field before
/// the text.
pub fn encode_native_otclient_private_message_from(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 6usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(4);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic GM-broadcast Talk record. The 760 message-mode map translates server mode
/// `9` to `MessageGamemasterBroadcast`, which the client parser reads as name + text with no
/// position or channel field, rendering in every console tab.
pub fn encode_native_otclient_gm_broadcast(
    profile: &NativeOtClientProfile,
    speaker_name: &str,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if speaker_name.is_empty()
        || text.is_empty()
        || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES
    {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let fixed_bytes = 6usize;
    if speaker_name.len() + text.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(
            speaker_name.len().saturating_add(text.len()),
        ));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TALK);
    writer.string(speaker_name);
    writer.byte(NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic look (`0xB4/0x16`) inspection record. The 760 client maps class `22` to
/// `MessageLook`, rendering green center-screen text and a Server Log console entry.
pub fn encode_native_otclient_look_message(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_typed_text_message(profile, NATIVE_OTCLIENT_MESSAGE_LOOK, message)
}

/// Encodes one classic failure (`0xB4/0x17`) rejection record rendered in the status area.
pub fn encode_native_otclient_failure_message(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_typed_text_message(profile, NATIVE_OTCLIENT_MESSAGE_FAILURE, message)
}

/// Encodes one classic login (`0xB4/0x14`) welcome record rendered in the console.
pub fn encode_native_otclient_login_message(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_typed_text_message(profile, NATIVE_OTCLIENT_MESSAGE_LOGIN, message)
}

/// Encodes one classic game (`0xB4/0x13`) announcement record: white center-screen text
/// mirrored into the Server Log console tab. This is the correct channel for server-wide
/// broadcasts on classic profiles - the GM-broadcast talk mode renders console-red only.
pub fn encode_native_otclient_game_announcement(
    profile: &NativeOtClientProfile,
    message: &str,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_typed_text_message(profile, NATIVE_OTCLIENT_MESSAGE_GAME, message)
}

/// One classic trade-window item: mapped client thing id plus the subtype/count byte that the
/// classic item record carries for stackable things. Non-stackable items omit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientTradeItem {
    pub client_thing_id: u16,
    pub count: Option<u8>,
}

impl NativeOtClientTradeItem {
    pub fn encoded_len(&self) -> usize {
        2 + usize::from(self.count.is_some())
    }
}

fn trade_item_list_fits(items: &[NativeOtClientTradeItem]) -> bool {
    items.len() <= u8::MAX as usize
        && items.iter().all(|item| item.client_thing_id != 0)
        && items.iter().map(|item| item.encoded_len()).sum::<usize>() < MAX_FRAME_SIZE
}

fn write_trade_items(writer: &mut Writer, items: &[NativeOtClientTradeItem]) {
    writer.byte(items.len() as u8);
    for item in items {
        writer.u16(item.client_thing_id);
        if let Some(count) = item.count {
            writer.byte(count);
        }
    }
}

/// Encodes one classic own-trade (`0x7D`) record: the counterparty name plus the sender's
/// offered item list, opening the local trade window.
pub fn encode_native_otclient_own_trade(
    profile: &NativeOtClientProfile,
    counterparty_name: &str,
    items: &[NativeOtClientTradeItem],
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_trade_record(
        profile,
        NATIVE_OTCLIENT_GAME_OWN_TRADE,
        counterparty_name,
        items,
    )
}

/// Encodes one classic counter-trade (`0x7E`) record: the counterparty name plus the items
/// they offer in return.
pub fn encode_native_otclient_counter_trade(
    profile: &NativeOtClientProfile,
    counterparty_name: &str,
    items: &[NativeOtClientTradeItem],
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_trade_record(
        profile,
        NATIVE_OTCLIENT_GAME_COUNTER_TRADE,
        counterparty_name,
        items,
    )
}

fn encode_native_otclient_trade_record(
    profile: &NativeOtClientProfile,
    opcode: u8,
    counterparty_name: &str,
    items: &[NativeOtClientTradeItem],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if counterparty_name.is_empty()
        || counterparty_name.len() > MAX_LOGIN_STRING_BYTES
        || !trade_item_list_fits(items)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(opcode);
    writer.string(counterparty_name);
    write_trade_items(&mut writer, items);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic close-trade (`0x7F`) record: zero payload; both trade windows close.
pub fn encode_native_otclient_close_trade(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CLOSE_TRADE]))
}

/// One classic NPC-shop catalog entry for the 0x7A open record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientShopItem {
    pub client_thing_id: u16,
    pub subtype: Option<u8>,
    pub name: String,
    /// Weight in hundredths of an ounce as the legacy wire format expects.
    pub weight: u32,
    pub buy_price: u32,
    pub sell_price: u32,
}

/// Encodes one classic open-NPC-trade (`0x7A`) record. On the classic profile the list count
/// is a single byte, no NPC-name string precedes it, and prices are 32-bit.
pub fn encode_native_otclient_open_npc_trade(
    profile: &NativeOtClientProfile,
    items: &[NativeOtClientShopItem],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if items.len() > u8::MAX as usize {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_OPEN_NPC_TRADE);
    writer.byte(items.len() as u8);
    for item in items {
        if item.client_thing_id == 0 || item.name.len() > MAX_LOGIN_STRING_BYTES {
            return Err(ProtocolError::UnsupportedNativeClientProfile);
        }
        writer.u16(item.client_thing_id);
        if let Some(subtype) = item.subtype {
            writer.byte(subtype);
        }
        writer.string(&item.name);
        writer.u32(item.weight);
        writer.u32(item.buy_price);
        writer.u32(item.sell_price);
    }
    Ok(Frame(writer.finish()))
}

/// One sellable-good entry for the classic player-goods (`0x7B`) record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientPlayerGood {
    pub client_thing_id: u16,
    pub amount: u8,
}

/// Encodes one classic player-goods (`0x7B`) record: the player's gold plus the items they can
/// sell to this NPC. Classic money is a 32-bit field and amounts are bytes.
pub fn encode_native_otclient_player_goods(
    profile: &NativeOtClientProfile,
    gold: u32,
    goods: &[NativeOtClientPlayerGood],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if goods.len() > u8::MAX as usize {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_GOODS);
    writer.u32(gold);
    writer.byte(goods.len() as u8);
    for good in goods {
        if good.client_thing_id == 0 {
            return Err(ProtocolError::UnsupportedNativeClientProfile);
        }
        writer.u16(good.client_thing_id);
        writer.byte(good.amount);
    }
    Ok(Frame(writer.finish()))
}

/// Encodes one classic close-NPC-trade (`0x7C`) record: zero payload.
pub fn encode_native_otclient_close_npc_trade(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CLOSE_NPC_TRADE]))
}

fn encode_native_otclient_typed_text_message(
    profile: &NativeOtClientProfile,
    class: u8,
    message: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if message.is_empty() || message.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
        return Err(ProtocolError::StringTooLong(message.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_TEXT_MESSAGE);
    writer.byte(class);
    writer.string(message);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic floating colored-text record over one tile. This opcode is parsed without
/// any message-mode translation on every classic protocol, so it remains visible even where
/// `0xAA`/`0xB4` records are discarded; the host uses it for spatial chat feedback.
pub fn encode_native_otclient_animated_text(
    profile: &NativeOtClientProfile,
    position: NativeOtClientPosition,
    color: u8,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if text.is_empty() || text.len() > NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_ANIMATED_TEXT);
    writer.u16(position.x);
    writer.u16(position.y);
    writer.byte(position.z);
    writer.byte(color);
    writer.string(text);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic tile graphical-effect record with a bounded effect id.
pub fn encode_native_otclient_magic_effect(
    profile: &NativeOtClientProfile,
    position: NativeOtClientPosition,
    effect_id: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || effect_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_MAGIC_EFFECT);
    writer.u16(position.x);
    writer.u16(position.y);
    writer.byte(position.z);
    writer.byte(effect_id);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic distance-effect missile (`0x85`): origin tile, destination tile, then the
/// bounded shot id (plan v49 slice 9). Same-floor tiles are the caller's responsibility; the
/// codec only rejects unmapped effect ids.
pub fn encode_native_otclient_distance_effect(
    profile: &NativeOtClientProfile,
    from: NativeOtClientPosition,
    to: NativeOtClientPosition,
    shot_id: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || shot_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_DISTANCE_EFFECT);
    writer.u16(from.x);
    writer.u16(from.y);
    writer.byte(from.z);
    writer.u16(to.x);
    writer.u16(to.y);
    writer.byte(to.z);
    writer.byte(shot_id);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic creature-skull record (`0x90`): creature id then bounded skull value
/// (plan v49 slice 11).
pub fn encode_native_otclient_creature_skull(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    skull: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() || skull > 6 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_SKULL);
    writer.u32(creature_id);
    writer.byte(skull);
    Ok(Frame(writer.finish()))
}

/// Encodes one classic creature-unpass record (`0x92`): creature id then the tile-blocking flag.
pub fn encode_native_otclient_creature_unpass(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    unpass: bool,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_UNPASS);
    writer.u32(creature_id);
    writer.byte(u8::from(unpass));
    Ok(Frame(writer.finish()))
}

/// Encodes the verified classic 740 read-only text-window layout. The selected profile reads the
/// item as a single client item ID, then the maximum length, text, and writer. Writable date and
/// traded fields belong to later protocol features and are deliberately absent.
pub fn encode_native_otclient_read_only_text_window(
    profile: &NativeOtClientProfile,
    window_id: u32,
    client_thing_id: u16,
    text: &str,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    const CLASSIC_740_TEXT_WINDOW_FIXED_BYTES: usize = 13;
    if text.is_empty() || text.len() + CLASSIC_740_TEXT_WINDOW_FIXED_BYTES > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(text.len()));
    }
    let text_length = text
        .len()
        .try_into()
        .map_err(|_| ProtocolError::StringTooLong(text.len()))?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_EDIT_TEXT);
    writer.u32(window_id);
    writer.u16(client_thing_id);
    writer.u16(text_length);
    writer.string(text);
    writer.string("");
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_login_state(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_LOGIN_STATE);
    writer.u32(snapshot.player_id);
    writer.u16(snapshot.server_beat);
    writer.byte(0);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_initialization(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_game_login_state(profile, snapshot)?.0;
    payload.extend_from_slice(&encode_native_otclient_empty_world_map(profile, snapshot)?.0);
    payload.extend_from_slice(&encode_native_otclient_player_bootstrap(profile, snapshot)?.0);
    Ok(Frame(payload))
}

pub fn encode_native_otclient_game_initialization_with_map(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_initialization_with_map_and_static_spawns(
        profile, snapshot, world_map, None,
    )
}

pub fn encode_native_otclient_game_initialization_with_map_and_static_spawns(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        static_spawns,
        None,
    )
}

pub fn encode_native_otclient_game_initialization_with_map_and_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_game_login_state(profile, snapshot)?.0;
    payload.extend_from_slice(
        &encode_native_otclient_map_viewport_with_static_spawns_and_players(
            profile,
            snapshot,
            world_map,
            static_spawns,
            visible_players,
        )?
        .0,
    );
    payload.extend_from_slice(&encode_native_otclient_player_bootstrap(profile, snapshot)?.0);
    Ok(Frame(payload))
}

/// Encodes the fixed-width classic 7.4 player-stats record. Health, mana, capacity, experience,
/// level, and magic level come from persisted player state. The classic record carries a u16
/// level and trailing soul byte; newer-protocol total-capacity and stamina fields are excluded.
pub fn encode_native_otclient_player_stats(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    let level = snapshot.player_level.max(1);
    let vitals = snapshot.player_vitals;
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_STATS);
    writer.u16(vitals.health);
    writer.u16(vitals.max_health);
    writer.u16(vitals.capacity);
    writer.u32(snapshot.player_experience.min(i32::MAX as u64) as u32);
    writer.u16(level);
    writer.byte(0);
    writer.u16(vitals.mana);
    writer.u16(vitals.max_mana);
    writer.byte(vitals.magic_level);
    writer.byte(0);
    writer.byte(0);
    Ok(Frame(writer.finish()))
}

/// Encodes the fixed-width classic 7.4 typed player-skills record. The selected protocol has one
/// byte each for a skill level and percentage. Authoritative levels higher than the packet range
/// are saturated only for client presentation; the persisted core value remains unchanged.
pub fn encode_native_otclient_player_skills(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_SKILLS);
    for (_, progress) in snapshot.player_skills.iter() {
        writer.byte(progress.level.min(u16::from(u8::MAX)) as u8);
        writer.byte(progress.percent);
    }
    Ok(Frame(writer.finish()))
}

/// Encodes the fixed-width classic 7.4 local-player records expected immediately after map
/// delivery, including authoritative typed skills supplied by the core runtime.
pub fn encode_native_otclient_player_bootstrap(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    let mut payload = encode_native_otclient_player_stats(profile, snapshot)?.0;
    let mut writer = Writer::default();
    payload.extend_from_slice(&encode_native_otclient_player_skills(profile, snapshot)?.0);

    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_STATE);
    writer.byte(0);
    payload.extend_from_slice(&writer.finish());
    Ok(Frame(payload))
}

/// Encodes one classic PlayerState record (`0xA2`) with explicit condition bits (plan v49
/// slice 13). Bit assignments follow the legacy client icon map: poison 0x0001, burning
/// 0x0002, energy 0x0004.
pub fn encode_native_otclient_player_state_bits(
    profile: &NativeOtClientProfile,
    state_bits: u16,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_PLAYER_STATE);
    writer.u16(state_bits);
    Ok(Frame(writer.finish()))
}

/// Encodes classic `SetInventory` (`0x78`) for the parser-verified native 740 layout. The caller
/// must obtain `client_thing_id` and subtype semantics from a validated operator-supplied item
/// catalog; this codec does not infer either property from a server item ID.
pub fn encode_native_otclient_set_inventory(
    profile: &NativeOtClientProfile,
    slot: EquipmentSlot,
    item: NativeOtClientClassicItemRecord,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || item.client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_SET_INVENTORY);
    writer.byte(slot.code());
    write_native_otclient_classic_item_record(&mut writer, item);
    Ok(Frame(writer.finish()))
}

/// Encodes classic `DeleteInventory` (`0x79`) for a fixed player equipment slot.
pub fn encode_native_otclient_delete_inventory(
    profile: &NativeOtClientProfile,
    slot: EquipmentSlot,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_DELETE_INVENTORY,
        slot.code(),
    ]))
}

/// Encodes classic `OpenContainer` (`0x6e`) in the exact non-pagination field order consumed by
/// the selected 740 profile: container ID, item record, name, capacity, parent flag, item count,
/// then item records. It does not enable client requests or runtime container ownership.
pub fn encode_native_otclient_open_container(
    profile: &NativeOtClientProfile,
    container: &NativeOtClientClassicOpenContainer,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records()
        || container.name.is_empty()
        || container.name.len() > MAX_LOGIN_STRING_BYTES
        || container.items.len() > u8::MAX as usize
        || container.container_item.client_thing_id == 0
        || container.items.iter().any(|item| item.client_thing_id == 0)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_OPEN_CONTAINER);
    writer.byte(container.container_id);
    write_native_otclient_classic_item_record(&mut writer, container.container_item);
    writer.string(&container.name);
    writer.byte(container.capacity);
    writer.byte(u8::from(container.has_parent));
    writer.byte(container.items.len() as u8);
    for item in &container.items {
        write_native_otclient_classic_item_record(&mut writer, *item);
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

pub fn encode_native_otclient_empty_world_map(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_FULL_MAP);
    write_native_otclient_position(&mut writer, snapshot.player_position);
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;

    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for x in 0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH {
            for y in 0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT {
                if snapshot.ground_thing_id != 0 {
                    writer.u16(snapshot.ground_thing_id);
                }
                let is_player_tile = z == snapshot.player_position.z
                    && x == NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1
                    && y == NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1;
                if is_player_tile && !asset_free {
                    write_native_otclient_unknown_player(&mut writer, snapshot);
                }
                writer.u16(NATIVE_OTCLIENT_TILE_END);
            }
        }
    }

    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes an 18×14×8 classic viewport using original operator-supplied map data.
/// A map tile with `ground_thing_id = 0` inherits the profile-configured fallback so a world
/// document can remain portable across lawful client asset sets.
pub fn encode_native_otclient_map_viewport(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_map_viewport_with_static_spawns(profile, snapshot, world_map, None)
}

pub fn encode_native_otclient_map_viewport_with_static_spawns(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_map_viewport_with_static_spawns_and_players(
        profile,
        snapshot,
        world_map,
        static_spawns,
        None,
    )
}

pub fn encode_native_otclient_map_viewport_with_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_FULL_MAP);
    write_native_otclient_position(&mut writer, snapshot.player_position);
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;
    let center_x = (NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) as i16;
    let center_y = (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1) as i16;
    let viewport_cells = NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH
        * NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT
        * NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS;
    let mut encoded_cells = 0usize;
    let mut static_entity_count = 0usize;
    let mut visible_player_count = 0usize;

    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for x in 0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH {
            for y in 0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT {
                let remaining_cells = viewport_cells.saturating_sub(encoded_cells + 1);
                let position = Position {
                    x: snapshot
                        .player_position
                        .x
                        .saturating_add_signed(x as i16 - center_x),
                    y: snapshot
                        .player_position
                        .y
                        .saturating_add_signed(y as i16 - center_y),
                    z,
                };
                let ground_thing_id = world_map
                    .tile(position)
                    .map(|tile| tile.ground_thing_id)
                    .filter(|ground_thing_id| *ground_thing_id != 0)
                    .unwrap_or(snapshot.ground_thing_id);
                if ground_thing_id != 0
                    && native_map_record_fits_budget(&writer, 2, remaining_cells)
                {
                    writer.u16(ground_thing_id);
                }
                if let Some(items) = world_map.tile_items(position) {
                    for item in items
                        .iter()
                        .skip(1)
                        .take(NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS)
                    {
                        let thing_id = item.client_thing_id.unwrap_or(item.server_id);
                        if thing_id != 0
                            && native_map_record_fits_budget(&writer, 2, remaining_cells)
                        {
                            writer.u16(thing_id);
                        } else if thing_id != 0 {
                            break;
                        }
                    }
                }
                if let Some(static_spawns) = static_spawns {
                    for entity in static_spawns.at(position) {
                        if static_entity_count >= NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT {
                            break;
                        }
                        let mut entity_record = Writer::default();
                        write_native_otclient_unknown_static_entity(&mut entity_record, entity);
                        if native_map_record_fits_budget(
                            &writer,
                            entity_record.len(),
                            remaining_cells,
                        ) {
                            writer.bytes(&entity_record.finish());
                            static_entity_count += 1;
                        } else {
                            break;
                        }
                    }
                }
                if !asset_free {
                    if let Some(visible_players) = visible_players {
                        for player in visible_players.iter().filter(|player| {
                            player.player_id != snapshot.player_id
                                && player.position.x == position.x
                                && player.position.y == position.y
                                && player.position.z == position.z
                        }) {
                            if visible_player_count
                                >= NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT
                            {
                                break;
                            }
                            let mut player_record = Writer::default();
                            write_native_otclient_unknown_visible_player(
                                &mut player_record,
                                player,
                            );
                            if native_map_record_fits_budget(
                                &writer,
                                player_record.len(),
                                remaining_cells,
                            ) {
                                writer.bytes(&player_record.finish());
                                visible_player_count += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
                let is_player_tile = z == snapshot.player_position.z
                    && x == NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1
                    && y == NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1;
                if is_player_tile && !asset_free {
                    let mut player_record = Writer::default();
                    write_native_otclient_unknown_player(&mut player_record, snapshot);
                    if native_map_record_fits_budget(&writer, player_record.len(), remaining_cells)
                    {
                        writer.bytes(&player_record.finish());
                    }
                }
                writer.u16(NATIVE_OTCLIENT_TILE_END);
                encoded_cells += 1;
            }
        }
    }

    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes the single newly exposed classic viewport edge after one confirmed cardinal step.
/// It is deliberately limited to map rendering; it does not add movement, combat, or AI behavior.
pub fn encode_native_otclient_map_step_with_static_spawns_and_players(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
    world_map: &WorldMap,
    static_spawns: Option<&FeTfsStaticSpawnCollection>,
    visible_players: Option<&[NativeOtClientVisiblePlayer]>,
    direction: NativeOtClientCardinalDirection,
) -> Result<Frame, ProtocolError> {
    validate_native_empty_world_snapshot(profile, snapshot)?;
    let mut writer = Writer::default();
    let center_x = (NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH / 2 - 1) as i16;
    let center_y = (NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT / 2 - 1) as i16;
    let asset_free = snapshot.ground_thing_id == 0 && snapshot.player_look_type == 0;
    let mut static_entity_count = 0usize;
    let mut visible_player_count = 0usize;
    let positions = match direction {
        NativeOtClientCardinalDirection::North => (0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH)
            .map(|x| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(x as i16 - center_x),
                y: snapshot.player_position.y.saturating_add_signed(-center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::East => (0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT)
            .map(|y| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH as i16 - 1 - center_x),
                y: snapshot
                    .player_position
                    .y
                    .saturating_add_signed(y as i16 - center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::South => (0..NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH)
            .map(|x| Position {
                x: snapshot
                    .player_position
                    .x
                    .saturating_add_signed(x as i16 - center_x),
                y: snapshot.player_position.y.saturating_add_signed(
                    NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT as i16 - 1 - center_y,
                ),
                z: 0,
            })
            .collect::<Vec<_>>(),
        NativeOtClientCardinalDirection::West => (0..NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT)
            .map(|y| Position {
                x: snapshot.player_position.x.saturating_add_signed(-center_x),
                y: snapshot
                    .player_position
                    .y
                    .saturating_add_signed(y as i16 - center_y),
                z: 0,
            })
            .collect::<Vec<_>>(),
    };
    writer.byte(match direction {
        NativeOtClientCardinalDirection::North => 0x65,
        NativeOtClientCardinalDirection::East => 0x66,
        NativeOtClientCardinalDirection::South => 0x67,
        NativeOtClientCardinalDirection::West => 0x68,
    });
    let step_cells = positions.len() * NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS;
    let mut encoded_cells = 0usize;
    for z in (0..NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8).rev() {
        for position in &positions {
            let remaining_cells = step_cells.saturating_sub(encoded_cells + 1);
            let position = Position { z, ..*position };
            let ground_thing_id = world_map
                .tile(position)
                .map(|tile| tile.ground_thing_id)
                .filter(|ground_thing_id| *ground_thing_id != 0)
                .unwrap_or(snapshot.ground_thing_id);
            if ground_thing_id != 0 && native_map_record_fits_budget(&writer, 2, remaining_cells) {
                writer.u16(ground_thing_id);
            }
            if let Some(items) = world_map.tile_items(position) {
                for item in items
                    .iter()
                    .skip(1)
                    .take(NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS)
                {
                    let thing_id = item.client_thing_id.unwrap_or(item.server_id);
                    if thing_id != 0 && native_map_record_fits_budget(&writer, 2, remaining_cells) {
                        writer.u16(thing_id);
                    } else if thing_id != 0 {
                        break;
                    }
                }
            }
            if let Some(static_spawns) = static_spawns {
                for entity in static_spawns.at(position) {
                    if static_entity_count >= NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT {
                        break;
                    }
                    let mut entity_record = Writer::default();
                    write_native_otclient_unknown_static_entity(&mut entity_record, entity);
                    if native_map_record_fits_budget(&writer, entity_record.len(), remaining_cells)
                    {
                        writer.bytes(&entity_record.finish());
                        static_entity_count += 1;
                    } else {
                        break;
                    }
                }
            }
            if !asset_free {
                if let Some(visible_players) = visible_players {
                    for player in visible_players.iter().filter(|player| {
                        player.position.x == position.x
                            && player.position.y == position.y
                            && player.position.z == position.z
                    }) {
                        if visible_player_count >= NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT {
                            break;
                        }
                        let mut player_record = Writer::default();
                        write_native_otclient_unknown_visible_player(&mut player_record, player);
                        if native_map_record_fits_budget(
                            &writer,
                            player_record.len(),
                            remaining_cells,
                        ) {
                            writer.bytes(&player_record.finish());
                            visible_player_count += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            writer.u16(NATIVE_OTCLIENT_TILE_END);
            encoded_cells += 1;
        }
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

fn native_map_record_fits_budget(
    writer: &Writer,
    record_bytes: usize,
    remaining_cells: usize,
) -> bool {
    writer
        .len()
        .saturating_add(record_bytes)
        .saturating_add(2)
        .saturating_add(remaining_cells.saturating_mul(4))
        <= MAX_FRAME_SIZE
}

pub fn decode_native_otclient_cardinal_move_request(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientCardinalDirection, ProtocolError> {
    match decode_native_otclient_game_action(frame, profile)? {
        NativeOtClientGameAction::CardinalMove(direction) => Ok(direction),
        _ => Err(ProtocolError::InvalidNativeGameRequest),
    }
}

pub fn decode_native_otclient_game_action(
    frame: &Frame,
    profile: &NativeOtClientProfile,
) -> Result<NativeOtClientGameAction, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut reader = Reader::new(&frame.0);
    let action = match reader.byte()? {
        NATIVE_OTCLIENT_ENTER_GAME => NativeOtClientGameAction::EnterGame,
        NATIVE_OTCLIENT_LEAVE_GAME => NativeOtClientGameAction::LeaveGame,
        NATIVE_OTCLIENT_CLIENT_PING => NativeOtClientGameAction::Ping,
        NATIVE_OTCLIENT_CLIENT_PING_BACK => NativeOtClientGameAction::PingBack,
        NATIVE_OTCLIENT_CLIENT_STOP => NativeOtClientGameAction::Stop,
        NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW => {
            NativeOtClientGameAction::CancelAttackAndFollow
        }
        NATIVE_OTCLIENT_CLIENT_AUTO_WALK => {
            let length = usize::from(reader.byte()?);
            if length > 64 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            let mut path = Vec::with_capacity(length);
            for _ in 0..length {
                path.push(
                    NativeOtClientAutoWalkDirection::from_native_byte(reader.byte()?)
                        .ok_or(ProtocolError::InvalidNativeGameRequest)?,
                );
            }
            NativeOtClientGameAction::AutoWalk(path)
        }
        NATIVE_OTCLIENT_CLIENT_THROW_ITEM => {
            if reader.remaining() != 14 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::ThrowItem {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                count: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM => {
            if reader.remaining() != 9 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItem {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                client_thing_id: reader.u16()?,
                stack_position: reader.byte()?,
                index: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX => {
            if reader.remaining() != 16 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItemEx {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                target_client_thing_id: reader.u16()?,
                target_stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE => {
            if reader.remaining() != 12 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::UseItemOnCreature {
                source_position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                source_client_thing_id: reader.u16()?,
                source_stack_position: reader.byte()?,
                target_creature_id: reader.u32()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM => {
            if reader.remaining() != 8 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RotateItem {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                client_thing_id: reader.u16()?,
                stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_LOOK_MAP => {
            if reader.remaining() != 8 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::LookMap {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                thing_id: reader.u16()?,
                stack_position: reader.byte()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE => {
            if reader.remaining() != 4 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::LookCreature {
                creature_id: reader.u32()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_REQUEST_TRADE => {
            if reader.remaining() != 15 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RequestTrade {
                position: NativeOtClientPosition {
                    x: reader.u16()?,
                    y: reader.u16()?,
                    z: reader.byte()?,
                },
                client_thing_id: reader.u16()?,
                stack_position: reader.byte()?,
                target_creature_id: reader.u32()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_ACCEPT_TRADE => {
            if reader.remaining() != 0 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::AcceptTrade
        }
        NATIVE_OTCLIENT_CLIENT_BUY_ITEM => {
            if reader.remaining() != 7 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::NpcBuy {
                client_thing_id: reader.u16()?,
                subtype: reader.byte()?,
                amount: reader.byte()?,
                _ignore_capacity: reader.byte()? != 0,
                _buy_with_backpack: reader.byte()? != 0,
            }
        }
        NATIVE_OTCLIENT_CLIENT_SELL_ITEM => {
            if reader.remaining() != 5 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::NpcSell {
                client_thing_id: reader.u16()?,
                subtype: reader.byte()?,
                amount: reader.byte()?,
                _ignore_equipped: reader.byte()? != 0,
            }
        }
        NATIVE_OTCLIENT_CLIENT_CLOSE_NPC_TRADE => {
            if reader.remaining() != 0 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::NpcTradeClose
        }
        NATIVE_OTCLIENT_CLIENT_REJECT_TRADE => {
            if reader.remaining() != 0 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RejectTrade
        }
        NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT => {
            if reader.remaining() != 5 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::ChangeOutfit(NativeOtClientClassicOutfit {
                look_type: reader.byte()?,
                head: reader.byte()?,
                body: reader.byte()?,
                legs: reader.byte()?,
                feet: reader.byte()?,
            })
        }
        NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER => {
            NativeOtClientGameAction::CloseContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER => {
            NativeOtClientGameAction::UpArrowContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER => {
            NativeOtClientGameAction::UpdateContainer(reader.byte()?)
        }
        NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT => NativeOtClientGameAction::RequestOutfit,
        NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG => NativeOtClientGameAction::RequestQuestLog,
        NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE => {
            if reader.remaining() != 2 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::RequestQuestLine {
                quest_id: reader.u16()?,
            }
        }
        NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS => NativeOtClientGameAction::RequestChannels,
        NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL => NativeOtClientGameAction::JoinChannel(reader.u16()?),
        NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL => {
            NativeOtClientGameAction::LeaveChannel(reader.u16()?)
        }
        NATIVE_OTCLIENT_CLIENT_ADD_VIP => {
            NativeOtClientGameAction::AddVip(reader.string(MAX_LOGIN_STRING_BYTES)?)
        }
        NATIVE_OTCLIENT_CLIENT_REMOVE_VIP => NativeOtClientGameAction::RemoveVip(reader.u32()?),
        NATIVE_OTCLIENT_CLIENT_EDIT_VIP => {
            let target_player_id = reader.u32()?;
            let description = reader.string(MAX_LOGIN_STRING_BYTES)?;
            let icon = reader.u32()?;
            let notify = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::InvalidNativeGameRequest),
            };
            NativeOtClientGameAction::EditVip {
                target_player_id,
                description,
                icon,
                notify,
            }
        }
        NATIVE_OTCLIENT_CLIENT_TALK => {
            let mode = reader.byte()?;
            let (channel_id, recipient, message) = match mode {
                4 | 5 | 11 => {
                    let recipient = reader.string(MAX_LOGIN_STRING_BYTES)?;
                    (
                        None,
                        Some(recipient),
                        reader.string(MAX_LOGIN_STRING_BYTES)?,
                    )
                }
                6 | 7 | 8 | 10 | 12 => {
                    let channel_id = reader.u16()?;
                    (
                        Some(channel_id),
                        None,
                        reader.string(MAX_LOGIN_STRING_BYTES)?,
                    )
                }
                _ => (None, None, reader.string(MAX_LOGIN_STRING_BYTES)?),
            };
            NativeOtClientGameAction::Talk(NativeOtClientTalkRequest {
                mode,
                channel_id,
                recipient,
                message,
            })
        }
        NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES => {
            NativeOtClientGameAction::ChangeFightModes(NativeOtClientFightModeRequest {
                mode: NativeOtClientFightMode::from_classic_value(reader.byte()?),
                chase: reader.byte()? != 0,
                secure: reader.byte()? != 0,
            })
        }
        NATIVE_OTCLIENT_CLIENT_SELECT_TARGET => {
            NativeOtClientGameAction::SelectTarget(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_SELECT_FOLLOW => {
            NativeOtClientGameAction::SelectFollow(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY => {
            NativeOtClientGameAction::PartyInvite(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_JOIN_PARTY => NativeOtClientGameAction::PartyJoin(reader.u32()?),
        NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION => {
            NativeOtClientGameAction::PartyRevokeInvitation(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP => {
            NativeOtClientGameAction::PartyPassLeadership(reader.u32()?)
        }
        NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY => NativeOtClientGameAction::PartyLeave,
        NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE => {
            let active = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::InvalidNativeGameRequest),
            };
            if reader.byte()? != 0 {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            NativeOtClientGameAction::PartySharedExperience(active)
        }
        opcode if is_native_otclient_compatibility_interaction(opcode) => {
            if reader.remaining() > NATIVE_OTCLIENT_MAX_IGNORED_INTERACTION_BYTES {
                return Err(ProtocolError::InvalidNativeGameRequest);
            }
            reader.take(reader.remaining())?;
            NativeOtClientGameAction::IgnoredInteraction(opcode)
        }
        opcode => NativeOtClientCardinalDirection::from_client_opcode(opcode)
            .map(NativeOtClientGameAction::CardinalMove)
            .or_else(|| {
                NativeOtClientAutoWalkDirection::from_direct_diagonal_opcode(opcode)
                    .map(NativeOtClientGameAction::DiagonalMove)
            })
            .or_else(|| {
                NativeOtClientCardinalDirection::from_turn_opcode(opcode)
                    .map(NativeOtClientGameAction::Turn)
            })
            .ok_or(ProtocolError::InvalidNativeGameRequest)?,
    };
    if !reader.done() {
        return Err(ProtocolError::InvalidNativeGameRequest);
    }
    Ok(action)
}

fn is_native_otclient_compatibility_interaction(opcode: u8) -> bool {
    matches!(
        opcode,
            0x77
            | 0x86..=0x8b
            | 0x97..=0x9f
            | 0xa3..=0xad
            | 0xca
    )
}

pub fn encode_native_otclient_game_ping_back(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_PING_BACK]))
}

pub fn encode_native_otclient_game_ping(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_PING]))
}

/// Encodes the classic zero-payload `ClearTarget` (`0xA3`) record for the supported native 740
/// profile. Later profiles may append an attack-sequence field and are intentionally excluded.
pub fn encode_native_otclient_clear_target(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CLEAR_TARGET]))
}

/// Encodes the classic three-byte `PlayerModes` (`0xA7`) record for the supported native 740
/// profile. Later protocol variants may append a PvP-mode byte and are intentionally excluded.
pub fn encode_native_otclient_player_modes(
    profile: &NativeOtClientProfile,
    modes: NativeOtClientFightModeRequest,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mode = match modes.mode {
        NativeOtClientFightMode::Attack => 1,
        NativeOtClientFightMode::Balanced => 2,
        NativeOtClientFightMode::Defense => 3,
    };
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_PLAYER_MODES,
        mode,
        u8::from(modes.chase),
        u8::from(modes.secure),
    ]))
}

/// Encodes classic `CloseContainer` (`0x6F`) for the supported native 740 profile. This closes
/// only the client view; authoritative container ownership and persistence are separate.
pub fn encode_native_otclient_close_container(
    profile: &NativeOtClientProfile,
    container_id: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER,
        container_id,
    ]))
}

/// Encodes classic `CreateInContainer` (`0x70`) for the parser-verified native 740 layout with
/// container pagination off (no slot u16). The caller must obtain `client_thing_id` and subtype
/// semantics from a validated operator-supplied item catalog.
pub fn encode_native_otclient_create_in_container(
    profile: &NativeOtClientProfile,
    container_id: u8,
    item: NativeOtClientClassicItemRecord,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || item.client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATE_IN_CONTAINER);
    writer.byte(container_id);
    write_native_otclient_classic_item_record(&mut writer, item);
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes classic `ChangeInContainer` (`0x71`): rewrites one existing slot's thing in an open
/// container window.
pub fn encode_native_otclient_change_in_container(
    profile: &NativeOtClientProfile,
    container_id: u8,
    slot: u8,
    item: NativeOtClientClassicItemRecord,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || item.client_thing_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER);
    writer.byte(container_id);
    writer.byte(slot);
    write_native_otclient_classic_item_record(&mut writer, item);
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes classic `DeleteInContainer` (`0x72`): removes one thing from an open container window.
pub fn encode_native_otclient_delete_in_container(
    profile: &NativeOtClientProfile,
    container_id: u8,
    slot: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![
        NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER,
        container_id,
        slot,
    ]))
}

/// Encodes the classic native death notification. The supported 7.4 profile admits only the
/// opcode: death type and penalty fields belong to later client feature sets and are not emitted.
pub fn encode_native_otclient_game_death(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_DEATH]))
}

/// Encodes an explicitly empty classic 7.4 Quest Log response. FE does not yet claim quest
/// content, persistence, mission lines, scripts, or gameplay semantics.
pub fn encode_native_otclient_empty_quest_log(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_QUEST_LOG);
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

/// Encodes the classic quest-list response: one u16 entry count followed by per-entry
/// u16 quest ID and bounded display name. Entries must be sorted by ID and carry nonzero IDs
/// with nonempty names; the caller owns that ordering.
pub fn encode_native_otclient_quest_list(
    profile: &NativeOtClientProfile,
    quests: &[(u16, String)],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if quests.len() > u16::MAX as usize
        || quests
            .iter()
            .any(|(id, name)| *id == 0 || name.is_empty() || name.len() > MAX_LOGIN_STRING_BYTES)
    {
        return Err(ProtocolError::InvalidLength(quests.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_QUEST_LOG);
    writer.u16(quests.len() as u16);
    for (id, name) in quests {
        writer.u16(*id);
        writer.string(name);
    }
    let frame = Frame(writer.finish());
    if frame.0.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::InvalidLength(frame.0.len()));
    }
    Ok(frame)
}

/// Encodes an explicitly empty classic 7.4 channel list. FE does not yet claim channel
/// membership, private messages, moderation, persistence, guild, party, or gameplay semantics.
pub fn encode_native_otclient_empty_channel_list(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_channel_list(profile, &[])
}

/// Encodes one bounded classic 7.4 channel list. The entries must originate from a validated
/// authoritative catalog; membership and social-system behavior are outside this codec.
pub fn encode_native_otclient_channel_list(
    profile: &NativeOtClientProfile,
    channels: &[NativeOtClientClassicChannel],
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let count = u8::try_from(channels.len()).map_err(|_| ProtocolError::TooManyChannels)?;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CHANNELS);
    writer.byte(count);
    for channel in channels {
        if channel.name.len() > u16::MAX as usize {
            return Err(ProtocolError::StringTooLong(channel.name.len()));
        }
        writer.u16(channel.id);
        writer.string(&channel.name);
    }
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 VIP entry for the profile's no-additional-info parser branch. The
/// record contains only the character ID, name, and explicit online/offline status; description,
/// icon, notification, and VIP group fields belong to later client features.
pub fn encode_native_otclient_classic_vip_entry(
    profile: &NativeOtClientProfile,
    target_player_id: u32,
    target_player_name: &str,
    online: bool,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records()
        || target_player_id == 0
        || target_player_name.is_empty()
        || target_player_name.len() > MAX_LOGIN_STRING_BYTES
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let fixed_bytes = 8usize;
    if target_player_name.len() + fixed_bytes > MAX_FRAME_SIZE {
        return Err(ProtocolError::StringTooLong(target_player_name.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_VIP_ADD);
    writer.u32(target_player_id);
    writer.string(target_player_name);
    writer.byte(u8::from(online));
    Ok(Frame(writer.finish()))
}

/// Encodes one classic 740 live VIP presence transition. This profile predates login-pending
/// state, so `0xd3 + id` means online and `0xd4 + id` means offline; no status byte is emitted.
/// Newer client layouts remain outside this profile-gated codec.
pub fn encode_native_otclient_classic_vip_presence(
    profile: &NativeOtClientProfile,
    target_player_id: u32,
    online: bool,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_classic_740_inventory_records() || target_player_id == 0 {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(if online {
        NATIVE_OTCLIENT_GAME_VIP_STATE
    } else {
        NATIVE_OTCLIENT_GAME_VIP_LOGOUT
    });
    writer.u32(target_player_id);
    Ok(Frame(writer.finish()))
}

/// Opens one validated configured public channel with explicit empty player and invitation lists.
/// This codec does not create session membership or enable channel messages.
pub fn encode_native_otclient_open_public_channel(
    profile: &NativeOtClientProfile,
    channel: &NativeOtClientClassicChannel,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    if channel.name.len() > u16::MAX as usize {
        return Err(ProtocolError::StringTooLong(channel.name.len()));
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_OPEN_CHANNEL);
    writer.u16(channel.id);
    writer.string(&channel.name);
    writer.u16(0);
    writer.u16(0);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_game_cancel_walk(
    profile: &NativeOtClientProfile,
) -> Result<Frame, ProtocolError> {
    encode_native_otclient_game_cancel_walk_facing(profile, 0)
}

pub fn encode_native_otclient_game_cancel_walk_facing(
    profile: &NativeOtClientProfile,
    direction: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(Frame(vec![NATIVE_OTCLIENT_GAME_CANCEL_WALK, direction]))
}

pub fn encode_native_otclient_choose_outfit(
    profile: &NativeOtClientProfile,
    current_outfit: NativeOtClientClassicOutfit,
    first_look_type: u8,
    last_look_type: u8,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || current_outfit.look_type == 0
        || first_look_type == 0
        || first_look_type > last_look_type
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT);
    write_native_otclient_classic_outfit(&mut writer, current_outfit);
    writer.byte(first_look_type);
    writer.byte(last_look_type);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_creature_outfit(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    outfit: NativeOtClientClassicOutfit,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
        || outfit.look_type == 0
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT);
    writer.u32(creature_id);
    write_native_otclient_classic_outfit(&mut writer, outfit);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_creature_health(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    health: u16,
    max_health: u16,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !is_native_otclient_creature_id(creature_id)
        || max_health == 0
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let health_percent = ((u32::from(health.min(max_health)) * 100) / u32::from(max_health)) as u8;
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_HEALTH);
    writer.u32(creature_id);
    writer.byte(health_percent);
    Ok(Frame(writer.finish()))
}

/// Encodes the exact classic creature-party (`0x91`) record for one active native player. The
/// caller owns party relationships, spectator visibility, and delivery timing; this protocol
/// function only permits the audited basic shield values for the supported classic profile.
pub fn encode_native_otclient_creature_party_shield(
    profile: &NativeOtClientProfile,
    creature_id: u32,
    shield: NativeOtClientClassicPartyShield,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_CREATURE_PARTY);
    writer.u32(creature_id);
    writer.byte(shield.classic_value());
    Ok(Frame(writer.finish()))
}

fn is_native_otclient_creature_id(creature_id: u32) -> bool {
    (NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&creature_id)
        || creature_id > NATIVE_OTCLIENT_PLAYER_ID_END
}

pub fn encode_native_otclient_move_creature(
    profile: &NativeOtClientProfile,
    player_id: u32,
    position: NativeOtClientPosition,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END).contains(&player_id)
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
    writer.u16(NATIVE_OTCLIENT_MAPPED_CREATURE);
    writer.u32(player_id);
    write_native_otclient_position(&mut writer, position);
    Ok(Frame(writer.finish()))
}

pub fn encode_native_otclient_move_creature_at(
    profile: &NativeOtClientProfile,
    old_position: NativeOtClientPosition,
    old_stack_position: u8,
    new_position: NativeOtClientPosition,
) -> Result<Frame, ProtocolError> {
    if !profile.supports_current_native_foundation() || old_stack_position == u8::MAX {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    let mut writer = Writer::default();
    writer.byte(NATIVE_OTCLIENT_GAME_MOVE_CREATURE);
    write_native_otclient_position(&mut writer, old_position);
    writer.byte(old_stack_position);
    write_native_otclient_position(&mut writer, new_position);
    Ok(Frame(writer.finish()))
}

fn validate_native_empty_world_snapshot(
    profile: &NativeOtClientProfile,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) -> Result<(), ProtocolError> {
    if !profile.supports_current_native_foundation()
        || !(NATIVE_OTCLIENT_PLAYER_ID_START..NATIVE_OTCLIENT_PLAYER_ID_END)
            .contains(&snapshot.player_id)
        || snapshot.player_position.z >= NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS as u8
        || snapshot.player_direction > 3
        || snapshot.player_speed == 0
        || snapshot.server_beat == 0
        || snapshot.player_name.len() > MAX_LOGIN_STRING_BYTES
    {
        return Err(ProtocolError::UnsupportedNativeClientProfile);
    }
    Ok(())
}

fn write_native_otclient_position(writer: &mut Writer, position: NativeOtClientPosition) {
    writer.u16(position.x);
    writer.u16(position.y);
    writer.byte(position.z);
}

fn write_native_otclient_classic_item_record(
    writer: &mut Writer,
    item: NativeOtClientClassicItemRecord,
) {
    writer.u16(item.client_thing_id);
    if let Some(subtype) = item.subtype {
        writer.byte(subtype);
    }
}

fn write_native_otclient_unknown_player(
    writer: &mut Writer,
    snapshot: &NativeOtClientEmptyWorldSnapshot,
) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(snapshot.player_id);
    writer.string(&snapshot.player_name);
    writer.byte(100);
    writer.byte(snapshot.player_direction);
    writer.byte(snapshot.player_look_type);
    if snapshot.player_look_type == 0 {
        writer.u16(0);
    } else {
        writer.byte(0);
        writer.byte(0);
        writer.byte(0);
        writer.byte(0);
    }
    writer.byte(0);
    writer.byte(0);
    writer.u16(snapshot.player_speed);
    writer.byte(0);
    writer.byte(0);
}

fn write_native_otclient_classic_outfit(writer: &mut Writer, outfit: NativeOtClientClassicOutfit) {
    writer.byte(outfit.look_type);
    writer.byte(outfit.head);
    writer.byte(outfit.body);
    writer.byte(outfit.legs);
    writer.byte(outfit.feet);
}

fn write_native_otclient_unknown_visible_player(
    writer: &mut Writer,
    player: &NativeOtClientVisiblePlayer,
) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(player.player_id);
    writer.string(&player.name);
    writer.byte(player.health_percent);
    writer.byte(player.direction);
    if player.outfit.look_type == 0 {
        writer.u16(0);
    } else {
        write_native_otclient_classic_outfit(writer, player.outfit);
        writer.byte(0);
    }
    writer.u16(player.speed);
    writer.byte(0);
    writer.byte(0);
}

fn write_native_otclient_unknown_static_entity(writer: &mut Writer, entity: &FeTfsStaticEntity) {
    writer.u16(NATIVE_OTCLIENT_UNKNOWN_CREATURE);
    writer.u32(0);
    writer.u32(entity.id);
    writer.string(&entity.name);
    writer.byte(entity.health_percent);
    writer.byte(entity.direction);
    writer.byte(entity.look_type);
    writer.byte(entity.head);
    writer.byte(entity.body);
    writer.byte(entity.legs);
    writer.byte(entity.feet);
    writer.byte(entity.addons);
    writer.byte(0);
    writer.u16(entity.speed);
    writer.byte(0);
    writer.byte(0);
}
