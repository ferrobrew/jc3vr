//! The reload's model parts at render time: a pistol's slide held back while its chamber is empty
//! and following the hand while it is worked, and a magazine part carried in the hand that took it
//! from the pouch. The input tick publishes what to draw ([`publish`]); the late update of the
//! held gun's parts ([`crate::controllers::hands::parts`]) asks for each part's matrix
//! ([`adjust`]) on the render thread.
//!
//! The game moves a slide itself: its barrel recoil component pushes the part back along its own Z
//! axis by a share of its travel ([`BarrelRecoilWeaponComponent::m_RecoilMaxLength`]) after each
//! shot, and all the way while the magazine is empty. The reload adds the difference between the
//! share it wants and the share the game holds the slide at with an empty magazine.

use glam::{Mat4, Quat, Vec3};
use jc3gi::equipment::weapon::{
    BarrelRecoilWeaponComponent, ModelAttachementWeaponComponent, WeaponComponentType,
};
use parking_lot::Mutex;

use crate::controllers::{frame::Hand, hands::late};

/// A slide's share of its travel, for gun `gun` of the weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Slide {
    pub gun: u32,
    /// The share the reload wants, `0..=1`.
    pub share: f32,
    /// The share the game holds the slide at on its own.
    pub game_share: f32,
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
    pub slides: [Option<Slide>; 2],
    pub held: Option<HeldPart>,
}

pub(super) fn publish(published: Published) {
    *RENDER.lock() = Some(published);
}

pub(super) fn clear() {
    *RENDER.lock() = None;
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
        let slide = published
            .slides
            .into_iter()
            .flatten()
            .find(|s| s.gun == part.m_WeaponDataIndex)?;
        // SAFETY: a barrel recoil part is a barrel recoil component, which extends the
        // model-attachment component the pointer was handed as.
        let recoil = unsafe {
            &*(part as *const ModelAttachementWeaponComponent).cast::<BarrelRecoilWeaponComponent>()
        };
        return slide_matrix(matrix, slide, recoil.m_RecoilMaxLength);
    }
    let held = published.held?;
    if part.m_NameHash != ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH {
        return None;
    }
    let grip = late::render_hands()[held.hand.index()]?.grip.at(dtf);
    Some(held_matrix(grip.position, grip.orientation, held))
}

/// `matrix` moved along its own Z axis by the difference between the slide's wanted and game
/// shares of `travel`.
pub(super) fn slide_matrix(matrix: Mat4, slide: Slide, travel: f32) -> Option<Mat4> {
    let extra = (slide.share - slide.game_share) * travel;
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

static RENDER: Mutex<Option<Published>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slide_moves_by_the_share_the_game_does_not_already_hold() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let back = Slide {
            gun: 0,
            share: 1.0,
            game_share: 0.0,
        };
        let moved = slide_matrix(matrix, back, 0.075).unwrap();
        assert!((moved.w_axis.truncate() - Vec3::new(1.0, 2.0, 3.075)).length() < 1e-6);
        let held_by_game = Slide {
            game_share: 1.0,
            ..back
        };
        assert!(slide_matrix(matrix, held_by_game, 0.075).is_none());
        let working = Slide {
            share: 0.5,
            game_share: 1.0,
            ..back
        };
        let moved = slide_matrix(matrix, working, 0.075).unwrap();
        assert!((moved.w_axis.z - (3.0 - 0.0375)).abs() < 1e-6);
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
