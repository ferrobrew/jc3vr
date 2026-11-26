//! Stepping into a vehicle's empty gunner seat: a player standing where the gunner of a vehicle's
//! mounted gun stands, behind its handles, is lowered into the seat as if entering the vehicle.
//!
//! The game enters a gunner seat from the vehicle-entry action held past its hold time while the
//! player's interaction node leads to one
//! ([`NStateTask_InputVehicleEnterTask::Update`]). Once the player has stood behind an unmanned
//! vehicle gun's handles for [`DWELL`], with the game offering the gunner seat
//! ([`NStateTask_InputVehicleEnterTask::HasAvailableGunnerDestination`]), the action is held until
//! the player mans the gun or [`HOLD_TIMEOUT`] runs out. The decision is pure ([`decide`]); the
//! game is read in [`step`].
//!
//! [`NStateTask_InputVehicleEnterTask::Update`]: jc3gi::vehicle::vehicle::NStateTask_InputVehicleEnterTask::Update
//! [`NStateTask_InputVehicleEnterTask::HasAvailableGunnerDestination`]: jc3gi::vehicle::vehicle::NStateTask_InputVehicleEnterTask::HasAvailableGunnerDestination

use std::{num::NonZeroUsize, time::Instant};

use glam::{Vec2, Vec3};
use jc3gi::{
    character::character::Character, equipment::weapon::WeaponState,
    input::input_action_map::Action, vehicle::vehicle::NStateTask_InputVehicleEnterTask,
};
use jc3vr_body::controllers::{
    held::native::weapon_at,
    intent::hold::{Desired, MapKind, Target, Want},
    physics::{bodies::physics_object_of, refs::Weak},
};
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::doors::engine::vehicle_of;
use parking_lot::Mutex;

use crate::controllers::vehicles::{
    mounted::{handle, pivot_and_barrel},
    tuning::VEHICLES,
};

/// Where the gunner of an unmanned vehicle gun stands: behind the handles' midpoint, along the
/// barrel's level direction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct GunnerSpot {
    pub(super) weapon: usize,
    pub(super) handles_mid: Vec3,
    /// The barrel's direction on the level, a unit vector.
    pub(super) forward: Vec2,
}

impl GunnerSpot {
    /// Whether a head at `head` stands at the spot: within the tuning's `gunner_seat.spot_radius_m`
    /// of the point `gunner_seat.spot_behind_m` behind the handles on the level, and not below
    /// them.
    pub(super) fn holds(&self, head: Vec3) -> bool {
        let tuning = &VEHICLES.get().gunner_seat;
        let spot =
            Vec2::new(self.handles_mid.x, self.handles_mid.z) - self.forward * tuning.spot_behind_m;
        let flat = Vec2::new(head.x, head.z);
        head.is_finite()
            && flat.distance(spot) <= tuning.spot_radius_m
            && head.y >= self.handles_mid.y - tuning.spot_below_m
    }
}

/// What the step found this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Observation {
    /// The unmanned vehicle gun whose spot the head stands at, if any.
    pub(super) spot: Option<usize>,
    /// Whether the game offers the gunner seat from where the player is.
    pub(super) offered: bool,
    /// Whether the player already mans a mounted gun.
    pub(super) manning: bool,
}

/// The entry's progress.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) enum Entry {
    #[default]
    Idle,
    /// The head has stood at a gun's spot since `since`.
    Standing { weapon: usize, since: Instant },
    /// The entry action is held for the gun since `since`.
    Holding { weapon: usize, since: Instant },
    /// The hold ended; nothing starts again until the head leaves the spot.
    Spent { weapon: usize },
}

/// What the entry does on a tick: whether the entry action is held, and what to log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Change {
    None,
    Started,
    Seated,
    TimedOut,
}

/// Advance `entry` by one tick's observation at `now`, and say whether the entry action is held.
pub(super) fn decide(entry: &mut Entry, seen: Observation, now: Instant) -> (bool, Change) {
    match *entry {
        Entry::Holding { weapon, since } => {
            if seen.manning {
                *entry = Entry::Spent { weapon };
                return (false, Change::Seated);
            }
            if now.duration_since(since) >= VEHICLES.get().gunner_seat.hold_timeout() {
                *entry = Entry::Spent { weapon };
                return (false, Change::TimedOut);
            }
            (true, Change::None)
        }
        Entry::Spent { weapon } => {
            if seen.spot != Some(weapon) {
                *entry = Entry::Idle;
            }
            (false, Change::None)
        }
        Entry::Standing { weapon, since } if seen.spot == Some(weapon) && !seen.manning => {
            if seen.offered && now.duration_since(since) >= VEHICLES.get().gunner_seat.dwell() {
                *entry = Entry::Holding { weapon, since: now };
                return (true, Change::Started);
            }
            (false, Change::None)
        }
        Entry::Idle | Entry::Standing { .. } => {
            *entry = match seen.spot.filter(|_| !seen.manning) {
                Some(weapon) => Entry::Standing { weapon, since: now },
                None => Entry::Idle,
            };
            (false, Change::None)
        }
    }
}

/// Look for the unmanned vehicle gun whose gunner spot the head at `head` stands at, among `seen`,
/// and hold the entry action while the player steps into its seat.
///
/// # Safety
///
/// The game thread, before the world update; `local` is the live local player.
pub(super) unsafe fn step(
    local: &Character,
    head: Option<Vec3>,
    seen: &[(usize, Weak)],
    desired: &mut Vec<Desired>,
) {
    let manning = unsafe { local.IsInMountedGunState() };
    let spot = head.filter(|_| !manning).and_then(|head| {
        seen.iter()
            .filter_map(|(object, weak)| unsafe { unmanned_vehicle_gun(*object, weak) })
            .find(|spot| spot.holds(head))
            .map(|spot| spot.weapon)
    });
    let offered = spot.is_some()
        && unsafe {
            NStateTask_InputVehicleEnterTask::HasAvailableGunnerDestination(
                &local.m_InteractionUserProxy,
            )
        };
    let now = Instant::now();
    let mut entry = ENTRY.lock();
    let (hold, change) = decide(
        &mut entry,
        Observation {
            spot,
            offered,
            manning,
        },
        now,
    );
    drop(entry);
    match change {
        Change::None => {}
        Change::Started => tracing::info!(
            target: "controllers",
            "gunner seat: the player stands at an empty gunner seat; entering it"
        ),
        Change::Seated => tracing::info!(
            target: "controllers",
            "gunner seat: the player mans the gun from its gunner seat"
        ),
        Change::TimedOut => tracing::info!(
            target: "controllers",
            timeout_s = VEHICLES.get().gunner_seat.hold_timeout().as_secs_f32(),
            "gunner seat: the entry did not seat the player"
        ),
    }
    if hold {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: Action::ENTER_VEHICLE,
            },
            want: Want::Hold(1.0),
        });
    }
}

/// Forget an entry in progress, for a paused game, a mode change, or a shutdown.
pub(super) fn reset() {
    *ENTRY.lock() = Entry::Idle;
}

/// The gunner spot of the mounted gun at `object` when it is on a vehicle, has no user, and has
/// both handles.
///
/// # Safety
///
/// The game thread; `weak` is the weapon's own weak pointer.
unsafe fn unmanned_vehicle_gun(object: usize, weak: &Weak) -> Option<GunnerSpot> {
    let alive = weak
        .lock()
        .filter(|alive| alive.object() as usize == object)?;
    let weapon_ptr = weapon_at(alive.object());
    let weapon = unsafe { &*weapon_ptr };
    if weapon.m_State != WeaponState::E_WS_MOUNTED || !weapon.m_User[0].is_null() {
        return None;
    }
    if weapon.m_Holder.pn.is_null() {
        return None;
    }
    let holder = unsafe { Weak::of(&weapon.m_Holder) }.lock()?;
    let holder = unsafe { physics_object_of(NonZeroUsize::new(holder.object() as usize)?) }?;
    unsafe { vehicle_of(holder) }?;
    let (_, barrel) = unsafe { pivot_and_barrel(weapon) }?;
    let [left, right] = Hand::BOTH.map(|hand| unsafe { handle(weapon, hand) });
    let handles_mid = (left? + right?) * 0.5;
    let forward = Vec2::new(barrel.x, barrel.z).try_normalize()?;
    handles_mid.is_finite().then_some(GunnerSpot {
        weapon: weapon_ptr as usize,
        handles_mid,
        forward,
    })
}

static ENTRY: Mutex<Entry> = Mutex::new(Entry::Idle);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::vehicles::tuning::GunnerSeatTuning;

    fn seat() -> &'static GunnerSeatTuning {
        &VEHICLES.get().gunner_seat
    }

    fn seen(spot: Option<usize>, offered: bool, manning: bool) -> Observation {
        Observation {
            spot,
            offered,
            manning,
        }
    }

    #[test]
    fn standing_at_the_spot_holds_the_entry_after_the_dwell_until_seated() {
        let start = Instant::now();
        let mut entry = Entry::Idle;
        assert_eq!(
            decide(&mut entry, seen(Some(1), true, false), start),
            (false, Change::None)
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), true, false),
                start + seat().dwell() / 2
            ),
            (false, Change::None)
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), true, false),
                start + seat().dwell()
            ),
            (true, Change::Started)
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(None, false, false),
                start + seat().dwell() * 2
            ),
            (true, Change::None)
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(None, false, true),
                start + seat().dwell() * 3
            ),
            (false, Change::Seated)
        );
    }

    #[test]
    fn walking_across_or_without_the_offer_enters_nothing() {
        let start = Instant::now();
        let mut entry = Entry::Idle;
        decide(&mut entry, seen(Some(1), true, false), start);
        decide(
            &mut entry,
            seen(None, false, false),
            start + seat().dwell() / 2,
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), true, false),
                start + seat().dwell()
            ),
            (false, Change::None)
        );
        let mut entry = Entry::Idle;
        decide(&mut entry, seen(Some(1), false, false), start);
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), false, false),
                start + seat().dwell() * 2
            ),
            (false, Change::None)
        );
    }

    #[test]
    fn a_failed_entry_waits_for_the_head_to_leave_the_spot() {
        let start = Instant::now();
        let mut entry = Entry::Holding {
            weapon: 1,
            since: start,
        };
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), true, false),
                start + seat().hold_timeout()
            ),
            (false, Change::TimedOut)
        );
        assert_eq!(
            decide(
                &mut entry,
                seen(Some(1), true, false),
                start + seat().hold_timeout() * 3
            ),
            (false, Change::None)
        );
        decide(
            &mut entry,
            seen(None, true, false),
            start + seat().hold_timeout() * 4,
        );
        assert_eq!(entry, Entry::Idle);
    }

    #[test]
    fn the_spot_is_behind_the_handles() {
        let spot = GunnerSpot {
            weapon: 1,
            handles_mid: Vec3::new(0.0, 2.0, 0.0),
            forward: Vec2::new(0.0, -1.0),
        };
        assert!(spot.holds(Vec3::new(0.0, 2.6, seat().spot_behind_m)));
        assert!(spot.holds(Vec3::new(0.0, 3.4, seat().spot_behind_m + 0.3)));
        assert!(!spot.holds(Vec3::new(0.0, 2.6, -seat().spot_behind_m)));
        assert!(!spot.holds(Vec3::new(0.0, 1.5, seat().spot_behind_m)));
        assert!(!spot.holds(Vec3::new(0.6, 2.6, seat().spot_behind_m)));
    }
}
