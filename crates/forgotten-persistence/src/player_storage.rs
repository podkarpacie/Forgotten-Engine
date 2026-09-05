//! Player storage persistence: depot containers, inbox items, house ownership, and
//! house access lists. All methods operate on the durable database within transactions
//! guarded by the shared world lock.

use super::*;

impl EngineDatabase {
    /// Replaces all durable top-level depot items owned by one player in one transaction. FE
    /// validates the audited 0Ä‚ËĂ˘â€šÂ¬Ă˘â‚¬Ĺ›19 TFS-shaped depot ID range and ordered complete items, but does
    /// not yet serialize nested containers, arbitrary attribute blobs, capacity, or client views.
    pub fn replace_player_depots(
        &mut self,
        player_id: u64,
        depots: &[PlayerDepotRecord],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        validate_player_depot_records(depots)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_depot_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for depot in depots {
            for (slot, item) in depot.items.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO player_depot_items (player_id, depot_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        player_id as i64,
                        i64::from(depot.depot_id),
                        slot as i64,
                        i64::from(item.server_id),
                        i64::from(item.count),
                        item.action_id.map(i64::from),
                        item.unique_id.map(i64::from),
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads one player's durable depots in deterministic depot and top-level item order. Raw
    /// database fields are validated before entering FE's authoritative item representation.
    pub fn player_depots(
        &self,
        player_id: u64,
    ) -> Result<Vec<PlayerDepotRecord>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT depot_id, slot, server_id, count, action_id, unique_id FROM player_depot_items WHERE player_id = ?1 ORDER BY depot_id, slot",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut depots: BTreeMap<u8, Vec<ItemInstance>> = BTreeMap::new();
        for (depot_id, slot, server_id, count, action_id, unique_id) in records {
            let depot_id = u8::try_from(depot_id).map_err(|_| {
                PersistenceError::InvalidDepotRecord("depot ID does not fit u8".into())
            })?;
            if depot_id > MAX_PLAYER_DEPOT_ID {
                return Err(PersistenceError::InvalidDepotRecord(format!(
                    "depot ID exceeds bounded maximum of {MAX_PLAYER_DEPOT_ID}"
                )));
            }
            let items = depots.entry(depot_id).or_default();
            if items.len() >= MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS {
                return Err(PersistenceError::InvalidDepotRecord(format!(
                    "depot exceeds {MAX_PLAYER_DEPOT_TOP_LEVEL_ITEMS} top-level items"
                )));
            }
            let expected_slot = i64::try_from(items.len()).map_err(|_| {
                PersistenceError::InvalidDepotRecord("depot item slot does not fit i64".into())
            })?;
            if slot != expected_slot {
                return Err(PersistenceError::InvalidDepotRecord(
                    "depot item slots must be contiguous from zero".into(),
                ));
            }
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidDepotRecord("server item ID does not fit u16".into())
            })?;
            let count = u16::try_from(count).map_err(|_| {
                PersistenceError::InvalidDepotRecord("item count does not fit u16".into())
            })?;
            let mut item = ItemInstance::new(server_id, count)
                .map_err(|error| PersistenceError::InvalidDepotRecord(error.to_string()))?;
            item.action_id = optional_u16_depot_attribute(action_id, "action ID")?;
            item.unique_id = optional_u16_depot_attribute(unique_id, "unique ID")?;
            items.push(item);
        }
        let records = depots
            .into_iter()
            .map(|(depot_id, items)| PlayerDepotRecord { depot_id, items })
            .collect::<Vec<_>>();
        validate_player_depot_records(&records)?;
        Ok(records)
    }

    /// Replaces a player's complete bounded inbox contents in one transaction. This TFS-shaped
    /// storage boundary retains only ordered top-level items; nesting, attributes beyond the
    /// bounded IDs, client windows, capacity policy, and inbox routing remain outside it.
    pub fn replace_player_inbox(
        &mut self,
        player_id: u64,
        items: &[ItemInstance],
    ) -> Result<(), PersistenceError> {
        self.ensure_player_exists(player_id)?;
        validate_player_inbox_items(items)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM player_inbox_items WHERE player_id = ?1",
            params![player_id as i64],
        )?;
        for (slot, item) in items.iter().enumerate() {
            transaction.execute(
                "INSERT INTO player_inbox_items (player_id, slot, server_id, count, action_id, unique_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    player_id as i64,
                    slot as i64,
                    i64::from(item.server_id),
                    i64::from(item.count),
                    item.action_id.map(i64::from),
                    item.unique_id.map(i64::from),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads a player's bounded inbox in deterministic top-level item order and rejects malformed
    /// raw SQLite fields before they reach the authoritative item representation.
    pub fn player_inbox(&self, player_id: u64) -> Result<Vec<ItemInstance>, PersistenceError> {
        self.ensure_player_exists(player_id)?;
        let mut statement = self.connection.prepare(
            "SELECT slot, server_id, count, action_id, unique_id FROM player_inbox_items WHERE player_id = ?1 ORDER BY slot",
        )?;
        let records = statement
            .query_map(params![player_id as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if records.len() > MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS {
            return Err(PersistenceError::InvalidInboxRecord(format!(
                "inbox exceeds {MAX_PLAYER_INBOX_TOP_LEVEL_ITEMS} top-level items"
            )));
        }
        let mut items = Vec::with_capacity(records.len());
        for (expected_slot, (slot, server_id, count, action_id, unique_id)) in
            records.into_iter().enumerate()
        {
            let expected_slot = i64::try_from(expected_slot).map_err(|_| {
                PersistenceError::InvalidInboxRecord("inbox item slot does not fit i64".into())
            })?;
            if slot != expected_slot {
                return Err(PersistenceError::InvalidInboxRecord(
                    "inbox item slots must be contiguous from zero".into(),
                ));
            }
            let server_id = u16::try_from(server_id).map_err(|_| {
                PersistenceError::InvalidInboxRecord("server item ID does not fit u16".into())
            })?;
            let count = u16::try_from(count).map_err(|_| {
                PersistenceError::InvalidInboxRecord("item count does not fit u16".into())
            })?;
            let mut item = ItemInstance::new(server_id, count)
                .map_err(|error| PersistenceError::InvalidInboxRecord(error.to_string()))?;
            item.action_id = optional_u16_inbox_attribute(action_id, "action ID")?;
            item.unique_id = optional_u16_inbox_attribute(unique_id, "unique ID")?;
            items.push(item);
        }
        validate_player_inbox_items(&items)?;
        Ok(items)
    }

    /// Assigns or clears the durable owner of one nonzero house identity. The selected owner must
    /// be a persisted player. This has no map, rent, access-list, auction, or client side effect.
    pub fn set_house_owner(
        &mut self,
        house_id: u32,
        owner_player_id: Option<u64>,
    ) -> Result<(), PersistenceError> {
        validated_house_id(house_id)?;
        if let Some(owner_player_id) = owner_player_id {
            self.ensure_player_exists(owner_player_id)?;
            self.connection.execute(
                "INSERT INTO house_ownership (house_id, owner_player_id) VALUES (?1, ?2) ON CONFLICT(house_id) DO UPDATE SET owner_player_id=excluded.owner_player_id",
                params![i64::from(house_id), owner_player_id as i64],
            )?;
        } else {
            self.connection.execute(
                "DELETE FROM house_ownership WHERE house_id = ?1",
                params![i64::from(house_id)],
            )?;
        }
        Ok(())
    }

    /// Returns the durable owner assignment for one nonzero house identity. An absent row is the
    /// explicit unowned state; malformed or stale raw owner data is rejected.
    pub fn house_owner(
        &self,
        house_id: u32,
    ) -> Result<Option<HouseOwnershipRecord>, PersistenceError> {
        validated_house_id(house_id)?;
        let owner_player_id = self
            .connection
            .query_row(
                "SELECT owner_player_id FROM house_ownership WHERE house_id = ?1",
                params![i64::from(house_id)],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(owner_player_id) = owner_player_id else {
            return Ok(None);
        };
        let owner_player_id = u64::try_from(owner_player_id).map_err(|_| {
            PersistenceError::InvalidHouseOwnershipRecord(
                "owner player ID must be a nonnegative u64".into(),
            )
        })?;
        self.ensure_player_exists(owner_player_id)?;
        Ok(Some(HouseOwnershipRecord {
            house_id,
            owner_player_id,
        }))
    }

    /// Replaces every raw bounded access-list text record for one nonzero house identity in a
    /// single transaction. Text interpretation and permission effects remain caller concerns.
    pub fn replace_house_access_lists(
        &mut self,
        house_id: u32,
        records: &[HouseAccessListRecord],
    ) -> Result<(), PersistenceError> {
        validate_house_access_list_records(house_id, records)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM house_access_lists WHERE house_id = ?1",
            params![i64::from(house_id)],
        )?;
        for record in records {
            transaction.execute(
                "INSERT INTO house_access_lists (house_id, list_id, text) VALUES (?1, ?2, ?3)",
                params![i64::from(house_id), i64::from(record.list_id), record.text,],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads raw bounded access-list records in deterministic list-ID order. Malformed raw rows
    /// are rejected rather than silently changing future authorization behavior.
    pub fn house_access_lists(
        &self,
        house_id: u32,
    ) -> Result<Vec<HouseAccessListRecord>, PersistenceError> {
        validated_house_id(house_id)?;
        let mut statement = self.connection.prepare(
            "SELECT list_id, text FROM house_access_lists WHERE house_id = ?1 ORDER BY list_id",
        )?;
        let records = statement
            .query_map(params![i64::from(house_id)], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let records = records
            .into_iter()
            .map(|(list_id, text)| {
                let list_id = u32::try_from(list_id).map_err(|_| {
                    PersistenceError::InvalidHouseAccessListRecord(
                        "list ID does not fit u32".into(),
                    )
                })?;
                Ok(HouseAccessListRecord {
                    house_id,
                    list_id,
                    text,
                })
            })
            .collect::<Result<Vec<_>, PersistenceError>>()?;
        validate_house_access_list_records(house_id, &records)?;
        Ok(records)
    }
}
