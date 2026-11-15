//! The physical controls of the driver's seat: a wheel, handlebars, a helicopter cyclic and
//! collective, and a plane's flight stick and throttle. Pure.
//!
//! Every control lives in the cockpit frame, placed relative to the seated character's eyes (see
//! [`crate::controllers::vehicles::config`]); the wheel and the handlebars are placed where the
//! vehicle's own are where they are found ([`crate::controllers::vehicles::wheel`]), so they arrive
//! here as frames already in the cockpit frame. A control is held only while the hand stays closed
//! ([`ClosedHand`](crate::controllers::bindings::controls::ClosedHand)): a grip press with the
//! hand's grip near the control takes it, and only the hand opening lets go. A holding hand that
//! drifts off the control keeps working it from its position and rotation however far it goes. A
//! hand that merely passes through a control, or rests there with the grip open, takes nothing.
//! While held:
//!
//! - the wheel turns by the holding hands' turns ([`crate::controllers::vehicles::steering`]), and
//!   its angle over the lock is the steering; let go, it returns to centre, steering as it goes,
//!   until the stick steers. The holding hands stay where they took the rim as it turns;
//! - the handlebars turn about their middle in the horizontal plane, and their turn past a small
//!   deadzone, shaped by a response exponent, steers and leans together, as the gamepad's stick
//!   does. With one hand on them, the other end mirrors it through the middle. They move by the
//!   grips' change since the hands holding them last changed, so a grab does not jump them;
//! - a stick's top follows the hand across the horizontal plane, and its deflection over the full
//!   deflection, past a small deadzone, drives both of its axes; let go, a centring spring returns
//!   it, still flying the vehicle on its way. The stick's twist about its shaft works the rudder in
//!   a plane and the yaw in a helicopter;
//! - a lever's handle follows the hand along its axis, and its position over half the travel is its
//!   output, with a detent around neutral. The throttle stays where it is let go, and has an
//!   afterburner gate at the top of its travel.
//!
//! The cockpit's buttons ([`buttons`]) are worked by hand too: the horn pressed on the wheel's hub,
//! a nitrous button on the dash, the handlebars' right grip twisted forward for the throttle and
//! the left for the brake, and the collective twisted for the nitrous.
//!
//! The sticks and levers give the holding hand a haptic cue at the ends of their travel, the
//! levers at neutral, and the throttle going into and out of its gate ([`cues`]).
//!
//! The holding hand is drawn on the control (the anchored tier): [`anchor_point`] gives where its
//! grip sits on the control for the hand's current position, which the hands compose at every rate
//! from the latest controller pose.
//!
//! The wheel's motion is in [`held_wheel`], the handlebars' in [`handlebars`], the sticks' in
//! [`sticks`], and the levers' in [`levers`].

pub mod buttons;
pub mod calibration;
pub mod cues;
mod handlebars;
mod held_wheel;
pub mod levers;
pub mod native;
pub mod sticks;
#[cfg(test)]
mod tests;
pub(crate) mod visual;
pub mod yoke;

use glam::{Quat, Vec2, Vec3};

pub use crate::controllers::vehicles::physical::{
    cues::ControlCue, handlebars::bar_ends, sticks::stick_deflection,
};
use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    mode::VehicleClass,
    vehicles::{
        config::PhysicalControlsConfig,
        physical::{
            buttons::{ButtonEvent, ButtonInputs, ButtonPlacement, ButtonState, step_buttons},
            cues::TravelCues,
            handlebars::{BarsLatch, bar_grab_point, bars_now, step_bars},
            held_wheel::step_wheel,
            levers::{ThrottleState, step_lever, throttle_constrain, throttle_zero},
            sticks::{deflection3, step_stick},
            yoke::{YokeFrame, YokeState, grab_yoke, nearest_horn, step_yoke},
        },
        steering::{CueArming, WheelCue, WheelGrip},
        tuning::{LeverSpec, PhysicalControlsTuning, StickSpec},
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
    /// A plane's control wheel, in place of the flight stick where the cockpit models one.
    Yoke,
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
            ControlKind::Yoke => "yoke",
            ControlKind::Throttle => "throttle",
        }
    }

    /// The controls a vehicle of `class` offers its driver. Cars and boats share their classes with
    /// the ATV, the jetski, and the duck, which steer by handlebars: a vehicle with a steering fork
    /// (`fork`) gets the handlebars in place of the wheel. A plane whose cockpit has a control wheel
    /// (`yoke`) gets the yoke in place of the flight stick.
    pub const fn for_seat(class: VehicleClass, fork: bool, yoke: bool) -> &'static [ControlKind] {
        match class {
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown if fork => {
                &[ControlKind::Handlebars]
            }
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown => &[ControlKind::Wheel],
            VehicleClass::Motorcycle => &[ControlKind::Handlebars],
            VehicleClass::Helicopter => &[ControlKind::Cyclic, ControlKind::Collective],
            VehicleClass::Plane if yoke => &[ControlKind::Yoke, ControlKind::Throttle],
            VehicleClass::Plane => &[ControlKind::FlightStick, ControlKind::Throttle],
            // The mech walks; it has no controls to hold.
            VehicleClass::Mech => &[],
        }
    }

    /// Whether the control's toggle is on.
    pub const fn enabled(self, config: &PhysicalControlsConfig) -> bool {
        match self {
            ControlKind::Wheel => config.wheel.enabled,
            ControlKind::Handlebars => config.handlebars,
            ControlKind::Cyclic | ControlKind::FlightStick | ControlKind::Yoke => config.sticks,
            ControlKind::Collective | ControlKind::Throttle => config.levers,
        }
    }

    /// Whether both hands may hold the control at once.
    const fn two_handed(self) -> bool {
        matches!(
            self,
            ControlKind::Wheel | ControlKind::Handlebars | ControlKind::Yoke
        )
    }
}

/// How a hand holds a control.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    pub kind: ControlKind,
    /// The hand's grip relative to the control's handle when it grabbed, for the sticks and levers,
    /// so that a hand that grabs off-centre does not snap the control to itself.
    pub offset: Vec3,
    /// The hand's grip orientation when it grabbed, for the stick's twist.
    pub orientation: Option<Quat>,
}

/// Why a hand let go of a control.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The hand released its grip.
    GripReleased,
    /// The other hand took a control only one hand holds.
    TakenOver,
    /// The hand lost tracking, the seat changed, or the control was switched off.
    Gone,
}

impl Release {
    pub const fn label(self) -> &'static str {
        match self {
            Release::GripReleased => "grip released",
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
    /// A cockpit button or twist grip pressed or let go.
    Button(ButtonEvent),
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
    /// The handlebars' turn, radians, positive with the right grip pulled back.
    pub bars: f32,
    /// The bars and the grips' measure of them when the hands holding them last changed.
    bars_latch: Option<BarsLatch>,
    /// Which of the bars' haptic cues may fire.
    bars_cues: CueArming,
    /// How long each hand's grip on the handlebars has been open, by [`Hand::index`], while the
    /// grace keeps the bars in it ([`BARS_GRIP_GRACE_S`]).
    bars_grace: [Option<f32>; 2],
    /// The stick's deflection in the horizontal plane (+X right, +Y back), metres.
    pub stick: Vec2,
    /// The collective's and the throttle's positions from their zeros, in lever positions (the
    /// whole travel spans two): the collective's zero is its middle, the throttle's its zero gate.
    pub levers: [f32; 2],
    /// The throttle's zero gate and its cues.
    pub throttle: ThrottleState,
    /// The yoke's turn and pull, and the hands' holds on it.
    pub yoke: YokeState,
    /// Which of the stick's cues may fire.
    stick_cues: TravelCues,
    /// Which of each lever's cues may fire, indexed as [`levers`](Self::levers).
    lever_cues: [TravelCues; 2],
    /// The cockpit's buttons and twist grips.
    pub buttons: ButtonState,
}

impl ControlState {
    pub const fn new() -> Self {
        Self {
            holds: [None; 2],
            wheel_angle: 0.0,
            wheel_grips: [None; 2],
            wheel_past_lock: 0.0,
            wheel_cues: CueArming::new(),
            bars: 0.0,
            bars_latch: None,
            bars_cues: CueArming::new(),
            bars_grace: [None; 2],
            stick: Vec2::ZERO,
            levers: [0.0; 2],
            throttle: ThrottleState::new(),
            yoke: YokeState::new(),
            stick_cues: TravelCues::new(),
            lever_cues: [TravelCues::new(); 2],
            buttons: ButtonState::new(),
        }
    }
}

/// The values the held controls produce this tick; `None` for a control that is not in use, so
/// that the stick bindings keep that axis.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Outputs {
    /// `-1..=1`, positive right.
    pub steer: Option<f32>,
    /// `-1..=1`, positive leaning right: the handlebars' steering, as the gamepad's one stick
    /// writes both.
    pub lean: Option<f32>,
    /// The cyclic: +X rolls right, +Y pushes forward.
    pub cyclic: Option<Vec2>,
    /// `-1..=1`, positive climbing.
    pub collective: Option<f32>,
    /// The flight stick: +X rolls right, +Y pushes forward (nose down).
    pub flight_stick: Option<Vec2>,
    /// The held stick's twist: `-1..=1`, positive yawing right. It is the flight stick's rudder in
    /// a plane and the cyclic's yaw in a helicopter.
    pub rudder: Option<f32>,
    /// `-1..=1`, positive accelerating.
    pub throttle: Option<f32>,
    /// Whether the throttle is through its afterburner gate.
    pub afterburner: bool,
    /// The cue the held wheel gives the hands holding it this tick.
    pub wheel_cue: Option<WheelCue>,
    /// The cue the held stick gives its hand this tick.
    pub stick_cue: Option<ControlCue>,
    /// The cue the held lever gives its hand this tick.
    pub lever_cue: Option<ControlCue>,
    /// Whether a hand presses the horn on the wheel's hub.
    pub horn: bool,
    /// Whether a hand presses the nitrous button or twists the collective's grip.
    pub nitrous: bool,
    /// The throttle the handlebars' right grip opens, `0..=1`, while the right hand holds it.
    pub bike_throttle: Option<f32>,
    /// The brake the handlebars' left grip applies, `0..=1`, while the left hand holds it.
    pub bike_brake: Option<f32>,
}

impl Outputs {
    pub const NONE: Outputs = Outputs {
        steer: None,
        lean: None,
        cyclic: None,
        collective: None,
        flight_stick: None,
        rudder: None,
        throttle: None,
        afterburner: false,
        wheel_cue: None,
        stick_cue: None,
        lever_cue: None,
        horn: false,
        nitrous: false,
        bike_throttle: None,
        bike_brake: None,
    };
}

/// The controls placed for one seat: the wheel and the handlebars where the vehicle's are, and the
/// aircraft controls measured from the cockpit mesh when available. Only fallback specs mirror
/// their sides for handedness.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Placement {
    pub config: PhysicalControlsTuning,
    pub dominant: Hand,
    pub wheel: WheelFrame,
    pub bars: BarsFrame,
    pub native_stick: Option<native::NativeStick>,
    /// The controls measured within a merged cockpit mesh, without moving that mesh.
    pub cockpit: Option<native::CockpitFit>,
}

impl Placement {
    /// Every control placed from the specs.
    pub fn new(config: PhysicalControlsTuning, dominant: Hand) -> Self {
        Self {
            wheel: WheelFrame::from_spec(&config.wheel),
            bars: BarsFrame::from_spec(&config.handlebars),
            config,
            dominant,
            native_stick: None,
            cockpit: None,
        }
    }

    pub(super) fn mirror(&self, v: Vec3) -> Vec3 {
        match self.dominant {
            Hand::Right => v,
            Hand::Left => Vec3::new(-v.x, v.y, v.z),
        }
    }

    /// The yoke, where the cockpit's model has one.
    pub fn yoke(&self) -> Option<YokeFrame> {
        self.cockpit
            .filter(|fit| fit.asset.stick_kind == ControlKind::Yoke)
            .and_then(|fit| fit.yoke())
    }

    pub(super) fn stick(&self, kind: ControlKind) -> Option<StickSpec> {
        match kind {
            ControlKind::Cyclic => Some(self.config.cyclic),
            ControlKind::FlightStick => Some(self.config.flight_stick),
            _ => None,
        }
    }

    pub(super) fn stick_top(&self, kind: ControlKind, spec: &StickSpec) -> Vec3 {
        self.native_stick
            .filter(|native| native.kind == kind)
            .map_or_else(
                || {
                    self.cockpit
                        .filter(|fit| fit.asset.stick_kind == kind)
                        .map_or_else(|| self.mirror(spec.top_m), |fit| fit.stick_top())
                },
                |native| native.top(),
            )
    }

    pub(super) fn stick_handle(&self, kind: ControlKind, deflection: Vec2) -> Option<Vec3> {
        let spec = self.stick(kind)?;
        Some(
            self.native_stick
                .filter(|native| native.kind == kind)
                .map_or_else(
                    || {
                        self.cockpit
                            .filter(|fit| fit.asset.stick_kind == kind)
                            .map_or_else(
                                || self.stick_top(kind, &spec) + deflection3(deflection),
                                |fit| fit.stick_handle(deflection),
                            )
                    },
                    |native| native.handle(deflection),
                ),
        )
    }

    pub(super) fn stick_deflection(
        &self,
        kind: ControlKind,
        spec: &StickSpec,
        point: Vec3,
    ) -> Vec2 {
        if let Some(fit) = self.cockpit.filter(|fit| fit.asset.stick_kind == kind) {
            stick_deflection(spec, Vec3::ZERO, fit.stick_displacement(point))
        } else {
            stick_deflection(spec, self.stick_top(kind, spec), point)
        }
    }

    pub(super) fn lever(&self, kind: ControlKind) -> Option<(usize, LeverSpec)> {
        match kind {
            ControlKind::Collective => Some((0, self.config.collective)),
            ControlKind::Throttle => Some((1, self.config.throttle)),
            _ => None,
        }
    }

    pub(super) fn lever_rest(&self, kind: ControlKind, spec: &LeverSpec) -> Vec3 {
        if let Some(rest) = self.cockpit.and_then(|fit| fit.lever_rest(kind)) {
            return rest;
        }
        self.mirror(spec.rest_m)
    }

    pub(super) fn lever_direction(&self, kind: ControlKind, spec: &LeverSpec) -> Vec3 {
        self.cockpit
            .filter(|fit| {
                matches!(
                    (fit.asset.stick_kind, kind),
                    (ControlKind::Cyclic, ControlKind::Collective)
                        | (ControlKind::FlightStick, ControlKind::Throttle)
                )
            })
            .map_or_else(
                || spec.axis.direction(),
                |fit| fit.direction(spec.axis.direction()),
            )
    }

    /// The handle of `kind` at `value` from its rest, its zero.
    pub(super) fn lever_handle(&self, kind: ControlKind, spec: &LeverSpec, value: f32) -> Vec3 {
        self.lever_rest(kind, spec)
            + self.lever_direction(kind, spec) * (spec.travel_m * 0.5 * value)
    }

    /// Where `kind`'s handle pulled to `point` is, from its rest, within its travel.
    pub(super) fn lever_value(&self, kind: ControlKind, spec: &LeverSpec, point: Vec3) -> f32 {
        let rest = self.lever_rest(kind, spec);
        let along = (point - rest).dot(self.lever_direction(kind, spec));
        let (low, high) = self.lever_range(kind);
        let half = (spec.travel_m * 0.5).max(1e-3);
        let value = along / half;
        if value.is_finite() {
            value.clamp(low, high)
        } else {
            0.0
        }
    }

    /// The ends of `kind`'s travel from its rest, in lever positions (the whole travel spans two):
    /// the collective's rest is its middle, and the throttle's is its zero gate, with the reverse
    /// section's share of the travel behind it.
    pub(super) fn lever_range(&self, kind: ControlKind) -> (f32, f32) {
        match kind {
            ControlKind::Throttle => {
                let zero = throttle_zero(&self.config.flight);
                (-1.0 - zero, 1.0 - zero)
            }
            _ => (-1.0, 1.0),
        }
    }

    /// Where `kind`'s afterburner gate starts along its forward section, for the throttle with the
    /// gate on: past the snap next to zero, short of the stop.
    pub(super) fn gate(&self, kind: ControlKind) -> Option<f32> {
        let flight = &self.config.flight;
        (kind == ControlKind::Throttle && flight.afterburner_gate).then(|| {
            let snap = flight.zero_snap.clamp(0.0, 0.9);
            flight.gate_at.clamp(snap + 0.05, 0.98)
        })
    }

    /// Where the throttle's afterburner gate sits from its rest, in lever positions, where it has
    /// one.
    pub(super) fn afterburner_mark(&self) -> Option<f32> {
        let (_, high) = self.lever_range(ControlKind::Throttle);
        self.gate(ControlKind::Throttle).map(|gate| gate * high)
    }

    /// The point `hand`'s grip grabs `kind` at, given the hand's grip `point`: the nearest point on
    /// the wheel's rim, the nearest point on the hand's own half of the handlebars, or a stick's or
    /// lever's handle.
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
            ControlKind::Handlebars => Some(bar_grab_point(&self.bars, state.bars, hand, point)),
            ControlKind::Yoke => Some(nearest_horn(&self.yoke()?, &state.yoke, point)),
            kind => {
                if let Some(handle) = self.stick_handle(kind, state.stick) {
                    return Some(handle);
                }
                let (index, spec) = self.lever(kind)?;
                Some(self.lever_handle(kind, &spec, state.levers[index]))
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
                    radius: match kind {
                        ControlKind::Handlebars => placement.config.handlebars.grab_radius_m,
                        _ => placement.config.grab_radius_m,
                    },
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
    /// Whether the tables write the cyclic's or the flight stick's axes this tick: the thumbstick
    /// is out of its deadzone.
    pub stick_moves: bool,
    /// Whether the tables write the collective's or the throttle's axis this tick.
    pub lever_moves: bool,
    pub dt: f32,
}

/// How long the handlebars stay in a hand whose grip has opened, seconds: long enough for fingers
/// lifted off the handle a moment to close again.
pub const BARS_GRIP_GRACE_S: f32 = 0.25;

/// Advance the controls by one input tick: act on the grabs and the releases, move the held controls
/// with their hands, and return the outputs.
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
            // The handlebars keep a hand whose grip opens for a moment: an Index grip's fingers
            // come off the handle as a rider rolls the throttle shut, and every one of the eleven
            // lost grips on the 14 November take was that, the bars dropping from under the hand.
            let bars = held.is_some_and(|h| h.kind == ControlKind::Handlebars);
            if held.is_some() && !inputs.held[i] {
                let open_s = state.bars_grace[i].map_or(0.0, |s| s + dt);
                if bars && open_s < BARS_GRIP_GRACE_S {
                    state.bars_grace[i] = Some(open_s);
                } else {
                    state.bars_grace[i] = None;
                    release(state, hand, Release::GripReleased, events);
                }
            } else {
                state.bars_grace[i] = None;
            }
            continue;
        };
        state.bars_grace[i] = None;
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
                let (steer, cue) = step_bars(state, placement, holding);
                if let Some(steer) = steer {
                    out.steer = Some(steer);
                    out.lean = Some(steer);
                }
                out.wheel_cue = cue;
            }
            ControlKind::Yoke => {
                let Some(frame) = placement.yoke() else {
                    continue;
                };
                let yoke = step_yoke(
                    state,
                    &frame,
                    &placement.config.yoke,
                    holding,
                    inputs.stick_moves,
                    dt,
                );
                out.flight_stick = yoke.value;
                out.stick_cue = yoke.cue;
            }
            ControlKind::Cyclic | ControlKind::FlightStick => {
                let stick = step_stick(
                    state,
                    placement,
                    kind,
                    holding,
                    (inputs.orientations, inputs.stick_moves),
                    dt,
                );
                out.stick_cue = stick.cue;
                if kind == ControlKind::Cyclic {
                    out.cyclic = stick.value;
                } else {
                    out.flight_stick = stick.value;
                }
                out.rudder = stick.rudder;
            }
            ControlKind::Collective | ControlKind::Throttle => {
                let lever = step_lever(state, placement, kind, holding, inputs.lever_moves);
                out.lever_cue = lever.cue;
                if kind == ControlKind::Collective {
                    out.collective = lever.value;
                } else {
                    out.throttle = lever.value;
                    out.afterburner =
                        placement.gate(kind).is_some() && lever.value.is_some_and(|v| v >= 1.0);
                }
            }
        }
    }

    let buttons =
        ButtonPlacement::for_seat(inputs.kinds, &placement.config.buttons, &placement.wheel);
    let mut button_events = Vec::new();
    let pressed = step_buttons(
        &mut state.buttons,
        &ButtonInputs {
            placement: &buttons,
            config: &placement.config.buttons,
            bars: &placement.bars,
            grips: inputs.grips,
            orientations: inputs.orientations,
            holds: state.holds,
            bars_turn: state.bars,
        },
        &mut button_events,
    );
    events.extend(button_events.into_iter().map(ControlEvent::Button));
    out.horn = pressed.horn;
    out.nitrous = pressed.nitrous;
    out.bike_throttle = pressed.throttle;
    out.bike_brake = pressed.brake;
    out
}

pub(super) fn refresh_native(
    state: &mut ControlState,
    previous: Option<native::NativeStick>,
    current: Option<native::NativeStick>,
    events: &mut Vec<ControlEvent>,
) {
    let identity = |native: native::NativeStick| {
        (
            native.kind,
            native.target.vehicle,
            native.target.part_index,
            native.target.properties,
        )
    };
    if previous.map(identity) == current.map(identity) {
        return;
    }
    for hand in Hand::BOTH {
        if state.holds[hand.index()].is_some_and(|hold| {
            previous.is_some_and(|native| native.kind == hold.kind)
                || current.is_some_and(|native| native.kind == hold.kind)
        }) {
            release(state, hand, Release::Gone, events);
        }
    }
    state.stick = Vec2::ZERO;
    state.stick_cues = TravelCues::new();
}

pub(super) fn refresh_cockpit(
    state: &mut ControlState,
    previous: Option<native::CockpitFit>,
    current: Option<native::CockpitFit>,
    events: &mut Vec<ControlEvent>,
) {
    let identity = |fit: native::CockpitFit| {
        (
            fit.target.vehicle,
            fit.target.part_index,
            fit.target.properties,
            fit.asset.seat,
        )
    };
    if previous.map(identity) == current.map(identity) {
        return;
    }
    for hand in Hand::BOTH {
        if state.holds[hand.index()].is_some_and(|hold| {
            previous.is_some_and(|fit| fit.contains(hold.kind))
                || current.is_some_and(|fit| fit.contains(hold.kind))
                || matches!(hold.kind, ControlKind::Collective | ControlKind::Throttle)
        }) {
            release(state, hand, Release::Gone, events);
        }
    }
    state.stick = Vec2::ZERO;
    state.levers = [0.0; 2];
    state.throttle = ThrottleState::new();
    state.yoke = YokeState::new();
    state.stick_cues = TravelCues::new();
    state.lever_cues = [TravelCues::new(); 2];
}

/// Let go of every control, as when the seat or the scheme changes.
pub fn release_all(state: &mut ControlState, events: &mut Vec<ControlEvent>) {
    for hand in Hand::BOTH {
        release(state, hand, Release::Gone, events);
    }
}

/// Where `hand`'s grip sits on the control it holds, for the hand's current grip positions
/// `grips`: the rim point at the hand's angle, the nearest point on the hand's half of the bars as
/// both hands turn them, or the stick's or lever's handle where the hand has moved it, within its
/// travel.
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
        ControlKind::Yoke => {
            let frame = placement.yoke()?;
            let grip = state.yoke.grips[hand.index()]?;
            Some(grip.anchor(&frame, state.yoke.roll, state.yoke.pitch))
        }
        ControlKind::Handlebars => {
            let bars = bars_now(
                state,
                placement,
                grips_holding(state, ControlKind::Handlebars, grips),
            );
            Some(bar_grab_point(&placement.bars, bars, hand, point))
        }
        kind => {
            if let Some(spec) = placement.stick(kind) {
                let deflection = placement.stick_deflection(kind, &spec, point - hold.offset);
                return placement
                    .stick_handle(kind, deflection)
                    .map(|handle| handle + hold.offset);
            }
            let (_, spec) = placement.lever(kind)?;
            let mut value = placement.lever_value(kind, &spec, point - hold.offset);
            if kind == ControlKind::Throttle {
                value =
                    throttle_constrain(&placement.config.flight, state.throttle.section, value).0;
            }
            Some(placement.lever_handle(kind, &spec, value) + hold.offset)
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
    let orientation = orientation.filter(|q| q.is_finite());
    let other = hand.other();
    if !kind.two_handed() && state.holds[other.index()].is_some_and(|h| h.kind == kind) {
        release(state, other, Release::TakenOver, events);
    }
    let offset = match kind {
        ControlKind::Wheel | ControlKind::Handlebars | ControlKind::Yoke => Vec3::ZERO,
        kind => placement
            .grab_point(state, kind, hand, point)
            .map_or(Vec3::ZERO, |handle| point - handle),
    };
    if kind == ControlKind::Yoke
        && let Some(frame) = placement.yoke()
    {
        grab_yoke(state, &frame, hand, point);
    }
    if kind == ControlKind::Wheel {
        state.wheel_grips[hand.index()] = Some(WheelGrip::new(
            &placement.wheel,
            point,
            orientation,
            state.wheel_angle,
        ));
    }
    state.holds[hand.index()] = Some(Hold {
        kind,
        offset,
        orientation,
    });
    events.push(ControlEvent::Grabbed(hand, kind));
}

fn release(state: &mut ControlState, hand: Hand, why: Release, events: &mut Vec<ControlEvent>) {
    let Some(hold) = state.holds[hand.index()].take() else {
        return;
    };
    state.wheel_grips[hand.index()] = None;
    state.yoke.grips[hand.index()] = None;
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
