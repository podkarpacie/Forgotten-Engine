//! FE-OTClient proprietary extended-opcode transport (capability offer/ack,
//! initial world/empty-world viewport, movement ack).

use super::*;

/// OTClient-oriented extended-opcode transport. A custom OTClient module must explicitly opt in;
/// this is not a general Tibia-client compatibility claim.
pub const FE_OTCLIENT_EXTENDED_OPCODE: u8 = 0x32;
pub const FE_OTCLIENT_CAPABILITY_SUBOPCODE: u8 = 0xf0;
pub const FE_OTCLIENT_WORLD_SUBOPCODE: u8 = 0xf1;
pub const FE_OTCLIENT_CAPABILITY_ACK: &str = "fe.otclient.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtClientEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialWorldSnapshot {
    pub character_name: String,
    pub start_x: u16,
    pub start_y: u16,
    pub start_z: u8,
    pub endpoint: OtClientEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyWorldMovementAck {
    pub tick: u64,
    pub from: Position,
    pub to: Position,
}

pub fn encode_fe_otclient_capability_offer(endpoint: &OtClientEndpoint) -> Frame {
    let offer = format!(
        "fe.capabilities.v1;session=challenge-rsa-xtea;world=empty-gated;endpoint={}:{}",
        endpoint.host, endpoint.port
    );
    encode_fe_otclient_extended(FE_OTCLIENT_CAPABILITY_SUBOPCODE, offer.as_bytes())
}

pub fn encode_fe_otclient_capability_ack_for_harness() -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_CAPABILITY_SUBOPCODE,
        FE_OTCLIENT_CAPABILITY_ACK.as_bytes(),
    )
}

pub fn decode_fe_otclient_capability_ack(frame: &Frame) -> Result<(), ProtocolError> {
    let (subopcode, payload) = decode_fe_otclient_extended(frame)?;
    if subopcode == FE_OTCLIENT_CAPABILITY_SUBOPCODE
        && payload == FE_OTCLIENT_CAPABILITY_ACK.as_bytes()
    {
        Ok(())
    } else {
        Err(ProtocolError::InvalidOtClientCapabilityAck)
    }
}

pub fn encode_fe_otclient_initial_world(snapshot: &InitialWorldSnapshot) -> Frame {
    let payload = format!(
        "fe.world.v1;character={};position={},{},{};endpoint={}:{};world=empty-gated",
        snapshot.character_name,
        snapshot.start_x,
        snapshot.start_y,
        snapshot.start_z,
        snapshot.endpoint.host,
        snapshot.endpoint.port
    );
    encode_fe_otclient_extended(FE_OTCLIENT_CAPABILITY_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_empty_viewport(viewport: &EmptyWorldViewport) -> Frame {
    let payload = format!(
        "fe.viewport.v1;tick={};center={},{},{};manifest={};radius={},{};world=empty",
        viewport.tick,
        viewport.center.x,
        viewport.center.y,
        viewport.center.z,
        viewport.manifest.identifier,
        viewport.manifest.viewport_radius_x,
        viewport.manifest.viewport_radius_y
    );
    encode_fe_otclient_extended(FE_OTCLIENT_WORLD_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_world_tick(tick: u64) -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_WORLD_SUBOPCODE,
        format!("fe.tick.v1;tick={tick}").as_bytes(),
    )
}

pub fn encode_fe_otclient_movement_ack(ack: &EmptyWorldMovementAck) -> Frame {
    let payload = format!(
        "fe.move.ack.v1;tick={};from={},{},{};to={},{},{};world=empty",
        ack.tick, ack.from.x, ack.from.y, ack.from.z, ack.to.x, ack.to.y, ack.to.z
    );
    encode_fe_otclient_extended(FE_OTCLIENT_WORLD_SUBOPCODE, payload.as_bytes())
}

pub fn encode_fe_otclient_move_request_for_harness(direction: CardinalDirection) -> Frame {
    encode_fe_otclient_extended(
        FE_OTCLIENT_WORLD_SUBOPCODE,
        format!("fe.move.v1;direction={}", direction_name(direction)).as_bytes(),
    )
}

pub fn decode_fe_otclient_move_request(frame: &Frame) -> Result<CardinalDirection, ProtocolError> {
    let (subopcode, payload) = decode_fe_otclient_extended(frame)?;
    if subopcode != FE_OTCLIENT_WORLD_SUBOPCODE {
        return Err(ProtocolError::InvalidOtClientMessage);
    }
    let payload =
        std::str::from_utf8(&payload).map_err(|_| ProtocolError::InvalidOtClientMessage)?;
    match payload {
        "fe.move.v1;direction=north" => Ok(CardinalDirection::North),
        "fe.move.v1;direction=east" => Ok(CardinalDirection::East),
        "fe.move.v1;direction=south" => Ok(CardinalDirection::South),
        "fe.move.v1;direction=west" => Ok(CardinalDirection::West),
        _ => Err(ProtocolError::InvalidOtClientMessage),
    }
}

fn direction_name(direction: CardinalDirection) -> &'static str {
    match direction {
        CardinalDirection::North => "north",
        CardinalDirection::East => "east",
        CardinalDirection::South => "south",
        CardinalDirection::West => "west",
    }
}

fn encode_fe_otclient_extended(subopcode: u8, payload: &[u8]) -> Frame {
    let mut writer = Writer::default();
    writer.byte(FE_OTCLIENT_EXTENDED_OPCODE);
    writer.byte(subopcode);
    let payload = String::from_utf8_lossy(payload);
    writer.string(&payload);
    Frame(writer.finish())
}

fn decode_fe_otclient_extended(frame: &Frame) -> Result<(u8, Vec<u8>), ProtocolError> {
    let mut reader = Reader::new(&frame.0);
    if reader.byte()? != FE_OTCLIENT_EXTENDED_OPCODE {
        return Err(ProtocolError::InvalidOtClientCapabilityAck);
    }
    let subopcode = reader.byte()?;
    let payload = reader.string(MAX_FRAME_SIZE)?.into_bytes();
    if !reader.done() {
        return Err(ProtocolError::InvalidOtClientCapabilityAck);
    }
    Ok((subopcode, payload))
}
