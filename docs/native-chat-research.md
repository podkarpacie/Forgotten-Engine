# Native 740 chat research

The local OTCv8 `740` parser reads a server-talk record as speaker name, raw message mode, a position for `Say` mode, and message text. Its version-specific mode map assigns `Say` to raw value `1`. FE’s bounded public-say encoder uses that same order and value.

The earlier real-client chat failure therefore cannot safely be attributed to the outbound public-say record alone. The remaining diagnosis must examine full-session feature flags, action/frame ordering, and the exact client build without logging packet bodies or user content. Public channel, private-message, and NPC dialogue layouts remain separately bounded.
