//! The render-time placement of the explosives a hand holds: the primed grenade, a caught grenade,
//! and a primed C4 charge.
//!
//! The throws place a held explosive at the hand's grip on the sim tick, by writing its cached
//! world matrix, and the engine draws it from its own cached matrices in its render update. That
//! draws it where the tick put the hand, a tick or two behind the hand the late update draws
//! ([`crate::controllers::hands::late`]), and a caught grenade, which its physics carries toward
//! the hand by velocity, further behind still. So each throw registers what its hand holds here
//! ([`hold`]), and a post-call detour on the explosive's render update re-pushes the model matrix
//! from the grip of the hand as it is drawn ([`late::drawn_hands`]) at the frame's interpolation
//! fraction, so it stays in the hand where the hand falls short of the controller:
//! the grenade's model for [`Explosive::UpdateRender`], and the charge's in-hand model for
//! [`PlantedExplosive::UpdateRender`].
//!
//! The registrations are refreshed every tick the object is held and lapse on their own a little
//! past a tick, so an explosive that left the hand without its throw clearing the slot is drawn by
//! the engine again within a few frames.
//!
//! Where a held model sits in the hand is a [`HoldPlacement`]: its origin and its axes in the grip
//! frame, given for the right hand and mirrored for the left. The grip's origin is the centre of
//! the closed hand, about where Rico's drawn palm lies, so a model placed at it with the grip's own
//! orientation sinks into the palm; each placement moves its model out of the palm
//! ([`away_from_palm`]) and turns it the way a hand holds it.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::{
    equipment::explosive::{Explosive, PlantedExplosive},
    game::GameObjectRenderContext,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::late,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&EXPLOSIVE_UPDATE_RENDER_BINDER)
        .with_static_binder(&PLANTED_EXPLOSIVE_UPDATE_RENDER_BINDER)
}

/// Who holds an explosive in a hand. Each owns one registration.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Slot {
    /// The grenade primed from the pouch.
    Grenade,
    /// A live grenade a hand caught.
    CaughtGrenade,
    /// The C4 charge primed from the pouch, drawn as its in-hand model.
    Charge,
}

impl Slot {
    const COUNT: usize = 3;

    const fn index(self) -> usize {
        match self {
            Slot::Grenade => 0,
            Slot::CaughtGrenade => 1,
            Slot::Charge => 2,
        }
    }
}

/// An explosive in a hand: where it sits in the hand's grip frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Held {
    /// The explosive's address: the start of the `Explosive`, not its game-object subobject.
    pub explosive: usize,
    pub hand: Hand,
    /// The explosive's model in the hand's grip frame ([`HoldPlacement::in_hand`]), metres.
    pub local: Pose,
}

/// Where a held model sits in a hand, given for the right hand and mirrored across the body's
/// sagittal plane for the left, as the grip offset is ([`crate::controllers::hands::pose::grip_to_wrist`]).
///
/// The grip frame is OpenXR's: the origin at the centre of the closed hand, -Z up the tube of the
/// curled fingers toward the thumb, +X out of the back of the right hand (into the left palm), and
/// +Y toward the wrist.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HoldPlacement {
    /// The model's origin in the right hand's grip frame, metres.
    pub offset_m: Vec3,
    /// The model's +X, +Y, and +Z axes in the right hand's grip frame: the columns of a rotation.
    pub axes: [Vec3; 3],
}

impl HoldPlacement {
    /// The placement in `hand`'s grip frame. A mirrored rotation `R` is `F R F` with `F` negating X,
    /// which is still a rotation: the model is turned, not mirrored.
    pub fn in_hand(self, hand: Hand) -> Pose {
        let rotation = Mat3::from_cols(self.axes[0], self.axes[1], self.axes[2]);
        let (rotation, offset) = match hand {
            Hand::Right => (rotation, self.offset_m),
            Hand::Left => {
                let flip_x = Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0));
                (flip_x * rotation * flip_x, flip_x * self.offset_m)
            }
        };
        Pose {
            position: offset,
            orientation: Quat::from_mat3(&rotation).normalize(),
        }
    }
}

/// The direction out of `hand`'s palm in its grip frame, toward what the hand holds: -X for the
/// right hand, whose +X is out of the back of the hand, and +X for the left.
pub const fn away_from_palm(hand: Hand) -> Vec3 {
    match hand {
        Hand::Right => Vec3::NEG_X,
        Hand::Left => Vec3::X,
    }
}

/// Register what `slot` holds this tick, or clear it with `None`.
pub fn hold(slot: Slot, held: Option<Held>) {
    HELD.lock()[slot.index()] = held.map(|held| (held, Instant::now()));
}

/// Explosive models re-pushed at render time since the payload loaded.
pub fn updated() -> u64 {
    UPDATED.load(Ordering::Relaxed)
}

/// The world matrix of a model held at `local` (its pose in the grip frame) in a hand whose grip is
/// `grip`. The throws place the explosive on the tick the same way.
pub fn held_matrix(grip: Pose, local: Pose) -> Mat4 {
    let orientation = grip.orientation.normalize();
    Mat4::from_rotation_translation(
        (orientation * local.orientation).normalize(),
        grip.position + orientation * local.position,
    )
}

/// The engine's render update of an explosive, then its model re-pushed at the holding hand.
#[detour(address = jc3gi::equipment::explosive::Explosive::UpdateRender_ADDRESS)]
fn explosive_update_render(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    EXPLOSIVE_UPDATE_RENDER
        .get()
        .unwrap()
        .call(game_object, context);
    let Some((explosive, dtf)) = (unsafe { target(game_object, context) }) else {
        return;
    };
    let Some(matrix) = render_matrix(explosive, [Slot::Grenade, Slot::CaughtGrenade], dtf) else {
        return;
    };
    // SAFETY: the explosive is live for its own render update.
    let explosive = unsafe { &*(explosive as *const Explosive) };
    if let Some(model) = unsafe { explosive.m_ModelInstance.as_mut() } {
        unsafe { model.UpdateMatrix(&Matrix4::from(matrix)) };
        UPDATED.fetch_add(1, Ordering::Relaxed);
    }
}

/// The engine's render update of a C4 charge, then its in-hand model re-pushed at the holding hand.
#[detour(address = jc3gi::equipment::explosive::PlantedExplosive::UpdateRender_ADDRESS)]
fn planted_explosive_update_render(
    game_object: *mut c_void,
    context: *const GameObjectRenderContext,
) {
    PLANTED_EXPLOSIVE_UPDATE_RENDER
        .get()
        .unwrap()
        .call(game_object, context);
    let Some((charge, dtf)) = (unsafe { target(game_object, context) }) else {
        return;
    };
    let Some(matrix) = render_matrix(charge, [Slot::Charge, Slot::Charge], dtf) else {
        return;
    };
    // SAFETY: the charge is live for its own render update.
    let charge = unsafe { &*(charge as *const PlantedExplosive) };
    if let Some(model) = unsafe { charge.m_InactiveModelInstance.as_mut() } {
        unsafe { model.UpdateMatrix(&Matrix4::from(matrix)) };
        UPDATED.fetch_add(1, Ordering::Relaxed);
    }
}

/// The explosive whose `CGameObject` subobject is `game_object`, and the frame's interpolation
/// fraction, while any slot is registered.
///
/// # Safety
///
/// `game_object` must be null or an explosive's live game-object subobject, and `context` null or
/// live, for the render update's duration.
unsafe fn target(
    game_object: *mut c_void,
    context: *const GameObjectRenderContext,
) -> Option<(usize, f32)> {
    let context = unsafe { context.as_ref() }?;
    if game_object.is_null() || HELD.lock().iter().all(Option::is_none) {
        return None;
    }
    let explosive =
        (game_object as usize).checked_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)?;
    Some((explosive, context.m_Dtf))
}

/// The matrix to draw `explosive` with at `dtf`, when one of `slots` holds it and its hand has a
/// render-time grip.
fn render_matrix(explosive: usize, slots: [Slot; 2], dtf: f32) -> Option<Mat4> {
    let held = current(explosive, slots, Instant::now())?;
    let grip = late::drawn_hands(dtf)[held.hand.index()]?.grip;
    let matrix = held_matrix(grip, held.local);
    matrix.is_finite().then_some(matrix)
}

/// The registration of `slots` that holds `explosive` and has not lapsed by `now`.
fn current(explosive: usize, slots: [Slot; 2], now: Instant) -> Option<Held> {
    let held = HELD.lock();
    slots.into_iter().find_map(|slot| {
        held[slot.index()]
            .filter(|(h, at)| {
                h.explosive == explosive && now.saturating_duration_since(*at) <= LAPSE_AFTER
            })
            .map(|(h, _)| h)
    })
}

/// How long a registration stays current without a refresh: a little past a tick.
const LAPSE_AFTER: Duration = Duration::from_millis(150);

static HELD: Mutex<[Option<(Held, Instant)>; Slot::COUNT]> = Mutex::new([None; Slot::COUNT]);
static UPDATED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_matrix_puts_the_placement_in_the_grips_frame() {
        let grip = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let local = Pose {
            position: Vec3::new(0.0, 0.0, -0.1),
            orientation: Quat::from_rotation_x(0.4),
        };
        let m = held_matrix(grip, local);
        // A quarter turn about +Y carries -Z to -X.
        assert!(
            m.w_axis
                .truncate()
                .abs_diff_eq(Vec3::new(0.9, 2.0, 3.0), 1e-5)
        );
        let (_, rotation, _) = m.to_scale_rotation_translation();
        assert!(rotation.abs_diff_eq(grip.orientation * local.orientation, 1e-5));
    }

    /// A placement that turns the model's +Y up the finger tube, its +Z toward the palm, and sits
    /// out of the palm.
    const UPRIGHT: HoldPlacement = HoldPlacement {
        offset_m: Vec3::new(-0.03, 0.01, 0.02),
        axes: [Vec3::NEG_Y, Vec3::NEG_Z, Vec3::X],
    };

    #[test]
    fn a_placement_is_a_rotation_in_both_hands() {
        for hand in Hand::BOTH {
            let local = UPRIGHT.in_hand(hand);
            let m = Mat3::from_quat(local.orientation);
            assert!((m.determinant() - 1.0).abs() < 1e-5, "{hand:?}");
        }
    }

    #[test]
    fn the_left_placement_mirrors_the_right() {
        let right = UPRIGHT.in_hand(Hand::Right);
        let left = UPRIGHT.in_hand(Hand::Left);
        let mirror = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        assert!(left.position.abs_diff_eq(mirror(right.position), 1e-6));
        // The model's up and its palm side mirror; the axis across them flips back.
        for axis in [Vec3::Y, Vec3::Z] {
            assert!((left.orientation * axis).abs_diff_eq(mirror(right.orientation * axis), 1e-5));
        }
        assert!(
            (left.orientation * Vec3::X).abs_diff_eq(-mirror(right.orientation * Vec3::X), 1e-5)
        );
    }

    #[test]
    fn a_placement_out_of_the_palm_is_away_from_it_in_both_hands() {
        for hand in Hand::BOTH {
            let local = UPRIGHT.in_hand(hand);
            assert!(local.position.dot(away_from_palm(hand)) > 0.0, "{hand:?}");
            // The model's +Z faces back into the palm.
            assert!((local.orientation * Vec3::Z).abs_diff_eq(-away_from_palm(hand), 1e-5));
        }
    }

    #[test]
    fn a_registration_names_its_explosive_and_lapses() {
        let held = Held {
            explosive: 0x4000,
            hand: Hand::Left,
            local: Pose::IDENTITY,
        };
        hold(Slot::CaughtGrenade, Some(held));
        let now = Instant::now();
        let slots = [Slot::Grenade, Slot::CaughtGrenade];
        assert_eq!(current(0x4000, slots, now), Some(held));
        assert_eq!(current(0x5000, slots, now), None);
        assert_eq!(current(0x4000, [Slot::Charge; 2], now), None);
        assert_eq!(current(0x4000, slots, now + LAPSE_AFTER * 2), None);
        hold(Slot::CaughtGrenade, None);
        assert_eq!(current(0x4000, slots, now), None);
    }
}
