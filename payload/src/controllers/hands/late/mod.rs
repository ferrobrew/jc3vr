//! The render-time late update: the held weapon's model and the hand and forearm bones in the
//! skinning palette, re-posed from the latest controller pose.
//!
//! The IK places the hands at the sim tick, and the engine interpolates the rendered pose between
//! the last two ticks, so the rendered hands trail the controllers by one to two ticks. Once per
//! rendered frame ([`begin_frame`], from `Game::UpdateRender` after the VR pose is published) this
//! module snapshots the latest controller poses composed with the render-time head pose pair, and
//! the targets the last two ticks' IK was asked to reach. At render time, for the engine's
//! interpolation fraction `dtf`, each hand's correction is
//!
//! `D = now(dtf) × reference(dtf)⁻¹`,
//!
//! where `now` interpolates the latest controller pose between the head pose pair's two sides (as
//! the camera does) and `reference` interpolates the two ticks' targets (as the skinning palette
//! and the weapon's cached transforms do). The physical move between those poses is bounded before
//! the correction is weighted, and the weighted pose is derived by interpolating `reference` toward
//! `now` before computing `D`. The grip offset is on both sides, so it cancels.
//!
//! The rendered hand sits on the *solved* wrist, which falls short of the tick's target when the
//! arm cannot reach it. With the detach on, `reference` is taken from the solved wrist instead: the
//! target moved back along the shortfall by the part of it the detach covers
//! ([`reach::detach_offset`]: past a small threshold, up to a cap), so `D` carries the rendered hand
//! that far off the end of the arm toward the controller, and the forearm stretches along its axis
//! to stay attached ([`palette::forearm_reach`]). Past the cap, the hand stays at the cap.
//!
//! Both render seams apply the same `D`, so the gun and the hand never separate:
//!
//! - the post-call `CWeaponBase::UpdateRender` detour re-pushes the gun's model matrix as `D` times
//!   the engine's own interpolated matrix, for a gun the local player wields in a hand attach bone;
//! - the character render-block hooks pre-multiply the hand bones' palette entries by `D` (in model
//!   space), and the forearm bones' by a swing about the elbow that follows the corrected wrist
//!   ([`palette_edit`]);
//! - the gun's separate model parts (a magazine, a slide, a spinning barrel) are re-pushed by
//!   [`crate::controllers::hands::parts`] with the correction of the hand they ride.
//!
//! The muzzle flash, attached effects, and shot origin stay on the sim-rate transform.
//!
//! The module is split by concern: [`bones`] resolves and publishes the skeleton bones and the
//! post-solve arm joints the edit and the status read; [`math`] holds the pure pose and transform
//! helpers, independent of any frame state; [`frame`] holds the frame snapshot, the correction, and
//! the palette edit it drives; [`weapon`] re-pushes the held gun's model matrix with the same
//! correction.

mod bones;
mod frame;
mod math;
mod weapon;

pub(crate) use bones::publish_solved;
pub use bones::{ArmJoints, HandBones, MAX_BONE_INDEX};
pub use frame::{HandLateStatus, LateStatus, PaletteEdit, drawn_hands, status};
pub(crate) use frame::{begin_frame, count_palette_edit, hand_correction, palette_edit};
pub(crate) use math::{correction, lerp_transform, plausible};
use re_utilities::hook_library::HookLibrary;
pub(crate) use weapon::attach_hand;

pub(super) fn hook_library() -> HookLibrary {
    weapon::hook_library()
}
