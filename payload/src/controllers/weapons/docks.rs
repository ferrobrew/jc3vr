//! Holster volumes that follow the guns docked on the body.
//!
//! The holsters are placed from the body's landmarks and the configured offsets
//! ([`crate::controllers::weapons::volumes`]), but a docked gun sits where its dock bone puts it on
//! the character's skeleton: a rifle across the back, a pistol on the thigh, wherever the animation
//! and the procedural body have that bone. So a holster whose slot holds a docked gun is moved to
//! the gun: by the gun's displacement from the same holster placed on the character's own skeleton
//! ([`follow`]). On the character's skeleton that puts the volume on the gun itself; on the player's
//! own body it moves the volume as far, and the same way, as the gun is from its usual place, so
//! the player reaches where the gun is seen. A holster with no gun docked in it (its weapon in the
//! hands, or none) stays where it is placed, since a held gun is holstered there.
//!
//! The guns' points are read on the input tick from their last placement, in the world, and carried
//! into the cockpit frame the volumes live in ([`cockpit_frames`]).

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponGripHand, WeaponState},
    types::math::Matrix4,
};

use crate::{
    controllers::{
        aim::gun,
        bindings::grab::{Candidate, GrabVolume, HolsterSite},
        body::alignment::measure::Frames,
        frame::Hand,
        hands::ActiveHands,
        weapons::{
            hold::{layout, second},
            holster::site_slot,
        },
    },
    headpose,
};

/// A gun docked on the body, and the holster it docks in.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DockedGun {
    pub site: HolsterSite,
    /// Where a hand closes on the gun: its grip, in the frame the volumes are placed in.
    pub point: Vec3,
}

/// Move each holster in `candidates` that has a gun in `guns` by the gun's displacement from the
/// same holster placed on the character's skeleton (`on_character`). A displacement longer than
/// `max_shift_m`, which no dock gives, is taken for a bad read and ignored. Returns how many
/// holsters moved.
pub fn follow(
    candidates: &mut [Candidate],
    on_character: &[Candidate],
    guns: &[DockedGun],
    max_shift_m: f32,
) -> usize {
    let mut moved = 0;
    for gun in guns {
        let volume = GrabVolume::Holster(gun.site);
        let Some(usual) = on_character.iter().find(|c| c.volume == volume) else {
            continue;
        };
        let shift = gun.point - usual.centre;
        if !shift.is_finite() || shift.length() > max_shift_m.max(0.0) {
            continue;
        }
        for candidate in candidates.iter_mut().filter(|c| c.volume == volume) {
            candidate.centre += shift;
            moved += 1;
        }
    }
    moved
}

/// The longest move [`follow`] makes, metres.
pub const MAX_SHIFT_M: f32 = 0.6;

/// The transforms between the world and the cockpit frame on this input tick, with `character`
/// the character's model-to-world transform.
pub(crate) fn cockpit_frames(active: &ActiveHands, character: Mat4) -> Option<Frames> {
    let frames = Frames {
        head_world: headpose::query().position,
        head_cockpit: active.head.position,
        view: headpose::xr::body_rotation(),
        world_scale: active.world_scale,
        character,
    };
    frames.is_usable().then_some(frames)
}

/// The guns docked on `owner`'s body, with the holsters they dock in (the shoulders by `dominant`),
/// at their points in the world: a long gun on its shoulder in the holstered state (not the second
/// weapon, which is in a hand), and each pistol on a hip holster bone, holstered or waiting there
/// while the other is drawn.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn docked_guns(owner: &Character, dominant: Hand) -> Vec<(HolsterSite, Vec3)> {
    let mut out = Vec::new();
    for side in Hand::BOTH {
        let site = HolsterSite::Shoulder(side);
        let Some(weapon) = (unsafe { slot_weapon(owner, site_slot(site, dominant)) }) else {
            continue;
        };
        let address = weapon as *const WeaponBase as usize;
        if weapon.m_State != WeaponState::E_WS_HOLSTERED || second::is(address) {
            continue;
        }
        let Some(first) = unsafe { gun::guns(weapon) }.next().flatten() else {
            continue;
        };
        let on_dock = first
            .m_lBoneAttachementPerWeaponState
            .get(WeaponState::E_WS_HOLSTERED as usize)
            .is_some_and(|&bone| layout::dock_bone(bone).is_some());
        if !on_dock {
            continue;
        }
        let mut grip = Matrix4::from(Mat4::IDENTITY);
        let point = if unsafe {
            weapon.GetGripPosition(WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT, &mut grip)
        } {
            Mat4::from(grip).w_axis.truncate()
        } else {
            Mat4::from(first.m_WorldMatrixT1).w_axis.truncate()
        };
        if point.is_finite() {
            out.push((site, point));
        }
    }
    if let Some(pair) = unsafe { slot_weapon(owner, WeaponSlot::E_SLOT_DUAL_WIELD) } {
        for gun in unsafe { gun::guns(pair) }.flatten() {
            let Some(&bone) = gun
                .m_lBoneAttachementPerWeaponState
                .get(pair.m_State as usize)
            else {
                continue;
            };
            let Some(side) = Hand::BOTH
                .into_iter()
                .find(|&h| layout::hip_bone(h) == bone)
            else {
                continue;
            };
            let point = Mat4::from(gun.m_WorldMatrixT1).w_axis.truncate();
            if point.is_finite() {
                out.push((HolsterSite::Hip(side), point));
            }
        }
    }
    out
}

/// The weapon in `owner`'s `slot`.
unsafe fn slot_weapon(owner: &Character, slot: WeaponSlot) -> Option<&WeaponBase> {
    unsafe { owner.m_Inventory.m_Weapons.get(slot as usize)?.px.as_ref() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holster(site: HolsterSite, centre: Vec3) -> Candidate {
        Candidate {
            volume: GrabVolume::Holster(site),
            centre,
            radius: 0.1,
            hand: None,
        }
    }

    const BACK: HolsterSite = HolsterSite::Shoulder(Hand::Right);
    const HIP: HolsterSite = HolsterSite::Hip(Hand::Left);

    #[test]
    fn a_holster_moves_to_its_docked_gun() {
        let usual = [holster(BACK, Vec3::new(0.2, 1.4, 0.1))];
        // On the character's skeleton the volume lands on the gun.
        let mut on_skeleton = usual;
        let gun = DockedGun {
            site: BACK,
            point: Vec3::new(0.1, 1.3, 0.25),
        };
        assert_eq!(follow(&mut on_skeleton, &usual, &[gun], MAX_SHIFT_M), 1);
        assert!(on_skeleton[0].centre.distance(gun.point) < 1e-6);
        // On the player's body it moves as far and the same way.
        let mut on_body = [holster(BACK, Vec3::new(0.25, 1.5, 0.05))];
        follow(&mut on_body, &usual, &[gun], MAX_SHIFT_M);
        assert!(on_body[0].centre.distance(Vec3::new(0.15, 1.4, 0.2)) < 1e-6);
    }

    #[test]
    fn a_holster_without_a_gun_stays() {
        let usual = [
            holster(BACK, Vec3::new(0.2, 1.4, 0.1)),
            holster(HIP, Vec3::new(-0.2, 0.9, 0.0)),
        ];
        let mut placed = usual;
        let gun = DockedGun {
            site: HIP,
            point: Vec3::new(-0.25, 0.85, 0.05),
        };
        assert_eq!(follow(&mut placed, &usual, &[gun], MAX_SHIFT_M), 1);
        assert_eq!(placed[0], usual[0]);
        assert!(placed[1].centre.distance(gun.point) < 1e-6);
    }

    #[test]
    fn a_bad_read_moves_nothing() {
        let usual = [holster(BACK, Vec3::new(0.2, 1.4, 0.1))];
        let mut placed = usual;
        let far = DockedGun {
            site: BACK,
            point: Vec3::new(5.0, 1.4, 0.1),
        };
        let bad = DockedGun {
            site: BACK,
            point: Vec3::NAN,
        };
        assert_eq!(follow(&mut placed, &usual, &[far, bad], MAX_SHIFT_M), 0);
        assert_eq!(placed, usual);
        // A gun whose holster is not placed on the character moves nothing either.
        assert_eq!(
            follow(
                &mut placed,
                &[],
                &[DockedGun {
                    site: BACK,
                    point: Vec3::ZERO
                }],
                MAX_SHIFT_M
            ),
            0
        );
    }
}
