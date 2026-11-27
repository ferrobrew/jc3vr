//! The controls' tuning, read from the shipped `tuning/controls.toml` ([`CONTROLS`]): the bindings'
//! thresholds and chord timings, the held actions' analog press, the mode detection's debounce, the
//! game rumble's forward, and the world-space overlay's drawing.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The controls' tuning file.
    pub static CONTROLS: ControlsTuning = "controls.toml";
}

/// The controls tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlsTuning {
    pub bindings: BindingsTuning,
    pub hold: HoldTuning,
    pub mode: ModeTuning,
    pub rumble: RumbleTuning,
    pub overlay: OverlayTuning,
}

impl Tuning for ControlsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let b = &self.bindings;
        range("bindings.trigger_deadzone", b.trigger_deadzone, 0.0, 0.99)?;
        positive("bindings.long_press_s", b.long_press_s, 10.0)?;
        range("bindings.crouch_flick", b.crouch_flick, 0.0, 1.0)?;
        range("bindings.menu_stick_push", b.menu_stick_push, 0.0, 1.0)?;
        nonnegative(
            "bindings.stick_chord_window_s",
            b.stick_chord_window_s,
            10.0,
        )?;
        nonnegative("bindings.recenter_hold_s", b.recenter_hold_s, 10.0)?;
        nonnegative(
            "bindings.panel_chord_window_s",
            b.panel_chord_window_s,
            10.0,
        )?;

        range("hold.analog_press", self.hold.analog_press, 0.0, 1.0)?;
        if !(1..=30).contains(&self.mode.debounce_ticks) {
            return Err(Invalid(format!(
                "mode.debounce_ticks must be in [1, 30], got {}",
                self.mode.debounce_ticks
            )));
        }

        let r = &self.rumble;
        range("rumble.min_amplitude", r.min_amplitude, 0.0, 1.0)?;
        nonnegative(
            "rumble.kick_amplitude_per_rad",
            r.kick_amplitude_per_rad,
            1000.0,
        )?;
        nonnegative("rumble.max_shot_s", r.max_shot_s, 10.0)?;

        let o = &self.overlay;
        nonnegative("overlay.min_line_px", o.min_line_px, 100.0)?;
        nonnegative("overlay.depth_tolerance_m", o.depth_tolerance_m, 10.0)?;
        nonnegative("overlay.depth_tolerance_rel", o.depth_tolerance_rel, 1.0)?;
        positive(
            "overlay.body_mirror_distance_m",
            o.body_mirror_distance_m,
            20.0,
        )?;
        nonnegative("overlay.ticked_jump_m", o.ticked_jump_m, 1000.0)?;
        nonnegative("overlay.surface_match_m", o.surface_match_m, 100.0)?;
        positive("overlay.grapple_ring_rad", o.grapple_ring_rad, 1.0)?;
        positive("overlay.assist_ring_scale", o.assist_ring_scale, 10.0)?;
        positive("overlay.marker_ring_rad", o.marker_ring_rad, 1.0)?;
        positive("overlay.marker_dot_rad", o.marker_dot_rad, 1.0)
    }
}

/// The controller-to-game bindings' thresholds and timings.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingsTuning {
    /// The trigger pull below which an analog trigger action (throttle, collective, thrust) reads
    /// as zero, `0..1`.
    pub trigger_deadzone: f32,
    /// How long the system button must be held to open the commlink rather than pause, seconds.
    pub long_press_s: f32,
    /// How far down the dominant stick must be flicked on foot to toggle the seated crouch,
    /// `0..1`. The stick must come back above half of it before the next flick counts.
    pub crouch_flick: f32,
    /// How far either stick must be pushed along one axis to step a menu, `0..1`. The step holds,
    /// and the game repeats it, until the push falls back below half of it.
    pub menu_stick_push: f32,
    /// How long a lone stick click waits for the other before it acts, seconds: both clicks
    /// pressed within it are the recenter chord, and neither acts on its own.
    pub stick_chord_window_s: f32,
    /// How long both stick clicks must be held together to recenter the VR view, seconds.
    pub recenter_hold_s: f32,
    /// How soon after the left stick click a fresh right B press can show or hide the debug panel,
    /// seconds. The left click waits for the longer of this and the recenter join window.
    pub panel_chord_window_s: f32,
}

/// The held actions ([`crate::controllers::intent::hold`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldTuning {
    /// The analog value above which an analog action also reads as pressed, `0..1`. The game's
    /// devices press at their own button threshold, which is not read; this stands in for it.
    pub(crate) analog_press: f32,
}

/// The gameplay mode detection ([`crate::controllers::mode`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTuning {
    /// How many consecutive ticks a new gameplay mode must be detected before it takes effect.
    /// One switches at once; two rides out a one-tick flicker during an animation blend. Counted
    /// at the game's 30 Hz tick.
    pub(crate) debounce_ticks: u32,
}

/// The game rumble's forward to the controllers ([`crate::controllers::haptics::rumble`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RumbleTuning {
    /// Below this, a kick plays nothing, `0..1`.
    pub(crate) min_amplitude: f32,
    /// How much a shot's recoil kick adds to its pulse's amplitude, per radian of kick.
    pub(crate) kick_amplitude_per_rad: f32,
    /// The longest a weapon's fire rumble plays on a controller, seconds: the game's rumbles can
    /// last long enough to blur into one another.
    pub(crate) max_shot_s: f32,
}

/// The world-space overlay's drawing ([`crate::controllers::overlay`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayTuning {
    /// A line thinner than this fades instead, pixels.
    pub min_line_px: f32,
    /// A primitive this far behind the scene's depth still counts as visible, metres, so that a
    /// marker on a surface is not hidden by that surface.
    pub depth_tolerance_m: f32,
    /// The depth tolerance as a share of the distance, which takes over at a distance, where the
    /// depth buffer's precision falls.
    pub depth_tolerance_rel: f32,
    /// How far ahead of Rico the body mirror stands, metres.
    pub body_mirror_distance_m: f32,
    /// A move of the grapple's aim point beyond this between ticks is a new target, metres, drawn
    /// there at once rather than swept across.
    pub(crate) ticked_jump_m: f32,
    /// How close the off-hand's hit must be to the grapple's point for the highlight to take its
    /// surface normal, metres.
    pub(crate) surface_match_m: f32,
    /// The grapple highlight's ring's size, radians seen from the head.
    pub(crate) grapple_ring_rad: f32,
    /// The aim assist's lock ring's size against the grapple's ring.
    pub(crate) assist_ring_scale: f32,
    /// The sight tuning marker's ring's size, radians seen from the head.
    pub(crate) marker_ring_rad: f32,
    /// The sight tuning marker's dots' size, radians seen from the head.
    pub(crate) marker_dot_rad: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(CONTROLS.get().validate(), Ok(()));
    }
}
