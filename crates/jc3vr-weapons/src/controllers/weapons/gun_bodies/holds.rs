//! The guns in the hands, read on the weapon handling's input tick: each hand whose gun the rigid
//! grip pinned lately, with its own body, and the hand that braces a holding hand's gun on its
//! foregrip, with where it braces it.

use std::{num::NonZeroUsize, time::Duration};

use glam::Vec3;
use jc3gi::equipment::weapon::{WeaponBase, WeaponData};
use jc3vr_body::controllers::{
    aim::gun,
    hands,
    hands::{CockpitHand, grip, pose::palm_centre_m},
    held::guns::{BraceContact, GunHolds, HeftClass, HeldGun},
};
use jc3vr_input::frame::Hand;

use crate::controllers::weapons::{foregrip, foregrip::GripGeometry, hold::act, regions};

/// Read the guns in the hands: each hand whose gun the rigid grip pinned lately, of one of
/// `weapons` (the wielded weapon and the second weapon, each with its weight class), with no slide
/// moving it, and the hand that braces a holding hand's gun on its foregrip.
///
/// # Safety
///
/// The weapon handling's input tick, on the game thread before the world update, with `weapons`
/// the weapons read live on it.
pub(crate) unsafe fn read_holds(weapons: [Option<(usize, HeftClass)>; 2]) -> GunHolds {
    let grips = grip::status().hands;
    let mut holds = GunHolds::default();
    for hand in Hand::BOTH {
        let Some(placed) = grips[hand.index()] else {
            continue;
        };
        let Some((weapon, class)) = weapons
            .into_iter()
            .flatten()
            .find(|(w, _)| *w == placed.weapon)
        else {
            continue;
        };
        if !placed.pinned
            || placed.at.elapsed() > PINNED_LATELY
            || act::snap_view(weapon, hand).is_some()
        {
            continue;
        }
        // SAFETY: forwarded from the caller; the weapon was read live this tick.
        holds.guns[hand.index()] =
            unsafe { held_gun(weapon, placed.gun, class) }.map(|gun| HeldGun {
                carry: regions::slide_mark(weapon, hand),
                ..gun
            });
    }
    if let Some(hold) = foregrip::posed_hold().filter(|h| h.engaged) {
        let off = hold.holder.other();
        let holding =
            holds.guns[hold.holder.index()].is_some_and(|g| g.weapon.get() == hold.weapon);
        if holding
            && holds.guns[off.index()].is_none()
            && let Some(active) = hands::active_hands_raw()
            && let Some(hand) = active.hands[off.index()]
        {
            holds.contacts[off.index()] =
                brace_contact(&hold.geometry, &hand, palm_centre_m(), active.world_scale);
            holds.braces[off.index()] = holds.contacts[off.index()].is_some();
        }
    }
    holds
}

/// Convert a measured support region and the tracked hand's grip-to-palm displacement into the
/// gun's local contact. The measured gun translation is in cockpit metres; native gun-local
/// coordinates and the physical palm offset are in game units after this conversion.
pub fn brace_contact(
    geometry: &GripGeometry,
    off: &CockpitHand,
    palm_offset_m: Vec3,
    world_scale: f32,
) -> Option<BraceContact> {
    if !(world_scale.is_finite() && world_scale > 0.0) {
        return None;
    }
    let point = geometry.gun.orientation.inverse()
        * (geometry.foregrip - geometry.gun.position)
        * world_scale;
    let palm = off.wrist.position + off.wrist.orientation * palm_offset_m;
    let palm_from_grip = off.wrist.orientation.inverse() * (palm - off.grip.position) * world_scale;
    (point.is_finite() && palm_from_grip.is_finite()).then_some(BraceContact {
        point,
        palm_from_grip,
    })
}

/// How lately the rigid grip must have pinned a gun for it to count as in the hand.
const PINNED_LATELY: Duration = Duration::from_millis(250);

/// The gun `index` of `weapon`, with its own body.
///
/// # Safety
///
/// As [`read_holds`].
unsafe fn held_gun(weapon: usize, index: usize, class: HeftClass) -> Option<HeldGun> {
    let base = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let guns = unsafe { gun::guns(base) }.count();
    let data = unsafe { gun::guns(base) }.nth(index).flatten()?;
    let wrapper = unsafe { data.m_PfxInstance.as_ref() }?;
    Some(HeldGun {
        weapon: NonZeroUsize::new(weapon)?,
        gun: NonZeroUsize::new(data as *const WeaponData as usize)?,
        wrapper: NonZeroUsize::new(data.m_PfxInstance as usize)?,
        body: wrapper.m_BodyId,
        class,
        guns,
        carry: None,
    })
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use jc3vr_input::frame::Pose;

    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn support_contact_uses_the_selected_region_and_scales_once() {
        let geometry = GripGeometry {
            foregrip: Vec3::new(-0.04, -0.06, -0.35),
            gun: pose(Vec3::new(0.02, -0.03, -0.01), Quat::from_rotation_y(0.4)),
            barrel: Vec3::NEG_Z,
            sight: Vec3::Y * 0.08,
            muzzle: Vec3::NEG_Z * 0.7,
            native: false,
        };
        let off = CockpitHand {
            wrist: pose(Vec3::new(-0.2, 0.1, -0.3), Quat::from_rotation_z(0.6)),
            grip: pose(Vec3::new(-0.18, 0.08, -0.32), Quat::IDENTITY),
            aim: Pose::IDENTITY,
        };
        let offset = Vec3::new(0.0, -0.01, -0.07);
        let contact = brace_contact(&geometry, &off, offset, 1.7).unwrap();
        let point = geometry.gun.position + geometry.gun.orientation * contact.point / 1.7;
        assert!(point.abs_diff_eq(geometry.foregrip, 1e-5));
        let palm = off.wrist.position + off.wrist.orientation * offset;
        assert!(
            (off.grip.position + off.wrist.orientation * contact.palm_from_grip / 1.7)
                .abs_diff_eq(palm, 1e-5)
        );
        assert!(brace_contact(&geometry, &off, offset, f32::NAN).is_none());
    }
}
