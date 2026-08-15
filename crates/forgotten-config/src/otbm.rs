use crate::ConfigError;
use forgotten_core::{
    OtbmMapHeader, Position, WorldMap, WorldMapItem, WorldMapSource, WorldMapTile, WorldMapTown,
};

const OTB_NODE_START: u8 = 0xfe;
const OTB_NODE_END: u8 = 0xff;
const OTB_NODE_ESCAPE: u8 = 0xfd;
const OTBM_IDENTIFIER: &[u8; 4] = b"OTBM";
const OTBM_ROOT: u8 = 1;
const OTBM_MAP_DATA: u8 = 2;
const OTBM_TILE_AREA: u8 = 4;
const OTBM_TILE: u8 = 5;
const OTBM_ITEM: u8 = 6;
const OTBM_TOWNS: u8 = 12;
const OTBM_TOWN: u8 = 13;
const OTBM_HOUSE_TILE: u8 = 14;
const OTBM_WAYPOINTS: u8 = 15;
const OTBM_WAYPOINT: u8 = 16;

const OTBM_ATTR_DESCRIPTION: u8 = 1;
const OTBM_ATTR_TILE_FLAGS: u8 = 3;
const OTBM_ATTR_ACTION_ID: u8 = 4;
const OTBM_ATTR_UNIQUE_ID: u8 = 5;
const OTBM_ATTR_TEXT: u8 = 6;
const OTBM_ATTR_DESC: u8 = 7;
const OTBM_ATTR_TELE_DEST: u8 = 8;
const OTBM_ATTR_ITEM: u8 = 9;
const OTBM_ATTR_EXT_SPAWN_FILE: u8 = 11;
const OTBM_ATTR_EXT_HOUSE_FILE: u8 = 13;
const OTBM_ATTR_COUNT: u8 = 15;
const OTBM_ATTR_DURATION: u8 = 16;
const OTBM_ATTR_RUNE_CHARGES: u8 = 12;
const OTBM_ATTR_CHARGES: u8 = 22;
const MAX_OTBM_BYTES: usize = 64 * 1024 * 1024;
const MAX_OTBM_NODES: usize = 300_000;
const MAX_OTBM_NODE_DEPTH: usize = 64;
const MAX_OTBM_STRING_BYTES: usize = 8 * 1024;

#[derive(Debug)]
struct Node {
    kind: u8,
    props: Vec<u8>,
    children: Vec<Node>,
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

    fn peek_u8(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn read_u8(&mut self) -> Result<u8, ConfigError> {
        let value = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or_else(|| invalid("unexpected end of OTBM properties"))?;
        self.offset += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> Result<u16, ConfigError> {
        let end = self
            .offset
            .checked_add(2)
            .ok_or_else(|| invalid("OTBM property offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("unexpected end of OTBM u16 property"))?;
        self.offset = end;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, ConfigError> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or_else(|| invalid("OTBM property offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("unexpected end of OTBM u32 property"))?;
        self.offset = end;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_position(&mut self) -> Result<Position, ConfigError> {
        Ok(Position {
            x: self.read_u16()?,
            y: self.read_u16()?,
            z: self.read_u8()?,
        })
    }

    fn read_string(&mut self) -> Result<String, ConfigError> {
        let length = usize::from(self.read_u16()?);
        if length > MAX_OTBM_STRING_BYTES {
            return Err(invalid("OTBM string exceeds the configured limit"));
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| invalid("OTBM string length overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("unexpected end of OTBM string property"))?;
        self.offset = end;
        String::from_utf8(bytes.to_vec()).map_err(|_| invalid("OTBM string is not UTF-8"))
    }
}

pub(crate) fn parse_otbm_world_map(
    identifier: &str,
    bytes: &[u8],
) -> Result<WorldMap, ConfigError> {
    if bytes.len() > MAX_OTBM_BYTES {
        return Err(invalid("OTBM file exceeds the configured 64 MiB limit"));
    }
    let framed_bytes = bytes
        .strip_prefix(OTBM_IDENTIFIER)
        .ok_or_else(|| invalid("OTBM file is missing its required OTBM identifier"))?;
    let root = parse_tree(framed_bytes)?;
    if root.kind != OTBM_ROOT {
        return Err(invalid("OTBM root node is not ROOTV1"));
    }
    let header = parse_header(&root.props)?;
    if root.children.len() != 1 || root.children[0].kind != OTBM_MAP_DATA {
        return Err(invalid("OTBM root must contain exactly one map-data node"));
    }
    let map_data = &root.children[0];
    let mut header = header;
    parse_map_data_attributes(&map_data.props, &mut header)?;

    let placeholder_spawn = Position { x: 0, y: 0, z: 0 };
    let mut world_map = WorldMap::new(identifier, placeholder_spawn);
    world_map.set_source(WorldMapSource::Otbm(header));
    for child in &map_data.children {
        match child.kind {
            OTBM_TILE_AREA => parse_tile_area(child, &mut world_map)?,
            OTBM_TOWNS => parse_towns(child, &mut world_map)?,
            OTBM_WAYPOINTS => parse_waypoints(child, &mut world_map)?,
            other => return Err(invalid(format!("unsupported OTBM map-data node {other}"))),
        }
    }
    let town_spawn = world_map
        .towns()
        .next()
        .map(|town| town.temple_position)
        .filter(|position| world_map.is_walkable(*position));
    let spawn = town_spawn
        .or_else(|| world_map.first_walkable_position())
        .ok_or_else(|| invalid("OTBM map does not contain a walkable tile"))?;
    world_map.set_spawn(spawn);
    world_map
        .validate()
        .map_err(|error| invalid(format!("invalid OTBM map: {error}")))?;
    Ok(world_map)
}

fn parse_tree(bytes: &[u8]) -> Result<Node, ConfigError> {
    let mut index = 0;
    let mut stack = Vec::<Node>::new();
    let mut root = None;
    let mut node_count = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            OTB_NODE_START => {
                index += 1;
                let kind = read_framed_byte(bytes, &mut index)?;
                node_count += 1;
                if node_count > MAX_OTBM_NODES {
                    return Err(invalid("OTBM node count exceeds the configured limit"));
                }
                if stack.len() >= MAX_OTBM_NODE_DEPTH {
                    return Err(invalid("OTBM node depth exceeds the configured limit"));
                }
                stack.push(Node {
                    kind,
                    props: Vec::new(),
                    children: Vec::new(),
                });
            }
            OTB_NODE_END => {
                index += 1;
                let completed = stack
                    .pop()
                    .ok_or_else(|| invalid("OTBM contains an unmatched node end"))?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(completed);
                } else if root.replace(completed).is_some() {
                    return Err(invalid("OTBM contains more than one root node"));
                }
            }
            OTB_NODE_ESCAPE => {
                index += 1;
                let escaped = bytes
                    .get(index)
                    .copied()
                    .ok_or_else(|| invalid("OTBM escape byte has no escaped value"))?;
                index += 1;
                stack
                    .last_mut()
                    .ok_or_else(|| invalid("OTBM property data appears outside a node"))?
                    .props
                    .push(escaped);
            }
            value => {
                index += 1;
                stack
                    .last_mut()
                    .ok_or_else(|| invalid("OTBM property data appears outside a node"))?
                    .props
                    .push(value);
            }
        }
    }
    if !stack.is_empty() {
        return Err(invalid("OTBM ended before every node was closed"));
    }
    root.ok_or_else(|| invalid("OTBM does not contain a root node"))
}

fn read_framed_byte(bytes: &[u8], index: &mut usize) -> Result<u8, ConfigError> {
    let value = bytes
        .get(*index)
        .copied()
        .ok_or_else(|| invalid("OTBM node start has no node type"))?;
    *index += 1;
    if value == OTB_NODE_ESCAPE {
        let escaped = bytes
            .get(*index)
            .copied()
            .ok_or_else(|| invalid("OTBM escaped node type is missing a value"))?;
        *index += 1;
        Ok(escaped)
    } else if matches!(value, OTB_NODE_START | OTB_NODE_END) {
        Err(invalid("OTBM node type uses a reserved framing byte"))
    } else {
        Ok(value)
    }
}

fn parse_header(props: &[u8]) -> Result<OtbmMapHeader, ConfigError> {
    let mut cursor = Cursor::new(props);
    let header = OtbmMapHeader {
        version: cursor.read_u32()?,
        width: cursor.read_u16()?,
        height: cursor.read_u16()?,
        item_major_version: cursor.read_u32()?,
        item_minor_version: cursor.read_u32()?,
        description: None,
        spawn_file: None,
        house_file: None,
    };
    if !cursor.is_empty() {
        return Err(invalid(
            "OTBM root header contains unexpected trailing bytes",
        ));
    }
    if !(1..=2).contains(&header.version) {
        return Err(invalid(format!(
            "unsupported OTBM map version {}; FE supports versions 1 and 2",
            header.version
        )));
    }
    if header.width == 0 || header.height == 0 {
        return Err(invalid("OTBM header map dimensions must be nonzero"));
    }
    Ok(header)
}

fn parse_map_data_attributes(props: &[u8], header: &mut OtbmMapHeader) -> Result<(), ConfigError> {
    let mut cursor = Cursor::new(props);
    while !cursor.is_empty() {
        match cursor.read_u8()? {
            OTBM_ATTR_DESCRIPTION => header.description = Some(cursor.read_string()?),
            OTBM_ATTR_EXT_SPAWN_FILE => header.spawn_file = Some(cursor.read_string()?),
            OTBM_ATTR_EXT_HOUSE_FILE => header.house_file = Some(cursor.read_string()?),
            attribute => {
                return Err(invalid(format!(
                    "unsupported OTBM map-data attribute {attribute}"
                )))
            }
        }
    }
    Ok(())
}

fn parse_tile_area(node: &Node, world_map: &mut WorldMap) -> Result<(), ConfigError> {
    let mut area = Cursor::new(&node.props);
    let base = area.read_position()?;
    if !area.is_empty() {
        return Err(invalid(
            "OTBM tile-area contains unexpected trailing properties",
        ));
    }
    for tile in &node.children {
        if tile.kind != OTBM_TILE && tile.kind != OTBM_HOUSE_TILE {
            return Err(invalid(format!(
                "unsupported OTBM tile-area child node {}",
                tile.kind
            )));
        }
        parse_tile(tile, base, world_map)?;
    }
    Ok(())
}

fn parse_tile(node: &Node, base: Position, world_map: &mut WorldMap) -> Result<(), ConfigError> {
    let mut cursor = Cursor::new(&node.props);
    let x = base
        .x
        .checked_add(u16::from(cursor.read_u8()?))
        .ok_or_else(|| invalid("OTBM tile x coordinate overflow"))?;
    let y = base
        .y
        .checked_add(u16::from(cursor.read_u8()?))
        .ok_or_else(|| invalid("OTBM tile y coordinate overflow"))?;
    let position = Position { x, y, z: base.z };
    let house_id = if node.kind == OTBM_HOUSE_TILE {
        Some(cursor.read_u32()?)
    } else {
        None
    };
    let mut flags = 0u32;
    let mut items = Vec::new();
    while !cursor.is_empty() {
        match cursor.read_u8()? {
            OTBM_ATTR_TILE_FLAGS => flags = cursor.read_u32()?,
            OTBM_ATTR_ITEM => items.push(parse_item_payload(&mut cursor, true)?),
            attribute => {
                return Err(invalid(format!(
                    "unsupported OTBM tile attribute {attribute} at {},{},{}",
                    position.x, position.y, position.z
                )))
            }
        }
    }
    for child in &node.children {
        if child.kind != OTBM_ITEM {
            return Err(invalid(format!(
                "unsupported OTBM tile child node {} at {},{},{}",
                child.kind, position.x, position.y, position.z
            )));
        }
        items.push(parse_item_node(child)?);
    }
    let ground_thing_id = items.first().map(|item| item.server_id).unwrap_or_default();
    world_map
        .set_tile(
            position,
            WorldMapTile {
                ground_thing_id,
                // Exact blocking semantics are refined after operator item metadata is loaded.
                walkable: true,
            },
        )
        .map_err(|error| invalid(format!("OTBM tile at {},{},{}: {error}", x, y, base.z)))?;
    world_map
        .set_tile_items(position, items)
        .map_err(|error| invalid(format!("OTBM tile at {},{},{}: {error}", x, y, base.z)))?;
    world_map.set_tile_flags(position, flags);
    if let Some(house_id) = house_id {
        world_map
            .set_house_tile(position, house_id)
            .map_err(|error| invalid(format!("OTBM tile at {},{},{}: {error}", x, y, base.z)))?;
    }
    Ok(())
}

fn parse_item_node(node: &Node) -> Result<WorldMapItem, ConfigError> {
    let mut item = parse_item_payload(&mut Cursor::new(&node.props), false)?;
    for child in &node.children {
        if child.kind != OTBM_ITEM {
            return Err(invalid(format!(
                "unsupported OTBM nested item node {}",
                child.kind
            )));
        }
        item.children.push(parse_item_node(child)?);
    }
    Ok(item)
}

fn parse_item_payload(
    cursor: &mut Cursor<'_>,
    stop_at_tile_attribute: bool,
) -> Result<WorldMapItem, ConfigError> {
    let mut item = WorldMapItem {
        server_id: cursor.read_u16()?,
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
    };
    while let Some(attribute) = cursor.peek_u8() {
        if stop_at_tile_attribute && matches!(attribute, OTBM_ATTR_TILE_FLAGS | OTBM_ATTR_ITEM) {
            break;
        }
        let attribute = cursor.read_u8()?;
        match attribute {
            OTBM_ATTR_ACTION_ID => item.action_id = Some(cursor.read_u16()?),
            OTBM_ATTR_UNIQUE_ID => item.unique_id = Some(cursor.read_u16()?),
            OTBM_ATTR_TEXT => item.text = Some(cursor.read_string()?),
            OTBM_ATTR_DESC => item.description = Some(cursor.read_string()?),
            OTBM_ATTR_TELE_DEST => item.teleport_destination = Some(cursor.read_position()?),
            OTBM_ATTR_RUNE_CHARGES => item.charges = Some(u16::from(cursor.read_u8()?)),
            OTBM_ATTR_COUNT => item.count = cursor.read_u8()?.max(1),
            OTBM_ATTR_DURATION => item.duration = Some(cursor.read_u32()?),
            OTBM_ATTR_CHARGES => item.charges = Some(cursor.read_u16()?),
            // Preserved in the source world later through item-specific metadata; these fields do
            // not change FE's initial map topology and have fixed-width payloads.
            10 | 14 => {
                let _ = cursor.read_u16()?;
            }
            17 => {
                let _ = cursor.read_u8()?;
            }
            18..=21 => {
                let _ = cursor.read_u32()?;
            }
            other => return Err(invalid(format!("unsupported OTBM item attribute {other}"))),
        }
    }
    Ok(item)
}

fn parse_towns(node: &Node, world_map: &mut WorldMap) -> Result<(), ConfigError> {
    for child in &node.children {
        if child.kind != OTBM_TOWN {
            return Err(invalid(format!(
                "unsupported OTBM town node {}",
                child.kind
            )));
        }
        let mut cursor = Cursor::new(&child.props);
        let town = WorldMapTown {
            id: cursor.read_u32()?,
            name: cursor.read_string()?,
            temple_position: cursor.read_position()?,
        };
        if !cursor.is_empty() || !child.children.is_empty() {
            return Err(invalid("OTBM town contains unsupported trailing data"));
        }
        world_map
            .set_town(town)
            .map_err(|error| invalid(format!("invalid OTBM town: {error}")))?;
    }
    Ok(())
}

fn parse_waypoints(node: &Node, world_map: &mut WorldMap) -> Result<(), ConfigError> {
    for child in &node.children {
        if child.kind != OTBM_WAYPOINT {
            return Err(invalid(format!(
                "unsupported OTBM waypoint node {}",
                child.kind
            )));
        }
        let mut cursor = Cursor::new(&child.props);
        let name = cursor.read_string()?;
        let position = cursor.read_position()?;
        if !cursor.is_empty() || !child.children.is_empty() {
            return Err(invalid("OTBM waypoint contains unsupported trailing data"));
        }
        world_map
            .set_waypoint(name, position)
            .map_err(|error| invalid(format!("invalid OTBM waypoint: {error}")))?;
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed_node(kind: u8, props: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![OTB_NODE_START, kind];
        for byte in props {
            if matches!(*byte, OTB_NODE_START | OTB_NODE_END | OTB_NODE_ESCAPE) {
                bytes.push(OTB_NODE_ESCAPE);
            }
            bytes.push(*byte);
        }
        for child in children {
            bytes.extend_from_slice(child);
        }
        bytes.push(OTB_NODE_END);
        bytes
    }

    fn string(value: &str) -> Vec<u8> {
        let mut bytes = (value.len() as u16).to_le_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    #[test]
    fn parses_a_hand_authored_modern_otbm_fixture() {
        let mut header = Vec::new();
        header.extend_from_slice(&2u32.to_le_bytes());
        header.extend_from_slice(&512u16.to_le_bytes());
        header.extend_from_slice(&512u16.to_le_bytes());
        header.extend_from_slice(&57u32.to_le_bytes());
        header.extend_from_slice(&1098u32.to_le_bytes());

        let mut map_data_props = vec![OTBM_ATTR_DESCRIPTION];
        map_data_props.extend(string("fixture"));
        map_data_props.push(OTBM_ATTR_EXT_SPAWN_FILE);
        map_data_props.extend(string("fixture-spawn.xml"));
        map_data_props.push(OTBM_ATTR_EXT_HOUSE_FILE);
        map_data_props.extend(string("fixture-house.xml"));

        let mut tile_area_props = Vec::new();
        tile_area_props.extend_from_slice(&100u16.to_le_bytes());
        tile_area_props.extend_from_slice(&100u16.to_le_bytes());
        tile_area_props.push(7);
        let mut tile_props = vec![0, 0, OTBM_ATTR_ITEM];
        tile_props.extend_from_slice(&4526u16.to_le_bytes());
        tile_props.push(OTBM_ATTR_TILE_FLAGS);
        tile_props.extend_from_slice(&1u32.to_le_bytes());
        let tile = framed_node(OTBM_TILE, &tile_props, &[]);
        let tile_area = framed_node(OTBM_TILE_AREA, &tile_area_props, &[tile]);

        let mut town_props = 1u32.to_le_bytes().to_vec();
        town_props.extend(string("Thais"));
        town_props.extend_from_slice(&100u16.to_le_bytes());
        town_props.extend_from_slice(&100u16.to_le_bytes());
        town_props.push(7);
        let town = framed_node(OTBM_TOWN, &town_props, &[]);
        let towns = framed_node(OTBM_TOWNS, &[], &[town]);

        let mut waypoint_props = string("temple");
        waypoint_props.extend_from_slice(&100u16.to_le_bytes());
        waypoint_props.extend_from_slice(&100u16.to_le_bytes());
        waypoint_props.push(7);
        let waypoint = framed_node(OTBM_WAYPOINT, &waypoint_props, &[]);
        let waypoints = framed_node(OTBM_WAYPOINTS, &[], &[waypoint]);
        let map_data = framed_node(
            OTBM_MAP_DATA,
            &map_data_props,
            &[tile_area, towns, waypoints],
        );
        let mut bytes = OTBM_IDENTIFIER.to_vec();
        bytes.extend(framed_node(OTBM_ROOT, &header, &[map_data]));

        let map = parse_otbm_world_map("fixture", &bytes).unwrap();
        assert_eq!(map.identifier(), "fixture");
        assert_eq!(
            map.spawn(),
            Position {
                x: 100,
                y: 100,
                z: 7
            }
        );
        assert_eq!(map.tile_flags(map.spawn()), 1);
        assert_eq!(map.tile_items(map.spawn()).unwrap()[0].server_id, 4526);
        assert_eq!(map.towns().next().unwrap().name, "Thais");
        assert_eq!(map.waypoint("temple"), Some(map.spawn()));
        assert!(matches!(map.source(), WorldMapSource::Otbm(_)));
    }

    #[test]
    fn rejects_malformed_or_unsupported_otbm_data() {
        assert!(parse_otbm_world_map("broken", b"OTBM\xff").is_err());
        let mut unsupported_header = Vec::new();
        unsupported_header.extend_from_slice(&3u32.to_le_bytes());
        unsupported_header.extend_from_slice(&1u16.to_le_bytes());
        unsupported_header.extend_from_slice(&1u16.to_le_bytes());
        unsupported_header.extend_from_slice(&57u32.to_le_bytes());
        unsupported_header.extend_from_slice(&1098u32.to_le_bytes());
        let map_data = framed_node(OTBM_MAP_DATA, &[], &[]);
        let mut bytes = OTBM_IDENTIFIER.to_vec();
        bytes.extend(framed_node(OTBM_ROOT, &unsupported_header, &[map_data]));
        assert!(parse_otbm_world_map("unsupported", &bytes).is_err());
    }
}
