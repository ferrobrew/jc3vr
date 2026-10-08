//! The opt-in physical controls of the driver's seat: a wheel, handlebars, a helicopter cyclic and
//! collective, and a plane's flight stick and throttle. Pure.
//!
//! Every control lives in the cockpit frame, placed relative to the seated character's eyes (see
//! [`crate::controllers::vehicles::config`]); the wheel and the handlebars are placed where the
//! vehicle's own are where they are found ([`crate::controllers::vehicles::wheel`]), so they arrive
//! here as frames already in the cockpit frame. A control is held only while the grip is: a grip
//! press with the hand's grip near the control takes it, and releasing the grip, or moving the hand
//! further than the release distance from where it holds the control, lets go. A hand that merely
//! passes through a control, or rests there with the grip open, takes nothing. While held:
//!
//! - the wheel turns by the holding hands' turns ([`crate::controllers::vehicles::steering`]), and
//!   its angle over the lock is the steering; let go, it returns to centre, steering as it goes,
//!   until the stick steers. The holding hands stay where they took the rim as it turns;
//! - the handlebars pivot about their middle: their turn in the horizontal plane steers and their
//!   roll leans. With one hand on them, the other end mirrors it through the middle. They move by
//!   the grips' change since the hands holding them last changed, so a grab does not jump them;
//! - a stick's top follows the hand across the horizontal plane, and its deflection over the full
//!   deflection drives both of its axes; let go, it springs back to centre;
//! - a lever's handle follows the hand along its axis, and its position over half the travel is its
//!   output, with a detent around neutral. The throttle stays where it is let go.
//!
//! The holding hand is drawn on the control (the anchored tier): [`anchor_point`] gives where its
//! grip sits on the control for the hand's current position, which the hands compose at every rate
//! from the latest controller pose.
//!
//! The wheel's motion is in [`held_wheel`], the handlebars' in [`handlebars`], and the sticks' and
//! levers' in [`sticks`].

mod handlebars;
mod held_wheel;
mod sticks;
#[cfg(test)]
mod tests;

use glam::{Quat, Vec2, Vec3};

pub use crate::controllers::vehicles::physical::{
    handlebars::bar_ends,
    sticks::{lever_handle, lever_value, stick_deflection},
};
use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    frame::Hand,
    mode::VehicleClass,
    vehicles::{
        config::{LeverSpec, PhysicalControlsConfig, StickSpec},
        physical::{
            handlebars::{BarsLatch, bars_now, step_bars},
            held_wheel::step_wheel,
            sticks::{deflection3, step_lever, step_stick},
        },
        steering::{CueArming, WheelCue, WheelGrip},
        wheel::{BarsFrame, WheelFrame},
    },
};

/// A physical control.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ControlKind {
    Wheel,
    Handlebars,
    Cyclic,
    Collective,
    FlightStick,
    Throttle,
}

impl ControlKind {
    pub const fn label(self) -> &'static str {
        match self {
            ControlKind::Wheel => "wheel",
            ControlKind::Handlebars => "handlebars",
            ControlKind::Cyclic => "cyclic",
            ControlKind::Collective => "collective",
            ControlKind::FlightStick => "flight stick",
            ControlKind::Throttle => "throttle",
        }
    }

    /// The controls a vehicle of `class` offers its driver. Cars and boats share their classes with
    /// the ATV, the jetski, and the duck, which steer by handlebars: a vehicle with a steering fork
    /// (`fork`) gets the handlebars in place of the wheel.
    pub const fn for_seat(class: VehicleClass, fork: bool) -> &'static [ControlKind] {
        match class {
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown if fork => {
                &[ControlKind::Handlebars]
            }
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown => &[ControlKind::Wheel],
            VehicleClass::Motorcycle => &[ControlKind::Handlebars],
            VehicleClass::Helicopter => &[ControlKind::Cyclic, ControlKind::Collective],
            VehicleClass::Plane => &[ControlKind::FlightStick, ControlKind::Throttle],
        }
    }

    /// Whether the control's toggle is on.
    pub const fn enabled(self, config: &PhysicalControlsConfig) -> bool {
        match self {
            ControlKind::Wheel => config.wheel.enabled,
            ControlKind::Handlebars => config.handlebars.enabled,
            ControlKind::Cyclic => config.cyclic.enabled,
            ControlKind::Collective => config.collective.enabled,
            ControlKind::FlightStick => config.flight_stick.enabled,
            ControlKind::Throttle => config.throttle.enabled,
        }
    }

    /// Whether both hands may hold the control at once.
    const fn two_handed(self) -> bool {
        matches!(self, ControlKind::Wheel | ControlKind::Handlebars)
    }
}

/// How a hand holds a control.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    pub kind: ControlKind,
    /// The hand's grip relative to the control's handle when it grabbed, for the sticks and levers,
    /// so that a hand that grabs off-centre does not snap the control to itself.
    pub offset: Vec3,
}

/// Why a hand let go of a control.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The hand released its grip.
    GripReleased,
    /// The hand moved away from the control.
    MovedAway,
    /// The other hand took a control only one hand holds.
    TakenOver,
    /// The hand lost tracking, the seat changed, or the control was switched off.
    Gone,
}

impl Release {
    pub const fn label(self) -> &'static str {
        match self {
            Release::GripReleased => "grip released",
            Release::MovedAway => "moved away",
            Release::TakenOver => "taken by the other hand",
            Release::Gone => "gone",
        }
    }
}

/// A grab or a release, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    Grabbed(Hand, ControlKind),
    Released(Hand, ControlKind, Release),
}

/// The controls' moving parts.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ControlState {
    /// What each hand holds, indexed by [`Hand::index`].
    pub holds: [Option<Hold>; 2],
    /// The wheel's angle, radians, positive clockwise as the driver sees it.
    pub wheel_angle: f32,
    /// Each holding hand's hold on the wheel: the wheel it took, its place on the rim, and where
    /// it was on the last tick.
    wheel_grips: [Option<WheelGrip>; 2],
    /// How far the holding hands have turned the wheel past its lock, radians clockwise.
    wheel_past_lock: f32,
    /// Which of the wheel's haptic cues may fire.
    wheel_cues: CueArming,
    /// The handlebars' turn and roll, radians.
    pub bars: Vec2,
    /// The bars and the grips' measure of them when the hands holding them last changed.
    bars_latch: Option<BarsLatch>,
    /// The stick's deflection in the horizontal plane (+X right, +Y back), metres.
    pub stick: Vec2,
    /// The collective's and the throttle's positions, `-1..=1`.
    pub levers: [f32; 2],
}

impl ControlState {
    pub const fn new() -> Self {
        Self {
            holds: [None; 2],
            wheel_angle: 0.0,
            wheel_grips: [None; 2],
            wheel_past_lock: 0.0,
            wheel_cues: CueArming::new(),
            bars: Vec2::ZERO,
            bars_latch: None,
            stick: Vec2::ZERO,
            levers: [0.0; 2],
        }
    }
}

/// The values the held controls produce this tick; `None` for a control that is not in use, so
/// that the stick bindings keep that axis.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Outputs {
    /// `-1..=1`, positive right.
    pub steer: Option<f32>,
    /// `-1..=1`, positive leaning right.
    pub lean: Option<f32>,
    /// The cyclic: +X rolls right, +Y pushes forward.
    pub cyclic: Option<Vec2>,
    /// `-1..=1`, positive climbing.
    pub collective: Option<f32>,
    /// The flight stick: +X rolls right, +Y pushes forward (nose down).
    pub flight_stick: Option<Vec2>,
    /// `-1..=1`, positive accelerating.
    pub throttle: Option<f32>,
    /// The cue the held wheel gives the hands holding it this tick.
    pub wheel_cue: Option<WheelCue>,
}

impl Outputs {
    pub const NONE: Outputs = Outputs {
        steer: None,
        lean: None,
        cyclic: None,
        collective: None,
        flight_stick: None,
        throttle: None,
        wheel_cue: None,
    };
}

/// The controls placed for one seat: the wheel and the handlebars where the vehicle's are, and the
/// sticks and levers from the specs with their sides mirrored for the handedness.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Placement {
    pub config: PhysicalControlsConfig,
    pub dominant: Hand,
    pub wheel: WheelFrame,
    pub bars: BarsFrame,
}

impl Placement {
    /// Every control placed from the specs.
    pub fn new(config: PhysicalControlsConfig, dominant: Hand) -> Self {
        Self {
            wheel: WheelFrame::from_spec(&config.wheel),
            bars: BarsFrame::from_spec(&config.handlebars),
            config,
            dominant,
        }
    }

    pub(super) fn mirror(&self, v: Vec3) -> Vec3 {
        match self.dominant {
            Hand::Right => v,
            Hand::Left => Vec3::new(-v.x, v.y, v.z),
        }
    }

    pub(super) fn stick(&self, kind: ControlKind) -> Option<StickSpec> {
        match kind {
            ControlKind::Cyclic => Some(self.config.cyclic),
            ControlKind::FlightStick => Some(self.config.flight_stick),
            _ => None,
        }
    }

    pub(super) fn lever(&self, kind: ControlKind) -> Option<(usize, LeverSpec)> {
        match kind {
            ControlKind::Collective => Some((0, self.config.collective)),
            ControlKind::Throttle => Some((1, self.config.throttle)),
            _ => None,
        }
    }

    /// The point `hand`'s grip grabs `kind` at, given the hand's grip `point`: the nearest point on
    /// the wheel's rim, the hand's own end of the handlebars, or a stick's or lever's handle.
    pub fn grab_point(
        &self,
        state: &ControlState,
        kind: ControlKind,
        hand: Hand,
        point: Vec3,
    ) -> Option<Vec3> {
        match kind {
            ControlKind::Wheel => {
                let angle = self.wheel.hand_angle(point).unwrap_or(0.0);
                Some(self.wheel.rim_point(angle))
            }
            ControlKind::Handlebars => {
                let ends = bar_ends(self.bars.centre, self.bars.half_width, state.bars);
                Some(ends[hand.index()])
            }
            kind => {
                if let Some(spec) = self.stick(kind) {
                    return Some(self.mirror(spec.top_m) + deflection3(state.stick));
                }
                let (index, spec) = self.lever(kind)?;
                Some(lever_handle(
                    &spec,
                    self.mirror(spec.rest_m),
                    state.levers[index],
                ))
            }
        }
    }
}

/// The grab volumes of `kinds` for each tracked hand's grip in `grips`, indexed by
/// [`Hand::index`]: one sphere per hand and control, at the point that hand would grab.
pub fn candidates(
    placement: &Placement,
    state: &ControlState,
    kinds: &[ControlKind],
    grips: [Option<Vec3>; 2],
) -> Vec<Candidate> {
    let mut out = Vec::with_capacity(kinds.len() * 2);
    for hand in Hand::BOTH {
        let Some(point) = grips[hand.index()] else {
            continue;
        };
        for &kind in kinds {
            if let Some(centre) = placement.grab_point(state, kind, hand, point) {
                out.push(Candidate {
                    volume: GrabVolume::VehicleControl(kind),
                    centre,
                    radius: placement.config.grab_radius_m,
                    hand: Some(hand),
                });
            }
        }
    }
    out
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepInputs<'a> {
    /// The controls the seat offers and whose toggles are on.
    pub kinds: &'a [ControlKind],
    /// Each hand's grip position in the cockpit frame, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Each hand's grip orientation in the cockpit frame, `None` while untracked.
    pub orientations: [Option<Quat>; 2],
    /// Whether each hand's grip is held this tick. A control is let go when its hand's grip is not.
    pub held: [bool; 2],
    /// The control each hand's grip press latched as a grab this tick.
    pub grabs: [Option<ControlKind>; 2],
    /// Whether the stick's turn axis is out of its deadzone this tick.
    pub stick_steers: bool,
    pub dt: f32,
}

/// Advance the controls by one input tick: act on the grabs, move the held controls with their
/// hands, let go of controls the hands moved away from, and return the outputs.
pub fn step(
    state: &mut ControlState,
    placement: &Placement,
    inputs: &StepInputs,
    events: &mut Vec<ControlEvent>,
) -> Outputs {
    let dt = if inputs.dt.is_finite() {
        inputs.dt.max(0.0)
    } else {
        0.0
    };
    for hand in Hand::BOTH {
        let i = hand.index();
        let held = state.holds[i];
        let gone = held.is_some_and(|h| !inputs.kinds.contains(&h.kind))
            || (held.is_some() && inputs.grips[i].is_none());
        if gone {
            release(state, hand, Release::Gone, events);
            continue;
        }
        let Some(kind) = inputs.grabs[i] else {
            if held.is_some() && !inputs.held[i] {
                release(state, hand, Release::GripReleased, events);
            }
            continue;
        };
        // A grab while holding is a release and a fresh press between two ticks.
        if held.is_some() {
            release(state, hand, Release::GripReleased, events);
        }
        if let Some(point) = inputs.grips[i]
            && inputs.kinds.contains(&kind)
        {
            grab(
                state,
                placement,
                hand,
                kind,
                (point, inputs.orientations[i]),
                events,
            );
        }
    }

    let mut out = Outputs::default();
    let held_by = |state: &ControlState, kind: ControlKind| {
        Hand::BOTH.map(|hand| {
            state.holds[hand.index()]
                .filter(|h| h.kind == kind)
                .and_then(|h| Some((h, inputs.grips[hand.index()]?)))
        })
    };
    for &kind in inputs.kinds {
        let holding = held_by(state, kind);
        match kind {
            ControlKind::Wheel => {
                let (steer, cue) = step_wheel(
                    state,
                    placement,
                    holding,
                    (inputs.orientations, inputs.stick_steers),
                    dt,
                );
                out.steer = steer;
                out.wheel_cue = cue;
            }
            ControlKind::Handlebars => {
                if let Some((steer, lean)) = step_bars(state, placement, holding) {
                    out.steer = Some(steer);
                    out.lean = Some(lean);
                }
            }
            ControlKind::Cyclic | ControlKind::FlightStick => {
                let value = step_stick(state, placement, kind, holding);
                if kind == ControlKind::Cyclic {
                    out.cyclic = value;
                } else {
                    out.flight_stick = value;
                }
            }
            ControlKind::Collective | ControlKind::Throttle => {
                let value = step_lever(state, placement, kind, holding);
                if kind == ControlKind::Collective {
                    out.collective = value;
                } else {
                    out.throttle = value;
                }
            }
        }
    }

    for hand in Hand::BOTH {
        let i = hand.index();
        let (Some(hold), Some(point)) = (state.holds[i], inputs.grips[i]) else {
            continue;
        };
        let anchored = anchor_point(state, placement, hold, hand, inputs.grips);
        if anchored.is_some_and(|a| a.distance(point) > placement.config.release_distance_m) {
            release(state, hand, Release::MovedAway, events);
        }
    }
    out
}

/// Let go of every control, as when the seat or the scheme changes.
pub fn release_all(state: &mut ControlState, events: &mut Vec<ControlEvent>) {
    for hand in Hand::BOTH {
        release(state, hand, Release::Gone, events);
    }
}

/// Where `hand`'s grip sits on the control it holds, for the hand's current grip positions
/// `grips`: the rim point at the hand's angle, the hand's end of the bars as both hands turn them,
/// or the stick's or lever's handle where the hand has moved it, within its travel.
pub fn anchor_point(
    state: &ControlState,
    placement: &Placement,
    hold: Hold,
    hand: Hand,
    grips: [Option<Vec3>; 2],
) -> Option<Vec3> {
    let point = grips[hand.index()]?;
    match hold.kind {
        // The hand stays where it took the rim as the wheel turns, so at the lock it stays on the
        // stopped wheel rather than running on round the rim.
        ControlKind::Wheel => {
            let angle = state.wheel_grips[hand.index()]
                .and_then(|grip| grip.rim_angle(state.wheel_angle))
                .or_else(|| placement.wheel.hand_angle(point))?;
            Some(placement.wheel.rim_point(angle))
        }
        ControlKind::Handlebars => {
            let bars = bars_now(
                state,
                placement,
                grips_holding(state, ControlKind::Handlebars, grips),
            );
            Some(bar_ends(placement.bars.centre, placement.bars.half_width, bars)[hand.index()])
        }
        kind => {
            if let Some(spec) = placement.stick(kind) {
                let top = placement.mirror(spec.top_m);
                let deflection = stick_deflection(&spec, top, point - hold.offset);
                return Some(top + deflection3(deflection) + hold.offset);
            }
            let (_, spec) = placement.lever(kind)?;
            let rest = placement.mirror(spec.rest_m);
            let value = lever_value(&spec, rest, point - hold.offset);
            Some(lever_handle(&spec, rest, value) + hold.offset)
        }
    }
}

fn grab(
    state: &mut ControlState,
    placement: &Placement,
    hand: Hand,
    kind: ControlKind,
    (point, orientation): (Vec3, Option<Quat>),
    events: &mut Vec<ControlEvent>,
) {
    let other = hand.other();
    if !kind.two_handed() && state.holds[other.index()].is_some_and(|h| h.kind == kind) {
        release(state, other, Release::TakenOver, events);
    }
    let offset = match kind {
        ControlKind::Wheel | ControlKind::Handlebars => Vec3::ZERO,
        kind => placement
            .grab_point(state, kind, hand, point)
            .map_or(Vec3::ZERO, |handle| point - handle),
    };
    if kind == ControlKind::Wheel {
        state.wheel_grips[hand.index()] = Some(WheelGrip::new(
            &placement.wheel,
            point,
            orientation,
            state.wheel_angle,
        ));
    }
    state.holds[hand.index()] = Some(Hold { kind, offset });
    events.push(ControlEvent::Grabbed(hand, kind));
}

fn release(state: &mut ControlState, hand: Hand, why: Release, events: &mut Vec<ControlEvent>) {
    let Some(hold) = state.holds[hand.index()].take() else {
        return;
    };
    state.wheel_grips[hand.index()] = None;
    events.push(ControlEvent::Released(hand, hold.kind, why));
}

pub(super) type Holding = [Option<(Hold, Vec3)>; 2];

fn grips_holding(
    state: &ControlState,
    kind: ControlKind,
    grips: [Option<Vec3>; 2],
) -> [Option<Vec3>; 2] {
    Hand::BOTH.map(|hand| {
        grips[hand.index()].filter(|_| state.holds[hand.index()].is_some_and(|h| h.kind == kind))
    })
}
