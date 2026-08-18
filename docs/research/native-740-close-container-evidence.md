# Native 740 CloseContainer Boundary

## Verified request and response records

The public TFS reference dispatches client opcode `0x87` to `parseCloseContainer`. The parser reads one unsigned-byte container ID and delegates it to `Game::playerCloseContainer`. That method removes the player’s open-container entry and sends a close-container record. See the read-only public reference at `src/protocolgame.cpp`, around lines 1185–1189, and `src/game.cpp`, around lines 2332–2341.

The public TFS close response uses opcode `0x6F` followed by the same one-byte container ID. The public OTClient parser routes that record to `parseCloseContainer`, which reads exactly that ID and closes the matching client container view. See the read-only public reference at `src/protocolgame.cpp`, around lines 2391–2396, and the public OTClient parser source at <https://raw.githubusercontent.com/edubart/otclient/master/src/client/protocolgameparse.cpp>, `parseCloseContainer`.

## FE scope

FE will treat this as a per-native-session view closure. It must not delete persisted container data, alter item ownership, infer nested-container semantics, or implement up-arrow/update controls. A client-closed top-level view should not be silently reopened by later shared-container refreshes during the same session.
