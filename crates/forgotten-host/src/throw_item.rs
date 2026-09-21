//! ThrowItem address decoding for the native 740 session loop.
//!
//! The `ThrowItem` matrix is the hardest remaining `session_loop.rs` block (hardest-first
//! order): source-by-target branches interleaved across ~1,300 lines that cannot split
//! verbatim. This module is step 1 of that restructure — the straight-line endpoint-address
//! preamble moved verbatim behind a pure, unit-tested classifier, with no behavior change.
//! Per-source handlers dispatching on `ThrowItemAddresses` follow in later increments.

use super::*;

/// Decoded ThrowItem endpoint addresses: each side is either an owned-equipment slot, an
/// owned-container window address, or (both `None`) a real ground tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThrowItemAddresses {
    pub source_slot: Option<EquipmentSlot>,
    pub source_container: Option<(u8, usize)>,
    pub target_slot: Option<EquipmentSlot>,
    pub target_container_id: Option<u8>,
}

/// Decodes ThrowItem source/target addresses verbatim from the session loop preamble.
/// Classic clients address owned endpoints with `x == 0xffff`: equipment slots carry the
/// slot code in `y` (no `0x40` flag, `z == 0`, stack position 0), container windows set
/// the `0x40` flag with the window identifier in the lower four bits of `y`.
pub(crate) fn decode_throw_item_addresses(
    source_position: NativeOtClientPosition,
    source_stack_position: u8,
    target_position: NativeOtClientPosition,
) -> ThrowItemAddresses {
    let source_slot = (source_position.x == 0xffff
        && source_position.y & 0x40 == 0
        && source_position.z == 0
        && source_stack_position == 0)
        .then(|| EquipmentSlot::from_code(source_position.y as u8))
        .flatten();
    let source_container = (source_position.x == 0xffff && source_position.y & 0x40 != 0)
        .then_some((
            (source_position.y & 0x0f) as u8,
            usize::from(source_position.z),
        ));
    let target_slot =
        (target_position.x == 0xffff && target_position.y & 0x40 == 0 && target_position.z == 0)
            .then(|| EquipmentSlot::from_code(target_position.y as u8))
            .flatten();
    // Classic clients address open container windows with the high container flag
    // in y and the window identifier in its lower four bits. For a whole item the
    // destination index remains a client-side drop location and FE appends to the
    // already-owned top-level container. A requested partial stack is narrower: it
    // must name an existing matching top-level container item at that exact index.
    let target_container_id = (target_position.x == 0xffff && target_position.y & 0x40 != 0)
        .then_some((target_position.y & 0x0f) as u8);
    ThrowItemAddresses {
        source_slot,
        source_container,
        target_slot,
        target_container_id,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn pos(x: u16, y: u16, z: u8) -> NativeOtClientPosition {
        NativeOtClientPosition { x, y, z }
    }

    #[test]
    fn decodes_equipment_source_and_target_slots() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 5, 0), 0, pos(0xffff, 6, 0));
        assert_eq!(addresses.source_slot, Some(EquipmentSlot::RightHand));
        assert_eq!(addresses.source_container, None);
        assert_eq!(addresses.target_slot, Some(EquipmentSlot::LeftHand));
        assert_eq!(addresses.target_container_id, None);
    }

    #[test]
    fn rejects_equipment_source_with_nonzero_stack_position() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 5, 0), 1, pos(0xffff, 6, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.source_container, None);
    }

    #[test]
    fn rejects_unknown_slot_codes_on_both_sides() {
        let addresses = decode_throw_item_addresses(pos(0xffff, 11, 0), 0, pos(0xffff, 0, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.target_slot, None);
    }

    #[test]
    fn decodes_container_window_addresses() {
        let addresses =
            decode_throw_item_addresses(pos(0xffff, 0x40 | 3, 2), 0, pos(0xffff, 0x40 | 7, 0));
        assert_eq!(addresses.source_slot, None);
        assert_eq!(addresses.source_container, Some((3, 2)));
        assert_eq!(addresses.target_slot, None);
        assert_eq!(addresses.target_container_id, Some(7));
    }

    #[test]
    fn treats_ground_tiles_as_neither_slot_nor_container() {
        let addresses = decode_throw_item_addresses(pos(100, 100, 7), 0, pos(101, 102, 7));
        assert_eq!(
            addresses,
            ThrowItemAddresses {
                source_slot: None,
                source_container: None,
                target_slot: None,
                target_container_id: None,
            }
        );
    }
}
