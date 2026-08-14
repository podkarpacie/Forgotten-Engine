# FE 1.2.0 — TFS 1.2 / Tibia 10.98 Compatibility Foundation

FE 1.2.0 establishes the first versioned Rust compatibility foundation for the TFS 1.2 / Tibia 10.98 target. It fixes the initial protocol label mismatch, introduces explicit profile metadata, strengthens packet-frame validation, and makes all release claims testable from the command line.

The release deliberately does not claim complete game-protocol or gameplay compatibility. Its purpose is to make later clean-room protocol, map, data-pack, scripting, and multiplayer work versioned against an explicit target rather than a generic protocol label.

See [VERSIONING.md](VERSIONING.md) for the exact supported scope, known limitations, and roadmap mappings.
