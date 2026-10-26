//! Checked actor-free moves against the live loadout table.
//!
//! Mutations publish hand occupancy immediately. Model allocation, native activation, and resource
//! cleanup remain with the grenade controller, outside the live table's lock.

use crate::controllers::{
    frame::Hand,
    loadout::{
        ItemId,
        live::{LIVE, Live},
        safe_items::{ModelLeaseId, SafeItemError, SafeItemView},
        table::Place,
    },
};

pub(crate) fn admit_safe_item(
    model: ModelLeaseId,
    hand: Hand,
) -> Result<SafeItemView, SafeItemError> {
    let mut live = LIVE.lock();
    require_available(&live)?;
    let result = live.table.admit_safe_item(model, hand);
    match &result {
        Ok(_) => {
            live.transitions += 1;
            live.publish_places();
        }
        Err(_) => live.refusals += 1,
    }
    result
}

pub(crate) fn move_safe_item(
    expected: SafeItemView,
    destination: Place,
) -> Result<SafeItemView, SafeItemError> {
    let mut live = LIVE.lock();
    require_available(&live)?;
    let result = live.table.move_safe_item(expected, destination);
    match &result {
        Ok(_) => {
            live.transitions += u64::from(destination != expected.place);
            live.publish_places();
        }
        Err(_) => live.refusals += 1,
    }
    result
}

/// Retire an item even during a reset so the resource owner can complete cleanup.
pub(crate) fn retire_safe_item(expected: SafeItemView) -> Result<ModelLeaseId, SafeItemError> {
    let mut live = LIVE.lock();
    let result = live.table.retire_safe_item(expected);
    match &result {
        Ok(_) => {
            live.transitions += 1;
            live.publish_places();
        }
        Err(_) => live.refusals += 1,
    }
    result
}

pub(crate) fn safe_item(id: ItemId) -> Option<SafeItemView> {
    LIVE.lock().table.safe_item(id)
}

pub(crate) fn safe_items() -> Vec<SafeItemView> {
    LIVE.lock().table.safe_items().collect()
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
