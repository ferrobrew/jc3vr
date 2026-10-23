//! Holster volumes on the guns docked on the body.
//!
//! The holsters are placed from the body's landmarks and the configured offsets
//! ([`crate::controllers::weapons::volumes`]), but a docked gun sits where its dock bone puts it on
//! the character's skeleton: a rifle across the back, a pistol on the thigh, wherever the animation
//! and the procedural body have that bone. So each hip and shoulder holster is seated on its gun
//! ([`seat`]), and reached by the hand as it is drawn, so the player reaches for the gun they see,
//! wherever it moves:
//!
//! - a holster whose gun is docked sits on the gun's grip;
//! - a holster whose gun is out of it (in a hand) sits where the gun docks again: its dock bone,
//!   carried out to where the gun's grip was on that bone when it was last seen docked;
//! - a holster with no gun in its slot is not offered, since there is nothing on the body there;
//! - a holster whose gun's place cannot be read stays on the landmarks.
//!
//! The chest holster carries the planted explosive, which shows nothing on the body, so it stays on
//! the landmarks.
//!
//! The guns' points are read on the input tick from their last placement, in the world, and carried
//! into the cockpit frame the volumes live in ([`cockpit_frames`]).

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponData, WeaponGripHand, WeaponState},
    types::math::Matrix4,
};
use parking_lot::Mutex;

use crate::{
    controllers::{
        aim::gun,
        body::rig::frames::Frames,
        frame::Hand,
        grab::{Candidate, GrabVolume},
        hands::{ActiveHands, grip},
        loadout::Socket,
        weapons::{
            hold::{armed, layout},
            holster::site_slot,
        },
    },
    headpose,
};

/// A holster's gun, and where a hand closes on it: its grip, docked or where it docks, in the
/// frame the volumes are placed in. `None` when that place cannot be read.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HolsterGun {
    pub site: Socket,
    pub point: Option<Vec3>,
}

/// Seat the hip and shoulder holsters in `candidates` on their guns in `guns`: each holster whose
/// gun has a point is moved there and out of `candidates` into the returned list, each whose gun
/// has none stays, and each with no gun in `guns` is removed. The chest holster and every other
/// volume stay.
pub fn seat(candidates: &mut Vec<Candidate>, guns: &[HolsterGun]) -> Vec<Candidate> {
    let mut seated = Vec::new();
    candidates.retain(|c| {
        let GrabVolume::Holster(site) = c.volume else {
            return true;
        };
        if site == Socket::Chest {
            return true;
        }
        match guns.iter().find(|g| g.site == site) {
            None => false,
            Some(HolsterGun { point: None, .. }) => true,
            Some(HolsterGun {
                point: Some(point), ..
            }) => {
                seated.push(Candidate {
                    centre: *point,
                    ..*c
                });
                false
            }
        }
    });
    seated
}

/// Where a grip at `grip` in the world is on a bone at `bone` (bone-to-world), in the bone's frame;
/// `None` for an offset longer than [`MAX_GRIP_OFFSET_M`], which no dock gives, or a bad read.
pub fn grip_on_bone(bone: Mat4, grip: Vec3) -> Option<Vec3> {
    let local = bone.inverse().transform_point3(grip);
    (local.is_finite() && local.length() <= MAX_GRIP_OFFSET_M).then_some(local)
}

/// The longest grip offset from a dock bone, metres.
pub const MAX_GRIP_OFFSET_M: f32 = 0.6;

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

/// The guns of `owner`'s holsters, at their grips in the world: each
/// shoulder's long gun, and each pistol of the pistol slot on the hip it holsters on. A gun on its
/// dock bone is at its grip there (a long gun's grip, [`WeaponBase::GetGripPosition`], or its
/// origin without one; a pistol's origin); a gun off it is where it would be on the bone, by where
/// its grip was there when it was last seen docked, or at the bone without that. A gun that docks on
/// no dock bone has no point.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn holster_guns(owner: &Character) -> Vec<(Socket, Option<Vec3>)> {
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    let mut grips = GRIPS.lock();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for side in Hand::BOTH {
        let site = Socket::Shoulder(side);
        let Some(weapon) = site_slot(site).and_then(|slot| unsafe { slot_weapon(owner, slot) })
        else {
            continue;
        };
        let Some(first) = unsafe { gun::guns(weapon) }.next().flatten() else {
            continue;
        };
        // The second weapon is in a hand, whatever its state says.
        let docked = weapon.m_State == WeaponState::E_WS_HOLSTERED
            && !armed::is(weapon as *const WeaponBase as usize);
        let at = || {
            let mut grip = Matrix4::from(Mat4::IDENTITY);
            if unsafe {
                weapon.GetGripPosition(WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT, &mut grip)
            } {
                Mat4::from(grip).w_axis.truncate()
            } else {
                gun_origin(first)
            }
        };
        let point = unsafe { place(owner, to_world, first, docked, at, &mut grips) };
        seen.push(first as *const WeaponData as usize);
        out.push((site, point));
    }
    if let Some(pair) = unsafe { slot_weapon(owner, WeaponSlot::E_SLOT_DUAL_WIELD) } {
        for gun in unsafe { gun::guns(pair) }.flatten() {
            let Some(side) = holstered_bone(gun).and_then(|bone| {
                Hand::BOTH
                    .into_iter()
                    .find(|&h| layout::hip_bone(h) == bone)
            }) else {
                continue;
            };
            // A pistol waits on its hip while the other of the pair is drawn.
            let docked = gun
                .m_lBoneAttachementPerWeaponState
                .get(pair.m_State as usize)
                .is_some_and(|&bone| bone == layout::hip_bone(side));
            let point =
                unsafe { place(owner, to_world, gun, docked, || gun_origin(gun), &mut grips) };
            seen.push(gun as *const WeaponData as usize);
            out.push((Socket::Hip(side), point));
        }
    }
    grips.retain(|g| seen.contains(&g.gun));
    out
}

/// A docked gun's grip on its dock bone, as last seen.
#[derive(Debug, Copy, Clone)]
struct DockGrip {
    gun: usize,
    bone: u32,
    local: Vec3,
}

/// The guns' grips on their dock bones, for the guns in the holsters' slots.
static GRIPS: Mutex<Vec<DockGrip>> = Mutex::new(Vec::new());

/// The weapon in `owner`'s `slot`.
unsafe fn slot_weapon(owner: &Character, slot: WeaponSlot) -> Option<&WeaponBase> {
    unsafe { owner.m_Inventory.m_Weapons.get(slot as usize)?.px.as_ref() }
}

/// The gun's origin in the world, from its last placement.
fn gun_origin(gun: &WeaponData) -> Vec3 {
    Mat4::from(gun.m_WorldMatrixT1).w_axis.truncate()
}

/// The dock bone `gun` rides when holstered.
fn holstered_bone(gun: &WeaponData) -> Option<u32> {
    gun.m_lBoneAttachementPerWeaponState
        .get(WeaponState::E_WS_HOLSTERED as usize)
        .copied()
        .filter(|&bone| layout::dock_bone(bone).is_some())
}

/// Where a hand closes on `gun` in its holster: at `grip()` while it is `docked`, noting where that
/// is on its dock bone in `grips`, and otherwise where the noted grip is on the bone now.
unsafe fn place(
    owner: &Character,
    to_world: Mat4,
    gun: &WeaponData,
    docked: bool,
    grip: impl FnOnce() -> Vec3,
    grips: &mut Vec<DockGrip>,
) -> Option<Vec3> {
    let bone = holstered_bone(gun)?;
    let safe = layout::dock_bone(bone)?;
    let pose = unsafe { grip::bone_model_pose(owner, safe) }?;
    let bone_world = to_world * Mat4::from_rotation_translation(pose.orientation, pose.position);
    let key = gun as *const WeaponData as usize;
    if docked {
        let point = grip();
        if !point.is_finite() {
            return None;
        }
        if let Some(local) = grip_on_bone(bone_world, point) {
            grips.retain(|g| g.gun != key);
            grips.push(DockGrip {
                gun: key,
                bone,
                local,
            });
        }
        return Some(point);
    }
    let local = grips
        .iter()
        .find(|g| g.gun == key && g.bone == bone)
        .map_or(Vec3::ZERO, |g| g.local);
    let point = bone_world.transform_point3(local);
    point.is_finite().then_some(point)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn volume(volume: GrabVolume, centre: Vec3) -> Candidate {
        Candidate {
            volume,
            centre,
            radius: 0.1,
            hand: None,
        }
    }

    const BACK: Socket = Socket::Shoulder(Hand::Right);
    const HIP: Socket = Socket::Hip(Hand::Left);

    fn placed() -> Vec<Candidate> {
        vec![
            volume(GrabVolume::Holster(BACK), Vec3::new(0.2, 1.4, 0.1)),
            volume(GrabVolume::Holster(HIP), Vec3::new(-0.2, 0.9, 0.0)),
            volume(
                GrabVolume::Holster(Socket::Chest),
                Vec3::new(0.0, 1.2, -0.1),
            ),
        ]
    }

    #[test]
    fn a_holster_sits_on_its_gun() {
        let mut candidates = placed();
        let gun = Vec3::new(0.1, 1.3, 0.25);
        let seated = seat(
            &mut candidates,
            &[
                HolsterGun {
                    site: BACK,
                    point: Some(gun),
                },
                HolsterGun {
                    site: HIP,
                    point: None,
                },
            ],
        );
        assert_eq!(seated.len(), 1);
        assert_eq!(seated[0].volume, GrabVolume::Holster(BACK));
        assert_eq!(seated[0].centre, gun);
        assert_eq!(seated[0].radius, 0.1);
        // The hip's gun could not be placed, so its holster stays on the landmarks with the chest's.
        assert_eq!(candidates, placed()[1..]);
    }

    #[test]
    fn a_holster_without_a_gun_is_not_offered() {
        let mut candidates = placed();
        let seated = seat(&mut candidates, &[]);
        assert!(seated.is_empty());
        assert_eq!(
            candidates.iter().map(|c| c.volume).collect::<Vec<_>>(),
            [GrabVolume::Holster(Socket::Chest)]
        );
    }

    #[test]
    fn a_grip_noted_on_its_bone_follows_the_bone() {
        let bone =
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(0.3, 1.2, -0.4));
        let grip = Vec3::new(0.45, 1.1, -0.3);
        let local = grip_on_bone(bone, grip).expect("a grip near its bone");
        assert!(bone.transform_point3(local).distance(grip) < 1e-5);
        // The bone moves and turns; the grip moves with it.
        let moved =
            Mat4::from_rotation_translation(Quat::from_rotation_y(-0.4), Vec3::new(1.0, 1.0, 1.0));
        let expected = moved.transform_point3(bone.inverse().transform_point3(grip));
        assert!(moved.transform_point3(local).distance(expected) < 1e-5);
    }

    #[test]
    fn a_far_grip_is_a_bad_read() {
        assert_eq!(grip_on_bone(Mat4::IDENTITY, Vec3::new(5.0, 0.0, 0.0)), None);
        assert_eq!(grip_on_bone(Mat4::IDENTITY, Vec3::NAN), None);
    }
}
