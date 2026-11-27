//! The view crate's tuning files (`docs/mod/tuning.md`):
//!
//! - [`VIEW`]: the head pose's turning, follow steps, flatscreen look, synthetic-rotation filter,
//!   and anchors, the camera's flatscreen stereo and context field of view, the tracking
//!   recorder's lookback and start-marker bookmark radius, the VR runtime's foveation, cull
//!   padding, recenter, floor watch, bring-up retry, press thresholds, and mirror zoom, and how
//!   long a drawn panel stays a pointer target.
//! - [`HAPTICS`]: the haptic output's primitives, mix, and cues, whose types the input crate
//!   declares ([`jc3vr_input::haptics::tuning`]) and which this crate loads because the input crate
//!   depends on no layer crate.

use std::ops::Deref;

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use jc3vr_input::{
    frame::PressThresholds,
    haptics::{
        cues::{Event, Sustain},
        tuning::{HapticsTuning, file_key},
        vocabulary::Primitive,
    },
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The view crate's tuning file.
    pub static VIEW: ViewTuning = "view.toml";
}

jc3vr_core::tuning_file! {
    /// The haptics' tuning file.
    pub static HAPTICS: HapticsFile = "haptics.toml";
}

/// The view crate's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewTuning {
    pub turn: TurnTuning,
    pub follow: FollowTuning,
    pub(crate) flatscreen_look: FlatscreenLookTuning,
    pub(crate) head_animation: HeadAnimationTuning,
    pub anchors: AnchorsTuning,
    pub camera: CameraTuning,
    pub recorder: RecorderTuning,
    pub foveation: FoveationTuning,
    pub frame: FrameTuning,
    pub recenter: RecenterTuning,
    pub stage: StageTuning,
    pub runtime: RuntimeTuning,
    pub press: PressTuning,
    pub mirror: MirrorTuning,
    pub pointer: PointerTuning,
}

/// How the look input turns the body in VR ([`crate::headpose::xr`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnTuning {
    /// Degrees of body yaw per unit of delta-based (mouse) look input. The mouse look effector is a
    /// per-tick displacement, not an absolute axis, so it is added directly at this scale rather
    /// than run through the stick's smooth scale and [`FlatscreenLookTuning::mouse_sensitivity`]
    /// rate (which oversteers on a mouse and is gated by the stick deadzone: the stop-start,
    /// overshoot, and runaway spin).
    pub(crate) mouse_turn_scale: f32,
    /// The most the yaw accumulator may lead the body's current facing, degrees. The body chases
    /// the accumulated target at a rate limit, so a big input jump (a mouse flick) otherwise pushes
    /// the target far ahead and the body keeps turning for many ticks after the input stops, and
    /// once the lead exceeds 180 degrees the shortest-arc catch-up reverses direction. Clamping the
    /// lead caps both.
    pub(crate) max_body_lead_deg: f32,
    /// Scales the turn rate while swimming, where the body tracks the accumulator one-for-one
    /// rather than through the game's own executor rate, so the same look input turns faster.
    pub(crate) swim_turn_scale: f32,
}

/// The view's steps toward a vehicle's turn of the body ([`crate::headpose::follow`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FollowTuning {
    /// How long the view holds its heading between steps, seconds.
    pub(crate) step_interval_s: f32,
    /// The largest step toward the body, radians.
    pub(crate) step_rad: f32,
    /// How close the view must face the body for the steps to end, radians.
    pub(crate) step_settled_rad: f32,
}

/// The flatscreen head simulation's look ([`crate::headpose::sim`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlatscreenLookTuning {
    /// Mouse-look sensitivity, degrees per unit of look-effector delta.
    pub(crate) mouse_sensitivity: f32,
    /// On foot: the half-angle, degrees, of the decoupled cone. Beyond it, body yaw begins
    /// following the head.
    pub(crate) latch_threshold_deg: f32,
    /// On foot: the half-angle, degrees, below which the latch disengages. Lower than
    /// [`FlatscreenLookTuning::latch_threshold_deg`] to provide hysteresis and prevent jitter at
    /// the boundary.
    pub(crate) latch_disengage_threshold_deg: f32,
    /// Off foot: the most body-relative yaw, degrees, in either direction. Ranges beyond a real
    /// head's ~80 degrees are made anatomically plausible by the neck twist.
    pub(crate) free_look_yaw_limit_deg: f32,
    /// The most pitch, degrees, in either direction, applied in every mode: a real head cannot
    /// pitch past vertical, and letting the euler pitch cross 90 degrees flips the yaw and roll
    /// decomposition.
    pub(crate) free_look_pitch_limit_deg: f32,
}

/// The synthetic head rotation's low-pass ([`crate::headpose::animation`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeadAnimationTuning {
    /// The filter's time constant, seconds.
    pub time_constant_s: f32,
    /// The longest step the filter smooths across, seconds: a longer gap snaps to the target.
    pub max_step_s: f32,
}

/// The head anchors ([`crate::headpose::anchors`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorsTuning {
    /// The oldest the live tracking packet may be for the current anchors to use it, milliseconds.
    pub(crate) max_tracking_age_ms: u64,
    /// The largest move of the character's root between two ticks across which the anchors are
    /// interpolated, metres; a larger one collapses the pair onto the current tick, as a teleport.
    pub max_root_step_m: f32,
}

/// The engine camera's patches ([`crate::hooks::camera`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraTuning {
    /// The interpupillary distance of the flatscreen stereo per-eye camera offset, metres. A VR
    /// session takes each eye's offset from the runtime's eye poses instead.
    pub(crate) flatscreen_ipd_m: f32,
    /// The field of view written into the camera context the game reads after the update,
    /// degrees.
    pub(crate) context_fov_deg: f32,
}

/// The tracking recorder ([`crate::recorder`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecorderTuning {
    /// How far back the lookback reaches, seconds.
    pub(crate) lookback_window_s: u64,
    /// How close to a named place the player must stand for a recording's start marker to name
    /// it, metres.
    pub(crate) bookmark_radius_m: f32,
}

/// The fixed foveation's rings ([`crate::vr::foveation`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoveationTuning {
    /// The foveal radius as a fraction of an eye's half-diagonal from its foveal centre: inside it
    /// the scene renders at full rate.
    pub inner_fraction: f32,
    /// The radius, in the units of [`FoveationTuning::inner_fraction`], at which the peripheral
    /// drop reaches [`FoveationTuning::max_drop`]; the drop ramps from zero at the inner radius to
    /// the maximum here.
    pub outer_fraction: f32,
    /// The fraction of peripheral pixels dropped (and reconstructed) at the outer radius and
    /// beyond.
    pub max_drop: f32,
}

/// The VR frame's culling ([`crate::vr::cull_projection_standard`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameTuning {
    /// The extra fraction the union-FOV cull frustum is expanded by on every side, on top of the
    /// per-eye FOV union and the lateral eye-shift margin. The engine culls at a single
    /// interpolated pose, so under fast motion (especially flying) geometry can otherwise pop in at
    /// the outer edges before the cull catches up. Each side's tangent is padded outward (`0.4` is
    /// 40% wider per side), the vertical axis included, at the cost of some over-draw of
    /// just-off-screen geometry.
    pub(crate) cull_fov_padding: f32,
}

/// The recenters ([`crate::vr::recenter`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecenterTuning {
    /// How recent the follow bank must be for the seat lock's yaw re-base to use it,
    /// milliseconds: a vehicle entry, a hijack, or a stunt position into a seat takes a few
    /// seconds from the moment the body stops following.
    pub(crate) seat_yaw_bank_window_ms: u64,
    /// A yaw re-base smaller than this is not worth turning the view for, radians.
    pub(crate) seat_yaw_min_rad: f32,
    /// How long the seat lock's yaw re-base takes to turn the view, seconds.
    pub(crate) seat_yaw_blend_s: f32,
    /// How long the head anchor must hold still for the automatic recenter, milliseconds.
    pub(crate) anchor_settle_ms: u64,
    /// The per-frame head-anchor movement below which the pose counts as settled, metres: above
    /// idle sway, below any scripted or locomotion motion.
    pub(crate) anchor_settle_m: f32,
    /// The longest gap between two head-anchor samples that still counts as one continuous run,
    /// milliseconds.
    pub(crate) anchor_sample_gap_ms: i64,
}

/// The floor watch after a reference space change ([`crate::vr::stage`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageTuning {
    /// How long after a reference space change a step in the head's height may be the floor's,
    /// milliseconds.
    pub(crate) floor_watch_ms: u64,
    /// The largest step in the head's height between two frames that is the head's own, metres: a
    /// head dropping as fast as a player can duck covers a few centimetres in a frame.
    pub(crate) max_head_step_m: f32,
}

/// The OpenXR runtime's bring-up ([`crate::vr::update`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTuning {
    /// How often OpenXR bring-up is retried after a failure while VR is on, whole seconds. The mod
    /// runs in flatscreen stereo between attempts.
    pub(crate) retry_interval_s: u64,
    /// How old the live tracking publication may be and still count as available to the diagnostic
    /// readouts, milliseconds ([`crate::vr::live_tracking`]).
    pub(crate) diagnostic_max_age_ms: u64,
    /// How many frames to wait for a requested deferred resize to be serviced before treating it as
    /// a fault ([`crate::vr::resolution`]). The engine services a resize in the very next draw
    /// prologue, so this is a generous ceiling that only trips on a stuck or faulted resize.
    pub(crate) resize_service_timeout_frames: u32,
}

/// The analog thresholds of the derived presses ([`crate::vr::input::config`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressTuning {
    /// The trigger values that press and release
    /// [`Button::TriggerPress`](jc3vr_input::frame::Button::TriggerPress).
    pub trigger: PressThresholds,
    /// The squeeze values that press and release
    /// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller without a
    /// force-sensing grip (Touch, or a profile the runtime emulates). The grip is used only as an
    /// edge, so the press threshold sits high enough that resting a hand on the grip does not press
    /// it.
    pub squeeze: PressThresholds,
    /// The squeeze forces that press and release
    /// [`Button::SqueezePress`](jc3vr_input::frame::Button::SqueezePress) on a controller whose
    /// profile provides the grip force (the Index). The Index grip's value is capacitive and reads
    /// a full squeeze with the hand merely closed round the handle, so the press waits for a
    /// deliberate squeeze, and the release sits low enough that a held grab does not need a hard
    /// squeeze to keep.
    pub squeeze_force: PressThresholds,
    /// The trackpad forces that press and release the trackpad's half presses, on a controller with
    /// a trackpad. A resting thumb reads no force, so a light press is enough.
    pub trackpad: PressThresholds,
    /// [`PressRules::trackpad_half_margin`](jc3vr_input::frame::PressRules::trackpad_half_margin).
    pub trackpad_half_margin: f32,
}

/// The desktop mirror ([`crate::vr::present_mirror`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorTuning {
    /// The extra magnification applied to a single-eye mirror image on top of its fill, about the
    /// centre. `1.0` is the plain fill; above that crops in further, tightening the desktop view
    /// onto the middle of the eye render.
    pub zoom: f32,
}

/// The panels as the controller pointers see them ([`crate::hud::panel`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointerTuning {
    /// How long a drawn panel, and the camera that placed it, stay a target for the pointer after
    /// the frame that drew them, milliseconds: a panel no longer drawn stops taking hits.
    pub(crate) panel_max_age_ms: u64,
}

impl Tuning for ViewTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let turn = &self.turn;
        positive("turn.mouse_turn_scale", turn.mouse_turn_scale, 100.0)?;
        range("turn.max_body_lead_deg", turn.max_body_lead_deg, 1.0, 179.0)?;
        positive("turn.swim_turn_scale", turn.swim_turn_scale, 10.0)?;

        let follow = &self.follow;
        positive("follow.step_interval_s", follow.step_interval_s, 10.0)?;
        positive("follow.step_rad", follow.step_rad, std::f32::consts::PI)?;
        positive(
            "follow.step_settled_rad",
            follow.step_settled_rad,
            follow.step_rad,
        )?;

        let look = &self.flatscreen_look;
        positive(
            "flatscreen_look.mouse_sensitivity",
            look.mouse_sensitivity,
            100.0,
        )?;
        range(
            "flatscreen_look.latch_threshold_deg",
            look.latch_threshold_deg,
            0.0,
            180.0,
        )?;
        range(
            "flatscreen_look.latch_disengage_threshold_deg",
            look.latch_disengage_threshold_deg,
            0.0,
            look.latch_threshold_deg,
        )?;
        range(
            "flatscreen_look.free_look_yaw_limit_deg",
            look.free_look_yaw_limit_deg,
            0.0,
            180.0,
        )?;
        range(
            "flatscreen_look.free_look_pitch_limit_deg",
            look.free_look_pitch_limit_deg,
            0.0,
            89.0,
        )?;

        let animation = &self.head_animation;
        positive(
            "head_animation.time_constant_s",
            animation.time_constant_s,
            10.0,
        )?;
        positive("head_animation.max_step_s", animation.max_step_s, 10.0)?;
        millis(
            "anchors.max_tracking_age_ms",
            self.anchors.max_tracking_age_ms,
        )?;
        positive(
            "anchors.max_root_step_m",
            self.anchors.max_root_step_m,
            1000.0,
        )?;

        let camera = &self.camera;
        positive("camera.flatscreen_ipd_m", camera.flatscreen_ipd_m, 0.2)?;
        range("camera.context_fov_deg", camera.context_fov_deg, 1.0, 179.0)?;

        let recorder = &self.recorder;
        positive(
            "recorder.lookback_window_s",
            recorder.lookback_window_s as f32,
            600.0,
        )?;
        nonnegative(
            "recorder.bookmark_radius_m",
            recorder.bookmark_radius_m,
            1000.0,
        )?;

        let foveation = &self.foveation;
        positive("foveation.inner_fraction", foveation.inner_fraction, 1.0)?;
        range(
            "foveation.outer_fraction",
            foveation.outer_fraction,
            foveation.inner_fraction,
            2.0,
        )?;
        range("foveation.max_drop", foveation.max_drop, 0.0, 1.0)?;
        nonnegative("frame.cull_fov_padding", self.frame.cull_fov_padding, 4.0)?;

        let recenter = &self.recenter;
        millis(
            "recenter.seat_yaw_bank_window_ms",
            recenter.seat_yaw_bank_window_ms,
        )?;
        nonnegative(
            "recenter.seat_yaw_min_rad",
            recenter.seat_yaw_min_rad,
            std::f32::consts::PI,
        )?;
        positive("recenter.seat_yaw_blend_s", recenter.seat_yaw_blend_s, 10.0)?;
        millis("recenter.anchor_settle_ms", recenter.anchor_settle_ms)?;
        positive("recenter.anchor_settle_m", recenter.anchor_settle_m, 1.0)?;
        positive(
            "recenter.anchor_sample_gap_ms",
            recenter.anchor_sample_gap_ms as f32,
            600_000.0,
        )?;

        millis("stage.floor_watch_ms", self.stage.floor_watch_ms)?;
        positive("stage.max_head_step_m", self.stage.max_head_step_m, 1.0)?;
        positive(
            "runtime.retry_interval_s",
            self.runtime.retry_interval_s as f32,
            600.0,
        )?;

        millis(
            "runtime.diagnostic_max_age_ms",
            self.runtime.diagnostic_max_age_ms,
        )?;
        range(
            "runtime.resize_service_timeout_frames",
            self.runtime.resize_service_timeout_frames as f32,
            1.0,
            36_000.0,
        )?;

        let press = &self.press;
        thresholds("press.trigger", press.trigger)?;
        thresholds("press.squeeze", press.squeeze)?;
        thresholds("press.squeeze_force", press.squeeze_force)?;
        thresholds("press.trackpad", press.trackpad)?;
        range(
            "press.trackpad_half_margin",
            press.trackpad_half_margin,
            0.0,
            1.0,
        )?;

        range("mirror.zoom", self.mirror.zoom, 0.25, 4.0)?;
        millis("pointer.panel_max_age_ms", self.pointer.panel_max_age_ms)
    }
}

/// The haptics' tuning, as the file holds it: [`HapticsTuning`], to which it derefs.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct HapticsFile(pub HapticsTuning);

impl Deref for HapticsFile {
    type Target = HapticsTuning;

    fn deref(&self) -> &HapticsTuning {
        &self.0
    }
}

impl Tuning for HapticsFile {
    fn validate(&self) -> Result<(), Invalid> {
        let tuning = &self.0;
        for primitive in Primitive::ALL {
            let shape = tuning.shape(primitive);
            let name = |field: &str| format!("vocabulary.{}.{field}", primitive.label());
            nonnegative(&name("soft"), shape.soft, 1.0)?;
            range(&name("full"), shape.full, shape.soft, 1.0)?;
            positive(&name("duration_s"), shape.duration_s, 10.0)?;
            nonnegative(&name("frequency_hz"), shape.frequency_hz, 1000.0)?;
            if primitive.is_sustained() && shape.soft != 0.0 {
                return Err(Invalid(format!(
                    "{} must be zero for a sustained primitive, so that a level of zero is still",
                    name("soft")
                )));
            }
        }

        let mix = &tuning.mix;
        nonnegative("mix.sustain_ceiling", mix.sustain_ceiling, 1.0)?;
        range(
            "mix.sustain_knee",
            mix.sustain_knee,
            0.0,
            mix.sustain_ceiling,
        )?;
        nonnegative("mix.knee", mix.knee, 1.0)?;
        nonnegative("mix.stack_share", mix.stack_share, 1.0)?;
        nonnegative("mix.duck_share", mix.duck_share, 1.0)?;
        positive("mix.refresh_s", mix.refresh_s, 1.0)?;
        positive("long_factor", tuning.long_factor, 10.0)?;

        for event in Event::ALL {
            let cue = tuning.events.get(event);
            let name = format!("events.{}", file_key(&event));
            nonnegative(&format!("{name}.strength"), cue.strength, 1.0)?;
            if cue.primitive.is_sustained() {
                return Err(Invalid(format!(
                    "{name}.primitive must be a one-off primitive, got {}",
                    cue.primitive.label()
                )));
            }
        }
        for source in Sustain::ALL {
            let cue = tuning.sustains.get(source);
            let name = format!("sustains.{}", file_key(&source));
            nonnegative(&format!("{name}.strength"), cue.strength, 1.0)?;
            if !cue.primitive.is_sustained() {
                return Err(Invalid(format!(
                    "{name}.primitive must be a sustained primitive, got {}",
                    cue.primitive.label()
                )));
            }
        }
        Ok(())
    }
}

/// Checks a duration in milliseconds against the longest any of the file's waits can usefully be:
/// ten minutes.
fn millis(name: &str, value: u64) -> Result<(), Invalid> {
    positive(name, value as f32, 600_000.0)
}

/// Checks a press's thresholds: both in `0..=1`, the release at or below the press, so a value
/// resting near one does not chatter.
fn thresholds(name: &str, thresholds: PressThresholds) -> Result<(), Invalid> {
    range(&format!("{name}.press"), thresholds.press, 0.0, 1.0)?;
    range(
        &format!("{name}.release"),
        thresholds.release,
        0.0,
        thresholds.press,
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(VIEW.get().validate(), Ok(()));
    }

    #[test]
    fn the_shipped_haptics_file_loads_and_validates() {
        assert_eq!(HAPTICS.get().validate(), Ok(()));
    }
}
