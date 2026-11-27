//! The gameplay layer's tuning, read from the shipped `tuning/gameplay.toml` ([`GAMEPLAY`]): the
//! bindings' press thresholds, hysteresis, and holds, the controller pointer's aim hold, scroll,
//! click slop, and laser, the controls summary's layout, and the online hooks' stall threshold.

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use jc3vr_view::controllers::overlay::primitive::Rgba;
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The gameplay layer's tuning file.
    pub static GAMEPLAY: GameplayTuning = "gameplay.toml";
}

/// The gameplay layer's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayTuning {
    pub bindings: BindingsTuning,
    pub pointer: PointerTuning,
    pub summary: SummaryTuning,
    pub online: OnlineTuning,
}

impl Tuning for GameplayTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.bindings.validate()?;
        self.pointer.validate()?;
        self.summary.validate()?;
        self.online.validate()
    }
}

/// The bindings' thresholds, hysteresis, and holds (see [`crate::controllers::bindings`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingsTuning {
    /// How far from the line between two pad zones, in the pad's `-1..=1` axes, a press must land
    /// to count for a zone.
    pub(crate) pad_zone_margin: f32,
    /// The share of the menu stick's latching push that the push along a latched direction must
    /// stay at or above for the direction to stay latched.
    pub(crate) menu_stick_hold_share: f32,
    /// A resting stick magnitude that always counts as neutral for a hand's recovery, even when a
    /// consumer's deadzone is smaller. Worn sticks rest this far off centre, and a stick this close
    /// to centre can only feed a continuous consumer (a slow walk or turn), never an edge.
    pub(crate) stick_neutral_floor: f32,
    /// How far from the locked line's hook the grapple's aim must be for a tap to fire the hook
    /// again, metres: past the aim's own wander on the hooked surface.
    pub(crate) grapple_retarget_m: f32,
    /// Leaving a native helicopter hang.
    pub rail: RailReleaseTuning,
    /// Pulling into a helicopter from a hang.
    pub(crate) heli_hang: HeliHangTuning,
    /// The seated exit's parachute bail.
    pub bail: BailTuning,
}

impl BindingsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("bindings.pad_zone_margin", self.pad_zone_margin, 0.0, 0.99)?;
        range(
            "bindings.menu_stick_hold_share",
            self.menu_stick_hold_share,
            0.0,
            1.0,
        )?;
        range(
            "bindings.stick_neutral_floor",
            self.stick_neutral_floor,
            0.0,
            1.0,
        )?;
        nonnegative(
            "bindings.grapple_retarget_m",
            self.grapple_retarget_m,
            100.0,
        )?;
        positive("bindings.rail.step_m", self.rail.step_m, 2.0)?;
        range("bindings.rail.stick_push", self.rail.stick_push, 0.0, 1.0)?;
        range("bindings.heli_hang.press", self.heli_hang.press, 0.0, 1.0)?;
        self.bail.validate()
    }
}

/// When lateral movement leaves a native helicopter hang
/// ([`crate::controllers::bindings::rail`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RailReleaseTuning {
    /// How far the head must move sideways from where the hang began, metres.
    pub step_m: f32,
    /// How far the move stick must be pushed sideways, `0..=1`.
    pub(crate) stick_push: f32,
}

/// When an analog hold of the helicopter entry counts as pressed during a hang.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeliHangTuning {
    /// The value at or above which the hold counts, `0..=1`.
    pub press: f32,
}

/// The seated exit's parachute bail ([`crate::controllers::bindings::bail`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BailTuning {
    /// The speed above the game's bail minimum at which the vehicle starts to count as fast enough
    /// to bail, metres per second. The game checks the speed only once its own hold time has
    /// passed after the press became long, so the margin leaves room to slow a little in between.
    pub(crate) enter_margin_mps: f32,
    /// The speed above the game's bail minimum below which a fast vehicle stops counting as fast
    /// enough to bail, metres per second. Smaller than the enter margin, for the hysteresis, and
    /// not below the game's minimum, where the bail would be refused.
    pub(crate) leave_margin_mps: f32,
    /// The shortest the bail's `OPEN_PARACHUTE` is held, seconds, once a bail begins, even when the
    /// press is let go sooner.
    pub(crate) hold_min_s: f32,
    /// The longest the bail's `OPEN_PARACHUTE` is held, seconds, however long the press lasts.
    pub(crate) hold_max_s: f32,
}

impl BailTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "bindings.bail.leave_margin_mps",
            self.leave_margin_mps,
            50.0,
        )?;
        range(
            "bindings.bail.enter_margin_mps",
            self.enter_margin_mps,
            self.leave_margin_mps,
            50.0,
        )?;
        nonnegative("bindings.bail.hold_min_s", self.hold_min_s, 10.0)?;
        range(
            "bindings.bail.hold_max_s",
            self.hold_max_s,
            self.hold_min_s,
            10.0,
        )
    }
}

/// The controller pointer over a panel ([`crate::controllers::bindings::pointer`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointerTuning {
    /// How far a controller's held trigger press may wander from where it began, as a fraction of
    /// the panel's height, and still click: until the hit leaves this radius the pointer stays at
    /// the press, so a hand's tremor never turns a press into a drag. Past it the press drags from
    /// the live hit.
    pub click_slop: f32,
    pub(crate) aim_hold: AimHoldTuning,
    pub scroll: ScrollTuning,
    pub(crate) laser: LaserTuning,
}

impl PointerTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("pointer.click_slop", self.click_slop, 1.0)?;
        self.aim_hold.validate()?;
        self.scroll.validate()?;
        self.laser.validate()
    }
}

/// The trigger values of a pointer's aim hold, `0..=1`: a light pull pins the cursor and only a
/// full pull clicks.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AimHoldTuning {
    /// The value at or above which the cursor pins.
    pub(crate) pin_press: f32,
    /// The value below which the cursor is freed.
    pub(crate) pin_release: f32,
    /// The value at or above which a click begins: a full pull.
    pub(crate) click_press: f32,
    /// The value below which a click ends, far enough below the press that a held click does not
    /// chatter.
    pub(crate) click_release: f32,
}

impl AimHoldTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("pointer.aim_hold.pin_release", self.pin_release, 0.0, 1.0)?;
        range(
            "pointer.aim_hold.pin_press",
            self.pin_press,
            self.pin_release,
            1.0,
        )?;
        range(
            "pointer.aim_hold.click_release",
            self.click_release,
            self.pin_press,
            1.0,
        )?;
        range(
            "pointer.aim_hold.click_press",
            self.click_press,
            self.click_release,
            1.0,
        )
    }
}

/// The thumbstick's scroll of a panel.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollTuning {
    /// The deadzone of each thumbstick axis before it scrolls, as a fraction of full deflection.
    pub deadzone: f32,
    /// The scroll speed at full deflection, egui points per second.
    pub speed: f32,
    /// The longest interval one tick's scroll covers, seconds, so a hitch does not jump the view.
    pub max_dt_s: f32,
}

impl ScrollTuning {
    fn validate(&self) -> Result<(), Invalid> {
        // The deadzone divides the deflection beyond it by what remains of the axis.
        range("pointer.scroll.deadzone", self.deadzone, 0.0, 0.99)?;
        nonnegative("pointer.scroll.speed", self.speed, 100_000.0)?;
        nonnegative("pointer.scroll.max_dt_s", self.max_dt_s, 1.0)
    }
}

/// The pointing hand's laser to the panel.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaserTuning {
    /// The line's width, metres.
    pub width_m: f32,
    /// The line's colour, linear RGBA.
    pub color: Rgba,
    /// The line's alpha where something in the world hides it.
    pub occluded_alpha: f32,
    /// The laser's time to live, milliseconds: a few input ticks, so it disappears on its own when
    /// they stop.
    pub ttl_ms: u64,
}

impl LaserTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("pointer.laser.width_m", self.width_m, 0.1)?;
        for (name, value) in ["r", "g", "b", "a"].iter().zip(self.color) {
            range(&format!("pointer.laser.color.{name}"), value, 0.0, 1.0)?;
        }
        range(
            "pointer.laser.occluded_alpha",
            self.occluded_alpha,
            0.0,
            1.0,
        )?;
        if self.ttl_ms == 0 {
            return Err(Invalid(
                "pointer.laser.ttl_ms must be positive, got 0".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The online hooks' timing (`crate::hooks::online`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnlineTuning {
    /// How long the Steam platform system's update may take before it is logged on its own,
    /// milliseconds. The call takes a fraction of a millisecond on an ordinary tick, and the stalls
    /// it carries are hundreds.
    pub(crate) slow_steam_callbacks_ms: u64,
}

impl OnlineTuning {
    fn validate(&self) -> Result<(), Invalid> {
        if self.slow_steam_callbacks_ms == 0 {
            return Err(Invalid(
                "online.slow_steam_callbacks_ms must be positive, got 0".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The controls summary's layout ([`crate::controllers::summary::content::metrics`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryTuning {
    /// The smallest row the panel stays readable at, pixels.
    pub(crate) min_row_px: f32,
    /// The largest row worth drawing, pixels.
    pub(crate) max_row_px: f32,
    /// The margin around the content, in rows.
    pub(crate) margin_rows: f32,
    /// The heading's height, in rows.
    pub(crate) heading_rows: f32,
    /// A group header's height (the controller glyph), in rows.
    pub(crate) group_rows: f32,
    /// A row's glyph size, in rows.
    pub(crate) glyph_rows: f32,
    /// The font's size, in rows.
    pub(crate) font_rows: f32,
}

impl SummaryTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("summary.min_row_px", self.min_row_px, 1000.0)?;
        range(
            "summary.max_row_px",
            self.max_row_px,
            self.min_row_px,
            1000.0,
        )?;
        positive("summary.margin_rows", self.margin_rows, 10.0)?;
        positive("summary.heading_rows", self.heading_rows, 10.0)?;
        positive("summary.group_rows", self.group_rows, 10.0)?;
        positive("summary.glyph_rows", self.glyph_rows, 10.0)?;
        positive("summary.font_rows", self.font_rows, 10.0)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(GAMEPLAY.get().validate(), Ok(()));
    }
}
