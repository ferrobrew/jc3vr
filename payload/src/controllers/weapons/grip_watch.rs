//! A watch on how each held gun sits in its hand, for diagnosing a gun that turns over in the hand.
//!
//! A gun rides its hand's attach bone, so with the wrist placed by the controller the gun's rotation
//! relative to the wrist target should hold still from one tick to the next. On every input tick
//! the watch measures each hand-held gun's last placement relative to the wrist target it was
//! placed from, for a target reached at full weight, and compares it with the previous tick's. A
//! change past [`FLIP_DEG`] counts as a flip and is logged with the hand tier state, so that a gun
//! seen turning over in the headset can be traced to the gun's attachment (the change shows here)
//! or to the hand's pose (it does not).

use glam::{Mat4, Quat};
use jc3gi::equipment::weapon::WeaponBase;
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::{ik, late, tiers},
};

/// The watch's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GripWatchStatus {
    /// The change in each hand's gun-in-wrist rotation over the last tick, degrees, indexed by
    /// [`Hand::index`].
    pub change_deg: [Option<f32>; 2],
    /// Flips seen per hand since the payload loaded.
    pub flips: [u64; 2],
    /// The last flip: its hand and its size, degrees.
    pub last_flip: Option<(Hand, f32)>,
}

pub fn status() -> GripWatchStatus {
    STATE.lock().status
}

/// A tick-to-tick change in the gun's rotation in the wrist frame larger than this is a flip,
/// degrees. The hand's own motion between the placement and the target is a few degrees.
pub const FLIP_DEG: f32 = 90.0;

/// The change between two gun-in-wrist rotations, degrees.
pub fn change_deg(previous: Quat, current: Quat) -> f32 {
    let delta = current * previous.inverse();
    let delta = if delta.w < 0.0 { -delta } else { delta };
    delta.to_axis_angle().1.to_degrees()
}

/// Measure `weapon`'s hand-held guns against this tick's wrist targets.
///
/// # Safety
///
/// `weapon` must be live, read on the main thread outside the world update.
pub(super) unsafe fn observe(weapon: &WeaponBase) {
    const FULL_WEIGHT: f32 = 0.999;
    let targets = ik::tick_records().current;
    let mut seen = [None; 2];
    for (index, gun) in unsafe { gun::guns(weapon) }.enumerate() {
        let Some(gun) = gun else {
            continue;
        };
        let Some(&bone) = gun
            .m_lBoneAttachementPerWeaponState
            .get(weapon.m_State as usize)
        else {
            continue;
        };
        let Some(hand) = late::attach_hand(bone) else {
            continue;
        };
        let Some(target) = targets[hand.index()].filter(|t| t.weight >= FULL_WEIGHT) else {
            continue;
        };
        let (_, rotation, _) = Mat4::from(gun.m_WorldMatrixT1).to_scale_rotation_translation();
        let in_wrist = (target.wrist.orientation.inverse() * rotation).normalize();
        if in_wrist.is_finite() && seen[hand.index()].is_none() {
            seen[hand.index()] = Some(Sample {
                weapon: weapon as *const WeaponBase as usize,
                gun: index,
                bone,
                in_wrist,
            });
        }
    }
    let mut state = STATE.lock();
    for hand in Hand::BOTH {
        let now = seen[hand.index()];
        let previous = state.previous[hand.index()];
        let change = match (previous, now) {
            (Some(p), Some(n)) if p.same_gun(&n) => Some(change_deg(p.in_wrist, n.in_wrist)),
            _ => None,
        };
        state.status.change_deg[hand.index()] = change;
        if let Some(change) = change.filter(|&c| c > FLIP_DEG) {
            state.status.flips[hand.index()] += 1;
            state.status.last_flip = Some((hand, change));
            tracing::warn!(
                target: "controllers",
                ?hand,
                change_deg = change,
                tier_state = tiers::status().state.map_or("none", |s| s.label()),
                "a held gun turned over in its hand between two ticks"
            );
        }
        state.previous[hand.index()] = now;
    }
}

/// Forget the last tick's measurements, for when the weapon handling stands down.
pub(super) fn clear() {
    STATE.lock().previous = [None; 2];
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Sample {
    weapon: usize,
    gun: usize,
    bone: u32,
    in_wrist: Quat,
}

impl Sample {
    fn same_gun(&self, other: &Sample) -> bool {
        self.weapon == other.weapon && self.gun == other.gun && self.bone == other.bone
    }
}

struct State {
    previous: [Option<Sample>; 2],
    status: GripWatchStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    previous: [None; 2],
    status: GripWatchStatus {
        change_deg: [None; 2],
        flips: [0; 2],
        last_flip: None,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_grip_changes_little_and_a_flip_a_lot() {
        let held = Quat::from_rotation_y(0.3);
        assert!(change_deg(held, Quat::from_rotation_y(0.35)) < 5.0);
        let flipped = Quat::from_rotation_z(std::f32::consts::PI) * held;
        assert!(change_deg(held, flipped) > FLIP_DEG);
        // The same rotation with the opposite sign is no change.
        assert!(change_deg(held, -held) < 1e-3);
    }
}
