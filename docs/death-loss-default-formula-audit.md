# Default death-loss formula audit

The local TFS reference distinguishes configured fixed percentage loss from its default calculation. The default branch depends on level, fractional level progress, experience, promotion status, and blessing count. FE currently has bounded fixed-percent loss and lacks complete promotion, blessing, and fractional-level ownership.

Therefore FE must not add an incomplete default-loss calculation. A future implementation needs typed persisted promotion and blessing state, an independently validated level-progress model, and profile-specific evidence for the applicable rule set. Until then, only the existing explicit fixed-percent policy is supportable.
