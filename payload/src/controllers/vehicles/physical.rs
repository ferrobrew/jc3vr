//! The opt-in physical controls of the driver's seat: a wheel, handlebars, a helicopter cyclic and
//! collective, and a plane's flight stick and throttle. Pure.
//!
//! Every control lives in the cockpit frame, placed relative to the seated character's eyes (see
//! [`crate::controllers::vehicles::config`]), and is grabbed like any other grab volume: a grip
//! press with the hand's grip near the control grabs it, and the next grip press, or moving the hand
//! further than the release distance from where it holds the control, lets go. While held:
//!
//! - the wheel turns by the holding hands' angular travel around its hub, in the wheel's plane, and
//!   its angle over the full-steer angle is the steering; let go, it returns to centre;
//! - the handlebars pivot about their middle: their turn in the horizontal plane steers and their
//!   roll leans. With one hand on them, the other end mirrors it through the middle;
//! - a stick's top follows the hand across the horizontal plane, and its deflection over the full
//!   deflection drives both of its axes; let go, it springs back to centre;
//! - a lever's handle follows the hand along its axis, and its position over half the travel is its
//!   output, with a detent around neutral. The throttle stays where it is let go.
//!
//! The holding hand is drawn on the control (the anchored tier): [`anchor_point`] gives where its
//! grip sits on the control for the hand's current position, which the hands compose at every rate
//! from the latest controller pose.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    frame::Hand,
    mode::VehicleClass,
    vehicles::config::{HandlebarsSpec, LeverSpec, PhysicalControlsConfig, StickSpec, WheelSpec},
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

    /// The controls a vehicle class offers its driver. Boats and jetskis share one class, so both
    /// get the wheel until the jetski can be told apart.
    pub const fn for_class(class: VehicleClass) -> &'static [ControlKind] {
        match class {
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
    /// The hand pressed its grip again.
    GripPress,
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
            Release::GripPress => "grip press",
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
    /// Each holding hand's angle around the hub on the last tick.
    wheel_last: [Option<f32>; 2],
    /// The handlebars' turn and roll, radians.
    pub bars: Vec2,
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
            wheel_last: [None; 2],
            bars: Vec2::ZERO,
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
}

impl Outputs {
    pub const NONE: Outputs = Outputs {
        steer: None,
        lean: None,
        cyclic: None,
        collective: None,
        flight_stick: None,
        throttle: None,
    };
}

/// The controls placed for one seat: the specs with their sides mirrored for the handedness.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Placement {
    pub config: PhysicalControlsConfig,
    pub dominant: Hand,
}

impl Placement {
    fn mirror(&self, v: Vec3) -> Vec3 {
        match self.dominant {
            Hand::Right => v,
            Hand::Left => Vec3::new(-v.x, v.y, v.z),
        }
    }

    fn wheel_centre(&self) -> Vec3 {
        self.mirror(self.config.wheel.centre_m)
    }

    fn bars_centre(&self) -> Vec3 {
        self.mirror(self.config.handlebars.centre_m)
    }

    fn stick(&self, kind: ControlKind) -> Option<StickSpec> {
        match kind {
            ControlKind::Cyclic => Some(self.config.cyclic),
            ControlKind::FlightStick => Some(self.config.flight_stick),
            _ => None,
        }
    }

    fn lever(&self, kind: ControlKind) -> Option<(usize, LeverSpec)> {
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
                let spec = &self.config.wheel;
                let centre = self.wheel_centre();
                let angle = wheel_hand_angle(spec, centre, point).unwrap_or(0.0);
                Some(rim_point(spec, centre, angle))
            }
            ControlKind::Handlebars => {
                let spec = &self.config.handlebars;
                let ends = bar_ends(self.bars_centre(), spec.half_width_m, state.bars);
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
    /// The control each hand's grip press latched as a grab this tick.
    pub grabs: [Option<ControlKind>; 2],
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
            continue;
        };
        if held.is_some() {
            release(state, hand, Release::GripPress, events);
        } else if let Some(point) = inputs.grips[i]
            && inputs.kinds.contains(&kind)
        {
            grab(state, placement, hand, kind, point, events);
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
            ControlKind::Wheel => out.steer = step_wheel(state, placement, holding, dt),
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
        ControlKind::Wheel => {
            let spec = &placement.config.wheel;
            let centre = placement.wheel_centre();
            let angle = wheel_hand_angle(spec, centre, point)?;
            Some(rim_point(spec, centre, angle))
        }
        ControlKind::Handlebars => {
            let spec = &placement.config.handlebars;
            let centre = placement.bars_centre();
            let bars = bar_angles(centre, grips_holding(state, ControlKind::Handlebars, grips))
                .map_or(state.bars, |angles| clamp_bars(angles, spec));
            Some(bar_ends(centre, spec.half_width_m, bars)[hand.index()])
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
    point: Vec3,
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
        state.wheel_last[hand.index()] =
            wheel_hand_angle(&placement.config.wheel, placement.wheel_centre(), point);
    }
    state.holds[hand.index()] = Some(Hold { kind, offset });
    events.push(ControlEvent::Grabbed(hand, kind));
}

fn release(state: &mut ControlState, hand: Hand, why: Release, events: &mut Vec<ControlEvent>) {
    let Some(hold) = state.holds[hand.index()].take() else {
        return;
    };
    state.wheel_last[hand.index()] = None;
    events.push(ControlEvent::Released(hand, hold.kind, why));
}

type Holding = [Option<(Hold, Vec3)>; 2];

fn grips_holding(
    state: &ControlState,
    kind: ControlKind,
    grips: [Option<Vec3>; 2],
) -> [Option<Vec3>; 2] {
    Hand::BOTH.map(|hand| {
        grips[hand.index()].filter(|_| state.holds[hand.index()].is_some_and(|h| h.kind == kind))
    })
}

fn step_wheel(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
    dt: f32,
) -> Option<f32> {
    let spec = &placement.config.wheel;
    let centre = placement.wheel_centre();
    let max = spec.max_angle_deg.to_radians().max(1e-3);
    let mut turn = 0.0;
    let mut count = 0;
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some((_, point)) = holding[i] else {
            state.wheel_last[i] = None;
            continue;
        };
        let Some(now) = wheel_hand_angle(spec, centre, point) else {
            continue;
        };
        if let Some(last) = state.wheel_last[i] {
            turn += wrap_angle(now - last);
            count += 1;
        }
        state.wheel_last[i] = Some(now);
    }
    if holding.iter().all(Option::is_none) {
        let step = spec.return_deg_per_s.to_radians().max(0.0) * dt;
        state.wheel_angle -= state.wheel_angle.clamp(-step, step);
        return None;
    }
    if count > 0 {
        state.wheel_angle = (state.wheel_angle + turn / count as f32).clamp(-max, max);
    }
    Some(state.wheel_angle / max)
}

fn step_bars(
    state: &mut ControlState,
    placement: &Placement,
    holding: Holding,
) -> Option<(f32, f32)> {
    let spec = &placement.config.handlebars;
    let grips = holding.map(|h| h.map(|(_, point)| point));
    let Some(angles) = bar_angles(placement.bars_centre(), grips) else {
        state.bars = Vec2::ZERO;
        return None;
    };
    state.bars = clamp_bars(angles, spec);
    let steer = state.bars.x / spec.max_steer_deg.to_radians().max(1e-3);
    let lean = state.bars.y / spec.max_lean_deg.to_radians().max(1e-3);
    Some((steer, lean))
}

fn step_stick(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
) -> Option<Vec2> {
    let spec = placement.stick(kind)?;
    let Some((hold, point)) = holding.into_iter().flatten().next() else {
        state.stick = Vec2::ZERO;
        return None;
    };
    let top = placement.mirror(spec.top_m);
    state.stick = stick_deflection(&spec, top, point - hold.offset);
    let max = spec.max_deflection_m.max(1e-3);
    Some(Vec2::new(state.stick.x / max, -state.stick.y / max))
}

fn step_lever(
    state: &mut ControlState,
    placement: &Placement,
    kind: ControlKind,
    holding: Holding,
) -> Option<f32> {
    let (index, spec) = placement.lever(kind)?;
    match holding.into_iter().flatten().next() {
        Some((hold, point)) => {
            state.levers[index] =
                lever_value(&spec, placement.mirror(spec.rest_m), point - hold.offset);
        }
        None if !spec.hold_on_release => {
            state.levers[index] = 0.0;
            return None;
        }
        None if state.levers[index] == 0.0 => return None,
        None => {}
    }
    Some(detent(state.levers[index], spec.detent))
}

/// The wheel's in-plane axes: right, and up along the tilted face (its top leans away from the
/// driver).
fn wheel_axes(spec: &WheelSpec) -> (Vec3, Vec3) {
    let (sin, cos) = spec.tilt_deg.to_radians().sin_cos();
    (Vec3::X, Vec3::new(0.0, cos, -sin))
}

/// The angle of `point` around the wheel's hub in the wheel's plane, radians clockwise from the
/// top as the driver sees it. `None` at the hub itself.
pub fn wheel_hand_angle(spec: &WheelSpec, centre: Vec3, point: Vec3) -> Option<f32> {
    let (right, up) = wheel_axes(spec);
    let d = point - centre;
    let (x, y) = (d.dot(right), d.dot(up));
    (x * x + y * y > 1e-6 && x.is_finite() && y.is_finite()).then(|| x.atan2(y))
}

/// The point on the rim at `angle` (clockwise from the top).
pub fn rim_point(spec: &WheelSpec, centre: Vec3, angle: f32) -> Vec3 {
    let (right, up) = wheel_axes(spec);
    let (sin, cos) = angle.sin_cos();
    centre + (right * sin + up * cos) * spec.radius_m
}

/// Wrap an angle difference into `-π..=π`.
fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// The handlebars' turn (positive with the right grip pulled back) and roll (positive with the
/// right grip lower), from the holding grips. With one grip the other end mirrors it through the
/// middle. `None` with no grip.
pub fn bar_angles(centre: Vec3, grips: [Option<Vec3>; 2]) -> Option<Vec2> {
    let across = match grips {
        [Some(left), Some(right)] => right - left,
        [None, Some(right)] => (right - centre) * 2.0,
        [Some(left), None] => (centre - left) * 2.0,
        [None, None] => return None,
    };
    let flat = Vec2::new(across.x, across.z).length();
    if flat < 1e-4 || !across.is_finite() {
        return None;
    }
    Some(Vec2::new(across.z.atan2(across.x), (-across.y).atan2(flat)))
}

fn clamp_bars(angles: Vec2, spec: &HandlebarsSpec) -> Vec2 {
    let steer = spec.max_steer_deg.to_radians().abs();
    let lean = spec.max_lean_deg.to_radians().abs();
    Vec2::new(angles.x.clamp(-steer, steer), angles.y.clamp(-lean, lean))
}

/// The handlebars' grips, indexed by [`Hand::index`], for the bars turned and rolled by `bars`.
pub fn bar_ends(centre: Vec3, half_width: f32, bars: Vec2) -> [Vec3; 2] {
    let across = Quat::from_rotation_y(-bars.x) * Quat::from_rotation_z(-bars.y) * Vec3::X;
    [centre - across * half_width, centre + across * half_width]
}

/// A stick's deflection (+X right, +Y back) for its top pulled to `top_now`, limited to the full
/// deflection.
pub fn stick_deflection(spec: &StickSpec, top: Vec3, top_now: Vec3) -> Vec2 {
    let d = top_now - top;
    let flat = Vec2::new(d.x, d.z);
    if !flat.is_finite() {
        return Vec2::ZERO;
    }
    flat.clamp_length_max(spec.max_deflection_m.max(0.0))
}

fn deflection3(d: Vec2) -> Vec3 {
    Vec3::new(d.x, 0.0, d.y)
}

/// A lever's position, `-1..=1`, for its handle pulled to `handle_now`.
pub fn lever_value(spec: &LeverSpec, rest: Vec3, handle_now: Vec3) -> f32 {
    let half = (spec.travel_m * 0.5).max(1e-3);
    let along = (handle_now - rest).dot(spec.axis.direction()) / half;
    if along.is_finite() {
        along.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A lever's handle at position `value`.
pub fn lever_handle(spec: &LeverSpec, rest: Vec3, value: f32) -> Vec3 {
    rest + spec.axis.direction() * (spec.travel_m * 0.5 * value)
}

/// `value` with a detent around zero, rescaled so that it still reaches one.
pub fn detent(value: f32, detent: f32) -> f32 {
    let detent = detent.clamp(0.0, 0.95);
    let magnitude = value.abs();
    if magnitude <= detent {
        return 0.0;
    }
    value.signum() * ((magnitude - detent) / (1.0 - detent)).min(1.0)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::controllers::vehicles::config::LeverAxis;

    const DT: f32 = 1.0 / 30.0;

    fn placement(set: impl FnOnce(&mut PhysicalControlsConfig)) -> Placement {
        let mut config = PhysicalControlsConfig::new();
        set(&mut config);
        Placement {
            config,
            dominant: Hand::Right,
        }
    }

    fn step_with(
        state: &mut ControlState,
        placement: &Placement,
        kinds: &[ControlKind],
        grips: [Option<Vec3>; 2],
        grabs: [Option<ControlKind>; 2],
    ) -> (Outputs, Vec<ControlEvent>) {
        let mut events = Vec::new();
        let out = step(
            state,
            placement,
            &StepInputs {
                kinds,
                grips,
                grabs,
                dt: DT,
            },
            &mut events,
        );
        (out, events)
    }

    #[test]
    fn angles_around_the_wheel_run_clockwise_from_the_top() {
        let spec = PhysicalControlsConfig::new().wheel;
        let c = Vec3::ZERO;
        assert!(
            wheel_hand_angle(&spec, c, rim_point(&spec, c, 0.0))
                .unwrap()
                .abs()
                < 1e-5
        );
        let right = rim_point(&spec, c, FRAC_PI_2);
        assert!(right.x > 0.0);
        assert!((wheel_hand_angle(&spec, c, right).unwrap() - FRAC_PI_2).abs() < 1e-5);
        assert_eq!(wheel_hand_angle(&spec, c, c), None);
    }

    #[test]
    fn turning_the_wheel_clockwise_steers_right_and_it_returns_when_let_go() {
        let p = placement(|c| c.wheel.enabled = true);
        let spec = p.config.wheel;
        let centre = spec.centre_m;
        let kinds = [ControlKind::Wheel];
        let mut state = ControlState::default();
        let left = rim_point(&spec, centre, -FRAC_PI_2);
        let right = rim_point(&spec, centre, FRAC_PI_2);
        let grabs = [Some(ControlKind::Wheel); 2];
        step_with(&mut state, &p, &kinds, [Some(left), Some(right)], grabs);
        assert!(state.holds.iter().all(Option::is_some));

        let turn = 0.5;
        let left = rim_point(&spec, centre, -FRAC_PI_2 + turn);
        let right = rim_point(&spec, centre, FRAC_PI_2 + turn);
        let (out, _) = step_with(&mut state, &p, &kinds, [Some(left), Some(right)], [None; 2]);
        let expected = turn / spec.max_angle_deg.to_radians();
        assert!((out.steer.unwrap() - expected).abs() < 1e-4, "{out:?}");

        let (out, events) = step_with(&mut state, &p, &kinds, [Some(left), Some(right)], grabs);
        assert_eq!(events.len(), 2);
        assert_eq!(out.steer, None);
        let before = state.wheel_angle;
        step_with(&mut state, &p, &kinds, [Some(left), Some(right)], [None; 2]);
        assert!(state.wheel_angle.abs() < before.abs());
    }

    #[test]
    fn a_hand_that_leaves_the_wheel_lets_go() {
        let p = placement(|c| c.wheel.enabled = true);
        let kinds = [ControlKind::Wheel];
        let mut state = ControlState::default();
        let on = rim_point(&p.config.wheel, p.config.wheel.centre_m, 0.3);
        step_with(
            &mut state,
            &p,
            &kinds,
            [None, Some(on)],
            [None, Some(ControlKind::Wheel)],
        );
        let away = on + Vec3::new(0.0, 0.0, 0.5);
        let (_, events) = step_with(&mut state, &p, &kinds, [None, Some(away)], [None; 2]);
        assert_eq!(
            events,
            vec![ControlEvent::Released(
                Hand::Right,
                ControlKind::Wheel,
                Release::MovedAway
            )]
        );
    }

    #[test]
    fn the_handlebars_steer_with_a_pulled_back_grip_and_lean_with_a_lowered_one() {
        let c = Vec3::ZERO;
        let steer = bar_angles(
            c,
            [
                Some(Vec3::new(-0.3, 0.0, -0.1)),
                Some(Vec3::new(0.3, 0.0, 0.1)),
            ],
        )
        .unwrap();
        assert!(steer.x > 0.0 && steer.y.abs() < 1e-5);
        let lean = bar_angles(
            c,
            [
                Some(Vec3::new(-0.3, 0.1, 0.0)),
                Some(Vec3::new(0.3, -0.1, 0.0)),
            ],
        )
        .unwrap();
        assert!(lean.y > 0.0 && lean.x.abs() < 1e-5);
        let one = bar_angles(c, [None, Some(Vec3::new(0.3, 0.0, 0.1))]).unwrap();
        assert!((one.x - steer.x).abs() < 1e-5);
        let ends = bar_ends(c, 0.3, Vec2::new(steer.x, 0.0));
        assert!(ends[1].z > 0.0 && ends[0].z < 0.0);
        let rolled = bar_ends(c, 0.3, Vec2::new(0.0, lean.y));
        assert!(rolled[1].y < 0.0);
    }

    #[test]
    fn a_stick_follows_the_hand_within_its_travel_and_springs_back() {
        let p = placement(|c| c.cyclic.enabled = true);
        let kinds = [ControlKind::Cyclic];
        let top = p.config.cyclic.top_m;
        let mut state = ControlState::default();
        let grip = top + Vec3::new(0.0, 0.05, 0.0);
        step_with(
            &mut state,
            &p,
            &kinds,
            [None, Some(grip)],
            [None, Some(ControlKind::Cyclic)],
        );
        let pushed = grip + Vec3::new(0.0, 0.0, -0.06);
        let (out, _) = step_with(&mut state, &p, &kinds, [None, Some(pushed)], [None; 2]);
        let cyclic = out.cyclic.unwrap();
        assert!(
            (cyclic.y - 0.5).abs() < 1e-4 && cyclic.x.abs() < 1e-5,
            "{cyclic}"
        );
        let far = grip + Vec3::new(0.2, 0.0, 0.0);
        let anchor = anchor_point(
            &state,
            &p,
            state.holds[1].unwrap(),
            Hand::Right,
            [None, Some(far)],
        )
        .unwrap();
        assert!((anchor.x - grip.x - p.config.cyclic.max_deflection_m).abs() < 1e-4);
        step_with(
            &mut state,
            &p,
            &kinds,
            [None, Some(pushed)],
            [None, Some(ControlKind::Cyclic)],
        );
        let (out, _) = step_with(&mut state, &p, &kinds, [None, Some(pushed)], [None; 2]);
        assert_eq!(out.cyclic, None);
        assert_eq!(state.stick, Vec2::ZERO);
    }

    #[test]
    fn a_throttle_stays_where_it_is_let_go_and_a_collective_returns() {
        let p = placement(|c| {
            c.throttle.enabled = true;
            c.collective.enabled = true;
        });
        for (kind, holds) in [
            (ControlKind::Throttle, true),
            (ControlKind::Collective, false),
        ] {
            let kinds = [kind];
            let (rest, axis) = match kind {
                ControlKind::Throttle => (p.config.throttle.rest_m, LeverAxis::ForeAft),
                _ => (p.config.collective.rest_m, LeverAxis::Vertical),
            };
            let mut state = ControlState::default();
            step_with(
                &mut state,
                &p,
                &kinds,
                [Some(rest), None],
                [Some(kind), None],
            );
            let moved = rest + axis.direction() * 0.12;
            let (out, _) = step_with(&mut state, &p, &kinds, [Some(moved), None], [None; 2]);
            let value = match kind {
                ControlKind::Throttle => out.throttle,
                _ => out.collective,
            };
            assert!(value.unwrap() > 0.5, "{kind:?} {out:?}");
            step_with(
                &mut state,
                &p,
                &kinds,
                [Some(moved), None],
                [Some(kind), None],
            );
            let (out, _) = step_with(&mut state, &p, &kinds, [Some(moved), None], [None; 2]);
            let value = match kind {
                ControlKind::Throttle => out.throttle,
                _ => out.collective,
            };
            assert_eq!(value.is_some(), holds, "{kind:?}");
        }
    }

    #[test]
    fn a_one_handed_control_passes_to_the_hand_that_takes_it() {
        let p = placement(|c| c.cyclic.enabled = true);
        let kinds = [ControlKind::Cyclic];
        let top = p.config.cyclic.top_m;
        let mut state = ControlState::default();
        step_with(
            &mut state,
            &p,
            &kinds,
            [None, Some(top)],
            [None, Some(ControlKind::Cyclic)],
        );
        let (_, events) = step_with(
            &mut state,
            &p,
            &kinds,
            [Some(top), Some(top)],
            [Some(ControlKind::Cyclic), None],
        );
        assert!(events.contains(&ControlEvent::Released(
            Hand::Right,
            ControlKind::Cyclic,
            Release::TakenOver
        )));
        assert!(state.holds[0].is_some() && state.holds[1].is_none());
    }

    #[test]
    fn a_left_handed_layout_mirrors_the_sides() {
        let p = Placement {
            config: PhysicalControlsConfig::new(),
            dominant: Hand::Left,
        };
        let state = ControlState::default();
        let stick = p
            .grab_point(&state, ControlKind::Cyclic, Hand::Left, Vec3::ZERO)
            .unwrap();
        assert!(stick.x < 0.0);
        let lever = p
            .grab_point(&state, ControlKind::Collective, Hand::Right, Vec3::ZERO)
            .unwrap();
        assert!(lever.x > 0.0);
    }

    #[test]
    fn the_detent_swallows_neutral_and_still_reaches_one() {
        assert_eq!(detent(0.05, 0.1), 0.0);
        assert_eq!(detent(1.0, 0.1), 1.0);
        assert!((detent(-0.55, 0.1) + 0.5).abs() < 1e-5);
    }

    #[test]
    fn only_the_seat_class_s_controls_are_offered() {
        assert_eq!(
            ControlKind::for_class(VehicleClass::Motorcycle),
            &[ControlKind::Handlebars]
        );
        let mut off = PhysicalControlsConfig::new();
        off.wheel.enabled = false;
        assert!(!ControlKind::Wheel.enabled(&off));
    }
}
