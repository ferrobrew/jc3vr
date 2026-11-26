//! The weapons the loadout table has lying in the world, as the inventory's input tick publishes
//! them for the HUD's markers ([`crate::hud::lost_weapons`]).
//!
//! The input tick publishes the weapons and their places ([`publish`]), and the HUD reads the
//! latest list on the render thread ([`with_fresh`]).

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::weapons::hud_feed::icon::IconId;

/// A weapon the table has lying in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct LostWeapon {
    /// Where it lies, in the world.
    pub position: Vec3,
    pub icon: IconId,
}

/// Publish the weapons lying in the world. Called each input tick of the inventory.
pub(crate) fn publish(weapons: Vec<LostWeapon>) {
    *SAMPLE.lock() = Some((weapons, Instant::now()));
}

/// Run `f` over the weapons published no longer than `stale` before `now`, or return `None` when
/// the latest list is older or none was published. The list stays locked while `f` runs.
pub(crate) fn with_fresh<R>(
    now: Instant,
    stale: Duration,
    f: impl FnOnce(&[LostWeapon]) -> R,
) -> Option<R> {
    let sample = SAMPLE.lock();
    let (weapons, _) = sample
        .as_ref()
        .filter(|(_, at)| now.duration_since(*at) <= stale)?;
    Some(f(weapons))
}

static SAMPLE: Mutex<Option<(Vec<LostWeapon>, Instant)>> = Mutex::new(None);
