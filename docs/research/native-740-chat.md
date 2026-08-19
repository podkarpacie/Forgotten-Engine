# Native 740 chat-delivery evidence

## Unmodified OTCv8 limitation

OTCv8 issue [#218](https://github.com/OTCv8/otclientv8/issues/218) reports that the client does not construct its server message-mode map below protocol 760. As a result, server-speech and text-message records resolve their message mode to an invalid value for protocol 740. The issue also states that locally changing the client gate from `>= 760` to `>= 740` makes 760 message modes work, which is a client modification and therefore outside Forgotten Engine's unmodified-client compatibility target.

## Current FE decision

Forgotten Engine must not emit `GameServerTalk` or generic text-message chat records for the selected unmodified 740 profile until an independently verified stock-client layout exists. The current native shared-chat route may sanitize and queue server-side events, but it suppresses client-visible delivery rather than sending a parser-invalid packet. This limitation applies to both creature speech and text messages named in the cited issue.

## Deferred scope

The following remain deferred: stock-client-visible public speech for protocol 740, a verified alternate client UI route, channels, private messages, chat history, and real-client confirmation. Any later implementation must include a byte-level codec regression and a manual test against an unmodified OTCv8 build.
