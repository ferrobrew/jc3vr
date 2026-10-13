//! The player's head as the body under it reads it. Pure, but for [`read`], which reads the
//! headpose and the steady view's anchor.
//!
//! The headpose is the eye point: the animated head anchor plus the HMD's travel from where it was
//! at the recenter. A head that looks down turns the eyes about the neck, so the eye point drops and
//! comes forward although nothing below the neck moved. The body is built under the neck pivot
//! instead ([`neck_pivot_offset`]): the eye point less the HMD's rotation of the arm from the pivot
//! to the eyes, which is where the head bone is.
//!
//! How far the hips drop is measured against the height the player is seen to stand at
//! ([`StandingReference`]), not against the stored eye height: the runtime's floor can sit a few
//! centimetres off the calibrated one, which read as a constant crouch. The reference is a high
//! percentile of the neck pivot's height over the head anchor, sampled while the player is on foot
//! looking about level, seeded at the calibration and kept within [`MAX_REFERENCE_SHIFT_M`] of it.
//! A drop within the deadband moves nothing ([`head_drop`]), so a standing player's small bobs and
//! nods leave the hips at their standing height.

use glam::{Mat4, Quat, Vec3};

use crate::{
    controllers::{
        body::rig::{lean_allowance, torso::EYES_TO_NECK_PIVOT},
        locomotion::anchor::status as anchor_status,
    },
    headpose::{
        anchor as headpose_anchor, body_relative_rotation, query,
        xr::body_rotation as view_rotation,
    },
};

/// The player's head as the body reads it, in the model space of the root at `world`, at the VR
/// world scale `scale`: the neck pivot, below and behind the eye point the headpose publishes by
/// the head's turn ([`neck_pivot_offset`]), and its height over the head anchor. The view
/// hangs from the root at its smoothed height on the ground
/// ([`AnchorStatus::height_offset`](crate::controllers::locomotion::anchor::AnchorStatus::height_offset)),
/// so on a stair the head trails the raw root by what the smoothing hides; that is taken back out,
/// so the solve measures the head against the same root the view does, and a staircase does not
/// read as a crouch.
pub(super) fn read(world: &Mat4, scale: f32) -> PlayerHead {
    let anchor = anchor_status();
    let hidden = anchor.height_offset * anchor.weight;
    let hidden = Vec3::Y * if hidden.is_finite() { hidden } else { 0.0 };
    let to_model = world.inverse();
    let rotation = body_relative_rotation();
    let head =
        to_model.transform_point3(query().position - hidden) + neck_pivot_offset(rotation, scale);
    let height = headpose_anchor()
        .map(|anchor| head.y - to_model.transform_point3(anchor - hidden).y)
        .filter(|h| h.is_finite());
    // The lean the chase leaves alone, from the cockpit frame into the model space: what the head
    // moved without the feet.
    let lean = lean_allowance()
        .filter(|l| l.is_finite())
        .map_or(Vec3::ZERO, |l| {
            to_model.transform_vector3(view_rotation() * Vec3::new(l.x, 0.0, l.y) * scale)
        });
    let lean = if lean.is_finite() {
        Vec3::new(lean.x, 0.0, lean.z)
    } else {
        Vec3::ZERO
    };
    PlayerHead {
        head,
        upright: head - lean,
        height,
        level: looks_level(rotation),
    }
}

/// The player's head as [`read`] reads it.
pub(super) struct PlayerHead {
    /// The neck pivot, model space.
    pub(super) head: Vec3,
    /// Where the neck pivot would be upright over the player's feet, model space: the head less
    /// the lean the chase leaves alone ([`crate::controllers::body::rig::lean`]). What the feet
    /// step under.
    pub(super) upright: Vec3,
    /// The neck pivot's height over the head anchor, metres, when the anchor is known.
    pub(super) height: Option<f32>,
    /// Whether the gaze is about level.
    pub(super) level: bool,
}

/// The neck pivot's place relative to the eye point, model space, for a head turned
/// `head_rotation` from neutral against the body, at the world scale `scale`: what to add to the
/// eye-derived head to have the pivot. Zero for a neutral head, since the anchor already is the
/// pivot then.
pub fn neck_pivot_offset(head_rotation: Quat, scale: f32) -> Vec3 {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let offset = (head_rotation.normalize() * EYES_TO_NECK_PIVOT - EYES_TO_NECK_PIVOT) * scale;
    if offset.is_finite() {
        offset
    } else {
        Vec3::ZERO
    }
}

/// Whether a head turned `head_rotation` from neutral looks about level: its forward within
/// [`LEVEL_GAZE_DEG`] of the horizon, so the neck is near upright and the pivot's height is the
/// player's own.
pub fn looks_level(head_rotation: Quat) -> bool {
    let forward = head_rotation.normalize() * Vec3::NEG_Z;
    forward.is_finite() && forward.y.abs() <= LEVEL_GAZE_DEG.to_radians().sin()
}

/// How far the head has dropped below the player's standing height, past the deadband, metres:
/// `height` and `reference` are the neck pivot's height over the head anchor now and standing.
pub fn head_drop(height: f32, reference: f32, deadband_m: f32) -> f32 {
    let deadband = if deadband_m.is_finite() {
        deadband_m.max(0.0)
    } else {
        0.0
    };
    let drop = reference - height - deadband;
    if drop.is_finite() { drop.max(0.0) } else { 0.0 }
}

/// The height the player is seen to stand at: the neck pivot's height over the head anchor that
/// [`STANDING_PERCENTILE`] of the recent standing samples lie below, metres. Until it has
/// [`MIN_SAMPLES`] of them, the calibration's: zero, the pivot at the anchor.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StandingReference {
    samples: [f32; WINDOW],
    len: usize,
    next: usize,
    /// Seconds since the last sample was taken.
    since_sample_s: f32,
    reference: f32,
}

impl StandingReference {
    pub const fn new() -> Self {
        Self {
            samples: [0.0; WINDOW],
            len: 0,
            next: 0,
            since_sample_s: SAMPLE_EVERY_S,
            reference: 0.0,
        }
    }

    /// Start over from the calibration: the floor under the player moved, or the player came back
    /// to the body under the head.
    pub fn reseed(&mut self) {
        *self = Self::new();
    }

    /// The standing height, metres over the head anchor.
    pub fn reference(&self) -> f32 {
        self.reference
    }

    /// How many samples it holds.
    pub fn samples(&self) -> usize {
        self.len
    }

    /// Advance by `dt` seconds with the pivot at `height` over the anchor, taking it as a sample
    /// when `standing` (on foot, the gaze about level) and one is due.
    pub fn observe(&mut self, height: f32, standing: bool, dt: f32) {
        if dt.is_finite() {
            self.since_sample_s += dt.max(0.0);
        }
        if standing && height.is_finite() && self.since_sample_s >= SAMPLE_EVERY_S {
            self.since_sample_s = 0.0;
            self.samples[self.next] = height;
            self.next = (self.next + 1) % WINDOW;
            self.len = (self.len + 1).min(WINDOW);
            if self.len >= MIN_SAMPLES {
                self.reference = self.percentile().clamp(
                    CALIBRATED - MAX_REFERENCE_SHIFT_M,
                    CALIBRATED + MAX_REFERENCE_SHIFT_M,
                );
            }
        }
    }

    fn percentile(&self) -> f32 {
        let mut sorted = self.samples;
        let held = &mut sorted[..self.len];
        held.sort_unstable_by(f32::total_cmp);
        let rank = ((self.len - 1) as f32 * STANDING_PERCENTILE).round() as usize;
        held[rank.min(self.len - 1)]
    }
}

impl Default for StandingReference {
    fn default() -> Self {
        Self::new()
    }
}

/// The pivot's height over the head anchor at the calibrated eye height: the anchor is where the
/// eyes are put for a player standing at it.
const CALIBRATED: f32 = 0.0;

/// The furthest the observed standing height may move from the calibration's, metres: about the
/// runtime's floor error, short of a crouch.
pub const MAX_REFERENCE_SHIFT_M: f32 = 0.15;

/// The share of the standing samples the reference lies above: high, since a standing player's
/// head only ever drops from their height, and short of the top, which a stretch on the toes sets.
pub const STANDING_PERCENTILE: f32 = 0.9;

/// How many samples the reference keeps: half a minute's.
const WINDOW: usize = 240;

/// How often a sample is taken, seconds.
const SAMPLE_EVERY_S: f32 = 0.125;

/// How many samples the reference waits for before it moves from the calibration: two seconds'.
const MIN_SAMPLES: usize = 16;

/// How far from the horizon the gaze may be for the pivot's height to count as standing, degrees.
pub const LEVEL_GAZE_DEG: f32 = 20.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_neutral_head_puts_the_pivot_at_the_anchor() {
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, 1.0), Vec3::ZERO);
    }

    #[test]
    fn looking_down_drops_the_eyes_but_not_the_pivot() {
        // Pitching the head down by 60 degrees about the pivot swings the eyes down and forward
        // around it.
        let down = Quat::from_rotation_x(-60f32.to_radians());
        let arm = -EYES_TO_NECK_PIVOT;
        let eye_travel = down * arm - arm;
        assert!(eye_travel.y < -0.05, "{eye_travel}");
        // The eye-derived head plus the offset is the pivot, which did not move.
        let pivot_travel = eye_travel + neck_pivot_offset(down, 1.0);
        assert!(pivot_travel.length() < 1e-5, "{pivot_travel}");
        // At twice the world scale, the arm is twice as long.
        let scaled = neck_pivot_offset(down, 2.0);
        assert!((scaled - neck_pivot_offset(down, 1.0) * 2.0).length() < 1e-5);
        assert_eq!(neck_pivot_offset(Quat::IDENTITY, f32::NAN), Vec3::ZERO);
    }

    #[test]
    fn the_gaze_is_level_within_the_limit() {
        assert!(looks_level(Quat::IDENTITY));
        assert!(looks_level(Quat::from_rotation_x(-0.2)));
        assert!(looks_level(Quat::from_rotation_y(2.0)));
        assert!(!looks_level(Quat::from_rotation_x(-0.6)));
        assert!(!looks_level(Quat::from_rotation_x(0.6)));
    }

    #[test]
    fn a_drop_within_the_deadband_moves_nothing() {
        assert_eq!(head_drop(0.0, 0.0, 0.04), 0.0);
        assert_eq!(head_drop(-0.03, 0.0, 0.04), 0.0);
        assert!((head_drop(-0.10, 0.0, 0.04) - 0.06).abs() < 1e-6);
        // Standing taller than the reference is not a drop.
        assert_eq!(head_drop(0.05, 0.0, 0.04), 0.0);
        assert_eq!(head_drop(f32::NAN, 0.0, 0.04), 0.0);
    }

    fn stand(reference: &mut StandingReference, height: f32, seconds: f32) -> f32 {
        let mut t = 0.0;
        while t < seconds {
            reference.observe(height, true, 0.05);
            t += 0.05;
        }
        reference.reference()
    }

    #[test]
    fn a_floor_a_few_centimetres_high_becomes_the_standing_height() {
        // The runtime's floor sits eight centimetres high: standing level, the pivot reads eight
        // centimetres under the anchor, and the calibration would call that a crouch.
        let mut reference = StandingReference::new();
        assert_eq!(reference.reference(), 0.0);
        let standing = stand(&mut reference, -0.08, 5.0);
        assert!((standing + 0.08).abs() < 1e-6, "{standing}");
        assert_eq!(head_drop(-0.08, standing, 0.04), 0.0);
        // A real crouch from there still drops.
        assert!(head_drop(-0.38, standing, 0.04) > 0.25);
    }

    #[test]
    fn the_reference_waits_for_enough_samples() {
        let mut reference = StandingReference::new();
        stand(&mut reference, -0.08, 1.0);
        assert_eq!(reference.reference(), 0.0);
        // Samples taken off foot or looking down do not count.
        let mut ignored = StandingReference::new();
        for _ in 0..200 {
            ignored.observe(-0.08, false, 0.05);
        }
        assert_eq!(ignored.samples(), 0);
        assert_eq!(ignored.reference(), 0.0);
    }

    #[test]
    fn a_long_crouch_does_not_become_standing() {
        let mut reference = StandingReference::new();
        stand(&mut reference, 0.0, 10.0);
        // Half a minute crouched behind cover, the gaze level.
        let crouched = stand(&mut reference, -0.45, 40.0);
        assert!(crouched >= -MAX_REFERENCE_SHIFT_M - 1e-6, "{crouched}");
        assert!(head_drop(-0.45, crouched, 0.04) > 0.25);
    }

    #[test]
    fn the_reference_takes_a_high_percentile_of_the_bobbing() {
        let mut reference = StandingReference::new();
        // Mostly standing at the reference, with dips of a few centimetres a fifth of the time.
        let mut t = 0.0;
        let mut i = 0;
        while t < 30.0 {
            let height = if i % 5 == 0 { -0.06 } else { -0.01 };
            reference.observe(height, true, 0.125);
            t += 0.125;
            i += 1;
        }
        assert!((reference.reference() + 0.01).abs() < 1e-6);
        reference.reseed();
        assert_eq!(reference.reference(), 0.0);
        assert_eq!(reference.samples(), 0);
    }
}
