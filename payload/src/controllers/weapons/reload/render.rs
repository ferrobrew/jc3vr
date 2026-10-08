//! The reload's model parts: an action's part where its travel has it, and a magazine part carried
//! in the hand that took it from the pouch. The input tick publishes what to draw ([`publish`]).
//! The late update of the held gun's parts ([`crate::controllers::hands::parts`]) asks for each
//! separate part's matrix ([`adjust`]) on the render thread, and the gun's own skeleton takes the
//! pump or the bolt bone from [`bone_part`] ([`crate::controllers::weapons::reload::bones`]).
//!
//! The game moves a separate slide itself: its barrel recoil component pushes the part back along
//! its own Z axis by a share of its travel ([`BarrelRecoilWeaponComponent::m_RecoilMaxLength`])
//! after each shot, and with [`BarrelRecoilWeaponComponent::m_MaxRecoilOnEmptyAmmo`] all the way
//! while the magazine is empty. The reload adds the difference between the share it wants and the
//! share the game holds the slide at with an empty magazine, so the game's own kick after a shot
//! still shows.

use std::sync::atomic::{AtomicUsize, Ordering};

use glam::{Mat4, Quat, Vec3};
use jc3gi::equipment::weapon::{
    BarrelRecoilWeaponComponent, ModelAttachementWeaponComponent, WeaponComponentType,
};
use parking_lot::Mutex;

use crate::controllers::{frame::Hand, hands::late, weapons::reload::chamber::ActionKind};

/// Gun `gun`'s action part where the reload has it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ActionPart {
    /// The gun's index in the weapon, which a separate part names as its own.
    pub gun: u32,
    /// The gun's address, whose skeleton carries a pump or a bolt bone.
    pub gun_address: usize,
    pub kind: ActionKind,
    /// How far back the part is, as a share of its travel, `0..=1`.
    pub share: f32,
    /// How far a bolt is turned up, as a share of its lift, `0..=1`.
    pub lift: f32,
    /// The part's travel in the gun's model frame, metres.
    pub travel_m: f32,
    /// A bolt's turn up at full lift, radians, signed so that it turns the handle's side toward
    /// the gun's top about the model's +Z.
    pub lift_rad: f32,
}

/// A magazine part carried in a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct HeldPart {
    pub hand: Hand,
    /// How far ahead of the grip the top of the held round is, in the world's metres.
    pub top_world_m: f32,
    /// How far the part's top is from its origin along its own +Y, in the model's metres.
    pub part_top_m: f32,
}

/// What the input tick publishes for the render thread.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct Published {
    pub weapon: usize,
    pub actions: [Option<ActionPart>; 2],
    /// The game's magazine is empty, which holds a slide back on its own.
    pub magazine_empty: bool,
    pub held: Option<HeldPart>,
}

pub(super) fn publish(published: Published) {
    *RENDER.lock() = Some(published);
    PUBLISHED_WEAPON.store(published.weapon, Ordering::Relaxed);
}

pub(super) fn clear() {
    *RENDER.lock() = None;
    PUBLISHED_WEAPON.store(0, Ordering::Relaxed);
}

/// The action part of the gun at `gun_address` of the weapon at `weapon`, for its skeleton. Every
/// character's guns ask, on the simulation's workers, so any weapon but the published one is
/// turned away before the lock.
pub(super) fn bone_part(weapon: usize, gun_address: usize) -> Option<ActionPart> {
    if weapon == 0 || PUBLISHED_WEAPON.load(Ordering::Relaxed) != weapon {
        return None;
    }
    let published = (*RENDER.lock()).filter(|p| p.weapon == weapon)?;
    published
        .actions
        .into_iter()
        .flatten()
        .find(|a| a.gun_address == gun_address)
}

/// The matrix to push for `part` of the weapon at `weapon` in place of `matrix` (the engine's
/// interpolated matrix with its hand's correction), at the render interpolation fraction `dtf`.
/// `None` leaves the part as it is.
pub(crate) fn adjust(
    part: &ModelAttachementWeaponComponent,
    weapon: usize,
    matrix: Mat4,
    dtf: f32,
) -> Option<Mat4> {
    let published = (*RENDER.lock()).filter(|p| p.weapon == weapon)?;
    if part.m_Type == WeaponComponentType::E_WEAPON_COMPONENT_BARREL_RECOIL {
        let action = published
            .actions
            .into_iter()
            .flatten()
            .find(|a| a.gun == part.m_WeaponDataIndex)?;
        if !matches!(action.kind, ActionKind::Slide | ActionKind::Bolt) {
            return None;
        }
        // SAFETY: a barrel recoil part is a barrel recoil component, which extends the
        // model-attachment component the pointer was handed as.
        let recoil = unsafe {
            &*(part as *const ModelAttachementWeaponComponent).cast::<BarrelRecoilWeaponComponent>()
        };
        let game_share = if recoil.m_MaxRecoilOnEmptyAmmo && published.magazine_empty {
            1.0
        } else {
            0.0
        };
        return slide_matrix(matrix, action.share, game_share, recoil.m_RecoilMaxLength);
    }
    let held = published.held?;
    if part.m_NameHash != ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH {
        return None;
    }
    let grip = late::render_hands()[held.hand.index()]?.grip.at(dtf);
    Some(held_matrix(grip.position, grip.orientation, held))
}

/// `matrix` moved along its own Z axis by the difference between the slide's wanted share and the
/// share the game holds it at, of `travel`.
pub(super) fn slide_matrix(matrix: Mat4, share: f32, game_share: f32, travel: f32) -> Option<Mat4> {
    let extra = (share - game_share) * travel;
    let axis = matrix.z_axis.truncate().try_normalize()?;
    if !extra.is_finite() || extra == 0.0 {
        return None;
    }
    let mut moved = matrix;
    moved.w_axis += (axis * extra).extend(0.0);
    Some(moved)
}

/// The magazine part carried by a hand whose grip is at `position` and `orientation` (world): the
/// part's +Y along the way the closed hand points, its top the held round's top.
pub(super) fn held_matrix(position: Vec3, orientation: Quat, held: HeldPart) -> Mat4 {
    let pointing = orientation * Vec3::NEG_Z;
    let rotation = orientation * Quat::from_rotation_arc(Vec3::Y, Vec3::NEG_Z);
    let top = position + pointing * held.top_world_m;
    Mat4::from_rotation_translation(rotation, top - rotation * (Vec3::Y * held.part_top_m))
}

/// A bone's model-space joint (`position`, `orientation`) moved by `part`: back along the model's
/// +Z by its share of the travel, and turned about the model's Z through the bone by a bolt's
/// lift.
pub(super) fn bone_joint(position: Vec3, orientation: Quat, part: ActionPart) -> (Vec3, Quat) {
    let back = Vec3::Z * (part.share.clamp(0.0, 1.0) * part.travel_m);
    let turn = Quat::from_rotation_z(part.lift.clamp(0.0, 1.0) * part.lift_rad);
    (position + back, (turn * orientation).normalize())
}

static RENDER: Mutex<Option<Published>> = Mutex::new(None);
/// The weapon [`RENDER`] holds, or zero, for the skeletons' check before the lock.
static PUBLISHED_WEAPON: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slide_moves_by_the_share_the_game_does_not_already_hold() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let moved = slide_matrix(matrix, 1.0, 0.0, 0.075).unwrap();
        assert!((moved.w_axis.truncate() - Vec3::new(1.0, 2.0, 3.075)).length() < 1e-6);
        assert!(slide_matrix(matrix, 1.0, 1.0, 0.075).is_none());
        let moved = slide_matrix(matrix, 0.5, 1.0, 0.075).unwrap();
        assert!((moved.w_axis.z - (3.0 - 0.0375)).abs() < 1e-6);
    }

    #[test]
    fn a_bone_moves_back_by_its_share_and_turns_by_its_lift() {
        let part = ActionPart {
            gun: 0,
            gun_address: 1,
            kind: ActionKind::Bolt,
            share: 0.5,
            lift: 1.0,
            travel_m: 0.08,
            lift_rad: std::f32::consts::FRAC_PI_2,
        };
        let (position, orientation) = bone_joint(Vec3::new(0.0, 0.1, -0.2), Quat::IDENTITY, part);
        assert!((position - Vec3::new(0.0, 0.1, -0.16)).length() < 1e-6);
        assert!((orientation * Vec3::X - Vec3::Y).length() < 1e-5);
        let left = ActionPart {
            lift_rad: -std::f32::consts::FRAC_PI_2,
            share: 0.0,
            ..part
        };
        let (position, orientation) = bone_joint(Vec3::ZERO, Quat::IDENTITY, left);
        assert_eq!(position, Vec3::ZERO);
        assert!((orientation * Vec3::NEG_X - Vec3::Y).length() < 1e-5);
    }

    #[test]
    fn a_held_magazine_points_the_way_the_hand_does_with_its_top_ahead() {
        let held = HeldPart {
            hand: Hand::Left,
            top_world_m: 0.07,
            part_top_m: 0.05,
        };
        let m = held_matrix(Vec3::ZERO, Quat::IDENTITY, held);
        let up = m.transform_vector3(Vec3::Y);
        assert!((up - Vec3::NEG_Z).length() < 1e-6);
        let top = m.transform_point3(Vec3::Y * 0.05);
        assert!((top - Vec3::new(0.0, 0.0, -0.07)).length() < 1e-6);
    }
}
