# Private Legacy World Migration

Forgotten Engine **FE 7.4.20** can load an operator-supplied OpenTibia/TFS-style map data set without bundling the map, item definitions, sprites, `.dat`, `.spr`, or any other third-party content. This is a **world-data compatibility layer**: it imports map topology, tile items, towns, waypoints, companion spawn and house definitions, item mappings, client-visible thing IDs, and blocking semantics. It does not claim that every higher-level TFS gameplay subsystem—Lua scripts, monster AI, actions, spells, NPC behavior, housing economics, or database migrations—is implemented yet.

| Operator file | FE behavior |
|---|---|
| `data/world/<mapName>.otbm` | Loads OTBM v1 or v2 map headers, tiles, ordered item records, tile flags, house tiles, embedded towns, and waypoints. |
| `data/items/items.otb` | Required for an OTBM world. Maps server item IDs to the client-visible thing IDs used in the native OTCv8 viewport. |
| `data/items/items.xml` | Optional metadata overlay. `blockSolid` and `blockPathFind` refine movement walkability. |
| `<mapName>-spawn.xml` | Loaded when present, unless the OTBM map declares another safe relative spawn filename. |
| `<mapName>-house.xml` | Loaded when present, unless the OTBM map declares another safe relative house filename. |
| `data/world/<mapName>.femap` | FE’s first-class original editable world format. It remains available independently of OTBM compatibility. |

## 1. Select your private OTBM world

Create or update `config.lua` in the local server directory:

```lua
-- Map
mapName = "myworld"
mapFormat = "otbm"
worldType = "pvp"
```

`mapFormat = "auto"` is also supported. In auto mode, FE selects `data/world/myworld.otbm` when it exists; otherwise it selects the FE-native `data/world/myworld.femap`. Use `mapFormat = "otbm"` while validating a legacy data set so a missing or incorrectly named OTBM file is reported directly.

Your local directory should then resemble:

```text
my-server/
  config.lua
  data/
    world/
      myworld.otbm
      myworld-spawn.xml       # if used by the world
      myworld-house.xml       # if used by the world
    items/
      items.otb
      items.xml               # optional metadata overlay
```

The OTBM map owns town records. FE therefore does **not** look for a separate `towns.xml` companion file. It also rejects companion paths that attempt to escape `data/world/`.

## 2. Validate before running services

Run validation from the folder containing the executable:

```powershell
.\forgotten-engine.exe validate .\my-server
```

For a valid legacy world, the summary reports the selected profile and map, map tile count and spawn, resolved item-definition count, and parsed spawn and house counts. FE rejects malformed node framing, unsupported OTBM header versions, map references to missing item IDs, oversized XML/OTB inputs, an invalid spawn tile, and blocked spawn positions.

> **Client assets remain operator-provided.** The OTB mapping supplies client thing identifiers, but OTCv8 must still be configured with your lawfully held matching `.dat` and `.spr` data. Forgotten Engine does not redistribute those assets.

## 3. Run and verify in OTCv8

Start the local host with your existing FE 7.4 native OTCv8 settings:

```powershell
.\forgotten-engine.exe run .\my-server
```

At native 740 login, FE encodes the selected map’s mapped ground and bounded ordered non-ground tile layers in the classic viewport. Successful cardinal movement updates the persisted position, sends the creature movement packet, and then sends an updated full map viewport so newly visible imported tiles are refreshed. Movement into tiles blocked by the resolved item metadata is cancelled safely.

## 4. Continue using FE-native maps

For a small original local map, select:

```lua
mapName = "forgotten"
mapFormat = "femap"
```

The FE-native v1 format is intentionally compact:

```ini
format=fe-map-v1
spawn=100,100,7
fill=80,80,120,120,7,0,true
```

The internal FE v2 interchange representation retains OTBM source metadata, tiles, simple item stacks, flags, house tiles, towns, and waypoints. Rich item container state, text, teleports, duration, and charges are retained while loading OTBM data but are deliberately rejected from the present textual v2 exporter rather than silently lost. A command-line conversion workflow can be added in a later tooling milestone.

## Practical boundary

This release makes old world **data** usable as an FE world input. It does not yet make FE a drop-in binary replacement for every historical TFS gameplay feature. Keep your private content backed up, validate on a copy first, and test with a non-production database before allowing players onto a migrated world.
