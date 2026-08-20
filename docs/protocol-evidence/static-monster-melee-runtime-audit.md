# Static Monster Melee Runtime Audit

FE now imports one bounded legacy direct melee declaration for each resolved monster definition.
The current shared-world attack pass, however, accepts only one globally configured fixed damage
value and applies it once per heartbeat to adjacent selected targets.

Using imported interval and min/max bounds would require persistent per-creature due state,
server-beat conversion, deterministic bounded damage selection, migration rules, and clear
interaction with the existing global attack policy. No per-monster combat execution is added by
this audit.
