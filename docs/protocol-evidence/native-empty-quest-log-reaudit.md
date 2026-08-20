# Native Empty Quest Log Re-audit

The local OTCv8 parser reads a Quest Log as opcode `0xF0`, a U16 quest count, then zero or more
quest records. FE's profile-gated empty encoder emits exactly `0xF0` followed by a zero U16 count,
and its native-session regression proves the session remains usable after the response.

No packet correction is added. Quest persistence, quest records, mission lines, scripts, and
real-client verification remain deferred.
