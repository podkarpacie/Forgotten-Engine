# Legacy World Compatibility Scope

Forgotten Engine will implement its legacy-world path as an original Rust reader for files supplied by the operator. It will not bundle, seed, or redistribute an operator's map, item database, sprites, client files, or other private world content.

## Initial compatibility boundary

The first clean-room reader targets the node-framed OpenTibia binary representation used by both OTBM maps and `items.otb`. Its structural control bytes are node start (`0xFE`), node end (`0xFF`), and escaped data (`0xFD`). The reader must impose FE-specific limits on node depth, decoded bytes, tiles, items per tile, strings, and XML nesting before materializing a world.[1]

The public OTBM vocabulary confirms a root header followed by map data and tile-area nodes; tile, item, town, house-tile, and waypoint records are distinct node kinds. The five-field root header carries map version, width, height, and the major and minor item-definition versions. FE will retain those source versions in its loaded-world report so an operator can diagnose a map/items mismatch before network services start.[2] [3]

For the modern public TFS map-loader contract, FE will target OTBM header versions **1** and **2**. Header version `0` is an older representation that contemporary TFS itself rejects, while values above `2` are unknown to that loader. FE will expose a deterministic diagnostic for unsupported versions rather than silently accepting them.[2]

| Operator file | TFS/OpenTibia role | FE compatibility target |
|---|---|---|
| `data/world/<mapName>.otbm` | Primary binary world: metadata, tile areas, tiles, tile items, embedded towns, house-tile data, and waypoints | Read into the extended FE map model; optionally export a semantically equivalent editable FE document |
| `<map>-spawn.xml` or OTBM-declared spawn file | External monster/NPC spawn areas | Parse and retain spawn declarations; runtime creature spawning is a separate gameplay milestone |
| `<map>-house.xml` or OTBM-declared house file | External house ownership and rent metadata | Parse and retain house metadata; ownership/rent behavior is a separate economy milestone |
| `data/items/items.otb` | Server-ID/client-ID and item type metadata | Parse required mapping and flags for tile-item interpretation |
| `data/items/items.xml` | Additional item attributes | Parse the metadata required for movement and visible item behavior |
| `data/world/<mapName>.femap` | FE-original editable map | Continue to support as a first-class local format and make its conversion boundaries explicit |

> **Correction to the earlier scope:** town records in the current public TFS map contract are embedded OTBM nodes, not a required `towns.xml` companion file. Spawn and house file names may be declared in OTBM metadata; if absent, TFS derives `<mapName>-spawn.xml` and `<mapName>-house.xml`.[2]

## Non-goals of the map compatibility reader

Parsing world data does not by itself recreate every TFS gameplay subsystem. Script execution, monster AI, NPC logic, house rent, depots, quests, combat, and every item-use interaction need their own original Rust runtime milestones. The loader will preserve source metadata where possible so those systems can be added without changing the map format again.

## References

[1] [OpenTibia Binary framing reference](https://pkg.go.dev/badc0de.net/pkg/go-tibia/otb)

[2] [TFS public map-loader format declarations](https://github.com/otland/forgottenserver/blob/master/src/iomap.h)
