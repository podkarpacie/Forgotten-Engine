# Player Capacity Foundation Audit

FE retains legacy item weights and persists player capacity in authoritative vitals. Its current
inventory model, however, consists of equipment plus bounded client container windows. Containers
are explicitly non-recursive, may be nested views, and do not constitute a complete player-owned
item graph.

Calculating or enforcing capacity from this partial view would omit legitimate carried content or
double-count container views. No capacity calculation or transfer rejection is added by this
audit.
