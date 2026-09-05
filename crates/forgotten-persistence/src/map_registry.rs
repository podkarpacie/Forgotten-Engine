//! Static-creature runtime snapshot and map-item removal journal/override persistence.
//! Static-creature runtime state (health, reactivation delay, melee cooldown) and the
//! revision-bound map-item removal journal with remaining-count overrides are stored
//! atomically in dedicated tables.

use super::*;

impl EngineDatabase {
    /// Replaces the complete static-creature runtime snapshot atomically. Callers must supply
    /// only known static spawn IDs; identity validation remains in the authoritative world when
    /// this storage record is applied.
    pub fn replace_static_creature_runtime(
        &mut self,
        records: &[StaticCreatureRuntimeRecord],
    ) -> Result<(), PersistenceError> {
        let mut seen = BTreeMap::new();
        for record in records {
            if record.health_percent > 100 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent must be at most 100".into(),
                ));
            }
            if record.active && record.reactivation_remaining_seconds.is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "active creatures cannot carry a reactivation delay".into(),
                ));
            }
            if record.direct_melee_damage_sequence > i64::MAX as u64 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "direct melee damage sequence does not fit SQLite INTEGER".into(),
                ));
            }
            if seen.insert(record.creature_id, ()).is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "duplicate static creature ID".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM static_creature_runtime", [])?;
        for record in records {
            transaction.execute(
                "INSERT INTO static_creature_runtime (creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds, direct_melee_cooldown_remaining_ticks, direct_melee_damage_sequence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    i64::from(record.creature_id),
                    i64::from(record.position.x),
                    i64::from(record.position.y),
                    i64::from(record.position.z),
                    i64::from(u8::from(record.active)),
                    i64::from(record.health_percent),
                    record.reactivation_remaining_seconds.map(i64::from),
                    record.direct_melee_cooldown_remaining_ticks.map(i64::from),
                      i64::try_from(record.direct_melee_damage_sequence)
                          .map_err(|_| {
                              PersistenceError::InvalidStaticCreatureRuntimeRecord(format!(
                                  "direct melee damage sequence {} exceeds SQLite INTEGER",
                                  record.direct_melee_damage_sequence
                              ))
                          })?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads the complete bounded static-creature runtime snapshot. Rows are independently
    /// validated so malformed external SQLite edits never enter the authoritative world.
    pub fn static_creature_runtime(
        &self,
    ) -> Result<Vec<StaticCreatureRuntimeRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT creature_id, x, y, z, active, health_percent, reactivation_remaining_seconds, direct_melee_cooldown_remaining_ticks, direct_melee_damage_sequence FROM static_creature_runtime ORDER BY creature_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (
                creature_id,
                x,
                y,
                z,
                active,
                health_percent,
                reactivation_remaining_seconds,
                direct_melee_cooldown_remaining_ticks,
                direct_melee_damage_sequence,
            ) = row?;
            let creature_id = u32::try_from(creature_id).map_err(|_| {
                PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "creature ID does not fit u32".into(),
                )
            })?;
            let position = Position {
                x: u16::try_from(x).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "x does not fit u16".into(),
                    )
                })?,
                y: u16::try_from(y).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "y does not fit u16".into(),
                    )
                })?,
                z: u8::try_from(z).map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord("z does not fit u8".into())
                })?,
            };
            let active = match active {
                0 => false,
                1 => true,
                _ => {
                    return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "active flag must be zero or one".into(),
                    ))
                }
            };
            let health_percent = u8::try_from(health_percent).map_err(|_| {
                PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent does not fit u8".into(),
                )
            })?;
            if health_percent > 100 {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "health percent must be at most 100".into(),
                ));
            }
            let reactivation_remaining_seconds = reactivation_remaining_seconds
                .map(|remaining_seconds| {
                    u32::try_from(remaining_seconds).map_err(|_| {
                        PersistenceError::InvalidStaticCreatureRuntimeRecord(
                            "reactivation delay does not fit u32".into(),
                        )
                    })
                })
                .transpose()?;
            if active && reactivation_remaining_seconds.is_some() {
                return Err(PersistenceError::InvalidStaticCreatureRuntimeRecord(
                    "active creatures cannot carry a reactivation delay".into(),
                ));
            }
            let direct_melee_cooldown_remaining_ticks = direct_melee_cooldown_remaining_ticks
                .map(|remaining_ticks| {
                    u32::try_from(remaining_ticks).map_err(|_| {
                        PersistenceError::InvalidStaticCreatureRuntimeRecord(
                            "direct melee cooldown delay does not fit u32".into(),
                        )
                    })
                })
                .transpose()?;
            let direct_melee_damage_sequence = u64::try_from(direct_melee_damage_sequence)
                .map_err(|_| {
                    PersistenceError::InvalidStaticCreatureRuntimeRecord(
                        "direct melee damage sequence must be non-negative".into(),
                    )
                })?;
            records.push(StaticCreatureRuntimeRecord {
                creature_id,
                position,
                active,
                health_percent,
                reactivation_remaining_seconds,
                direct_melee_cooldown_remaining_ticks,
                direct_melee_damage_sequence,
            });
        }
        Ok(records)
    }

    /// Atomically replaces the complete revision-bound removal journal. Future recovery must
    /// compare `map_revision` with the loaded immutable map before applying any removal.
    pub fn replace_map_item_removal_journal(
        &mut self,
        journal: &MapItemRemovalJournal,
    ) -> Result<(), PersistenceError> {
        let mut seen = BTreeMap::new();
        for item in &journal.removed_items {
            if item.map_revision != journal.map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every item must use the journal map revision".into(),
                ));
            }
            if seen.insert((item.position, item.item_index), ()).is_some() {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate source item identity".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM map_item_removal_journal", [])?;
        for item in &journal.removed_items {
            transaction.execute(
                "INSERT INTO map_item_removal_journal (map_revision, x, y, z, item_index) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    format!("{:016x}", journal.map_revision.0),
                    i64::from(item.position.x),
                    i64::from(item.position.y),
                    i64::from(item.position.z),
                    i64::from(item.item_index),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Atomically replaces the complete revision-bound remaining-count override collection. Each
    /// override applies only to one still-present source item, so it must use the journal revision
    /// and keep a strictly positive bounded remaining count.
    pub fn replace_map_item_count_overrides(
        &mut self,
        map_revision: WorldMapSourceRevision,
        overrides: &[MapItemCountOverrideRecord],
    ) -> Result<(), PersistenceError> {
        let mut seen = BTreeMap::new();
        for override_record in overrides {
            if override_record.source_identity.map_revision != map_revision {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "every count override must use the requested map revision".into(),
                ));
            }
            if !(1..=MAX_ITEM_STACK_COUNT).contains(&override_record.remaining_count) {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "count override remaining count must stay within the bounded stack range"
                        .into(),
                ));
            }
            if seen
                .insert(
                    (
                        override_record.source_identity.position,
                        override_record.source_identity.item_index,
                    ),
                    (),
                )
                .is_some()
            {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "duplicate source item identity in count overrides".into(),
                ));
            }
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM map_item_count_overrides", [])?;
        for override_record in overrides {
            transaction.execute(
                "INSERT INTO map_item_count_overrides (map_revision, x, y, z, item_index, remaining_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{:016x}", map_revision.0),
                    i64::from(override_record.source_identity.position.x),
                    i64::from(override_record.source_identity.position.y),
                    i64::from(override_record.source_identity.position.z),
                    i64::from(override_record.source_identity.item_index),
                    i64::from(override_record.remaining_count),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads the complete journal without applying it to any map. Callers must compare the loaded
    /// revision with the current `WorldMap::source_revision()` before considering recovery.
    pub fn map_item_removal_journal(
        &self,
    ) -> Result<Option<MapItemRemovalJournal>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT map_revision, x, y, z, item_index FROM map_item_removal_journal ORDER BY map_revision, x, y, z, item_index",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut journal: Option<MapItemRemovalJournal> = None;
        for row in rows {
            let (revision, x, y, z, item_index) = row?;
            let revision = u64::from_str_radix(&revision, 16).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "map revision must be hexadecimal u64".into(),
                )
            })?;
            let identity = WorldMapItemSourceIdentity {
                map_revision: WorldMapSourceRevision(revision),
                position: Position {
                    x: u16::try_from(x).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("x does not fit u16".into())
                    })?,
                    y: u16::try_from(y).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("y does not fit u16".into())
                    })?,
                    z: u8::try_from(z).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal("z does not fit u8".into())
                    })?,
                },
                item_index: u8::try_from(item_index).map_err(|_| {
                    PersistenceError::InvalidMapItemJournal("item index does not fit u8".into())
                })?,
            };
            match &mut journal {
                Some(existing) if existing.map_revision != identity.map_revision => {
                    return Err(PersistenceError::InvalidMapItemJournal(
                        "journal contains multiple map revisions".into(),
                    ))
                }
                Some(existing) => existing.removed_items.push(identity),
                None => {
                    journal = Some(MapItemRemovalJournal {
                        map_revision: identity.map_revision,
                        removed_items: vec![identity],
                    })
                }
            }
        }
        Ok(journal)
    }

    /// Loads the complete revision-bound remaining-count override collection without applying it to
    /// any map. Callers must validate it against the immutable source map together with the full
    /// removal journal before recovery.
    pub fn map_item_count_overrides(
        &self,
    ) -> Result<Option<(WorldMapSourceRevision, Vec<MapItemCountOverrideRecord>)>, PersistenceError>
    {
        let mut statement = self.connection.prepare(
            "SELECT map_revision, x, y, z, item_index, remaining_count FROM map_item_count_overrides ORDER BY map_revision, x, y, z, item_index",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut map_revision: Option<WorldMapSourceRevision> = None;
        let mut overrides = Vec::new();
        for row in rows {
            let (revision, x, y, z, item_index, remaining_count) = row?;
            let parsed_revision = u64::from_str_radix(&revision, 16).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "count override map revision must be hexadecimal u64".into(),
                )
            })?;
            let parsed_revision = WorldMapSourceRevision(parsed_revision);
            match map_revision {
                Some(existing) if existing != parsed_revision => {
                    return Err(PersistenceError::InvalidMapItemJournal(
                        "count overrides contain multiple map revisions".into(),
                    ))
                }
                None => map_revision = Some(parsed_revision),
                Some(_) => {}
            }
            let remaining_count = u16::try_from(remaining_count).map_err(|_| {
                PersistenceError::InvalidMapItemJournal(
                    "count override remaining count must fit u16".into(),
                )
            })?;
            if !(1..=MAX_ITEM_STACK_COUNT).contains(&remaining_count) {
                return Err(PersistenceError::InvalidMapItemJournal(
                    "count override remaining count must stay within the bounded stack range"
                        .into(),
                ));
            }
            overrides.push(MapItemCountOverrideRecord {
                source_identity: WorldMapItemSourceIdentity {
                    map_revision: parsed_revision,
                    position: Position {
                        x: u16::try_from(x).map_err(|_| {
                            PersistenceError::InvalidMapItemJournal(
                                "count override x does not fit u16".into(),
                            )
                        })?,
                        y: u16::try_from(y).map_err(|_| {
                            PersistenceError::InvalidMapItemJournal(
                                "count override y does not fit u16".into(),
                            )
                        })?,
                        z: u8::try_from(z).map_err(|_| {
                            PersistenceError::InvalidMapItemJournal(
                                "count override z does not fit u8".into(),
                            )
                        })?,
                    },
                    item_index: u8::try_from(item_index).map_err(|_| {
                        PersistenceError::InvalidMapItemJournal(
                            "count override item index does not fit u8".into(),
                        )
                    })?,
                },
                remaining_count,
            });
        }
        Ok(map_revision.map(|revision| (revision, overrides)))
    }
}
