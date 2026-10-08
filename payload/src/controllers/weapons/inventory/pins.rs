//! Which weapons lying in the world the mod keeps from the game's despawn. Pure.
//!
//! The spawn system despawns a dropped weapon by its distance from the player and the view, and
//! skips any object something else holds a strong reference to
//! (`docs/engine/gameplay/weapon-ownership-and-despawn.md`). So a weapon the player let go of is
//! pinned (held by a strong reference) while it is near: within the pin radius, and once pinned,
//! until it is past the release radius, so a weapon at the edge does not flicker. At most a set
//! number are pinned, the nearest first, since a pinned weapon keeps its resources loaded. A
//! weapon left unpinned and far away, or beyond the most the registry keeps, is forgotten: the
//! game's own clean-up takes it from there.

use crate::controllers::weapons::inventory::registry::{Entry, EntryId, Location};

/// How the pins are chosen.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PinSettings {
    /// Whether weapons in the world are pinned at all.
    pub enabled: bool,
    /// A weapon this near the player is pinned, metres.
    pub radius_m: f32,
    /// A pinned weapon stays pinned until it is this far away, metres; below `radius_m` it is
    /// taken as `radius_m`.
    pub release_m: f32,
    /// The most weapons pinned at once.
    pub max_pinned: usize,
    /// An unpinned weapon this far away is forgotten, metres.
    pub forget_m: f32,
    /// The most weapons in the world the registry keeps.
    pub max_tracked: usize,
}

/// The changes to the pins one tick makes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PinPlan {
    pub pin: Vec<EntryId>,
    pub unpin: Vec<EntryId>,
    pub forget: Vec<EntryId>,
}

/// The pins for `entries` under `settings`.
pub fn plan(entries: &[Entry], settings: &PinSettings) -> PinPlan {
    let radius = finite_or(settings.radius_m, 0.0).max(0.0);
    let release = finite_or(settings.release_m, radius).max(radius);
    let mut world: Vec<(EntryId, Option<f32>, bool)> = entries
        .iter()
        .filter_map(|e| match e.location {
            Location::World { pinned } => {
                Some((e.id, e.distance_m.filter(|d| d.is_finite()), pinned))
            }
            _ => None,
        })
        .collect();
    // The nearest first; a weapon whose distance could not be read last.
    world.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    });
    let wants = |distance: Option<f32>, pinned: bool| {
        settings.enabled
            && match distance {
                Some(d) => d <= radius || (pinned && d <= release),
                // Without a reading, a pinned weapon keeps its pin.
                None => pinned,
            }
    };
    let mut plan = PinPlan::default();
    let mut kept = 0;
    let mut tracked = 0;
    for (id, distance, pinned) in world {
        let want = wants(distance, pinned) && kept < settings.max_pinned;
        if want {
            kept += 1;
        }
        match (want, pinned) {
            (true, false) => plan.pin.push(id),
            (false, true) => plan.unpin.push(id),
            _ => {}
        }
        let far = distance.is_some_and(|d| d > finite_or(settings.forget_m, f32::INFINITY));
        if !want && (far || tracked >= settings.max_tracked) {
            plan.forget.push(id);
        } else {
            tracked += 1;
        }
    }
    plan
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use jc3gi::character::inventory::WeaponSlot;

    use super::*;
    use crate::controllers::weapons::inventory::registry::WeaponKind;

    const SETTINGS: PinSettings = PinSettings {
        enabled: true,
        radius_m: 8.0,
        release_m: 12.0,
        max_pinned: 2,
        forget_m: 60.0,
        max_tracked: 4,
    };

    fn world(id: u32, distance_m: Option<f32>, pinned: bool) -> Entry {
        Entry {
            id: EntryId(id),
            object: id as usize * 0x100,
            kind: WeaponKind {
                tag_hash: 1,
                slot: WeaponSlot::E_SLOT_TWO_HANDED,
            },
            magazine: 0,
            location: Location::World { pinned },
            distance_m,
        }
    }

    #[test]
    fn a_near_weapon_is_pinned_and_released_past_the_hysteresis() {
        let p = plan(&[world(1, Some(5.0), false)], &SETTINGS);
        assert_eq!(p.pin, vec![EntryId(1)]);
        // Between the radii a pinned weapon keeps its pin and an unpinned one gets none.
        assert_eq!(
            plan(&[world(1, Some(10.0), true)], &SETTINGS),
            PinPlan::default()
        );
        assert!(
            plan(&[world(1, Some(10.0), false)], &SETTINGS)
                .pin
                .is_empty()
        );
        let p = plan(&[world(1, Some(13.0), true)], &SETTINGS);
        assert_eq!(p.unpin, vec![EntryId(1)]);
        assert!(p.forget.is_empty(), "not far enough to forget");
    }

    #[test]
    fn the_cap_keeps_the_nearest() {
        let entries = [
            world(1, Some(7.0), true),
            world(2, Some(2.0), false),
            world(3, Some(4.0), false),
        ];
        let p = plan(&entries, &SETTINGS);
        assert_eq!(p.pin, vec![EntryId(2), EntryId(3)]);
        assert_eq!(p.unpin, vec![EntryId(1)], "the farthest gives way");
    }

    #[test]
    fn far_or_excess_weapons_are_forgotten() {
        let p = plan(&[world(1, Some(80.0), false)], &SETTINGS);
        assert_eq!(p.forget, vec![EntryId(1)]);
        let many: Vec<Entry> = (1..=6)
            .map(|i| world(i, Some(20.0 + i as f32), false))
            .collect();
        let p = plan(&many, &SETTINGS);
        assert_eq!(p.forget, vec![EntryId(5), EntryId(6)]);
    }

    #[test]
    fn an_unreadable_distance_keeps_what_is_there() {
        assert_eq!(plan(&[world(1, None, true)], &SETTINGS), PinPlan::default());
        assert_eq!(
            plan(&[world(1, None, false)], &SETTINGS),
            PinPlan::default()
        );
        assert_eq!(
            plan(&[world(1, Some(f32::NAN), true)], &SETTINGS),
            PinPlan::default()
        );
    }

    #[test]
    fn switched_off_every_pin_goes() {
        let off = PinSettings {
            enabled: false,
            ..SETTINGS
        };
        let p = plan(&[world(1, Some(1.0), true), world(2, None, true)], &off);
        assert_eq!(p.unpin, vec![EntryId(1), EntryId(2)]);
    }

    #[test]
    fn only_weapons_in_the_world_are_planned() {
        let mut slot = world(1, Some(0.0), false);
        slot.location = Location::MainSlot(WeaponSlot::E_SLOT_TWO_HANDED);
        assert_eq!(plan(&[slot], &SETTINGS), PinPlan::default());
    }
}
