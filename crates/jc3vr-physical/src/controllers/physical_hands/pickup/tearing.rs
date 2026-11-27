//! The pickup tick's part in tearing a held door off its vehicle: each held door's pull is read
//! as a tear gesture once, however many hands hold it ([`step`]), and a door that comes off, or
//! was already off when a hand took it, turns every hold of it into a hold of a loose prop
//! ([`apply`], [`acquired`]).

use std::num::NonZeroUsize;

use body_rig::math::geometry::point_velocity;
use glam::{Mat4, Vec3};
use jc3vr_body::controllers::physics::{bodies, vehicles};

use crate::controllers::physical_hands::{
    doors::{
        tearing::{self, Acquisition, Gesture, TearGrip, TearResult},
        tuning::TearTuning,
    },
    grasp::PalmMotion,
    pickup::{
        state::{Held, HeldKind, PickupState},
        tick::{HoldLoss, TickOutcome},
    },
};

/// Body motion measured before the carry command, shared by both hands on the same body.
#[derive(Debug, Copy, Clone)]
pub(super) struct Observation {
    object: NonZeroUsize,
    body: u32,
    state: bodies::BodyState,
}

impl Observation {
    pub fn of(held: &Held, state: bodies::BodyState) -> Self {
        Self {
            object: held.object,
            body: held.body,
            state,
        }
    }

    fn matches(&self, held: &Held) -> bool {
        self.object == held.object && self.body == held.body
    }
}

/// Record the door grip a hand's new hold of a door took (`result`, from [`TearGrip::take`]): a
/// door still on its vehicle keeps its grip for the tearing, a door already off turns every hold
/// of it into a loose prop, and a refused grip ends the hold, so that no door is held untracked.
/// Returns whether the hold stands.
pub(super) fn acquired(state: &mut PickupState, index: usize, result: Option<Acquisition>) -> bool {
    state.doors[index] = None;
    let Some(result) = result else {
        state.set_held(index, None);
        return false;
    };
    match result {
        Acquisition::Attached(grip) => state.doors[index] = Some(grip),
        Acquisition::Detached => {
            if let Some(held) = state.held[index] {
                let joined = state.held[1 - index]
                    .filter(|h| h.same(&held))
                    .is_some_and(|h| h.joined);
                for holder in state.held.iter_mut().flatten().filter(|h| h.same(&held)) {
                    holder.kind = HeldKind::Prop;
                    holder.joined = joined;
                }
            }
        }
    }
    true
}

pub(super) struct Frame {
    pub palms: [Option<PalmMotion>; 2],
    pub observed: [Option<Observation>; 2],
    pub dt: f32,
    pub world_scale: f32,
}

/// Read each held door's pull as a tear gesture, once per door, and tear the door off when a
/// hand asks.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn step(
    state: &mut PickupState,
    frame: Frame,
    config: &TearTuning,
    outcome: &mut TickOutcome,
) {
    step_with(state, frame, config, outcome, &mut NativeTearing);
}

fn step_with(
    state: &mut PickupState,
    frame: Frame,
    config: &TearTuning,
    outcome: &mut TickOutcome,
    engine: &mut impl TearEngine,
) {
    let Frame {
        palms,
        observed,
        dt,
        world_scale,
    } = frame;
    let mut visited = [false; 2];
    for i in 0..2 {
        if state.held[i].is_none_or(|h| h.kind != HeldKind::Door) {
            state.doors[i] = None;
        }
        let Some(held) = state.held[i].filter(|h| h.kind == HeldKind::Door) else {
            continue;
        };
        let Some(grip) = state.doors[i] else {
            lost(state, &held, HoldLoss::DoorIdentity, outcome);
            continue;
        };
        if visited[i] {
            continue;
        }
        let holders = state.held.map(|h| h.is_some_and(|h| h.same(&held)));
        for k in 0..2 {
            visited[k] |= holders[k];
        }
        let Some(observation) = observed.iter().flatten().find(|o| o.matches(&held)) else {
            lost(state, &held, HoldLoss::DoorGone, outcome);
            continue;
        };
        if !engine.alive(&held) {
            lost(state, &held, HoldLoss::DoorGone, outcome);
            continue;
        }
        let continuity = engine.continuity(&grip, &held);
        if continuity == TearResult::Lost {
            lost(state, &held, HoldLoss::DoorIdentity, outcome);
            continue;
        }
        if continuity != TearResult::Refused {
            apply(state, &held, continuity, outcome);
            continue;
        }
        let vehicle_frame = engine.vehicle_frame(&grip);
        let local_from_world =
            if vehicle_frame.is_finite() && vehicle_frame.determinant().abs() > 1e-6 {
                vehicle_frame.inverse()
            } else {
                for (grip, holder) in state.doors.iter_mut().zip(holders) {
                    if holder && let Some(grip) = grip.as_mut() {
                        grip.reset();
                    }
                }
                continue;
            };
        let mut requested = false;
        for k in 0..2 {
            if !holders[k] {
                continue;
            }
            let (Some(palm), Some(report), Some(mut grip)) =
                (palms[k], outcome.reports[k], state.doors[k])
            else {
                continue;
            };
            let lag = report.target - report.anchor;
            let body = observation.state;
            let body_velocity = point_velocity(
                body.pose.centre,
                body.velocities.linear,
                body.velocities.angular,
                report.anchor,
            );
            let scale = world_scale.max(0.01);
            let speed = (palm.point_velocity(report.target) - body_velocity)
                .dot(lag.normalize_or_zero())
                / scale;
            let hinge = engine.hinge(&mut grip, report.anchor);
            let margin = config.limit_margin_rad.clamp(0.0, 0.2);
            let (at_limit, opening_push_m, free_swing) =
                hinge.map_or((false, 0.0, false), |(angle, range, tangent)| {
                    let at_open = angle >= range - margin;
                    let at_shut = angle <= margin;
                    // How much of the lag runs along the swing, opening positive.
                    let along = lag.normalize_or_zero().dot(tangent.normalize_or_zero());
                    let free = along.abs() >= config.free_swing_share.clamp(0.0, 1.0)
                        && if along > 0.0 { !at_open } else { !at_shut };
                    (at_open, lag.dot(tangent) / scale, free)
                });
            let gesture = Gesture {
                dt,
                lag_m: lag.length() / scale,
                lag_direction: local_from_world.transform_vector3(lag).normalize_or_zero(),
                separating_speed_mps: speed,
                opening_push_m,
                load: report.load,
                at_limit,
                free_swing,
            };
            if let Some(kind) = grip.gesture(gesture, config) {
                requested = true;
                tracing::info!(
                    target: "controllers",
                    hand = k,
                    gesture = kind.label(),
                    lag_m = gesture.lag_m,
                    separating_speed_mps = gesture.separating_speed_mps,
                    opening_push_m = gesture.opening_push_m,
                    load = gesture.load,
                    at_limit = gesture.at_limit,
                    "physical hands: doors: a hand asked to tear a door"
                );
            }
            state.doors[k] = Some(grip);
        }
        if requested {
            match engine.tear(&grip, &held) {
                TearResult::Lost => lost(state, &held, HoldLoss::DoorIdentity, outcome),
                result => apply(state, &held, result, outcome),
            }
        }
    }
}

trait TearEngine {
    fn alive(&mut self, held: &Held) -> bool;
    fn continuity(&mut self, grip: &TearGrip, held: &Held) -> TearResult;
    fn vehicle_frame(&mut self, grip: &TearGrip) -> Mat4;
    fn hinge(&mut self, grip: &mut TearGrip, anchor: Vec3) -> Option<(f32, f32, Vec3)>;
    fn tear(&mut self, grip: &TearGrip, held: &Held) -> TearResult;
}

struct NativeTearing;

impl TearEngine for NativeTearing {
    fn alive(&mut self, held: &Held) -> bool {
        unsafe { bodies::owner_of(held.body) == Some(held.object) }
    }

    fn continuity(&mut self, grip: &TearGrip, held: &Held) -> TearResult {
        unsafe { tearing::continuity(grip, held.object, held.body) }
    }

    fn vehicle_frame(&mut self, grip: &TearGrip) -> Mat4 {
        unsafe { vehicles::vehicle_frame(grip.door.vehicle) }
    }

    fn hinge(&mut self, grip: &mut TearGrip, anchor: Vec3) -> Option<(f32, f32, Vec3)> {
        unsafe { tearing::hinge(grip, anchor) }
    }

    fn tear(&mut self, grip: &TearGrip, held: &Held) -> TearResult {
        unsafe { tearing::tear(grip, held.object, held.body) }
    }
}

/// Let go of `held` from every hand that holds it, as lost, `loss`.
fn lost(state: &mut PickupState, held: &Held, loss: HoldLoss, outcome: &mut TickOutcome) {
    outcome.lose_hold(state.held.map(|h| h.is_some_and(|h| h.same(held))), loss);
    apply(state, held, TearResult::Lost, outcome);
}

/// Apply a tear's `result` to every hold of `held`.
pub(super) fn apply(
    state: &mut PickupState,
    held: &Held,
    result: TearResult,
    outcome: &mut TickOutcome,
) {
    let mut changed = false;
    for k in 0..2 {
        if !state.held[k].is_some_and(|h| h.same(held)) {
            continue;
        }
        match result {
            TearResult::Detached => {
                let holder = state.held[k].as_mut().expect("matching hold");
                changed |= holder.kind == HeldKind::Door;
                holder.kind = HeldKind::Prop;
                // Keep the existing joint and grab frame while the native body survives.
                state.doors[k] = None;
            }
            TearResult::Lost => {
                state.set_held(k, None);
                state.doors[k] = None;
                outcome.joints[k] = None;
                outcome.reports[k] = None;
                outcome.released[k] = true;
                changed = true;
            }
            TearResult::Refused => {
                // Both hands must settle again after any attempted tear.
                if let Some(grip) = state.doors[k].as_mut() {
                    grip.reset();
                }
            }
        }
    }
    match result {
        TearResult::Detached if changed => {
            outcome.tears += 1;
        }
        TearResult::Lost if changed => {
            outcome.lost += 1;
        }
        TearResult::Refused => {
            outcome.tear_refusals += 1;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use glam::{Mat4, Vec3};
    use jc3vr_body::controllers::{
        grab::world_reach::DoorTarget,
        physics::rigid::{BodyPose, Velocities},
    };
    use jc3vr_input::frame::Pose;

    use super::*;
    use crate::controllers::physical_hands::{
        doors::tuning::DOORS,
        grasp::Grip,
        hold,
        hold::{HandHold, HoldTuning},
        pickup::{HandStrength, HeldReport, PICKUP, tick::carry_tuning},
    };

    fn held() -> Held {
        Held {
            object: NonZeroUsize::new(8).unwrap(),
            body: 7,
            grip: Grip::of(
                Pose::default(),
                BodyPose {
                    centre: Vec3::ZERO,
                    orientation: glam::Quat::IDENTITY,
                },
                Vec3::X,
            ),
            mass_kg: 15.0,
            ragdoll: None,
            joined: true,
            kind: HeldKind::Door,
        }
    }

    fn grip() -> TearGrip {
        TearGrip::fixture(
            DoorTarget {
                vehicle: NonZeroUsize::new(16).unwrap(),
                vehicle_body: Some(3),
                part_index: 2,
                slot: jc3gi::vehicle::vehicle::DoorSlot::FRONT_LEFT,
                latched: false,
                outward: Vec3::X,
            },
            3,
        )
    }

    #[derive(Default)]
    struct Engine {
        requests: usize,
        checks: usize,
        vehicle_frame: Option<Mat4>,
    }

    impl TearEngine for Engine {
        fn alive(&mut self, _: &Held) -> bool {
            true
        }
        fn continuity(&mut self, _: &TearGrip, _: &Held) -> TearResult {
            self.checks += 1;
            TearResult::Refused
        }
        fn vehicle_frame(&mut self, _: &TearGrip) -> Mat4 {
            self.vehicle_frame.unwrap_or(Mat4::IDENTITY)
        }
        fn hinge(&mut self, _: &mut TearGrip, _: Vec3) -> Option<(f32, f32, Vec3)> {
            None
        }
        fn tear(&mut self, _: &TearGrip, _: &Held) -> TearResult {
            self.requests += 1;
            TearResult::Detached
        }
    }

    #[test]
    fn initial_identity_failure_never_leaves_an_untracked_door_hold() {
        let mut state = PickupState::new();
        state.held[0] = Some(held());
        assert!(!acquired(&mut state, 0, None));
        assert_eq!(state.held[0], None);
        assert_eq!(state.doors[0], None);
        state.held[0] = Some(held());
        assert!(acquired(&mut state, 0, Some(Acquisition::Attached(grip()))));
        assert_eq!(state.held[0].unwrap().kind, HeldKind::Door);
        assert!(state.doors[0].is_some());
        assert!(acquired(&mut state, 0, Some(Acquisition::Detached)));
        assert_eq!(state.held[0].unwrap().kind, HeldKind::Prop);
        assert!(!state.held[0].unwrap().joined);
        assert_eq!(state.doors[0], None);
    }

    #[test]
    fn missing_tracking_state_ends_both_door_holds_instead_of_skipping_continuity() {
        let mut state = PickupState::new();
        state.held = [Some(held()); 2];
        let mut out = TickOutcome::default();
        let mut engine = Engine::default();
        step_with(
            &mut state,
            Frame {
                palms: [None; 2],
                observed: [None; 2],
                dt: 0.02,
                world_scale: 1.0,
            },
            &DOORS.get().tearing,
            &mut out,
            &mut engine,
        );
        assert_eq!(state.held, [None; 2]);
        assert_eq!(out.lost, 1);
        assert_eq!(engine.requests, 0);
    }

    #[test]
    fn pre_drive_observation_preserves_two_hand_yank_for_carried_and_joined_doors() {
        let dt = 0.02;
        let body = bodies::BodyState {
            pose: BodyPose {
                centre: Vec3::ZERO,
                orientation: glam::Quat::IDENTITY,
            },
            velocities: Velocities::default(),
            mass_kg: Some(10.0),
            inverse_inertia: Vec3::ONE,
        };
        let config = *PICKUP.get();
        let strength = HandStrength {
            force_n: 350.0,
            ..HandStrength::full()
        };
        let palm = PalmMotion {
            pose: Pose::default(),
            velocity: Vec3::X * 3.0,
            angular_velocity: Vec3::ZERO,
        };
        let mut door = held();
        door.mass_kg = 10.0;
        door.grip = Grip::of(Pose::default(), body.pose, Vec3::ZERO);
        let target = door.grip.target(Pose {
            position: Vec3::X * 0.22,
            ..Pose::default()
        });
        let holds = [Some(HandHold {
            anchor: Vec3::ZERO,
            target,
            velocity: palm.velocity,
            angular_velocity: Vec3::ZERO,
        }); 2];
        let solution = hold::solve(
            &body.rigid(),
            holds,
            dt,
            &HoldTuning {
                gravity: Vec3::ZERO,
                ..carry_tuning(&door, &holds, &config, strength)
            },
        );
        assert!(
            (solution.velocities.linear.x - 1.4).abs() < 0.001,
            "{solution:?}"
        );
        // A yank faster than the hand's motion relative to the commanded body, so that only the
        // observation before the drive sees it.
        let tuning = TearTuning {
            yank_speed_mps: 1.8,
            ..DOORS.get().tearing
        };
        assert!((3.0 - solution.velocities.linear.x) < tuning.yank_speed_mps);
        let commanded = bodies::BodyState {
            velocities: solution.velocities,
            ..body
        };
        for joined in [false, true] {
            for observed in [body, commanded] {
                let h = Held { joined, ..door };
                let mut state = PickupState::new();
                state.held = [Some(h); 2];
                state.doors = [Some(grip()); 2];
                let mut engine = Engine::default();
                let report = |lag| HeldReport {
                    mass_kg: 10.0,
                    liftable: true,
                    shared: true,
                    centre: Vec3::ZERO,
                    anchor: Vec3::ZERO,
                    target: Vec3::X * lag,
                    lag_m: lag,
                    load: 1.0,
                    twist: 0.0,
                    joined,
                };
                for _ in 0..15 {
                    let mut out = TickOutcome {
                        reports: [Some(report(0.01)); 2],
                        ..TickOutcome::default()
                    };
                    step_with(
                        &mut state,
                        Frame {
                            palms: [Some(PalmMotion {
                                velocity: Vec3::ZERO,
                                ..palm
                            }); 2],
                            observed: [Some(Observation::of(&h, body)); 2],
                            dt,
                            world_scale: 1.0,
                        },
                        &tuning,
                        &mut out,
                        &mut engine,
                    );
                }
                engine.checks = 0;
                let mut out = TickOutcome {
                    reports: [Some(report(0.22)); 2],
                    ..TickOutcome::default()
                };
                step_with(
                    &mut state,
                    Frame {
                        palms: [Some(palm); 2],
                        observed: [Some(Observation::of(&h, observed)); 2],
                        dt,
                        world_scale: 1.0,
                    },
                    &tuning,
                    &mut out,
                    &mut engine,
                );
                assert_eq!(engine.checks, 1);
                if observed == body {
                    assert_eq!(engine.requests, 1);
                    assert_eq!(out.tears, 1);
                    assert!(state.held.into_iter().flatten().all(|actual| actual
                        == Held {
                            kind: HeldKind::Prop,
                            ..h
                        }));
                } else {
                    assert_eq!(engine.requests, 0, "commanded motion hides the actual yank");
                }
            }
        }
    }

    #[test_case::test_case(None; "valid frame")]
    #[test_case::test_case(Some(Mat4::ZERO); "singular frame resets intent")]
    #[test_case::test_case(Some(Mat4::NAN); "nonfinite frame resets intent")]
    fn rotating_and_translating_vehicle_keeps_the_local_tear_baseline(invalid_frame: Option<Mat4>) {
        for scale in [0.7, 1.0, 2.0] {
            let h = held();
            let mut state = PickupState::new();
            state.held[0] = Some(h);
            state.doors[0] = Some(grip());
            let mut engine = Engine::default();
            for tick in 0..=30 {
                let yank = tick == 30;
                let orientation = glam::Quat::from_rotation_z(tick as f32 * 0.1);
                let centre = Vec3::new(tick as f32 * 0.2, -2.0, 1.0);
                let world = Mat4::from_rotation_translation(orientation, centre);
                engine.vehicle_frame = if tick == 29 {
                    invalid_frame.or(Some(world))
                } else {
                    Some(world)
                };
                let anchor = world.transform_point3(Vec3::Y * scale);
                let lag = if yank { 0.22 } else { 0.01 };
                let target = anchor + world.transform_vector3(Vec3::X * lag * scale);
                let velocities = Velocities {
                    linear: Vec3::X * 10.0,
                    angular: Vec3::Z * 5.0,
                };
                let body = bodies::BodyState {
                    pose: BodyPose {
                        centre,
                        orientation,
                    },
                    velocities,
                    mass_kg: Some(10.0),
                    inverse_inertia: Vec3::ONE,
                };
                let palm = PalmMotion {
                    pose: Pose {
                        position: target,
                        orientation,
                    },
                    velocity: point_velocity(centre, velocities.linear, velocities.angular, target)
                        + if yank {
                            world.transform_vector3(Vec3::X * 3.0 * scale)
                        } else {
                            Vec3::ZERO
                        },
                    angular_velocity: velocities.angular,
                };
                let report = HeldReport {
                    mass_kg: 10.0,
                    liftable: true,
                    shared: false,
                    centre,
                    anchor,
                    target,
                    lag_m: lag,
                    load: 1.0,
                    twist: 0.0,
                    joined: true,
                };
                let mut out = TickOutcome {
                    reports: [Some(report), None],
                    ..TickOutcome::default()
                };
                step_with(
                    &mut state,
                    Frame {
                        palms: [Some(palm), None],
                        observed: [Some(Observation::of(&h, body)), None],
                        dt: 0.02,
                        world_scale: scale,
                    },
                    &DOORS.get().tearing,
                    &mut out,
                    &mut engine,
                );
                let tears = yank && invalid_frame.is_none();
                assert_eq!(
                    engine.requests,
                    usize::from(tears),
                    "tick={tick}, scale={scale}"
                );
                assert_eq!(out.tears, u64::from(tears));
            }
        }
    }

    #[test]
    fn both_hands_keep_the_same_body_joint_and_grab_after_detachment_once() {
        let h = held();
        let mut state = PickupState::new();
        state.held = [Some(h); 2];
        let mut out = TickOutcome::default();
        apply(&mut state, &h, TearResult::Detached, &mut out);
        apply(&mut state, &h, TearResult::Detached, &mut out);
        assert_eq!(out.tears, 1);
        for actual in state.held.into_iter().flatten() {
            assert_eq!(
                actual,
                Held {
                    kind: HeldKind::Prop,
                    ..h
                }
            );
        }
        assert_eq!(out.released, [false; 2]);
    }

    #[test]
    fn lost_or_changed_native_body_ends_both_old_holds_without_rebinding() {
        let h = held();
        let mut state = PickupState::new();
        state.held = [Some(h); 2];
        let mut out = TickOutcome::default();
        apply(&mut state, &h, TearResult::Lost, &mut out);
        assert_eq!(state.held, [None; 2]);
        assert_eq!(out.lost, 1);
        assert_eq!(out.released, [true; 2]);
    }

    #[test]
    fn refused_damage_leaves_the_holds_unchanged() {
        let h = held();
        let mut state = PickupState::new();
        state.held = [Some(h); 2];
        let mut out = TickOutcome::default();
        apply(&mut state, &h, TearResult::Refused, &mut out);
        assert_eq!(state.held, [Some(h); 2]);
        assert_eq!(out.tear_refusals, 1);
        assert_eq!(out.tears, 0);
    }
}
