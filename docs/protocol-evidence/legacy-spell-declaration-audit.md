# Legacy Spell Declaration Audit

FE's `forgotten-engine-spells.xml` parser is intentionally an FE-owned, scriptless declaration
format that creates only typed mana-and-cooldown events. It does not claim to parse, execute, or
adapt legacy TFS spell behavior.

The local TFS reference base has an empty `data/spells/spells.xml`; its spell behavior is supplied
through Lua spell files. Those references already belong to FE's bounded registry and sandbox
boundary. Importing an empty XML file into FE's declarative catalog would create a misleading
compatibility claim, so this audit adds no runtime adapter.
