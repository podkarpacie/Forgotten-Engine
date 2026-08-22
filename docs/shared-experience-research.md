# Session-Local Shared-Experience Research

The local TFS reference treats shared experience as a party-leader-controlled request with a separate eligibility result. The observed eligibility categories include a non-empty party, a level spread threshold derived from the highest party level, a bounded range and floor check relative to the leader, and recent participant activity.

The next FE slice will model only the authoritative session-local request, typed eligibility inputs, and deterministic status calculation. It will not award or split experience, change native party shields, render client messages, create party channels, persist party state, invoke Lua, or reproduce full TFS combat-activity timing.

The current FE party model already has deterministic leaders, members, invitations, leadership transfer, and cleanup. The new state must be rekeyed on leadership transfer and removed when a party disbands or a live player leaves, so no eligibility request can outlive the session-local party that owns it.

The clean-room core state now follows that ownership rule: a requested state moves to the replacement leader during either explicit transfer or deterministic leader departure, while participant activity is removed on departure and full party disbanding. This remains session-local and does not award experience.
