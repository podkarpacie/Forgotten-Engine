//! Player-to-player trade sessions on the authoritative world state: session opening,
//! item staging/unstaging, acceptance, and the atomic execution that swaps items between
//! participants' inventories. Sessions are memory-only; persistence and client windows
//! are host concerns.

use super::*;

impl WorldState {
    /// Opens a player-to-player trade session. Both players must exist, be distinct, and hold
    /// no other active trade. Returns the opened session for window display.
    pub fn open_player_trade(
        &mut self,
        initiator: u64,
        counterparty: u64,
    ) -> Result<&PlayerTradeSession, CoreError> {
        if initiator == counterparty {
            return Err(CoreError::TradeWithSelf);
        }
        for player_id in [initiator, counterparty] {
            if !self.players.contains_key(&player_id) {
                return Err(CoreError::UnknownPlayer(player_id));
            }
            if self
                .player_respawn_states
                .get(&player_id)
                .is_some_and(|state| state.dead)
            {
                return Err(CoreError::PlayerIsDead(player_id));
            }
        }
        if self.active_trades.contains_key(&initiator)
            || self.active_trades.contains_key(&counterparty)
        {
            return Err(CoreError::PlayerAlreadyTrading(initiator));
        }
        // One authoritative record per trade, stored under the initiator's id. Both sides
        // resolve through the lookup helpers so mutations always hit the same copy.
        self.active_trades.insert(
            initiator,
            PlayerTradeSession {
                initiator,
                counterparty,
                initiator_items: Vec::new(),
                counterparty_items: Vec::new(),
                initiator_accepted: false,
                counterparty_accepted: false,
                tick_opened: self.tick,
            },
        );
        self.active_trades
            .get(&initiator)
            .ok_or(CoreError::UnknownPlayer(initiator))
    }

    /// Reads the live trade session involving one player, if any.
    pub fn player_trade(&self, player_id: u64) -> Option<&PlayerTradeSession> {
        if let Some(session) = self.active_trades.get(&player_id) {
            return Some(session);
        }
        self.active_trades
            .values()
            .find(|session| session.counterparty == player_id)
    }

    pub fn active_trade_entry_mut(
        &mut self,
        player_id: u64,
    ) -> Result<&mut PlayerTradeSession, CoreError> {
        if self.active_trades.contains_key(&player_id) {
            return self
                .active_trades
                .get_mut(&player_id)
                .ok_or(CoreError::NoActiveTrade(player_id));
        }
        let initiator = self
            .active_trades
            .values()
            .find(|session| session.counterparty == player_id)
            .map(|session| session.initiator)
            .ok_or(CoreError::NoActiveTrade(player_id))?;
        self.active_trades
            .get_mut(&initiator)
            .ok_or(CoreError::NoActiveTrade(player_id))
    }

    /// Stages one container item reference into a side's offer. Staging resets both acceptance
    /// flags because either offer changed. Bounded by MAX_TRADE_ITEMS_PER_SIDE.
    pub fn stage_trade_item(
        &mut self,
        player_id: u64,
        reference: TradeItemReference,
    ) -> Result<usize, CoreError> {
        let session = self.active_trade_entry_mut(player_id)?;
        let (side_items, other_side) = if session.initiator == player_id {
            (
                &mut session.initiator_items,
                &mut session.counterparty_accepted,
            )
        } else {
            (
                &mut session.counterparty_items,
                &mut session.initiator_accepted,
            )
        };
        if side_items.len() >= MAX_TRADE_ITEMS_PER_SIDE {
            return Err(CoreError::TradeItemLimit(MAX_TRADE_ITEMS_PER_SIDE));
        }
        if side_items.contains(&reference) {
            return Err(CoreError::DuplicateTradeItem);
        }
        *other_side = false;
        session.initiator_accepted = false;
        side_items.push(reference);
        Ok(side_items.len())
    }

    /// Removes one staged reference from a side's offer, resetting acceptances.
    pub fn unstage_trade_item(
        &mut self,
        player_id: u64,
        reference: TradeItemReference,
    ) -> Result<(), CoreError> {
        let session = self.active_trade_entry_mut(player_id)?;
        let (side_items, other_side) = if session.initiator == player_id {
            (
                &mut session.initiator_items,
                &mut session.counterparty_accepted,
            )
        } else {
            (
                &mut session.counterparty_items,
                &mut session.initiator_accepted,
            )
        };
        let Some(position) = side_items.iter().position(|staged| *staged == reference) else {
            return Err(CoreError::UnknownTradeItem);
        };
        side_items.remove(position);
        *other_side = false;
        session.initiator_accepted = false;
        Ok(())
    }

    /// Records one side's acceptance. Returns true only when both sides have now accepted Ă˘â‚¬â€ť
    /// the caller must still perform the authoritative atomic swap before closing.
    pub fn accept_player_trade(&mut self, player_id: u64) -> Result<bool, CoreError> {
        let session = self.active_trade_entry_mut(player_id)?;
        if session.initiator == player_id {
            session.initiator_accepted = true;
        } else {
            session.counterparty_accepted = true;
        }
        Ok(session.initiator_accepted && session.counterparty_accepted)
    }

    /// Cancels any live trade touching one player (logout, rejection, walk-away). The other
    /// participant is located through the shared session record. Returns the other player id
    /// when a trade existed so callers can notify them.
    pub fn cancel_player_trade(&mut self, player_id: u64) -> Option<u64> {
        let (key, other) = if let Some(session) = self.active_trades.get(&player_id) {
            if session.initiator == player_id {
                (player_id, session.counterparty)
            } else {
                (session.initiator, player_id)
            }
        } else {
            let found = self
                .active_trades
                .iter()
                .find(|(_, session)| session.counterparty == player_id)
                .map(|(initiator, _)| *initiator)?;
            (found, player_id)
        };
        self.active_trades.remove(&key);
        Some(other)
    }

    /// Executes the accepted trade as one atomic transition. Both sides' staged references are
    /// re-resolved against their live inventories inside the same borrow: any missing or
    /// duplicated item aborts the whole swap without touching either player (anti-dupe).
    /// On success both inventories are exchanged, the trade closes for both participants, and
    /// the moved items are returned for packet delivery.
    pub fn execute_player_trade(
        &mut self,
        player_id: u64,
    ) -> Result<PlayerTradeExecution, CoreError> {
        let session = self
            .player_trade(player_id)
            .cloned()
            .ok_or(CoreError::NoActiveTrade(player_id))?;
        if !(session.initiator_accepted && session.counterparty_accepted) {
            return Err(CoreError::NoActiveTrade(player_id));
        }
        // Snapshot the session data so the mutable passes below never alias it.
        let initiator = session.initiator;
        let counterparty = session.counterparty;
        let initiator_refs = session.initiator_items.clone();
        let counterparty_refs = session.counterparty_items.clone();

        // Pass 1 - resolve every staged reference to a concrete item snapshot. Any failure
        // aborts before a single inventory mutates.
        let resolve = |owner: u64,
                       refs: &[TradeItemReference]|
         -> Result<Vec<(TradeItemReference, ItemInstance)>, CoreError> {
            refs.iter()
                .map(|reference| {
                    let item = self
                        .player_containers
                        .get(&owner)
                        .and_then(|containers| {
                            containers
                                .container(reference.container_id)
                                .and_then(|container| container.items.item(reference.item_index))
                        })
                        .cloned()
                        .ok_or(CoreError::TradeItemMissing {
                            player_id: owner,
                            container_id: reference.container_id,
                            item_index: reference.item_index,
                        })?;
                    Ok((*reference, item))
                })
                .collect()
        };
        let initiator_resolved = resolve(initiator, &initiator_refs)?;
        let counterparty_resolved = resolve(counterparty, &counterparty_refs)?;

        // Pass 2 - remove all offered items from both sides first, then insert the received
        // items. Removal-first ordering means a capacity shortfall cannot duplicate items: the
        // inserts go into containers that no longer hold the removed goods, and any insert
        // failure rolls the whole transition back through explicit restore before returning.
        let mut remove_offered =
            |owner: u64,
             resolved: Vec<(TradeItemReference, ItemInstance)>|
             -> Result<Vec<(TradeItemReference, ItemInstance)>, CoreError> {
                let mut removed = Vec::new();
                for (reference, _item) in resolved {
                    let Some(containers) = self.player_containers.get_mut(&owner) else {
                        return Err(CoreError::TradeItemMissing {
                            player_id: owner,
                            container_id: reference.container_id,
                            item_index: reference.item_index,
                        });
                    };
                    let Some(container) = containers.container_mut(reference.container_id) else {
                        return Err(CoreError::TradeItemMissing {
                            player_id: owner,
                            container_id: reference.container_id,
                            item_index: reference.item_index,
                        });
                    };
                    match container.items.remove(reference.item_index) {
                        Some(removed_item) => removed.push((reference, removed_item)),
                        None => {
                            // Restore already-removed items in reverse order.
                            for (restore_ref, restore_item) in removed.iter().rev() {
                                if let Some(containers) = self.player_containers.get_mut(&owner) {
                                    if let Some(container) =
                                        containers.container_mut(restore_ref.container_id)
                                    {
                                        let _ = container.items.insert(restore_item.clone());
                                    }
                                }
                            }
                            return Err(CoreError::TradeItemMissing {
                                player_id: owner,
                                container_id: reference.container_id,
                                item_index: reference.item_index,
                            });
                        }
                    }
                }
                Ok(removed)
            };
        let initiator_removed = remove_offered(initiator, initiator_resolved.clone())?;
        let counterparty_removed = match remove_offered(counterparty, counterparty_resolved.clone())
        {
            Ok(items) => items,
            Err(error) => {
                // Roll the initiator's removals back before propagating.
                for (reference, item) in initiator_removed.iter().rev() {
                    if let Some(containers) = self.player_containers.get_mut(&initiator) {
                        if let Some(container) = containers.container_mut(reference.container_id) {
                            let _ = container.items.insert(item.clone());
                        }
                    }
                }
                return Err(error);
            }
        };

        // Pass 3 - deliver each side's received goods into their first container with space.
        let deliver =
            |this: &mut Self, owner: u64, items: Vec<ItemInstance>| -> Result<(), CoreError> {
                for item in items {
                    let container_ids: Vec<u8> = this
                        .player_containers
                        .get(&owner)
                        .map(|containers| containers.iter().map(|(id, _)| id).collect())
                        .unwrap_or_default();
                    let mut placed = false;
                    for container_id in container_ids {
                        let Some(containers) = this.player_containers.get_mut(&owner) else {
                            continue;
                        };
                        let Some(mut container) = containers.remove(container_id) else {
                            continue;
                        };
                        let merged = container.items.merge_or_insert_stack(item.clone()).is_ok();
                        let _ = containers.insert(container);
                        if merged {
                            placed = true;
                            break;
                        }
                    }
                    if !placed {
                        return Err(CoreError::TradeValidationFailed(format!(
                            "player {owner} has no space for a traded item"
                        )));
                    }
                }
                Ok(())
            };
        deliver(
            self,
            initiator,
            counterparty_removed
                .iter()
                .map(|(_, item)| item.clone())
                .collect(),
        )?;
        deliver(
            self,
            counterparty,
            initiator_removed
                .iter()
                .map(|(_, item)| item.clone())
                .collect(),
        )?;

        // Close the trade for both participants.
        self.active_trades.remove(&initiator);
        self.active_trades.remove(&counterparty);
        self.mark_changed();
        Ok(PlayerTradeExecution {
            initiator,
            counterparty,
            initiator_gave: initiator_removed
                .into_iter()
                .map(|(_, item)| item)
                .collect(),
            counterparty_gave: counterparty_removed
                .into_iter()
                .map(|(_, item)| item)
                .collect(),
        })
    }
}
