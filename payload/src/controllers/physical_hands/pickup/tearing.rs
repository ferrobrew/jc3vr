//! Once-per-body door tearing and conversion of surviving holds to loose props.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::physical_hands::{
    bodies,
    doors::{
        config::TearConfig,
        tearing::{self, Acquisition, Gesture, TearGrip, TearResult},
    },
    grasp::PalmMotion,
    pickup::{
        state::{Held, HeldKind, PickupState},
        tick::TickOutcome,
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

/// Install the checked identity, or refuse the new hold without leaving an untracked door.
pub(super) fn acquired(state: &mut PickupState, index: usize, result: Option<Acquisition>) -> bool {
    state.doors[index] = None;
    let Some(result) = result else {
        state.held[index] = None;
        return false;
    };
    match result {
        Acquisition::Attached(grip) => state.doors[index] = Some(grip),
        Acquisition::Detached => {
            if let Some(held) = state.held[index].as_mut() {
                held.kind = HeldKind::Prop;
                held.joined = false;
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

/// # Safety
/// Runs on the game thread, outside the physics step.
pub(super) unsafe fn step(
    state: &mut PickupState,
    frame: Frame,
    config: &TearConfig,
    outcome: &mut TickOutcome,
) {
    step_with(state, frame, config, outcome, &mut NativeTearing);
}

fn step_with(
    state: &mut PickupState,
    frame: Frame,
    config: &TearConfig,
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
            apply(state, &held, TearResult::Lost, outcome);
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
            apply(state, &held, TearResult::Lost, outcome);
            continue;
        };
        if !engine.alive(&held) {
            apply(state, &held, TearResult::Lost, outcome);
            continue;
        }
        let continuity = engine.continuity(&grip, &held);
        if continuity != TearResult::Refused {
            apply(state, &held, continuity, outcome);
            continue;
        }
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
            let body_velocity = body.velocities.linear
                + body
                    .velocities
                    .angular
                    .cross(report.anchor - body.pose.centre);
            let scale = world_scale.max(0.01);
            let speed = (palm.point_velocity(report.target) - body_velocity)
                .dot(lag.normalize_or_zero())
                / scale;
            let hinge = engine.hinge(&mut grip, report.anchor);
            let (at_limit, opening_push_m) =
                hinge.map_or((false, 0.0), |(angle, range, tangent)| {
                    (
                        angle >= range - config.limit_margin_rad.clamp(0.0, 0.2),
                        lag.dot(tangent) / scale,
                    )
                });
            requested |= grip.gesture(
                Gesture {
                    dt,
                    lag_m: lag.length() / scale,
                    separating_speed_mps: speed,
                    opening_push_m,
                    load: report.load,
                    at_limit,
                },
                config,
            );
            state.doors[k] = Some(grip);
        }
        if requested {
            let result = engine.tear(&grip, &held);
            apply(state, &held, result, outcome);
        }
    }
}

trait TearEngine {
    fn alive(&mut self, held: &Held) -> bool;
    fn continuity(&mut self, grip: &TearGrip, held: &Held) -> TearResult;
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

    fn hinge(&mut self, grip: &mut TearGrip, anchor: Vec3) -> Option<(f32, f32, Vec3)> {
        unsafe { tearing::hinge(grip, anchor) }
    }

    fn tear(&mut self, grip: &TearGrip, held: &Held) -> TearResult {
        unsafe { tearing::tear(grip, held.object, held.body) }
    }
}

fn apply(state: &mut PickupState, held: &Held, result: TearResult, outcome: &mut TickOutcome) {
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
                state.held[k] = None;
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

    use glam::Vec3;

    use super::*;
    use crate::controllers::{
        frame::Pose,
        physical_hands::{
            config::PickupConfig,
            doors::DoorTarget,
            grasp::Grip,
            hold::{self, HandHold},
            pickup::{HeldReport, tick::carry_tuning},
            rigid::{BodyPose, Velocities},
        },
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
    }

    impl TearEngine for Engine {
        fn alive(&mut self, _: &Held) -> bool {
            true
        }
        fn continuity(&mut self, _: &TearGrip, _: &Held) -> TearResult {
            self.checks += 1;
            TearResult::Refused
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
            &TearConfig::new(),
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
        let config = PickupConfig {
            gravity_mps2: 0.0,
            ..PickupConfig::new()
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
            &carry_tuning(&door, &holds, &config),
        );
        assert!(
            (solution.velocities.linear.x - 1.4).abs() < 0.001,
            "{solution:?}"
        );
        assert!((3.0 - solution.velocities.linear.x) < TearConfig::new().yank_speed_mps);
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
                        &TearConfig::new(),
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
                    &TearConfig::new(),
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
