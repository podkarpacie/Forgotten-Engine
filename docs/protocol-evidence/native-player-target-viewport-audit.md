# Native Player Target Viewport Availability Audit

The shared host produces candidate player render records, but the classic protocol renderer decides
actual client visibility while traversing tiles. It applies floor and viewport bounds, a per-frame
player cap, and frame-budget clipping before serializing a peer.

A simple same-floor coordinate test would not prove that the target record was delivered to the
current client. The existing target helper also has no selected-target render-delivery token or
typed PvP/query policy. No target availability gate is added by this audit.
