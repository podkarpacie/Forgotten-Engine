# Native Capacity Audit

FE stores and displays authoritative player capacity. The TFS reference data uses an `items.xml`
`weight` attribute, but FE's deliberately bounded item parser does not retain that field in its
runtime catalog.

No capacity guard is added by this audit. Calculating carried weight from the current retained
item fields would silently treat unknown weights as zero and would not account for contained or
nested inventory. A future capacity slice requires validated weight import, explicit per-stack
weight arithmetic, recursive ownership semantics, and profile-specific transfer policy.
