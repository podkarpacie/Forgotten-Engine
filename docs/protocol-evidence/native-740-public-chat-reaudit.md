# Native 740 Public Chat Re-audit

The local OTCv8 source dispatches server Talk as `0xAA`. Its `parseTalk` function reads the speaker name, translates one server message-mode byte, reads a position for `Say`, and then reads the text. The local versioned mode map has a `version >= 760` branch; therefore protocol `740` uses that branch. It maps `MessageSay` to server mode `1`.

FE now emits only this parser-backed public-speech layout for the native 740 profile: `0xAA`, bounded speaker name, mode `1`, authoritative speaker position, and bounded sanitized text. The existing shared recipient queue remains the fanout boundary. Direct socket coverage asserts the full record after a native Talk request.

The correction deliberately does not claim other social behavior. Whisper, yell, private messages, channels, statements, levels, mute/spam rules, visibility/range filtering, history, persistence, guild/party chat, scripts, and real-client confirmation remain deferred.
