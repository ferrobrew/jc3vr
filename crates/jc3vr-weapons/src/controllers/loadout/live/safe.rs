//! Checked actor-free moves against the live loadout table.
//!
//! Mutations publish hand occupancy immediately. Model allocation, native activation, and resource
//! cleanup remain with the grenade controller, outside the live table's lock.

use jc3vr_input::frame::Hand;

use crate::controllers::loadout::{
    ItemId,
    live::{LIVE, Live},
    safe_items::{ItemLeaseId, SafeItemError, SafeItemView},
    table::Place,
};

pub fn admit_safe_item(model: ItemLeaseId, hand: Hand) -> Result<SafeItemView, SafeItemError> {
    let mut live = LIVE.lock();
    let result = require_available(&live).and_then(|()| live.table.admit_safe_item(model, hand));
    live.record_safe("admitted to", result, true)
}

pub fn admit_safe_item_to_world(model: ItemLeaseId) -> Result<SafeItemView, SafeItemError> {
    let mut live = LIVE.lock();
    let result = require_available(&live).and_then(|()| live.table.admit_safe_item_to_world(model));
    live.record_safe("admitted to", result, true)
}

pub fn move_safe_item(
    expected: SafeItemView,
    destination: Place,
) -> Result<SafeItemView, SafeItemError> {
    let mut live = LIVE.lock();
    let result =
        require_available(&live).and_then(|()| live.table.move_safe_item(expected, destination));
    live.record_safe("moved to", result, destination != expected.place)
}

/// Retire an item even during a reset so the resource owner can complete cleanup.
pub fn retire_safe_item(expected: SafeItemView) -> Result<ItemLeaseId, SafeItemError> {
    let mut live = LIVE.lock();
    let result = live.table.retire_safe_item(expected);
    match result {
        Ok(lease) => {
            live.transitions += 1;
            live.push(format!("loadout: grenade {} retired", expected.item));
            live.publish_places();
            Ok(lease)
        }
        Err(error) => Err(live.refuse_safe(error)),
    }
}

pub fn safe_item(id: ItemId) -> Option<SafeItemView> {
    LIVE.lock().table.safe_item(id)
}

pub fn safe_items() -> Vec<SafeItemView> {
    LIVE.lock().table.safe_items().collect()
}

impl Live {
    /// Record `result`, a safe item's admission or move: a move that changed its place (`moved`)
    /// counts as a transition and is logged with `verb` and the new place, and a refusal is logged
    /// and counted as the table's folds are.
    fn record_safe(
        &mut self,
        verb: &str,
        result: Result<SafeItemView, SafeItemError>,
        moved: bool,
    ) -> Result<SafeItemView, SafeItemError> {
        match result {
            Ok(item) => {
                if moved {
                    self.transitions += 1;
                    self.push(format!(
                        "loadout: grenade {} {verb} {}",
                        item.item, item.place
                    ));
                }
                self.publish_places();
                Ok(item)
            }
            Err(error) => Err(self.refuse_safe(error)),
        }
    }

    fn refuse_safe(&mut self, error: SafeItemError) -> SafeItemError {
        tracing::info!(target: "controllers", %error, "loadout: a safe item move was refused");
        self.refusals += 1;
        self.push(format!("refused: {error}"));
        error
    }
}

fn require_available(live: &Live) -> Result<(), SafeItemError> {
    if !live.started {
        return Err(SafeItemError::NotRunning);
    }
    if live.reset.began || live.reset.completed {
        return Err(SafeItemError::Resetting);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_and_moves_wait_for_a_running_settled_table() {
        let mut live = Live::new();
        assert_eq!(require_available(&live), Err(SafeItemError::NotRunning));
        live.started = true;
        assert_eq!(require_available(&live), Ok(()));
        live.reset.began = true;
        assert_eq!(require_available(&live), Err(SafeItemError::Resetting));
        live.reset.began = false;
        live.reset.completed = true;
        assert_eq!(require_available(&live), Err(SafeItemError::Resetting));
    }
}
