//! Native OTClient 7.4 type surface: opcode constants, profile, request records,
//! action/enum types, and their small helper impls used by the codec layer.

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
    pub(crate) const fn classic_value(self) -> u8 {
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
    pub(crate) fn from_client_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH => Some(Self::North),
            NATIVE_OTCLIENT_CLIENT_WALK_EAST => Some(Self::East),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH => Some(Self::South),
            NATIVE_OTCLIENT_CLIENT_WALK_WEST => Some(Self::West),
            _ => None,
        }
    }

    pub(crate) fn from_turn_opcode(opcode: u8) -> Option<Self> {
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
    pub(crate) fn from_direct_diagonal_opcode(opcode: u8) -> Option<Self> {
        match opcode {
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_EAST => Some(Self::NorthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_EAST => Some(Self::SouthEast),
            NATIVE_OTCLIENT_CLIENT_WALK_SOUTH_WEST => Some(Self::SouthWest),
            NATIVE_OTCLIENT_CLIENT_WALK_NORTH_WEST => Some(Self::NorthWest),
            _ => None,
        }
    }

    pub(crate) fn from_native_byte(byte: u8) -> Option<Self> {
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
    pub(crate) const fn from_classic_value(value: u8) -> Self {
        match value {
            1 => Self::Attack,
            2 => Self::Balanced,
            _ => Self::Defense,
        }
    }
}
