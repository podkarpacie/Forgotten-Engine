# Classic 740 read-only text-window evidence

## Verified packet shape

The local TFS reference sends an edit-text window using server opcode `0x96`, a `u32` window ID, an item record, a `u16` maximum text length, the text string, and writer metadata. For classic clients below protocol 1010, the public OTCv8 parser reads the item as a single `u16` item ID. It then reads `u16 maxLength`, the text string, and the writer string.

For protocol 740, OTCv8 does **not** enable `GameWritableDate`; its public feature configuration enables that field only from protocol 790. Therefore, an FE 740 read-only text-window record must end after the empty writer string. It must not send later writable-date, traded, or contemporary item-record fields.

## Bounded FE subset

FE can independently support one read-only text window only after ordinary native `UseItem` validation proves an exact catalog-mapped top-level map item. The item must carry non-empty imported OTBM text. The server emits the classic window with a deterministic window ID and the client item ID from the already validated request. Imported text remains bounded to the established native frame limit.

## Deferred scope

Editing, client text-save input, writer/date metadata, writable item policy, item attributes, scripts, arbitrary descriptions, containers, map mutation, and generic item actions remain deferred.

## Sources

1. Local read-only reference: `/home/ubuntu/forgotten-work/forgotten-server/src/protocolgame.cpp`, `sendTextWindow`.
2. Public OTCv8 parser: <https://github.com/OTCv8/otcv8-dev/blob/master/src/client/protocolgameparse.cpp>, `parseEditText`.
3. Public OTCv8 feature configuration: <https://github.com/OTCv8/otcv8-dev/blob/master/modules/game_features/features.lua>, `GameWritableDate` gate.
