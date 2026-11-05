//! The reload's model parts: each hand's gun's action part where its travel has it, and the round
//! (a magazine, a cartridge, a pod, or a rocket) carried in the hand that took it from the pouch.
//! The input tick publishes what to draw ([`publish`]), each part with the weapon it belongs to.
//! The late update of the held gun's parts ([`crate::controllers::hands::parts`]) asks for each
//! separate part's matrix ([`adjust`]) on the render thread, and the gun's own skeleton takes the
//! pump or the bolt bone from [`bone_part`] ([`crate::controllers::weapons::reload::bones`]).
//!
//! A magazine in the hand is an instance of the gun's spare magazine model
//! ([`crate::controllers::weapons::pouch_models::hand_tick`]), placed each frame at the matrix
//! [`held_round_matrix`] gives, so the gun keeps its own magazine part until that magazine leaves
//! it. A round with no such model (the UPU-210's cartridge, a launcher's rocket or pod, or a
//! magazine whose gun has no spare magazine model) is the gun's own round part, moved into the
//! hand at the same matrix.
//!
//! The game moves a separate slide itself: its barrel recoil component pushes the part back along
//! its own Z axis by a share of its travel ([`BarrelRecoilWeaponComponent::m_RecoilMaxLength`])
//! after each shot, and with [`BarrelRecoilWeaponComponent::m_MaxRecoilOnEmptyAmmo`] all the way
//! while the magazine is empty. The reload adds the difference between the share it wants and the
//! share the game holds the slide at with an empty magazine, so the game's own kick after a shot
//! still shows.

use std::sync::atomic::{AtomicUsize, Ordering};

use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::equipment::weapon::{
    BarrelRecoilWeaponComponent, ModelAttachementWeaponComponent, WeaponComponentType,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::{held::away_from_palm, late},
    weapons::reload::chamber::ActionKind,
};

/// Gun `gun`'s action part where the reload has it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ActionPart {
    /// The address of the weapon the gun belongs to.
    pub weapon: usize,
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
    /// The weapon's magazine is empty, which holds a slide back on its own.
    pub magazine_empty: bool,
}

/// A round part carried in a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct HeldPart {
    /// The address of the weapon the held round is for.
    pub weapon: usize,
    pub hand: Hand,
    /// How far ahead of the grip the top of the held round is, in the world's metres.
    pub top_world_m: f32,
    /// How far out of the palm the held round's centre line is, in the world's metres.
    pub palm_gap_world_m: f32,
    /// The top of the round's model, centred across it, in the model's own frame and metres.
    pub part_top: Vec3,
    /// The direction the round travels into the gun, in the model's own frame, unit length.
    pub part_axis: Vec3,
    /// What draws the round.
    pub drawn: HeldDraw,
    /// The captured round in its gun hand's wrist frame, in world metres: its top, and the gun's
    /// orientation, which the part keeps while it slides along the well.
    pub captured: Option<(Hand, Pose)>,
}

/// What draws a round in the hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum HeldDraw {
    /// An instance of the gun's spare magazine model, which the gun's part leaves alone.
    Instance,
    /// The gun's own round part, moved into the hand.
    Part,
}

/// What the input tick publishes for the render thread.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct Published {
    /// The weapons the parts may belong to, or zero, for the check before the lock.
    pub weapons: [usize; 2],
    /// Each hand's gun's action part, by [`Hand::index`].
    pub actions: [Option<ActionPart>; 2],
    pub held: Option<HeldPart>,
}

pub(super) fn publish(published: Published) {
    *RENDER.lock() = Some(published);
    for (stored, weapon) in PUBLISHED_WEAPONS.iter().zip(published.weapons) {
        stored.store(weapon, Ordering::Relaxed);
    }
}

pub(super) fn clear() {
    *RENDER.lock() = None;
    for stored in &PUBLISHED_WEAPONS {
        stored.store(0, Ordering::Relaxed);
    }
}

/// The action part of the gun at `gun_address` of the weapon at `weapon`, for its skeleton. Every
/// character's guns ask, on the simulation's workers, so any weapon but the published ones is
/// turned away before the lock.
pub(super) fn bone_part(weapon: usize, gun_address: usize) -> Option<ActionPart> {
    if !published(weapon) {
        return None;
    }
    (*RENDER.lock())?
        .actions
        .into_iter()
        .flatten()
        .find(|a| a.weapon == weapon && a.gun_address == gun_address)
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
    if !published(weapon) {
        return None;
    }
    let published = (*RENDER.lock())?;
    if part.m_Type == WeaponComponentType::E_WEAPON_COMPONENT_BARREL_RECOIL {
        let action = published
            .actions
            .into_iter()
            .flatten()
            .find(|a| a.weapon == weapon && a.gun == part.m_WeaponDataIndex)?;
        if !matches!(action.kind, ActionKind::Slide | ActionKind::Bolt) {
            return None;
        }
        // SAFETY: a barrel recoil part is a barrel recoil component, which extends the
        // model-attachment component the pointer was handed as.
        let recoil = unsafe {
            &*(part as *const ModelAttachementWeaponComponent).cast::<BarrelRecoilWeaponComponent>()
        };
        let game_share = if recoil.m_MaxRecoilOnEmptyAmmo && action.magazine_empty {
            1.0
        } else {
            0.0
        };
        return slide_matrix(matrix, action.share, game_share, recoil.m_RecoilMaxLength);
    }
    let held = published
        .held
        .filter(|h| h.weapon == weapon && h.drawn == HeldDraw::Part)?;
    if !is_round_part(part.m_NameHash) {
        return None;
    }
    round_matrix(held, dtf)
}

/// The world matrix of the round in the hand for the weapon at `weapon`, drawn by an instance of
/// its magazine model, at the render interpolation fraction `dtf`, or `None` while there is none
/// or the hand is not drawn.
pub(super) fn held_round_matrix(weapon: usize, dtf: f32) -> Option<Mat4> {
    if !published(weapon) {
        return None;
    }
    let held = (*RENDER.lock())?
        .held
        .filter(|h| h.weapon == weapon && h.drawn == HeldDraw::Instance)?;
    round_matrix(held, dtf)
}

/// Where `held` is drawn at `dtf`: on the drawn hand's grip, or once the well caught it, along
/// the well in the drawn gun hand's wrist frame.
fn round_matrix(held: HeldPart, dtf: f32) -> Option<Mat4> {
    let drawn = late::drawn_hands(dtf);
    if let Some((hand, local)) = held.captured {
        let wrist = drawn[hand.index()]?.wrist;
        let top = wrist.position + wrist.orientation * local.position;
        let rotation = (wrist.orientation * local.orientation).normalize();
        return Some(Mat4::from_rotation_translation(
            rotation,
            top - rotation * held.part_top,
        ));
    }
    let grip = drawn[held.hand.index()]?.grip;
    Some(held_matrix(grip, held))
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

/// Whether a model part named `name_hash` is the round a reload by hand carries: a gun's magazine,
/// the UPU-210's full cartridge, the Hydra's pod, or a launcher's rocket or the mortar's shell.
pub(super) fn is_round_part(name_hash: u32) -> bool {
    matches!(
        name_hash,
        ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH
            | ModelAttachementWeaponComponent::ROCKET_POD_NAME_HASH
            | ModelAttachementWeaponComponent::CARTRIDGE_FULL_NAME_HASH
            | ModelAttachementWeaponComponent::RPG_NAME_HASH
    )
}

/// The round carried by a hand whose grip is `grip` (world): held as a magazine is for loading,
/// its insertion axis up the finger tube (the way the closed hand points), its front (the gun
/// model's -Z, square to the axis) out of the palm with its back against it, its centre line
/// `palm_gap_world_m` out of the palm and its top `top_world_m` ahead of the grip.
pub(super) fn held_matrix(grip: Pose, held: HeldPart) -> Mat4 {
    let axis = held.part_axis.try_normalize().unwrap_or(Vec3::Y);
    // A part that goes in along the model's Z (a rocket) has no front square to it; its top faces
    // out of the palm instead.
    let square = |v: Vec3| (v - axis * axis.dot(v)).try_normalize();
    let front = square(Vec3::NEG_Z)
        .or_else(|| square(Vec3::Y))
        .unwrap_or(Vec3::X);
    let away = away_from_palm(held.hand);
    let part = Mat3::from_cols(front, axis, front.cross(axis));
    let in_grip = Mat3::from_cols(away, Vec3::NEG_Z, away.cross(Vec3::NEG_Z));
    let local = Quat::from_mat3(&(in_grip * part.transpose())).normalize();
    let rotation = (grip.orientation * local).normalize();
    let top = grip.position
        + grip.orientation * (away * held.palm_gap_world_m + Vec3::NEG_Z * held.top_world_m);
    Mat4::from_rotation_translation(rotation, top - rotation * held.part_top)
}

/// A bone's model-space joint (`position`, `orientation`) moved by `part`: back along the model's
/// +Z by its share of the travel, and turned about the model's Z through the bone by a bolt's
/// lift.
pub(super) fn bone_joint(position: Vec3, orientation: Quat, part: ActionPart) -> (Vec3, Quat) {
    let back = Vec3::Z * (part.share.clamp(0.0, 1.0) * part.travel_m);
    let turn = Quat::from_rotation_z(part.lift.clamp(0.0, 1.0) * part.lift_rad);
    (position + back, (turn * orientation).normalize())
}

/// Whether `weapon` is one [`RENDER`] may hold parts of, checked without the lock.
fn published(weapon: usize) -> bool {
    weapon != 0
        && PUBLISHED_WEAPONS
            .iter()
            .any(|w| w.load(Ordering::Relaxed) == weapon)
}

static RENDER: Mutex<Option<Published>> = Mutex::new(None);
/// The weapons [`RENDER`] holds parts of, or zero, for the checks before the lock.
static PUBLISHED_WEAPONS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

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
            weapon: 1,
            gun: 0,
            gun_address: 1,
            kind: ActionKind::Bolt,
            share: 0.5,
            lift: 1.0,
            travel_m: 0.08,
            lift_rad: std::f32::consts::FRAC_PI_2,
            magazine_empty: false,
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
        for hand in Hand::BOTH {
            let held = HeldPart {
                weapon: 1,
                hand,
                top_world_m: 0.07,
                palm_gap_world_m: 0.03,
                part_top: Vec3::Y * 0.05,
                part_axis: Vec3::Y,
                drawn: HeldDraw::Part,
                captured: None,
            };
            let m = held_matrix(Pose::default(), held);
            let up = m.transform_vector3(Vec3::Y);
            assert!((up - Vec3::NEG_Z).length() < 1e-6);
            let away = away_from_palm(hand);
            // The magazine's front, the gun's -Z, faces out of the palm, its back against it.
            assert!((m.transform_vector3(Vec3::NEG_Z) - away).length() < 1e-6);
            let top = m.transform_point3(Vec3::Y * 0.05);
            assert!((top - (away * 0.03 + Vec3::new(0.0, 0.0, -0.07))).length() < 1e-6);
        }
    }

    #[test]
    fn a_magazine_model_off_its_origin_puts_its_own_top_ahead_of_the_grip() {
        // A pistol's spare magazine model is drawn in the gun's frame, its top in the grip, away
        // from the model's origin.
        let top = Vec3::new(0.0, 0.056, -0.025);
        let held = HeldPart {
            weapon: 1,
            hand: Hand::Left,
            top_world_m: 0.07,
            palm_gap_world_m: 0.03,
            part_top: top,
            part_axis: Vec3::Y,
            drawn: HeldDraw::Instance,
            captured: None,
        };
        let grip = Pose {
            position: Vec3::new(0.3, 1.2, -0.4),
            orientation: Quat::from_rotation_y(-0.8),
        };
        let m = held_matrix(grip, held);
        let away = away_from_palm(Hand::Left);
        let expected = grip.position + grip.orientation * (away * 0.03 + Vec3::NEG_Z * 0.07);
        assert!((m.transform_point3(top) - expected).length() < 1e-5);
    }

    #[test]
    fn a_held_rocket_points_its_feed_end_out_of_the_hand() {
        // A rocket goes into the tube along the model's +Z, its rear end first.
        let held = HeldPart {
            weapon: 1,
            hand: Hand::Right,
            top_world_m: 0.07,
            palm_gap_world_m: 0.0,
            part_top: Vec3::Z * 0.35,
            part_axis: Vec3::Z,
            drawn: HeldDraw::Part,
            captured: None,
        };
        let grip = Pose {
            position: Vec3::ONE,
            orientation: Quat::from_rotation_y(0.6),
        };
        let m = held_matrix(grip, held);
        let pointing = grip.orientation * Vec3::NEG_Z;
        assert!((m.transform_vector3(Vec3::Z) - pointing).length() < 1e-5);
        let top = m.transform_point3(Vec3::Z * 0.35);
        assert!((top - (Vec3::ONE + pointing * 0.07)).length() < 1e-5);
    }

    #[test]
    fn every_round_part_the_reload_shows_is_one_it_draws_in_the_hand() {
        for hash in [
            ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH,
            ModelAttachementWeaponComponent::CARTRIDGE_FULL_NAME_HASH,
            ModelAttachementWeaponComponent::ROCKET_POD_NAME_HASH,
            ModelAttachementWeaponComponent::RPG_NAME_HASH,
        ] {
            assert!(is_round_part(hash));
        }
        assert!(!is_round_part(
            ModelAttachementWeaponComponent::CARTRIDGE_EMPTY_NAME_HASH
        ));
    }
}
