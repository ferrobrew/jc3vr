//! The handlebars' motion: they turn about their middle with the holding grips.
//!
//! The grips measure the bars' turn ([`bar_turn`]), but a hand rarely takes the bars exactly where
//! they rest: it grabs a few centimetres in front of or below its end, and one hand alone measures
//! the bars as if the other end mirrored it through the middle. So the measure is latched when the
//! hands that hold the bars change, and the bars move from where they were by the change the grips
//! measure since ([`BarsLatch`]). A grab, a second hand joining, or one letting go therefore never
//! jumps the bars or biases them.
//!
//! The bars only turn. Their turn past a small deadzone, shaped by a response exponent, is the
//! steering ([`steer_for_turn`]), and the lean is the same value: the gamepad's one stick writes the
//! motorcycle's steering and its lean together, so the bars do as the stick does. A hand's height
//! on the bars works nothing, so the grip that drops as a wrist turns the bars neither leans nor
//! steers.

use body_rig::angle::wrap;
use glam::{Quat, Vec3};

use crate::controllers::{
    frame::Hand,
    vehicles::{
        physical::{ControlState, Holding, Placement},
        tuning::HandlebarsSpec,
        wheel::BarsFrame,
    },
};

/// The handlebars as the holding hands took them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct BarsLatch {
    /// Which hands held the bars when the latch was taken, indexed by
    /// [`Hand::index`](crate::controllers::frame::Hand::index).
    holders: [bool; 2],
    /// The bars' turn then, radians.
    bars: f32,
    /// What the holding grips measured then.
    measured: f32,
}

impl BarsLatch {
    /// The bars' turn for the grips measuring `measured` now, within the lock.
    fn bars(&self, measured: f32, spec: &HandlebarsSpec) -> f32 {
        let lock = lock(spec);
        (self.bars + wrap(measured - self.measured)).clamp(-lock, lock)
    }
}

/// Move the held bars with the holding grips, and return the steering they give, `None` with no
/// hand on them.
pub(super) fn step_bars(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
) -> Option<f32> {
    let spec = &placement.config.handlebars;
    let grips = holding.map(|h| h.map(|(_, point)| point));
    if grips.iter().all(Option::is_none) {
        state.bars = 0.0;
        state.bars_latch = None;
        return None;
    }
    // Grips the bars' turn cannot be measured from (a hand on the pivot) leave the bars where they
    // are.
    if let Some(measured) = bar_turn(placement.bars.centre, grips) {
        let holders = grips.map(|g| g.is_some());
        let latch = match state.bars_latch {
            Some(latch) if latch.holders == holders => latch,
            _ => BarsLatch {
                holders,
                bars: state.bars,
                measured,
            },
        };
        state.bars_latch = Some(latch);
        state.bars = latch.bars(measured, spec);
    }
    Some(steer_for_turn(state.bars, spec))
}

/// The steering, `-1..=1` with positive right, the bars turned by `turn` radians give: nothing
/// within the deadzone, then the rest of the way to the lock shaped by the response exponent.
pub(super) fn steer_for_turn(turn: f32, spec: &HandlebarsSpec) -> f32 {
    let lock = lock(spec);
    let deadzone = spec.deadzone_deg.to_radians().clamp(0.0, lock * 0.5);
    let magnitude = turn.abs();
    if !magnitude.is_finite() || magnitude <= deadzone {
        return 0.0;
    }
    let share = ((magnitude - deadzone) / (lock - deadzone).max(1e-3)).min(1.0);
    let response = if spec.response.is_finite() {
        spec.response.clamp(0.2, 5.0)
    } else {
        1.0
    };
    turn.signum() * share.powf(response)
}

/// The bars' turn for the holding `grips` as they are now, which may be fresher than the last
/// tick's: moved from the latch by the change the grips measure, or where the last tick left them
/// when the latch is for other hands or the grips measure nothing.
pub(super) fn bars_now(
    state: &ControlState,
    placement: &Placement,
    grips: [Option<Vec3>; 2],
) -> f32 {
    let holders = grips.map(|g| g.is_some());
    state
        .bars_latch
        .filter(|latch| latch.holders == holders)
        .zip(bar_turn(placement.bars.centre, grips))
        .map_or(state.bars, |(latch, measured)| {
            latch.bars(measured, &placement.config.handlebars)
        })
}

/// The handlebars' turn in the horizontal plane, positive with the right grip pulled back, from the
/// holding grips. With one grip the other end mirrors it through the middle. `None` with no grip,
/// or with the grips stacked one above the other.
pub(super) fn bar_turn(centre: Vec3, grips: [Option<Vec3>; 2]) -> Option<f32> {
    let across = match grips {
        [Some(left), Some(right)] => right - left,
        [None, Some(right)] => (right - centre) * 2.0,
        [Some(left), None] => (centre - left) * 2.0,
        [None, None] => return None,
    };
    let flat = across.x.hypot(across.z);
    if flat < 1e-4 || !across.is_finite() {
        return None;
    }
    Some(across.z.atan2(across.x))
}

/// The handlebars' grips, indexed by [`Hand::index`](crate::controllers::frame::Hand::index), for
/// the bars turned by `turn` radians.
pub fn bar_ends(centre: Vec3, half_width: f32, turn: f32) -> [Vec3; 2] {
    let across = Quat::from_rotation_y(-turn) * Vec3::X;
    [centre - across * half_width, centre + across * half_width]
}

/// Where `hand`'s grip at `point` grabs the bars turned by `turn`: the nearest point on the hand's
/// half of the bars, from near the middle to a little past the end, so a hand that reaches for the
/// grip a hand's width off it still takes the bars.
pub(super) fn bar_grab_point(bars: &BarsFrame, turn: f32, hand: Hand, point: Vec3) -> Vec3 {
    let end = bar_ends(bars.centre, bars.half_width, turn)[hand.index()];
    let outward = end - bars.centre;
    let along = (point - bars.centre).dot(outward) / outward.length_squared().max(1e-6);
    let along = if along.is_finite() { along } else { 1.0 };
    bars.centre + outward * along.clamp(GRAB_FROM_MIDDLE, GRAB_PAST_END)
}

/// The share of the half width from the middle where a hand's half of the bars starts to grab.
const GRAB_FROM_MIDDLE: f32 = 0.35;

/// The share of the half width from the middle to where a hand's half of the bars stops grabbing.
const GRAB_PAST_END: f32 = 1.15;

fn lock(spec: &HandlebarsSpec) -> f32 {
    spec.max_steer_deg.to_radians().abs().max(1e-3)
}

