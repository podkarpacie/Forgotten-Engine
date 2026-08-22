# Sandboxed Lua host-integration audit

The sandboxed callback dispatcher currently lives in the isolated `forgotten-scripting` crate. It creates a fresh no-standard-library Lua VM per dispatch, accepts only an explicit callback name plus a bounded primitive event kind, subject ID, and scalar value, and does not expose host world state or mutable game APIs.

The native host currently has no scripting dependency and no configuration-owned callback registry. A future host event bridge must therefore be explicit and disabled by default. It must load only declared callback files from a canonical operator-owned root, pass typed primitive observations, ignore or separately validate any returned value, and preserve the lock order by dispatching outside authoritative mutation. No direct Lua world mutation, filesystem, network, modules, TFS userdata, or automatic registry discovery is justified by the current implementation.
