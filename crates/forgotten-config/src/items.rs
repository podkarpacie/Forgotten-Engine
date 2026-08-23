use crate::ConfigError;
use forgotten_core::{NativeItemPresentation, NativeItemPresentationCatalog, PlayerSkill};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::{BTreeMap, BTreeSet};

const OTB_IDENTIFIER: &[u8; 4] = b"OTBI";
const NODE_START: u8 = 0xfe;
const NODE_END: u8 = 0xff;
const NODE_ESCAPE: u8 = 0xfd;
const ROOT_ATTR_VERSION: u8 = 1;
const ITEM_ATTR_SERVER_ID: u8 = 0x10;
const ITEM_ATTR_CLIENT_ID: u8 = 0x11;
const FLAG_BLOCK_SOLID: u32 = 1 << 0;
const FLAG_BLOCK_PATHFIND: u32 = 1 << 2;
const FLAG_STACKABLE: u32 = 1 << 7;
const MAX_ITEM_NAME_BYTES: usize = 128;
const FLAG_CLIENT_CHARGES: u32 = 1 << 22;
const OTB_ITEM_GROUP_SPLASH: u8 = 10;
const OTB_ITEM_GROUP_FLUID: u8 = 11;
const MAX_OTB_BYTES: usize = 64 * 1024 * 1024;
const MAX_OTB_NODES: usize = 300_000;
const MAX_OTB_NODE_DEPTH: usize = 64;
const MAX_ITEM_RANGE: usize = 16_384;
const MAX_ITEM_COMBAT_VALUE: u16 = 10_000;
const MAX_ITEM_ATTACK_SPEED_MILLIS: u32 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyItemCatalog {
    pub otb_major_version: u32,
    pub client_version: u32,
    pub build_number: u32,
    definitions: BTreeMap<u16, LegacyItemDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyItemDefinition {
    pub server_id: u16,
    pub client_id: u16,
    pub group: u8,
    pub flags: u32,
    pub xml_blocks_solid: Option<bool>,
    pub xml_blocks_pathfind: Option<bool>,
    /// Operator-supplied legacy XML metadata retained for a future profile-specific combat
    /// adapter. It never changes current FE combat behavior by itself.
    pub xml_armor: Option<u16>,
    /// Operator-supplied legacy XML weight retained in its original unsigned integer form for a
    /// future capacity adapter. It never enforces capacity by itself.
    pub xml_weight: Option<u32>,
    /// Operator-supplied bounded legacy XML item name retained for exact inspected-item text. It
    /// does not generate articles, descriptions, or other item behavior by itself.
    pub xml_name: Option<String>,
    /// Operator-supplied legacy equipment labels retained for future placement validation. They
    /// do not currently allow, reject, swap, or otherwise change equipped item placement.
    pub xml_slot_types: std::collections::BTreeSet<LegacyItemSlotType>,
    pub xml_defense: Option<u16>,
    pub xml_extra_defense: Option<u16>,
    pub xml_attack_speed_millis: Option<u32>,
    pub xml_weapon_type: Option<LegacyWeaponType>,
}

/// The verified legacy XML weapon-type labels needed by a future item-combat adapter. This is
/// metadata only: parsing one of these values does not activate weapon formulas or client use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyWeaponType {
    Sword,
    Club,
    Axe,
    Shield,
    Distance,
    Wand,
    Ammunition,
    Quiver,
}

/// The bounded legacy `items.xml` slotType vocabulary. These values describe source metadata
/// only; FE does not yet derive TFS slot masks or equipment-placement behavior from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LegacyItemSlotType {
    Head,
    Body,
    Legs,
    Feet,
    Backpack,
    TwoHanded,
    RightHand,
    LeftHand,
    Necklace,
    Ring,
    Ammo,
    Hand,
}

impl LegacyItemSlotType {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "head" => Ok(Self::Head),
            "body" => Ok(Self::Body),
            "legs" => Ok(Self::Legs),
            "feet" => Ok(Self::Feet),
            "backpack" => Ok(Self::Backpack),
            "two-handed" => Ok(Self::TwoHanded),
            "right-hand" => Ok(Self::RightHand),
            "left-hand" => Ok(Self::LeftHand),
            "necklace" => Ok(Self::Necklace),
            "ring" => Ok(Self::Ring),
            "ammo" => Ok(Self::Ammo),
            "hand" => Ok(Self::Hand),
            _ => Err(invalid(
                "items.xml slotType must be head, body, legs, feet, backpack, two-handed, right-hand, left-hand, necklace, ring, ammo, or hand",
            )),
        }
    }
}

impl LegacyWeaponType {
    /// Returns a typed skill only for legacy weapon classes that FE currently routes through the
    /// bounded adjacent declarative melee action. Shielding, distance, wand, ammunition, and
    /// quiver behavior need distinct verified combat paths and remain outside this projection.
    pub const fn adjacent_melee_skill(self) -> Option<PlayerSkill> {
        match self {
            Self::Sword => Some(PlayerSkill::Sword),
            Self::Club => Some(PlayerSkill::Club),
            Self::Axe => Some(PlayerSkill::Axe),
            Self::Shield | Self::Distance | Self::Wand | Self::Ammunition | Self::Quiver => None,
        }
    }

    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "sword" => Ok(Self::Sword),
            "club" => Ok(Self::Club),
            "axe" => Ok(Self::Axe),
            "shield" => Ok(Self::Shield),
            "distance" => Ok(Self::Distance),
            "wand" => Ok(Self::Wand),
            "ammunition" => Ok(Self::Ammunition),
            "quiver" => Ok(Self::Quiver),
            _ => Err(invalid(
                "items.xml weaponType must be sword, club, axe, shield, distance, wand, ammunition, or quiver",
            )),
        }
    }
}

impl LegacyItemDefinition {
    pub fn blocks_movement(&self) -> bool {
        self.xml_blocks_solid
            .unwrap_or((self.flags & FLAG_BLOCK_SOLID) != 0)
            || self
                .xml_blocks_pathfind
                .unwrap_or((self.flags & FLAG_BLOCK_PATHFIND) != 0)
    }

    /// Whether the classic 740 client parser expects one subtype/count byte after the client
    /// thing ID. This is derived only from validated OTB metadata; caller-provided item IDs are
    /// never treated as an implicit subtype contract.
    pub fn requires_classic_740_subtype(&self) -> bool {
        self.flags & (FLAG_STACKABLE | FLAG_CLIENT_CHARGES) != 0
            || matches!(self.group, OTB_ITEM_GROUP_SPLASH | OTB_ITEM_GROUP_FLUID)
    }

    /// Whether this legacy OTB entry represents an item whose item count contributes to its
    /// source weight. This does not change FE item-stack transfer behavior.
    pub fn is_stackable(&self) -> bool {
        self.flags & FLAG_STACKABLE != 0
    }
}

impl LegacyItemCatalog {
    pub fn definition(&self, server_id: u16) -> Option<&LegacyItemDefinition> {
        self.definitions.get(&server_id)
    }

    pub fn client_thing_id(&self, server_id: u16) -> Option<u16> {
        self.definition(server_id)
            .map(|definition| definition.client_id)
    }

    pub fn requires_classic_740_subtype(&self, server_id: u16) -> Option<bool> {
        self.definition(server_id)
            .map(LegacyItemDefinition::requires_classic_740_subtype)
    }

    pub fn native_item_presentation_catalog(
        &self,
    ) -> Result<NativeItemPresentationCatalog, ConfigError> {
        let mut catalog = NativeItemPresentationCatalog::default();
        for definition in self.definitions.values() {
            catalog
                .insert(
                    definition.server_id,
                    NativeItemPresentation {
                        client_thing_id: definition.client_id,
                        requires_classic_740_subtype: definition.requires_classic_740_subtype(),
                    },
                )
                .map_err(|error| {
                    invalid(format!(
                        "items.otb has an invalid native item presentation for {}: {error}",
                        definition.server_id
                    ))
                })?;
        }
        Ok(catalog)
    }

    /// Returns only bounded legacy XML armor values keyed by their authoritative server IDs.
    /// The result is immutable input for FE's explicit armor-only mitigation bridge; it does not
    /// interpret shield defense, weapon defense, vocation multipliers, random blocking, or any
    /// profile-specific TFS formula.
    pub fn native_xml_armor_by_server_id(&self) -> BTreeMap<u16, u16> {
        self.definitions
            .iter()
            .filter_map(|(&server_id, definition)| {
                definition
                    .xml_armor
                    .filter(|armor| *armor > 0)
                    .map(|armor| (server_id, armor))
            })
            .collect()
    }

    /// Returns bounded legacy XML defense values keyed by their authoritative server IDs. The
    /// result is immutable input for FE's bounded left-hand (shield-hand) mitigation extension;
    /// weapon-hand defense, blocking chance, and TFS formula semantics stay outside this map.
    pub fn native_xml_defense_by_server_id(&self) -> BTreeMap<u16, u16> {
        self.definitions
            .iter()
            .filter_map(|(&server_id, definition)| {
                definition
                    .xml_defense
                    .filter(|defense| *defense > 0)
                    .map(|defense| (server_id, defense))
            })
            .collect()
    }

    /// Returns only sword, club, and axe legacy weapon classifications that FE's existing
    /// adjacent declarative melee route can map to a typed skill-try award. This does not enable
    /// ranged, shielding, wand, ammunition, quiver, or generic TFS weapon behavior.
    pub fn adjacent_melee_skill_by_server_id(&self) -> BTreeMap<u16, PlayerSkill> {
        self.definitions
            .iter()
            .filter_map(|(&server_id, definition)| {
                definition
                    .xml_weapon_type
                    .and_then(LegacyWeaponType::adjacent_melee_skill)
                    .map(|skill| (server_id, skill))
            })
            .collect()
    }

    /// Returns source XML weight metadata keyed by authoritative server ID. The result does not
    /// calculate carried weight, recursive container weight, vocation capacity, or eligibility.
    pub fn xml_weight_by_server_id(&self) -> BTreeMap<u16, u32> {
        self.definitions
            .iter()
            .filter_map(|(&server_id, definition)| {
                definition.xml_weight.map(|weight| (server_id, weight))
            })
            .collect()
    }

    /// Returns bounded source item names keyed by authoritative server ID. The result is
    /// inspection metadata only and does not generate TFS name descriptions or articles.
    pub fn xml_name_by_server_id(&self) -> BTreeMap<u16, String> {
        self.definitions
            .iter()
            .filter_map(|(&server_id, definition)| {
                definition
                    .xml_name
                    .as_ref()
                    .map(|name| (server_id, name.clone()))
            })
            .collect()
    }

    /// Returns non-empty bounded legacy slotType sets keyed by authoritative server ID. This is
    /// source metadata only and does not provide a permission decision for any equipment slot.
    pub fn xml_slot_types_by_server_id(&self) -> BTreeMap<u16, BTreeSet<LegacyItemSlotType>> {
        self.definitions
            .iter()
            .filter(|(_, definition)| !definition.xml_slot_types.is_empty())
            .map(|(&server_id, definition)| (server_id, definition.xml_slot_types.clone()))
            .collect()
    }

    /// Returns authoritative server IDs for source items whose legacy OTB stack count contributes
    /// to item weight. It is immutable presentation input and has no transfer-policy effect.
    pub fn stackable_server_ids(&self) -> BTreeSet<u16> {
        self.definitions
            .iter()
            .filter(|(_, definition)| definition.is_stackable())
            .map(|(&server_id, _)| server_id)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[derive(Debug)]
struct Node {
    kind: u8,
    props: Vec<u8>,
    children: Vec<Node>,
}

pub(crate) fn parse_items_otb(bytes: &[u8]) -> Result<LegacyItemCatalog, ConfigError> {
    if bytes.len() > MAX_OTB_BYTES {
        return Err(invalid("items.otb exceeds the configured 64 MiB limit"));
    }
    let framed = bytes
        .strip_prefix(OTB_IDENTIFIER)
        .ok_or_else(|| invalid("items.otb is missing its required OTBI identifier"))?;
    let root = parse_tree(framed)?;
    let (otb_major_version, client_version, build_number) = parse_root_version(&root.props)?;
    let mut definitions = BTreeMap::new();
    for node in &root.children {
        if let Some(definition) = parse_item_node(node.kind, &node.props)? {
            if definitions
                .insert(definition.server_id, definition)
                .is_some()
            {
                return Err(invalid("items.otb contains duplicate server item IDs"));
            }
        }
    }
    if definitions.is_empty() {
        return Err(invalid("items.otb does not contain any item definitions"));
    }
    Ok(LegacyItemCatalog {
        otb_major_version,
        client_version,
        build_number,
        definitions,
    })
}

pub(crate) fn apply_items_xml(
    catalog: &mut LegacyItemCatalog,
    bytes: &[u8],
) -> Result<(), ConfigError> {
    if bytes.len() > MAX_OTB_BYTES {
        return Err(invalid("items.xml exceeds the configured 64 MiB limit"));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut active_ids = None::<Vec<u16>>;
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buffer).map_err(xml_error)? {
            Event::Start(event) => {
                depth += 1;
                if depth > MAX_OTB_NODE_DEPTH {
                    return Err(invalid("items.xml nesting exceeds the configured limit"));
                }
                if event.name().as_ref() == b"item" {
                    active_ids = Some(item_ids(&event)?);
                } else if event.name().as_ref() == b"attribute" {
                    apply_xml_attribute(catalog, active_ids.as_deref(), &event)?;
                }
            }
            Event::Empty(event) => {
                if event.name().as_ref() == b"item" {
                    let ids = item_ids(&event)?;
                    apply_inline_item_attributes(catalog, &ids, &event)?;
                } else if event.name().as_ref() == b"attribute" {
                    apply_xml_attribute(catalog, active_ids.as_deref(), &event)?;
                }
            }
            Event::End(event) => {
                if event.name().as_ref() == b"item" {
                    active_ids = None;
                }
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("malformed items.xml depth"))?;
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if depth != 0 {
        return Err(invalid("items.xml ended before all elements were closed"));
    }
    Ok(())
}

fn parse_tree(bytes: &[u8]) -> Result<Node, ConfigError> {
    let mut index = 0;
    let mut stack = Vec::<Node>::new();
    let mut root = None;
    let mut count = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            NODE_START => {
                index += 1;
                let kind = read_framed_byte(bytes, &mut index)?;
                count += 1;
                if count > MAX_OTB_NODES {
                    return Err(invalid("OTB node count exceeds the configured limit"));
                }
                if stack.len() >= MAX_OTB_NODE_DEPTH {
                    return Err(invalid("OTB node depth exceeds the configured limit"));
                }
                stack.push(Node {
                    kind,
                    props: Vec::new(),
                    children: Vec::new(),
                });
            }
            NODE_END => {
                index += 1;
                let node = stack
                    .pop()
                    .ok_or_else(|| invalid("OTB has an unmatched node end"))?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(invalid("OTB contains more than one root node"));
                }
            }
            NODE_ESCAPE => {
                index += 1;
                let value = bytes
                    .get(index)
                    .copied()
                    .ok_or_else(|| invalid("OTB escape byte has no escaped value"))?;
                index += 1;
                stack
                    .last_mut()
                    .ok_or_else(|| invalid("OTB property data appears outside a node"))?
                    .props
                    .push(value);
            }
            value => {
                index += 1;
                stack
                    .last_mut()
                    .ok_or_else(|| invalid("OTB property data appears outside a node"))?
                    .props
                    .push(value);
            }
        }
    }
    if !stack.is_empty() {
        return Err(invalid("OTB ended before every node was closed"));
    }
    root.ok_or_else(|| invalid("OTB does not contain a root node"))
}

fn read_framed_byte(bytes: &[u8], index: &mut usize) -> Result<u8, ConfigError> {
    let value = bytes
        .get(*index)
        .copied()
        .ok_or_else(|| invalid("OTB node start has no type"))?;
    *index += 1;
    if value == NODE_ESCAPE {
        let escaped = bytes
            .get(*index)
            .copied()
            .ok_or_else(|| invalid("OTB escaped node type has no value"))?;
        *index += 1;
        Ok(escaped)
    } else if matches!(value, NODE_START | NODE_END) {
        Err(invalid("OTB node type uses a reserved framing byte"))
    } else {
        Ok(value)
    }
}

fn parse_root_version(props: &[u8]) -> Result<(u32, u32, u32), ConfigError> {
    let mut cursor = Cursor::new(props);
    let _flags = cursor.read_u32()?;
    if cursor.read_u8()? != ROOT_ATTR_VERSION {
        return Err(invalid("items.otb root does not declare version data"));
    }
    let length = usize::from(cursor.read_u16()?);
    if length != 140 {
        return Err(invalid(
            "items.otb version record must be exactly 140 bytes",
        ));
    }
    let record = cursor.read_bytes(length)?;
    if !cursor.is_empty() {
        return Err(invalid("items.otb root contains unexpected trailing data"));
    }
    Ok((
        u32::from_le_bytes([record[0], record[1], record[2], record[3]]),
        u32::from_le_bytes([record[4], record[5], record[6], record[7]]),
        u32::from_le_bytes([record[8], record[9], record[10], record[11]]),
    ))
}

fn parse_item_node(group: u8, props: &[u8]) -> Result<Option<LegacyItemDefinition>, ConfigError> {
    let mut cursor = Cursor::new(props);
    let flags = cursor.read_u32()?;
    let mut server_id = None;
    let mut client_id = None;
    while !cursor.is_empty() {
        let attribute = cursor.read_u8()?;
        let length = usize::from(cursor.read_u16()?);
        let value = cursor.read_bytes(length)?;
        match attribute {
            ITEM_ATTR_SERVER_ID if value.len() == 2 => {
                server_id = Some(u16::from_le_bytes([value[0], value[1]]));
            }
            ITEM_ATTR_CLIENT_ID if value.len() == 2 => {
                client_id = Some(u16::from_le_bytes([value[0], value[1]]));
            }
            ITEM_ATTR_SERVER_ID | ITEM_ATTR_CLIENT_ID => {
                return Err(invalid("items.otb item ID attribute has an invalid length"));
            }
            _ => {}
        }
    }
    match (server_id, client_id) {
        (Some(server_id), Some(client_id)) if server_id != 0 && client_id != 0 => {
            Ok(Some(LegacyItemDefinition {
                server_id,
                client_id,
                group,
                flags,
                xml_blocks_solid: None,
                xml_blocks_pathfind: None,
                xml_armor: None,
                xml_weight: None,
                xml_name: None,
                xml_slot_types: std::collections::BTreeSet::new(),
                xml_defense: None,
                xml_extra_defense: None,
                xml_attack_speed_millis: None,
                xml_weapon_type: None,
            }))
        }
        (None, None) => Ok(None),
        _ => Err(invalid(
            "items.otb item record has an incomplete server/client ID mapping",
        )),
    }
}

fn apply_inline_item_attributes(
    catalog: &mut LegacyItemCatalog,
    ids: &[u16],
    event: &BytesStart<'_>,
) -> Result<(), ConfigError> {
    for (key, value) in [
        (b"blockSolid".as_slice(), b"blocksolid".as_slice()),
        (b"blockPathFind".as_slice(), b"blockpathfind".as_slice()),
    ] {
        if let Some(value) =
            optional_attribute_string(event, key)?.or(optional_attribute_string(event, value)?)
        {
            set_block_attribute(catalog, ids, key, parse_bool(&value, key)?)?;
        }
    }
    for key in [b"slotType".as_slice(), b"slottype".as_slice()] {
        if let Some(value) = optional_attribute_string(event, key)? {
            set_slot_type_attribute(catalog, ids, &value)?;
        }
    }
    if let Some(value) = optional_attribute_string(event, b"name")? {
        set_name_attribute(catalog, ids, &value)?;
    }
    for key in [
        b"armor".as_slice(),
        b"weight".as_slice(),
        b"defense".as_slice(),
        b"extraDef".as_slice(),
        b"extradef".as_slice(),
        b"attackSpeed".as_slice(),
        b"attackspeed".as_slice(),
        b"weaponType".as_slice(),
        b"weapontype".as_slice(),
    ] {
        if let Some(value) = optional_attribute_string(event, key)? {
            set_legacy_numeric_attribute(catalog, ids, key, &value)?;
        }
    }
    Ok(())
}

fn apply_xml_attribute(
    catalog: &mut LegacyItemCatalog,
    ids: Option<&[u16]>,
    event: &BytesStart<'_>,
) -> Result<(), ConfigError> {
    let Some(ids) = ids else {
        return Ok(());
    };
    let key = attribute_string(event, b"key")?;
    let value = attribute_string(event, b"value")?;
    match key.as_str() {
        "blockSolid" | "blocksolid" => set_block_attribute(
            catalog,
            ids,
            b"blockSolid",
            parse_bool(&value, b"blockSolid")?,
        ),
        "blockPathFind" | "blockpathfind" => set_block_attribute(
            catalog,
            ids,
            b"blockPathFind",
            parse_bool(&value, b"blockPathFind")?,
        ),
        "slotType" | "slottype" => set_slot_type_attribute(catalog, ids, &value),
        "name" => set_name_attribute(catalog, ids, &value),
        "armor" | "weight" | "defense" | "extraDef" | "extradef" | "attackSpeed"
        | "attackspeed" | "weaponType" | "weapontype" => {
            set_legacy_numeric_attribute(catalog, ids, key.as_bytes(), &value)
        }
        _ => Ok(()),
    }
}

fn set_block_attribute(
    catalog: &mut LegacyItemCatalog,
    ids: &[u16],
    key: &[u8],
    value: bool,
) -> Result<(), ConfigError> {
    for id in ids {
        if let Some(definition) = catalog.definitions.get_mut(id) {
            if key == b"blockSolid" {
                definition.xml_blocks_solid = Some(value);
            } else {
                definition.xml_blocks_pathfind = Some(value);
            }
        }
    }
    Ok(())
}

fn set_slot_type_attribute(
    catalog: &mut LegacyItemCatalog,
    ids: &[u16],
    value: &str,
) -> Result<(), ConfigError> {
    let slot_type = LegacyItemSlotType::parse(&value.to_ascii_lowercase())?;
    for id in ids {
        if let Some(definition) = catalog.definitions.get_mut(id) {
            definition.xml_slot_types.insert(slot_type);
        }
    }
    Ok(())
}

fn set_name_attribute(
    catalog: &mut LegacyItemCatalog,
    ids: &[u16],
    value: &str,
) -> Result<(), ConfigError> {
    let name = value.trim();
    if name.is_empty() {
        return Err(invalid("items.xml attribute `name` cannot be empty"));
    }
    if name.len() > MAX_ITEM_NAME_BYTES {
        return Err(invalid(
            "items.xml attribute `name` exceeds the supported metadata bound",
        ));
    }
    for id in ids {
        if let Some(definition) = catalog.definitions.get_mut(id) {
            definition.xml_name = Some(name.to_owned());
        }
    }
    Ok(())
}

fn set_legacy_numeric_attribute(
    catalog: &mut LegacyItemCatalog,
    ids: &[u16],
    key: &[u8],
    value: &str,
) -> Result<(), ConfigError> {
    let normalized_key = match key {
        b"extraDef" => b"extradef".as_slice(),
        b"attackSpeed" => b"attackspeed".as_slice(),
        b"weaponType" => b"weapontype".as_slice(),
        _ => key,
    };
    for id in ids {
        let Some(definition) = catalog.definitions.get_mut(id) else {
            continue;
        };
        match normalized_key {
            b"armor" => definition.xml_armor = Some(parse_item_combat_u16(value, b"armor")?),
            b"weight" => definition.xml_weight = Some(parse_item_weight(value)?),
            b"defense" => definition.xml_defense = Some(parse_item_combat_u16(value, b"defense")?),
            b"extradef" => {
                definition.xml_extra_defense = Some(parse_item_combat_u16(value, b"extradef")?)
            }
            b"attackspeed" => {
                definition.xml_attack_speed_millis = Some(parse_item_attack_speed(value)?)
            }
            b"weapontype" => definition.xml_weapon_type = Some(LegacyWeaponType::parse(value)?),
            _ => unreachable!("combat attribute dispatch accepts only normalized known keys"),
        }
    }
    Ok(())
}

fn parse_item_combat_u16(value: &str, key: &[u8]) -> Result<u16, ConfigError> {
    let value = value.parse::<u16>().map_err(|_| {
        invalid(format!(
            "items.xml attribute `{}` must be an unsigned integer",
            String::from_utf8_lossy(key)
        ))
    })?;
    if value > MAX_ITEM_COMBAT_VALUE {
        return Err(invalid(format!(
            "items.xml attribute `{}` exceeds the supported combat metadata bound",
            String::from_utf8_lossy(key)
        )));
    }
    Ok(value)
}

fn parse_item_weight(value: &str) -> Result<u32, ConfigError> {
    value
        .parse::<u32>()
        .map_err(|_| invalid("items.xml attribute `weight` must be an unsigned integer"))
}

fn parse_item_attack_speed(value: &str) -> Result<u32, ConfigError> {
    let value = value
        .parse::<u32>()
        .map_err(|_| invalid("items.xml attribute `attackspeed` must be an unsigned integer"))?;
    if value > MAX_ITEM_ATTACK_SPEED_MILLIS {
        return Err(invalid(
            "items.xml attribute `attackspeed` exceeds the supported metadata bound",
        ));
    }
    Ok(value)
}

fn item_ids(event: &BytesStart<'_>) -> Result<Vec<u16>, ConfigError> {
    if let Some(id) = optional_attribute_string(event, b"id")? {
        let values = id
            .split(';')
            .map(|part| {
                part.trim().parse::<u16>().map_err(|_| {
                    invalid("items.xml id must contain semicolon-separated u16 values")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() || values.len() > MAX_ITEM_RANGE {
            return Err(invalid("items.xml item ID list is empty or too large"));
        }
        return Ok(values);
    }
    let from = attribute_string(event, b"fromid")?
        .parse::<u16>()
        .map_err(|_| invalid("items.xml fromid must be a u16"))?;
    let to = attribute_string(event, b"toid")?
        .parse::<u16>()
        .map_err(|_| invalid("items.xml toid must be a u16"))?;
    if from > to || usize::from(to - from) + 1 > MAX_ITEM_RANGE {
        return Err(invalid("items.xml item ID range is invalid or too large"));
    }
    Ok((from..=to).collect())
}

fn parse_bool(value: &str, key: &[u8]) -> Result<bool, ConfigError> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(invalid(format!(
            "items.xml attribute `{}` must be true, false, 1, or 0",
            String::from_utf8_lossy(key)
        ))),
    }
}

fn attribute_string(event: &BytesStart<'_>, name: &[u8]) -> Result<String, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(xml_error)?
        .ok_or_else(|| {
            invalid(format!(
                "missing XML attribute `{}`",
                String::from_utf8_lossy(name)
            ))
        })?
        .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
        .map_err(xml_error)
        .map(|value| value.into_owned())
}

fn optional_attribute_string(
    event: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, ConfigError> {
    event
        .try_get_attribute(name)
        .map_err(xml_error)?
        .map(|attribute| {
            attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, event.decoder())
                .map_err(xml_error)
                .map(|value| value.into_owned())
        })
        .transpose()
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_u8(&mut self) -> Result<u8, ConfigError> {
        let value = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or_else(|| invalid("unexpected end of OTB data"))?;
        self.offset += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> Result<u16, ConfigError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, ConfigError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], ConfigError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| invalid("OTB property offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("unexpected end of OTB property"))?;
        self.offset = end;
        Ok(bytes)
    }
}

fn xml_error(error: impl std::fmt::Display) -> ConfigError {
    invalid(format!("items.xml parse error: {error}"))
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn framed_node(kind: u8, props: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![NODE_START, kind];
        for byte in props {
            if matches!(*byte, NODE_START | NODE_END | NODE_ESCAPE) {
                bytes.push(NODE_ESCAPE);
            }
            bytes.push(*byte);
        }
        for child in children {
            bytes.extend_from_slice(child);
        }
        bytes.push(NODE_END);
        bytes
    }

    #[test]
    fn parses_hand_authored_otb_mapping_and_xml_overrides() {
        let mut root = 0u32.to_le_bytes().to_vec();
        root.push(ROOT_ATTR_VERSION);
        root.extend_from_slice(&140u16.to_le_bytes());
        root.extend_from_slice(&3u32.to_le_bytes());
        root.extend_from_slice(&57u32.to_le_bytes());
        root.extend_from_slice(&1098u32.to_le_bytes());
        root.extend([0u8; 128]);
        let mut item = FLAG_BLOCK_SOLID.to_le_bytes().to_vec();
        item.push(ITEM_ATTR_SERVER_ID);
        item.extend_from_slice(&2u16.to_le_bytes());
        item.extend_from_slice(&4526u16.to_le_bytes());
        item.push(ITEM_ATTR_CLIENT_ID);
        item.extend_from_slice(&2u16.to_le_bytes());
        item.extend_from_slice(&102u16.to_le_bytes());
        let mut bytes = OTB_IDENTIFIER.to_vec();
        bytes.extend(framed_node(0, &root, &[framed_node(1, &item, &[])]));

        let mut catalog = parse_items_otb(&bytes).unwrap();
        assert_eq!(catalog.definition(4526).unwrap().client_id, 102);
        assert_eq!(catalog.definition(4526).unwrap().group, 1);
        assert_eq!(catalog.client_thing_id(4526), Some(102));
        assert_eq!(catalog.client_thing_id(9999), None);
        assert!(catalog.definition(4526).unwrap().blocks_movement());
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="4526"><attribute key="blockSolid" value="0"/></item></items>"#,
        )
        .unwrap();
        assert!(!catalog.definition(4526).unwrap().blocks_movement());
    }

    #[test]
    fn classifies_classic_subtype_requirements_from_otb_group_and_flags() {
        let ordinary = LegacyItemDefinition {
            server_id: 100,
            client_id: 200,
            group: 1,
            flags: 0,
            xml_blocks_solid: None,
            xml_blocks_pathfind: None,
            xml_armor: None,
            xml_weight: None,
            xml_name: None,
            xml_slot_types: BTreeSet::new(),
            xml_defense: None,
            xml_extra_defense: None,
            xml_attack_speed_millis: None,
            xml_weapon_type: None,
        };
        let fluid = LegacyItemDefinition {
            group: OTB_ITEM_GROUP_FLUID,
            ..ordinary.clone()
        };
        let charged = LegacyItemDefinition {
            flags: FLAG_CLIENT_CHARGES,
            ..ordinary.clone()
        };
        let catalog = LegacyItemCatalog {
            otb_major_version: 3,
            client_version: 57,
            build_number: 1,
            definitions: BTreeMap::from([(ordinary.server_id, ordinary.clone())]),
        };
        assert!(!ordinary.requires_classic_740_subtype());
        assert!(fluid.requires_classic_740_subtype());
        assert!(charged.requires_classic_740_subtype());
        assert_eq!(catalog.client_thing_id(100), Some(200));
        assert_eq!(catalog.requires_classic_740_subtype(100), Some(false));
        assert_eq!(
            catalog
                .native_item_presentation_catalog()
                .unwrap()
                .presentation(100),
            Some(NativeItemPresentation {
                client_thing_id: 200,
                requires_classic_740_subtype: false,
            })
        );
    }

    fn combat_metadata_catalog() -> LegacyItemCatalog {
        LegacyItemCatalog {
            otb_major_version: 3,
            client_version: 57,
            build_number: 1,
            definitions: BTreeMap::from([(
                100,
                LegacyItemDefinition {
                    server_id: 100,
                    client_id: 200,
                    group: 1,
                    flags: 0,
                    xml_blocks_solid: None,
                    xml_blocks_pathfind: None,
                    xml_armor: None,
                    xml_weight: None,
                    xml_name: None,
                    xml_slot_types: BTreeSet::new(),
                    xml_defense: None,
                    xml_extra_defense: None,
                    xml_attack_speed_millis: None,
                    xml_weapon_type: None,
                },
            )]),
        }
    }

    #[test]
    fn parses_bounded_legacy_item_combat_metadata_without_enabling_runtime_behavior() {
        let mut catalog = combat_metadata_catalog();
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100"><attribute key="armor" value="12"/><attribute key="weight" value="1800"/><attribute key="defense" value="24"/><attribute key="extradef" value="3"/><attribute key="attackspeed" value="1800"/><attribute key="weapontype" value="shield"/></item></items>"#,
        )
        .unwrap();
        let definition = catalog.definition(100).unwrap();
        assert_eq!(definition.xml_armor, Some(12));
        assert_eq!(definition.xml_weight, Some(1_800));
        assert_eq!(definition.xml_defense, Some(24));
        assert_eq!(definition.xml_extra_defense, Some(3));
        assert_eq!(definition.xml_attack_speed_millis, Some(1_800));
        assert_eq!(definition.xml_weapon_type, Some(LegacyWeaponType::Shield));

        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100" armor="13" weight="2100" defense="25" extraDef="4" attackSpeed="1600" weaponType="sword"/><item id="101" armor="99"/></items>"#,
        )
        .unwrap();
        let definition = catalog.definition(100).unwrap();
        assert_eq!(definition.xml_armor, Some(13));
        assert_eq!(definition.xml_weight, Some(2_100));
        assert_eq!(definition.xml_defense, Some(25));
        assert_eq!(definition.xml_extra_defense, Some(4));
        assert_eq!(definition.xml_attack_speed_millis, Some(1_600));
        assert_eq!(definition.xml_weapon_type, Some(LegacyWeaponType::Sword));
        assert_eq!(
            catalog.adjacent_melee_skill_by_server_id(),
            BTreeMap::from([(100, PlayerSkill::Sword)])
        );
        assert_eq!(catalog.definition(101), None);
    }

    #[test]
    fn exports_only_positive_validated_xml_armor_values_by_server_id() {
        let mut catalog = combat_metadata_catalog();
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100" armor="13"/></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.native_xml_armor_by_server_id(),
            BTreeMap::from([(100, 13)])
        );
    }

    #[test]
    fn exports_validated_xml_weight_metadata_by_server_id_without_capacity_behavior() {
        let mut catalog = combat_metadata_catalog();
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100" weight="2500"/></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.xml_weight_by_server_id(),
            BTreeMap::from([(100, 2_500)])
        );
    }

    #[test]
    fn retains_validated_legacy_item_names_without_description_behavior() {
        let mut catalog = combat_metadata_catalog();
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100"><attribute key="name" value="  Dragon Ham  "/></item></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.definition(100).unwrap().xml_name.as_deref(),
            Some("Dragon Ham")
        );

        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100" name="Magic Sword"/></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.xml_name_by_server_id(),
            BTreeMap::from([(100, "Magic Sword".to_string())])
        );
    }

    #[test]
    fn retains_validated_legacy_slot_type_metadata_without_equipment_behavior() {
        let mut catalog = combat_metadata_catalog();
        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100"><attribute key="slotType" value="head"/><attribute key="slottype" value="right-hand"/></item></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.definition(100).unwrap().xml_slot_types,
            BTreeSet::from([LegacyItemSlotType::Head, LegacyItemSlotType::RightHand])
        );

        apply_items_xml(
            &mut catalog,
            br#"<items><item id="100" slotType="Two-Handed"/></items>"#,
        )
        .unwrap();
        assert_eq!(
            catalog.xml_slot_types_by_server_id(),
            BTreeMap::from([(
                100,
                BTreeSet::from([
                    LegacyItemSlotType::Head,
                    LegacyItemSlotType::TwoHanded,
                    LegacyItemSlotType::RightHand,
                ]),
            )])
        );
    }

    #[test]
    fn rejects_invalid_or_unbounded_legacy_item_combat_metadata() {
        for source in [
            br#"<items><item id="100" armor="10001"/></items>"#.as_slice(),
            br#"<items><item id="100"><attribute key="defense" value="nope"/></item></items>"#,
            br#"<items><item id="100" weaponType="laser"/></items>"#,
            br#"<items><item id="100" attackSpeed="60001"/></items>"#,
            br#"<items><item id="100" weight="-1"/></items>"#,
            br#"<items><item id="100"><attribute key="weight" value="invalid"/></item></items>"#,
            br#"<items><item id="100" slotType="belt"/></items>"#,
            br#"<items><item id="100" name=" "/></items>"#,
        ] {
            assert!(apply_items_xml(&mut combat_metadata_catalog(), source).is_err());
        }
        let oversized_name = "x".repeat(MAX_ITEM_NAME_BYTES + 1);
        let oversized_source =
            format!(r#"<items><item id="100" name="{oversized_name}"/></items>"#);
        assert!(
            apply_items_xml(&mut combat_metadata_catalog(), oversized_source.as_bytes()).is_err()
        );
    }

    #[test]
    fn rejects_missing_identifier_and_invalid_item_ranges() {
        assert!(parse_items_otb(&[NODE_START, 0, NODE_END]).is_err());
        let mut catalog = LegacyItemCatalog {
            otb_major_version: 3,
            client_version: 57,
            build_number: 1,
            definitions: BTreeMap::new(),
        };
        assert!(apply_items_xml(
            &mut catalog,
            br#"<items><item fromid="2" toid="1"/></items>"#
        )
        .is_err());
    }

    #[test]
    fn applies_server_to_client_mapping_and_blocking_flags_to_a_world_tile() {
        let mut root = 0u32.to_le_bytes().to_vec();
        root.push(ROOT_ATTR_VERSION);
        root.extend_from_slice(&140u16.to_le_bytes());
        root.extend_from_slice(&3u32.to_le_bytes());
        root.extend_from_slice(&57u32.to_le_bytes());
        root.extend_from_slice(&1098u32.to_le_bytes());
        root.extend([0u8; 128]);
        let mut item = FLAG_BLOCK_SOLID.to_le_bytes().to_vec();
        item.push(ITEM_ATTR_SERVER_ID);
        item.extend_from_slice(&2u16.to_le_bytes());
        item.extend_from_slice(&4526u16.to_le_bytes());
        item.push(ITEM_ATTR_CLIENT_ID);
        item.extend_from_slice(&2u16.to_le_bytes());
        item.extend_from_slice(&102u16.to_le_bytes());
        let mut bytes = OTB_IDENTIFIER.to_vec();
        bytes.extend(framed_node(0, &root, &[framed_node(1, &item, &[])]));
        let catalog = parse_items_otb(&bytes).unwrap();

        let spawn = forgotten_core::Position {
            x: 99,
            y: 100,
            z: 7,
        };
        let position = forgotten_core::Position {
            x: 100,
            y: 100,
            z: 7,
        };
        let mut map = forgotten_core::WorldMap::new("fixture", spawn);
        map.set_tile(
            spawn,
            forgotten_core::WorldMapTile {
                ground_thing_id: 0,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile(
            position,
            forgotten_core::WorldMapTile {
                ground_thing_id: 4526,
                walkable: true,
            },
        )
        .unwrap();
        map.set_tile_items(
            position,
            vec![forgotten_core::WorldMapItem {
                server_id: 4526,
                client_thing_id: None,
                count: 1,
                action_id: None,
                unique_id: None,
                text: None,
                description: None,
                teleport_destination: None,
                duration: None,
                charges: None,
                children: Vec::new(),
            }],
        )
        .unwrap();

        let normalized = crate::apply_legacy_item_metadata(&map, &catalog).unwrap();
        assert_eq!(normalized.tile(position).unwrap().ground_thing_id, 102);
        assert!(!normalized.is_walkable(position));
    }
}
