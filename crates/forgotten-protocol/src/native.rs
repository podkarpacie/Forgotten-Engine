//! Native OTClient 7.4 client codecs: character list, login request/game request decode,
//! action decoding, and all classic 740 game/status/container/trade/chat/effect encoders.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterListEntry {
    pub name: String,
    pub world_name: String,
    pub address: IpAddr,
    pub port: u16,
}

pub const NATIVE_OTCLIENT_ENTER_ACCOUNT: u8 = 0x01;
pub const NATIVE_OTCLIENT_PENDING_GAME: u8 = 0x0a;
pub const NATIVE_OTCLIENT_LOGIN_ERROR: u8 = 0x0a;
pub const NATIVE_OTCLIENT_LOGIN_CHARACTER_LIST: u8 = 0x64;
pub const NATIVE_OTCLIENT_GAME_LOGIN_ERROR: u8 = 0x14;
pub const NATIVE_OTCLIENT_GAME_LOGIN_STATE: u8 = 0x0a;
pub const NATIVE_OTCLIENT_GAME_DEATH: u8 = 0x28;
pub const NATIVE_OTCLIENT_GAME_FULL_MAP: u8 = 0x64;
pub const NATIVE_OTCLIENT_GAME_MOVE_CREATURE: u8 = 0x6d;
pub const NATIVE_OTCLIENT_GAME_OPEN_CONTAINER: u8 = 0x6e;
pub const NATIVE_OTCLIENT_GAME_SET_INVENTORY: u8 = 0x78;
pub const NATIVE_OTCLIENT_GAME_DELETE_INVENTORY: u8 = 0x79;
pub const NATIVE_OTCLIENT_GAME_PING_BACK: u8 = 0x1d;
pub const NATIVE_OTCLIENT_GAME_PING: u8 = 0x1e;
pub const NATIVE_OTCLIENT_GAME_PLAYER_STATS: u8 = 0xa0;
pub const NATIVE_OTCLIENT_GAME_PLAYER_SKILLS: u8 = 0xa1;
pub const NATIVE_OTCLIENT_GAME_PLAYER_STATE: u8 = 0xa2;
pub const NATIVE_OTCLIENT_GAME_CLEAR_TARGET: u8 = 0xa3;
pub const NATIVE_OTCLIENT_GAME_PLAYER_MODES: u8 = 0xa7;
pub const NATIVE_OTCLIENT_GAME_TALK: u8 = 0xaa;
pub const NATIVE_OTCLIENT_GAME_CLOSE_CONTAINER: u8 = 0x6f;
/// Classic CreateInContainer (0x70): one appended thing in an open container window.
pub const NATIVE_OTCLIENT_GAME_CREATE_IN_CONTAINER: u8 = 0x70;
/// Classic ChangeInContainer (0x71): one rewritten thing at an existing container slot.
pub const NATIVE_OTCLIENT_GAME_CHANGE_IN_CONTAINER: u8 = 0x71;
/// Classic DeleteInContainer (0x72): one removed thing from an open container window.
pub const NATIVE_OTCLIENT_GAME_DELETE_IN_CONTAINER: u8 = 0x72;
pub const NATIVE_OTCLIENT_GAME_CREATURE_HEALTH: u8 = 0x8c;
pub const NATIVE_OTCLIENT_GAME_CREATURE_OUTFIT: u8 = 0x8e;
pub const NATIVE_OTCLIENT_GAME_CREATURE_PARTY: u8 = 0x91;
pub const NATIVE_OTCLIENT_GAME_EDIT_TEXT: u8 = 0x96;
/// Classic open-NPC-trade record (0x7A): NPC name + item list with names, weight, buy/sell.
pub const NATIVE_OTCLIENT_GAME_OPEN_NPC_TRADE: u8 = 0x7a;
/// Classic player-goods record (0x7B): player gold + sellable item list.
pub const NATIVE_OTCLIENT_GAME_PLAYER_GOODS: u8 = 0x7b;
/// Classic close-NPC-trade record (0x7C): zero payload.
pub const NATIVE_OTCLIENT_GAME_CLOSE_NPC_TRADE: u8 = 0x7c;
/// Classic own-trade record (0x7D): counterparty name + item list the sender offers.
pub const NATIVE_OTCLIENT_GAME_OWN_TRADE: u8 = 0x7d;
/// Classic counter-trade record (0x7E): counterparty name + item list offered back.
pub const NATIVE_OTCLIENT_GAME_COUNTER_TRADE: u8 = 0x7e;
/// Classic close-trade record (0x7F): zero payload; both windows close.
pub const NATIVE_OTCLIENT_GAME_CLOSE_TRADE: u8 = 0x7f;
pub const NATIVE_OTCLIENT_GAME_TEXT_MESSAGE: u8 = 0xb4;
pub const NATIVE_OTCLIENT_MESSAGE_STATUS_DEFAULT: u8 = 0x15;
pub const NATIVE_OTCLIENT_MESSAGE_SAY: u8 = 0x01;
/// Classic server-talk mode for a whispered message delivered only to nearby listeners.
pub const NATIVE_OTCLIENT_MESSAGE_WHISPER: u8 = 0x02;
/// Classic server-talk mode for a yelled message delivered to a wider same-floor audience.
pub const NATIVE_OTCLIENT_MESSAGE_YELL: u8 = 0x03;
/// Classic text-message class rendered as green center-screen inspection text (Look replies).
pub const NATIVE_OTCLIENT_MESSAGE_LOOK: u8 = 0x16;
/// Classic text-message class rendered in the console and status area for rejected actions.
pub const NATIVE_OTCLIENT_MESSAGE_FAILURE: u8 = 0x17;
/// Classic text-message class rendered in the console for login/welcome information.
pub const NATIVE_OTCLIENT_MESSAGE_LOGIN: u8 = 0x14;
/// Classic text-message class rendered as white center-screen text and mirrored into the
/// Server Log console tab - the channel for server-wide announcements on classic profiles.
pub const NATIVE_OTCLIENT_MESSAGE_GAME: u8 = 0x13;
/// Classic server-talk mode for a gamemaster broadcast delivered to every connected player.
pub const NATIVE_OTCLIENT_MESSAGE_GM_BROADCAST: u8 = 0x09;
/// Classic citizen look type used when an operator enables a real world without configuring a
/// player appearance. Look type zero renders clientside as the invisible effect, so FE never
/// emits it for players outside the deliberate asset-free diagnostic fixture.
pub const NATIVE_OTCLIENT_DEFAULT_PLAYER_LOOK_TYPE: u8 = 128;
/// Classic ground tile sprite used when a real world runs native clients without an explicit
/// ground id; zero floors render clientside as nothing.
pub const NATIVE_OTCLIENT_DEFAULT_GROUND_THING_ID: u16 = 102;
/// Inclusive classic chooser range offered when no explicit outfit range is configured. Covers
/// the standard citizen appearances so the outfit window works out of the box.
pub const NATIVE_OTCLIENT_DEFAULT_OUTFIT_FIRST_LOOK_TYPE: u8 = 128;
pub const NATIVE_OTCLIENT_DEFAULT_OUTFIT_LAST_LOOK_TYPE: u8 = 134;
/// Server opcode `0x84`: floating colored text over one tile, parsed without any message-mode
/// translation, so it stays client-visible even on mode-map-less classic profiles.
pub const NATIVE_OTCLIENT_GAME_ANIMATED_TEXT: u8 = 0x84;
/// Server opcode `0x83`: one tile graphical effect.
pub const NATIVE_OTCLIENT_GAME_MAGIC_EFFECT: u8 = 0x83;
/// Server opcode `0x85`: one distance-effect missile from tile to tile (plan v49 slice 9).
pub const NATIVE_OTCLIENT_GAME_DISTANCE_EFFECT: u8 = 0x85;
/// Server opcode `0x90`: creature skull state (classic values 0..=6).
pub const NATIVE_OTCLIENT_GAME_CREATURE_SKULL: u8 = 0x90;
/// Server opcode `0x92`: creature unpass (tile-blocking) flag.
pub const NATIVE_OTCLIENT_GAME_CREATURE_UNPASS: u8 = 0x92;
/// Classic white skull value (first unjustified player kill).
pub const NATIVE_OTCLIENT_SKULL_WHITE: u8 = 3;
pub const NATIVE_OTCLIENT_GAME_CHOOSE_OUTFIT: u8 = 0xc8;
pub const NATIVE_OTCLIENT_GAME_VIP_ADD: u8 = 0xd2;
pub const NATIVE_OTCLIENT_GAME_VIP_STATE: u8 = 0xd3;
pub const NATIVE_OTCLIENT_GAME_VIP_LOGOUT: u8 = 0xd4;
pub const NATIVE_OTCLIENT_GAME_CANCEL_WALK: u8 = 0xb5;
pub const NATIVE_OTCLIENT_ENTER_GAME: u8 = 0x0f;
pub const NATIVE_OTCLIENT_LEAVE_GAME: u8 = 0x14;
pub const NATIVE_OTCLIENT_CLIENT_PING: u8 = 0x1d;
pub const NATIVE_OTCLIENT_CLIENT_PING_BACK: u8 = 0x1e;
pub const NATIVE_OTCLIENT_CLIENT_AUTO_WALK: u8 = 0x64;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH: u8 = 0x65;
pub const NATIVE_OTCLIENT_CLIENT_WALK_EAST: u8 = 0x66;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH: u8 = 0x67;
pub const NATIVE_OTCLIENT_CLIENT_WALK_WEST: u8 = 0x68;
pub const NATIVE_OTCLIENT_CLIENT_STOP: u8 = 0x69;
pub const NATIVE_OTCLIENT_CLIENT_THROW_ITEM: u8 = 0x78;
/// Classic request-player-trade opcode: position + thing id + stack pos + target creature id.
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_TRADE: u8 = 0x7d;
/// Classic accept-trade opcode: zero payload.
pub const NATIVE_OTCLIENT_CLIENT_ACCEPT_TRADE: u8 = 0x7f;
/// Classic reject/cancel-trade opcode: zero payload.
pub const NATIVE_OTCLIENT_CLIENT_REJECT_TRADE: u8 = 0x80;
/// Classic buy-from-NPC opcode: item id + subtype + amount + ignore-capacity + backpack flags.
pub const NATIVE_OTCLIENT_CLIENT_BUY_ITEM: u8 = 0x7a;
/// Classic sell-to-NPC opcode: item id + subtype + amount + ignore-equipped flag.
pub const NATIVE_OTCLIENT_CLIENT_SELL_ITEM: u8 = 0x7b;
/// Classic close-NPC-trade opcode: zero payload.
pub const NATIVE_OTCLIENT_CLIENT_CLOSE_NPC_TRADE: u8 = 0x7c;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST: u8 = 0x6a;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST: u8 = 0x6b;
pub const NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST: u8 = 0x6c;
pub const NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST: u8 = 0x6d;
pub const NATIVE_OTCLIENT_CLIENT_TURN_NORTH: u8 = 0x6f;
pub const NATIVE_OTCLIENT_CLIENT_TURN_EAST: u8 = 0x70;
pub const NATIVE_OTCLIENT_CLIENT_TURN_SOUTH: u8 = 0x71;
pub const NATIVE_OTCLIENT_CLIENT_TURN_WEST: u8 = 0x72;
pub const NATIVE_OTCLIENT_CLIENT_CHANGE_FIGHT_MODES: u8 = 0xa0;
pub const NATIVE_OTCLIENT_CLIENT_SELECT_TARGET: u8 = 0xa1;
pub const NATIVE_OTCLIENT_CLIENT_SELECT_FOLLOW: u8 = 0xa2;
pub const NATIVE_OTCLIENT_CLIENT_INVITE_TO_PARTY: u8 = 0xa3;
pub const NATIVE_OTCLIENT_CLIENT_JOIN_PARTY: u8 = 0xa4;
pub const NATIVE_OTCLIENT_CLIENT_REVOKE_PARTY_INVITATION: u8 = 0xa5;
pub const NATIVE_OTCLIENT_CLIENT_PASS_PARTY_LEADERSHIP: u8 = 0xa6;
pub const NATIVE_OTCLIENT_CLIENT_LEAVE_PARTY: u8 = 0xa7;
pub const NATIVE_OTCLIENT_CLIENT_SHARE_PARTY_EXPERIENCE: u8 = 0xa8;
pub const NATIVE_OTCLIENT_CLIENT_CANCEL_ATTACK_AND_FOLLOW: u8 = 0xbe;
pub const NATIVE_OTCLIENT_CLIENT_TALK: u8 = 0x96;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_CHANNELS: u8 = 0x97;
pub const NATIVE_OTCLIENT_CLIENT_JOIN_CHANNEL: u8 = 0x98;
pub const NATIVE_OTCLIENT_CLIENT_LEAVE_CHANNEL: u8 = 0x99;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM: u8 = 0x82;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM_EX: u8 = 0x83;
pub const NATIVE_OTCLIENT_CLIENT_USE_ITEM_ON_CREATURE: u8 = 0x84;
pub const NATIVE_OTCLIENT_CLIENT_ROTATE_ITEM: u8 = 0x85;
pub const NATIVE_OTCLIENT_CLIENT_CLOSE_CONTAINER: u8 = 0x87;
pub const NATIVE_OTCLIENT_CLIENT_UP_ARROW_CONTAINER: u8 = 0x88;
pub const NATIVE_OTCLIENT_CLIENT_UPDATE_CONTAINER: u8 = 0xca;
pub const NATIVE_OTCLIENT_CLIENT_LOOK_MAP: u8 = 0x8c;
pub const NATIVE_OTCLIENT_CLIENT_LOOK_CREATURE: u8 = 0x8d;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_OUTFIT: u8 = 0xd2;
pub const NATIVE_OTCLIENT_CLIENT_CHANGE_OUTFIT: u8 = 0xd3;
pub const NATIVE_OTCLIENT_CLIENT_ADD_VIP: u8 = 0xdc;
pub const NATIVE_OTCLIENT_CLIENT_REMOVE_VIP: u8 = 0xdd;
pub const NATIVE_OTCLIENT_CLIENT_EDIT_VIP: u8 = 0xde;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LOG: u8 = 0xf0;
pub const NATIVE_OTCLIENT_GAME_QUEST_LOG: u8 = 0xf0;
pub const NATIVE_OTCLIENT_CLIENT_REQUEST_QUEST_LINE: u8 = 0xf1;
pub const NATIVE_OTCLIENT_GAME_QUEST_LINE: u8 = 0xf1;
pub const NATIVE_OTCLIENT_GAME_CHANNELS: u8 = 0xab;
pub const NATIVE_OTCLIENT_GAME_OPEN_CHANNEL: u8 = 0xac;
pub const NATIVE_OTCLIENT_MAX_IGNORED_INTERACTION_BYTES: usize = 512;
pub const NATIVE_OTCLIENT_UNKNOWN_CREATURE: u16 = 0x0061;
pub const NATIVE_OTCLIENT_MAPPED_CREATURE: u16 = 0xffff;
pub const NATIVE_OTCLIENT_TILE_END: u16 = 0xff00;
pub const NATIVE_OTCLIENT_CLASSIC_MAP_WIDTH: usize = 18;
pub const NATIVE_OTCLIENT_CLASSIC_MAP_HEIGHT: usize = 14;
pub const NATIVE_OTCLIENT_CLASSIC_SURFACE_FLOORS: usize = 8;
pub const NATIVE_OTCLIENT_MAX_EXTRA_TILE_ITEMS: usize = 8;
pub const NATIVE_OTCLIENT_MAX_STATIC_ENTITIES_PER_VIEWPORT: usize = 32;
pub const NATIVE_OTCLIENT_MAX_SHARED_PLAYERS_PER_VIEWPORT: usize = 32;
pub const NATIVE_OTCLIENT_PLAYER_ID_START: u32 = 0x1000_0000;
pub const NATIVE_OTCLIENT_PLAYER_ID_END: u32 = 0x4000_0000;
pub const NATIVE_OTCLIENT_MAX_CHAT_TEXT_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientProfile {
    pub protocol_version: u16,
    pub numeric_account_ids: bool,
    pub login_packet_encryption: bool,
    pub protocol_checksum: bool,
    pub challenge_on_login: bool,
    pub max_padding_bytes: usize,
}

/// The native transport boundary selected by a protocol profile. This is intentionally narrower
/// than a claim of full protocol compatibility: FE currently runs plain classic 7.4 and 7.6
/// foundations (byte-identical login, framing, and record layouts) and recognizes the distinct
/// encrypted transport required by classic 8.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientFoundation {
    PlainClassic740,
    PlainClassic760,
    Classic800RequiresRsaXtea,
    Unsupported,
}

impl NativeOtClientFoundation {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainClassic740 => "plain-classic-740",
            Self::PlainClassic760 => "plain-classic-760",
            Self::Classic800RequiresRsaXtea => "classic-800-requires-rsa-xtea",
            Self::Unsupported => "unsupported",
        }
    }
}

impl NativeOtClientProfile {
    pub fn foundation(&self) -> NativeOtClientFoundation {
        if self.protocol_version == 0
            || !self.numeric_account_ids
            || self.max_padding_bytes > MAX_FRAME_SIZE
        {
            return NativeOtClientFoundation::Unsupported;
        }
        match self.protocol_version {
            740 | 760
                if !self.login_packet_encryption
                    && !self.protocol_checksum
                    && !self.challenge_on_login =>
            {
                if self.protocol_version == 740 {
                    NativeOtClientFoundation::PlainClassic740
                } else {
                    NativeOtClientFoundation::PlainClassic760
                }
            }
            800 => NativeOtClientFoundation::Classic800RequiresRsaXtea,
            _ => NativeOtClientFoundation::Unsupported,
        }
    }

    pub fn supports_current_native_foundation(&self) -> bool {
        matches!(
            self.foundation(),
            NativeOtClientFoundation::PlainClassic740 | NativeOtClientFoundation::PlainClassic760
        )
    }

    /// Classic equipment/container/outfit records were verified on the 740 native profile and are
    /// byte-identical on the 760 profile: no OTCv8 feature flag differs between the two versions,
    /// so every record layout FE emits is unchanged. Only the client-side message-mode translation
    /// differs (the reason the 760 profile exists). Other configured protocol versions must add
    /// their own parser-backed layout before reuse.
    pub fn supports_classic_740_inventory_records(&self) -> bool {
        self.supports_current_native_foundation()
            && (self.protocol_version == 740 || self.protocol_version == 760)
    }

    /// True when this profile's client actually translates message-mode bytes for talk and
    /// text-message records. An unmodified OTCv8 selecting protocol 740 keeps an empty mode map
    /// and discards every `0xAA`/`0xB4` record, so visible-text features must gate on this.
    pub fn supports_visible_text_messages(&self) -> bool {
        self.foundation() == NativeOtClientFoundation::PlainClassic760
    }
}

/// One classic wire item record. `subtype` is present only when the validated operator-supplied
/// item catalog identifies the client thing as stackable, chargeable, fluid, or splash. FE does
/// not infer that field from a server item ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientClassicItemRecord {
    pub client_thing_id: u16,
    pub subtype: Option<u8>,
}

/// Parser-verified classic 740 `OpenContainer` (`0x6e`) payload. Pagination and modern quick-loot
/// fields are deliberately absent because they are feature-gated outside the selected profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientClassicOpenContainer {
    pub container_id: u8,
    pub container_item: NativeOtClientClassicItemRecord,
    pub name: String,
    pub capacity: u8,
    pub has_parent: bool,
    pub items: Vec<NativeOtClientClassicItemRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientLoginRequest {
    pub operating_system: u16,
    pub protocol_version: u16,
    pub dat_signature: u32,
    pub spr_signature: u32,
    pub pic_signature: u32,
    pub account_id: u32,
    pub password: String,
    pub client_tag: String,
    pub client_build: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientGameRequest {
    pub operating_system: u16,
    pub protocol_version: u16,
    pub account_id: u32,
    pub character_name: String,
    pub password: String,
    pub client_tag: String,
    pub client_build: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientPosition {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientEmptyWorldSnapshot {
    pub player_id: u32,
    pub player_name: String,
    pub player_position: NativeOtClientPosition,
    pub player_level: u16,
    pub player_experience: u64,
    pub player_vitals: NativeOtClientPlayerVitals,
    pub player_skills: PlayerSkills,
    pub ground_thing_id: u16,
    pub player_look_type: u8,
    pub player_direction: u8,
    pub player_speed: u16,
    pub server_beat: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientPlayerVitals {
    pub health: u16,
    pub max_health: u16,
    pub mana: u16,
    pub max_mana: u16,
    pub capacity: u16,
    pub magic_level: u8,
}

impl Default for NativeOtClientPlayerVitals {
    fn default() -> Self {
        Self {
            health: 150,
            max_health: 150,
            mana: 50,
            max_mana: 50,
            capacity: 40_000,
            magic_level: 0,
        }
    }
}

/// A bounded classic 7.4 creature appearance. The selected 740 OTCv8 feature profile uses an
/// 8-bit look type followed by four color bytes and has no addon or mount fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientClassicOutfit {
    pub look_type: u8,
    pub head: u8,
    pub body: u8,
    pub legs: u8,
    pub feet: u8,
}

impl NativeOtClientClassicOutfit {
    pub fn from_snapshot(snapshot: &NativeOtClientEmptyWorldSnapshot) -> Self {
        Self {
            look_type: snapshot.player_look_type,
            head: 0,
            body: 0,
            legs: 0,
            feet: 0,
        }
    }
}

/// The audited classic party shield values FE can represent without shared-experience state.
/// Invitation, leader, and member values are intentionally limited to the TFS-compatible basic
/// display states; shared-experience, blink, and unrelated social shield values remain deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientClassicPartyShield {
    None,
    InvitationFromLeader,
    InvitationToLeader,
    Member,
    Leader,
}

impl NativeOtClientClassicPartyShield {
    const fn classic_value(self) -> u8 {
        match self {
            Self::None => 0,
            Self::InvitationFromLeader => 1,
            Self::InvitationToLeader => 2,
            Self::Member => 3,
            Self::Leader => 4,
        }
    }
}

/// Immutable rendering data for an active player other than the local snapshot owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientVisiblePlayer {
    pub player_id: u32,
    pub name: String,
    pub position: NativeOtClientPosition,
    pub health_percent: u8,
    pub outfit: NativeOtClientClassicOutfit,
    pub direction: u8,
    pub speed: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientCardinalDirection {
    North,
    East,
    South,
    West,
}

impl NativeOtClientCardinalDirection {
    fn from_client_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH => Some(Self::North),
            NATIVE_OTCLIENT_CLIENT_WALK_EAST => Some(Self::East),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH => Some(Self::South),
            NATIVE_OTCLIENT_CLIENT_WALK_WEST => Some(Self::West),
            _ => None,
        }
    }

    fn from_turn_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_TURN_NORTH => Some(Self::North),
            NATIVE_OTCLIENT_CLIENT_TURN_EAST => Some(Self::East),
            NATIVE_OTCLIENT_CLIENT_TURN_SOUTH => Some(Self::South),
            NATIVE_OTCLIENT_CLIENT_TURN_WEST => Some(Self::West),
            _ => None,
        }
    }

    pub fn protocol_direction(self) -> u8 {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }

    /// Restores a direction from its classic protocol byte (0 north, 1 east, 2 south, 3 west).
    /// Returns None for unknown bytes so callers can fall back to their default facing.
    pub fn from_protocol_direction(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::North),
            1 => Some(Self::East),
            2 => Some(Self::South),
            3 => Some(Self::West),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientAutoWalkDirection {
    East,
    NorthEast,
    North,
    NorthWest,
    West,
    SouthWest,
    South,
    SouthEast,
}

impl NativeOtClientAutoWalkDirection {
    fn from_direct_diagonal_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST => Some(Self::NorthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST => Some(Self::SouthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST => Some(Self::SouthWest),
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST => Some(Self::NorthWest),
            _ => None,
        }
    }

    fn from_native_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::East),
            2 => Some(Self::NorthEast),
            3 => Some(Self::North),
            4 => Some(Self::NorthWest),
            5 => Some(Self::West),
            6 => Some(Self::SouthWest),
            7 => Some(Self::South),
            8 => Some(Self::SouthEast),
            _ => None,
        }
    }

    pub fn cardinal_steps(self) -> &'static [NativeOtClientCardinalDirection] {
        match self {
            Self::East => &[NativeOtClientCardinalDirection::East],
            Self::NorthEast => &[
                NativeOtClientCardinalDirection::North,
                NativeOtClientCardinalDirection::East,
            ],
            Self::North => &[NativeOtClientCardinalDirection::North],
            Self::NorthWest => &[
                NativeOtClientCardinalDirection::North,
                NativeOtClientCardinalDirection::West,
            ],
            Self::West => &[NativeOtClientCardinalDirection::West],
            Self::SouthWest => &[
                NativeOtClientCardinalDirection::South,
                NativeOtClientCardinalDirection::West,
            ],
            Self::South => &[NativeOtClientCardinalDirection::South],
            Self::SouthEast => &[
                NativeOtClientCardinalDirection::South,
                NativeOtClientCardinalDirection::East,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeOtClientGameAction {
    EnterGame,
    LeaveGame,
    Ping,
    PingBack,
    Stop,
    AutoWalk(Vec<NativeOtClientAutoWalkDirection>),
    Talk(NativeOtClientTalkRequest),
    ThrowItem {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_position: NativeOtClientPosition,
        count: u8,
    },
    UseItem {
        position: NativeOtClientPosition,
        client_thing_id: u16,
        stack_position: u8,
        index: u8,
    },
    UseItemEx {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_position: NativeOtClientPosition,
        target_client_thing_id: u16,
        target_stack_position: u8,
    },
    UseItemOnCreature {
        source_position: NativeOtClientPosition,
        source_client_thing_id: u16,
        source_stack_position: u8,
        target_creature_id: u32,
    },
    RotateItem {
        position: NativeOtClientPosition,
        client_thing_id: u16,
        stack_position: u8,
    },
    LookMap {
        position: NativeOtClientPosition,
        thing_id: u16,
        stack_position: u8,
    },
    LookCreature {
        creature_id: u32,
    },
    /// Classic request-player-trade: the sender offers the item at the given position to the
    /// target creature. FE resolves the item through its own authoritative inventory instead
    /// of trusting the client's thing id.
    RequestTrade {
        position: NativeOtClientPosition,
        client_thing_id: u16,
        stack_position: u8,
        target_creature_id: u32,
    },
    AcceptTrade,
    RejectTrade,
    /// Classic buy-from-NPC: item id + subtype + amount + ignore-capacity + backpack flags.
    NpcBuy {
        client_thing_id: u16,
        subtype: u8,
        amount: u8,
        _ignore_capacity: bool,
        _buy_with_backpack: bool,
    },
    /// Classic sell-to-NPC: item id + subtype + amount + ignore-equipped flag.
    NpcSell {
        client_thing_id: u16,
        subtype: u8,
        amount: u8,
        _ignore_equipped: bool,
    },
    NpcTradeClose,
    RequestOutfit,
    RequestQuestLog,
    RequestQuestLine {
        quest_id: u16,
    },
    RequestChannels,
    JoinChannel(u16),
    LeaveChannel(u16),
    AddVip(String),
    RemoveVip(u32),
    EditVip {
        target_player_id: u32,
        description: String,
        icon: u32,
        notify: bool,
    },
    ChangeOutfit(NativeOtClientClassicOutfit),
    CloseContainer(u8),
    UpArrowContainer(u8),
    UpdateContainer(u8),
    SelectTarget(u32),
    SelectFollow(u32),
    PartyInvite(u32),
    PartyJoin(u32),
    PartyRevokeInvitation(u32),
    PartyPassLeadership(u32),
    PartyLeave,
    PartySharedExperience(bool),
    CancelAttackAndFollow,
    IgnoredInteraction(u8),
    Turn(NativeOtClientCardinalDirection),
    ChangeFightModes(NativeOtClientFightModeRequest),
    CardinalMove(NativeOtClientCardinalDirection),
    DiagonalMove(NativeOtClientAutoWalkDirection),
}

/// One classic channel-list entry prepared by an authoritative caller. The protocol layer only
/// serializes a bounded list; it does not model joining, membership, messages, or permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientClassicChannel {
    pub id: u16,
    pub name: String,
}

/// Parsed bounded client talk input. Classic mode `5` retains its recipient string for a separate
/// host-owned private-message route, while configured channels retain their classic channel ID.
/// Authorization and delivery are deliberately outside this protocol boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOtClientTalkRequest {
    pub mode: u8,
    pub channel_id: Option<u16>,
    pub recipient: Option<String>,
    pub message: String,
}

/// Parsed classic fight-mode intent. This is an inbound state request only; its use in combat and
/// client output must be established separately by the host and profile-specific evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOtClientFightModeRequest {
    pub mode: NativeOtClientFightMode,
    pub chase: bool,
    pub secure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOtClientFightMode {
    Attack,
    Balanced,
    Defense,
}

impl NativeOtClientFightMode {
    const fn from_classic_value(value: u8) -> Self {
        match value {
            1 => Self::Attack,
            2 => Self::Balanced,
            _ => Self::Defense,
        }
    }
}

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
    let request = NativeOtClientLoginRequest {
        operating_system: reader.u16()?,
        protocol_version: reader.u16()?,
        dat_signature: reader.u32()?,
        spr_signature: reader.u32()?,
        pic_signature: reader.u32()?,
        account_id: reader.u32()?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_tag: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_build: reader.u16()?,
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
    let request = NativeOtClientGameRequest {
        operating_system,
        protocol_version,
        account_id: reader.u32()?,
        character_name: reader.string(MAX_LOGIN_STRING_BYTES)?,
        password: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_tag: reader.string(MAX_LOGIN_STRING_BYTES)?,
        client_build: reader.u16()?,
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
