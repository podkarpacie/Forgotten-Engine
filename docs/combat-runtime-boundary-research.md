# Combat runtime boundary research

The local TFS reference routes combat through a broad health-change path that combines target admission, armor/defense, resistance and immunity handling, multi-part damage, reflection, callbacks, Lua/event hooks, conditions, deaths, corpses, effects, and notifications. That is a system-level behavioral map, not an implementation source for FE.

FE’s current selected-player melee and static-creature routes deliberately own only bounded fixed damage, an explicit cooldown, restricted world-type admission, and separately persisted vitality/death foundations. The next compatible combat slice must therefore remain narrow and profile-evidenced. It must not claim generic spell, weapon, defense, corpse, callback, or Lua parity merely because a common upstream combat entry point exists.

The preferred next investigation is a validated fixed data-backed damage-type admission rule for an already accepted FE event, only if the native 740 packet/result consequences are independently established. Broader TFS combat behavior remains deferred.
