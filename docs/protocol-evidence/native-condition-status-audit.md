# Native 740 Condition Status Audit

FE has authoritative persisted poison, burning, and energy schedules. They update vitals through
the native heartbeat, but their client-effect delivery is deliberately separate.

The local OTCv8 parser evidence available for this audit covers a feature-gated per-creature icon
byte (`GameCreatureIcons`) inside the newer creature-add record. It is not a verified classic 740
player-condition status packet. No local 740 parser branch established an exact standalone player
condition icon or effect record that FE can safely emit.

No packet is added by this audit. The current native 740 path remains limited to authoritative
vital updates and the existing viewport refresh behavior. A future condition delivery slice
requires profile-specific packet evidence, explicit icon/effect mapping, lifecycle update rules,
and unmodified-client validation.
