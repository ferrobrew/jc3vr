//! Mounted guns and gunner seats: the hands hold the handles, and the handles aim the gun.
//!
//! A mounted gun, standalone or in a vehicle's gunner seat, is the weapon in the heavy slot in its
//! mounted state ([`Character::IsInMountedGunState`]). The game draws the gun toward the character's
//! weapon aim point, and the gun's per-weapon update builds its shot toward the point it is handed,
//! so both are given the point this module derives, the first through the character-level point
//! ([`crate::controllers::aim::character_aim_point`]) and the second through the per-weapon seam
//! ([`crate::controllers::aim::gun`]).
//!
//! The aim follows the hands as handles on a pivot. When the hands take the gun, the reference is
//! taken: the hands' midpoint, the lever from the gun's handles to its pivot (the gun's origin) in
//! the body frame, and the calibration that makes the lever point along the barrel as it is then.
//! From then on the barrel points from the hands' midpoint through the pivot, so pushing the hands
//! left swings the muzzle right, and lowering them raises it. The rendered hands sit on the handles
//! the game places for the mounted state ([`WeaponBase::GetGripPosition`]), keeping the
//! controllers' rotation.
//!
//! Everything is read on the input tick, and the aim point is published for the simulation that
//! follows it.
//!
//! [`WeaponBase::GetGripPosition`]: jc3gi::equipment::weapon::WeaponBase::GetGripPosition

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponGripHand, WeaponState},
    types::math::Matrix4,
};

use crate::controllers::{aim::gun, frame::Hand};

/// The mounted gun the local player mans, as read on the input tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MountedGun {
    pub address: usize,
    /// The gun's origin in the world, which it turns about.
    pub pivot: Vec3,
    /// The barrel's direction in the world, a unit vector.
    pub barrel: Vec3,
    /// The handle for each hand in the world, indexed by [`Hand::index`], where the game places one.
    pub handles: [Option<Vec3>; 2],
}

/// The hands' hold on the gun, taken when they took it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Reference {
    /// The gun it was taken on.
    pub weapon: usize,
    /// Which hands held it: a change re-takes the reference.
    pub hands: [bool; 2],
    /// The hands' midpoint in the cockpit frame.
    pub hands_mid: Vec3,
    /// From the handles to the pivot, in the cockpit frame.
    pub lever: Vec3,
    /// The turn from the lever onto the barrel at the moment the reference was taken.
    pub calibration: Quat,
}

/// Take the reference for hands at `hands_mid` on a gun whose pivot, handle midpoint (if placed),
/// and barrel are given in the cockpit frame. `None` for a degenerate barrel.
pub fn take_reference(
    weapon: usize,
    hands: [bool; 2],
    hands_mid: Vec3,
    pivot: Vec3,
    handles_mid: Option<Vec3>,
    barrel: Vec3,
    min_lever: f32,
) -> Option<Reference> {
    let barrel = barrel.try_normalize()?;
    let min_lever = min_lever.max(0.05);
    let lever = handles_mid
        .map(|h| pivot - h)
        .filter(|l| l.is_finite() && l.length() >= min_lever)
        .unwrap_or(barrel * min_lever);
    let calibration = Quat::from_rotation_arc(lever.try_normalize()?, barrel);
    Some(Reference {
        weapon,
        hands,
        hands_mid,
        lever,
        calibration,
    })
}

/// The barrel direction the hands at `hands_mid` ask for, in the cockpit frame: from the hands
/// through the pivot, turned by the calibration.
pub fn aim_direction(reference: &Reference, hands_mid: Vec3) -> Option<Vec3> {
    let through = reference.hands_mid + reference.lever - hands_mid;
    Some(reference.calibration * through.try_normalize()?)
}

/// The midpoint of the tracked hands' grips, and which hands were tracked.
pub fn hands_midpoint(grips: [Option<Vec3>; 2]) -> Option<(Vec3, [bool; 2])> {
    let tracked = grips.map(|g| g.is_some());
    let points: Vec<Vec3> = grips.into_iter().flatten().collect();
    (!points.is_empty()).then(|| (points.iter().sum::<Vec3>() / points.len() as f32, tracked))
}

/// The aim point published for the per-weapon update.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MountedTarget {
    pub weapon: usize,
    pub point: Vec3,
    pub at: Instant,
}

impl MountedTarget {
    /// Whether the point is from a recent input tick. An older one belongs to a gun the player has
    /// left or a paused game.
    pub fn fresh(&self) -> bool {
        self.at.elapsed() < MAX_AGE
    }
}

const MAX_AGE: Duration = Duration::from_millis(250);

/// Read the mounted gun `owner` mans, if any.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub unsafe fn read(owner: &Character) -> Option<MountedGun> {
    if !unsafe { owner.IsInMountedGunState() } {
        return None;
    }
    let weapon = unsafe {
        owner
            .m_Inventory
            .m_Weapons
            .get(WeaponSlot::E_SLOT_HEAVY as usize)?
            .px
            .as_ref()?
    };
    let owned = std::ptr::eq(weapon.m_User[0].cast::<Character>(), owner);
    if !owned || weapon.m_State != WeaponState::E_WS_MOUNTED {
        return None;
    }
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let world = Mat4::from(first.m_WorldMatrixT1);
    let pivot = world.w_axis.truncate();
    let barrel = (-world.z_axis.truncate()).try_normalize()?;
    let handles = Hand::BOTH.map(|hand| unsafe { handle(weapon, hand) });
    pivot.is_finite().then_some(MountedGun {
        address: weapon as *const WeaponBase as usize,
        pivot,
        barrel,
        handles,
    })
}

/// The gun's handle for `hand` in the world, where the weapon's grip table places one for the
/// mounted state.
unsafe fn handle(weapon: &WeaponBase, hand: Hand) -> Option<Vec3> {
    let grip_hand = match hand {
        Hand::Left => WeaponGripHand::E_WEAPON_GRIP_HAND_LEFT,
        Hand::Right => WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT,
    };
    let mut grip = Matrix4::from(Mat4::IDENTITY);
    let placed = unsafe { weapon.GetGripPosition(grip_hand, &mut grip) };
    let point = Mat4::from(grip).w_axis.truncate();
    (placed && point.is_finite()).then_some(point)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    fn reference() -> Reference {
        // Handles 0.5 m behind and 0.2 m below a pivot; the barrel level and forward.
        take_reference(
            1,
            [true; 2],
            Vec3::new(0.0, -0.4, -0.3),
            Vec3::new(0.0, -0.2, -0.8),
            Some(Vec3::new(0.0, -0.4, -0.3)),
            Vec3::NEG_Z,
            0.4,
        )
        .unwrap()
    }

    #[test]
    fn the_gun_keeps_its_aim_when_the_hands_take_it() {
        let r = reference();
        assert!(close(aim_direction(&r, r.hands_mid).unwrap(), Vec3::NEG_Z));
    }

    #[test]
    fn pushing_the_handles_left_swings_the_muzzle_right_and_lowering_them_raises_it() {
        let r = reference();
        let left = aim_direction(&r, r.hands_mid + Vec3::new(-0.1, 0.0, 0.0)).unwrap();
        assert!(left.x > 0.1, "{left}");
        let lowered = aim_direction(&r, r.hands_mid + Vec3::new(0.0, -0.1, 0.0)).unwrap();
        assert!(lowered.y > 0.1, "{lowered}");
    }

    #[test]
    fn a_gun_without_handles_pivots_ahead_of_the_hands() {
        let r = take_reference(
            1,
            [true, false],
            Vec3::ZERO,
            Vec3::ZERO,
            None,
            Vec3::NEG_Z,
            0.4,
        )
        .unwrap();
        assert!(close(r.lever, Vec3::new(0.0, 0.0, -0.4)));
        assert_eq!(
            take_reference(1, [true; 2], Vec3::ZERO, Vec3::ZERO, None, Vec3::ZERO, 0.4),
            None
        );
    }

    #[test]
    fn the_midpoint_is_of_the_tracked_hands() {
        let (mid, tracked) = hands_midpoint([
            Some(Vec3::new(-1.0, 0.0, 0.0)),
            Some(Vec3::new(1.0, 2.0, 0.0)),
        ])
        .unwrap();
        assert!(close(mid, Vec3::new(0.0, 1.0, 0.0)));
        assert_eq!(tracked, [true; 2]);
        let (mid, tracked) = hands_midpoint([None, Some(Vec3::X)]).unwrap();
        assert!(close(mid, Vec3::X));
        assert_eq!(tracked, [false, true]);
        assert_eq!(hands_midpoint([None; 2]), None);
    }
}
