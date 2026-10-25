mod current;
mod review;

use std::f32::consts::FRAC_PI_2;

use glam::Vec2;

use super::*;
use crate::controllers::{
    frame::{ActionSetKind, HandFrame},
    mode::{OnFootVariant, VehicleClass, VehicleSeat},
    vehicles::arm_steering::{combine, reserve_brake_pitch},
};

const CONFIG: WingsuitGesturesConfig = WingsuitGesturesConfig::new();
const FALLING: Mode = Mode::Airborne(AirborneVariant::Falling);
const SUIT: Mode = Mode::Airborne(AirborneVariant::Wingsuit);
const CANOPY: Mode = Mode::Airborne(AirborneVariant::Parachute);
const FLIGHT: FlightPose = FlightPose {
    lateral: [0.30; 2],
    reach: [0.32; 2],
    height: [0.0; 2],
    angle_deg: [5.0; 2],
    palm_forward: [0.0; 2],
};
const FOLDED: FlightPose = FlightPose {
    lateral: [0.03; 2],
    reach: [0.20; 2],
    height: [-0.19; 2],
    angle_deg: [80.0; 2],
    palm_forward: [0.0; 2],
};
const BRAKE: FlightPose = FlightPose {
    palm_forward: [1.0; 2],
    ..FLIGHT
};

struct History {
    state: WingsuitGestures,
    sample: GestureSample,
    mode: Mode,
    gameplay: bool,
    canopy: bool,
    occupied: [bool; 2],
    claims: [HandClaims; 2],
}

impl History {
    fn new(mode: Mode) -> Self {
        Self {
            state: WingsuitGestures::default(),
            sample: GestureSample {
                epoch: 1,
                sequence: 0,
                time_ns: 0,
            },
            mode,
            gameplay: true,
            canopy: false,
            occupied: [false; 2],
            claims: [HandClaims::default(); 2],
        }
    }

    fn observe(&mut self, pose: Option<FlightPose>) -> GestureCommands {
        self.sample.sequence += 1;
        self.sample.time_ns += 50_000_000;
        self.state.step(
            GestureInput {
                sample: self.sample,
                mode: self.mode,
                gameplay: self.gameplay,
                canopy_requested: self.canopy,
                pose,
                occupied: self.occupied,
                claims: self.claims,
            },
            &CONFIG,
        )
    }

    fn hold(&mut self, pose: FlightPose, count: usize) -> Vec<GestureCommands> {
        (0..count).map(|_| self.observe(Some(pose))).collect()
    }

    fn arm(&mut self) {
        assert!(self.hold(FOLDED, 5).iter().all(|c| c.transition.is_none()));
        assert!(self.hold(FLIGHT, 6).iter().all(|c| c.transition.is_none()));
        assert!(self.state.armed);
    }
}

#[test]
fn deployment_requires_a_fresh_folded_then_flight_sequence_in_freefall() {
    let mut h = History::new(FALLING);
    assert!(h.hold(FLIGHT, 20).iter().all(|c| c.transition.is_none()));
    h.hold(FOLDED, 5);
    let deployed = h.hold(FLIGHT, 6);
    assert_eq!(
        deployed
            .iter()
            .filter(|c| c.transition == Some(SuitTransition::Deploy))
            .count(),
        1
    );
    assert!(h.hold(FLIGHT, 20).iter().all(|c| c.transition.is_none()));
}

#[test]
fn button_deployment_with_arms_down_does_not_close_the_suit() {
    let mut h = History::new(SUIT);
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
    h.hold(FLIGHT, 6);
    let close = h.hold(FOLDED, 9);
    assert_eq!(
        close
            .iter()
            .filter(|c| c.transition == Some(SuitTransition::Close))
            .count(),
        1
    );
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
}

#[test]
fn braking_reserves_pitch_and_cannot_close_the_suit() {
    let mut h = History::new(SUIT);
    h.arm();
    let entering = h.observe(Some(BRAKE));
    assert!(entering.reserve_pitch && !entering.brake);
    let braking = h.hold(BRAKE, 12);
    assert!(braking.iter().any(|c| c.brake));
    assert!(
        braking
            .iter()
            .all(|c| c.transition.is_none() && c.reserve_pitch)
    );
    let halfway = FlightPose {
        palm_forward: [0.5; 2],
        ..FLIGHT
    };
    assert!(h.observe(Some(halfway)).reserve_pitch);
    assert!(h.observe(Some(halfway)).reserve_pitch);
    let released = h.observe(Some(FLIGHT));
    assert!(!released.brake && !released.reserve_pitch);
    assert!(!h.observe(Some(FLIGHT)).reserve_pitch);
    assert_eq!(
        combine(
            Vec2::new(0.2, 0.3),
            reserve_brake_pitch(Some(Vec2::new(0.4, 1.0)), true)
        ),
        Vec2::new(0.6, 0.3)
    );
}

#[test]
fn a_single_withdrawn_arm_and_the_deadzone_do_not_close() {
    let mut h = History::new(SUIT);
    h.arm();
    let one_arm = FlightPose {
        lateral: [0.03, 0.30],
        reach: [0.20, 0.32],
        ..FLIGHT
    };
    assert!(h.hold(one_arm, 20).iter().all(|c| c.transition.is_none()));
    let deadzone = FlightPose {
        lateral: [0.16; 2],
        reach: [0.22; 2],
        ..FLIGHT
    };
    assert!(h.hold(deadzone, 20).iter().all(|c| c.transition.is_none()));
    h.hold(FOLDED, 3);
    h.observe(Some(deadzone));
    assert!(h.hold(FOLDED, 3).iter().all(|c| c.transition.is_none()));
}

#[test]
fn missing_invalid_and_stale_tracking_never_become_withdrawal() {
    for invalid in [
        None,
        Some(FlightPose {
            reach: [f32::NAN, 0.2],
            ..FOLDED
        }),
    ] {
        let mut h = History::new(SUIT);
        h.arm();
        h.hold(FOLDED, 3);
        assert_eq!(h.observe(invalid), GestureCommands::default());
        assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
        h.hold(FLIGHT, 6);
        assert!(
            h.hold(FOLDED, 9)
                .iter()
                .any(|c| c.transition == Some(SuitTransition::Close))
        );
    }
    let mut h = History::new(SUIT);
    h.arm();
    h.hold(FOLDED, 3);
    h.sample.time_ns += 1_000_000_000;
    assert_eq!(h.observe(Some(FOLDED)), GestureCommands::default());
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
}

#[test]
fn either_gun_hand_and_each_transient_claim_suspend_and_require_a_fresh_pose() {
    for hand in Hand::BOTH {
        for claim in [
            HandClaims {
                grip: true,
                ..Default::default()
            },
            HandClaims {
                trigger: true,
                ..Default::default()
            },
            HandClaims {
                grapple: true,
                ..Default::default()
            },
            HandClaims {
                mechanical: true,
                ..Default::default()
            },
        ] {
            let mut h = History::new(SUIT);
            h.arm();
            h.claims[hand.index()] = claim;
            assert!(
                h.hold(FOLDED, 12)
                    .iter()
                    .all(|c| *c == GestureCommands::default())
            );
            h.claims = [HandClaims::default(); 2];
            assert!(h.hold(FOLDED, 12).iter().all(|c| c.transition.is_none()));
            h.hold(FLIGHT, 6);
            assert!(
                h.hold(FOLDED, 9)
                    .iter()
                    .any(|c| c.transition == Some(SuitTransition::Close))
            );
        }
        let mut h = History::new(SUIT);
        h.arm();
        h.occupied[hand.index()] = true;
        h.hold(FOLDED, 12);
        h.occupied = [false; 2];
        assert!(h.hold(FOLDED, 12).iter().all(|c| c.transition.is_none()));
        h.hold(FLIGHT, 6);
        assert!(h.state.armed);
    }
}

#[test]
fn canopy_request_cancels_pending_close_and_brake_before_the_mode_changes() {
    let mut h = History::new(SUIT);
    h.arm();
    h.hold(FOLDED, 5);
    h.canopy = true;
    assert_eq!(h.observe(Some(FOLDED)), GestureCommands::default());
    h.canopy = false;
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
    h.arm();
    h.hold(BRAKE, 6);
    h.canopy = true;
    assert_eq!(h.observe(Some(BRAKE)), GestureCommands::default());
    h.mode = CANOPY;
    h.hold(BRAKE, 6);
    h.canopy = false;
    h.mode = FALLING;
    assert!(h.hold(FLIGHT, 20).iter().all(|c| c.transition.is_none()));
}

#[test]
fn every_mode_and_epoch_boundary_discards_a_stale_flight_pose() {
    for mode in [
        CANOPY,
        Mode::Ui,
        Mode::OnFoot(OnFootVariant::Ground),
        Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        }),
    ] {
        let mut h = History::new(SUIT);
        h.arm();
        h.mode = mode;
        h.observe(Some(FLIGHT));
        h.mode = FALLING;
        assert!(h.hold(FLIGHT, 12).iter().all(|c| c.transition.is_none()));
        h.hold(FOLDED, 5);
        assert!(
            h.hold(FLIGHT, 6)
                .iter()
                .any(|c| c.transition == Some(SuitTransition::Deploy))
        );
    }
    let mut h = History::new(SUIT);
    h.arm();
    h.sample.epoch += 1;
    h.sample.sequence = 0;
    h.sample.time_ns = 0;
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
    h.hold(FLIGHT, 6);
    assert!(h.state.armed);
    h.state.reset();
    assert!(h.hold(FLIGHT, 20).iter().all(|c| c.transition.is_none()));
}

#[test]
fn menus_focus_and_repeated_samples_do_not_accumulate_pose_dwell() {
    let mut h = History::new(SUIT);
    h.arm();
    h.gameplay = false;
    assert_eq!(h.observe(Some(FOLDED)), GestureCommands::default());
    h.gameplay = true;
    assert!(h.hold(FOLDED, 20).iter().all(|c| c.transition.is_none()));
    h.hold(FLIGHT, 6);
    h.hold(FOLDED, 2);
    for _ in 0..100 {
        let command = h.state.step(
            GestureInput {
                sample: h.sample,
                mode: SUIT,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FOLDED),
                occupied: [false; 2],
                claims: [HandClaims::default(); 2],
            },
            &CONFIG,
        );
        assert!(command.transition.is_none());
    }
    assert!(
        h.hold(FOLDED, 9)
            .iter()
            .any(|c| c.transition == Some(SuitTransition::Close))
    );
}

#[test]
fn calibrated_measurement_uses_each_shoulders_side_and_the_wrist_palm_normal() {
    let span = 1.8;
    let heading = Quat::from_rotation_y(FRAC_PI_2);
    // The player faces -X, with the left shoulder and wrist on +Z and the right on -Z.
    let shoulders = [Vec3::new(0.0, 0.0, 0.2), Vec3::new(0.0, 0.0, -0.2)];
    let positions = [Vec3::new(0.0, 0.0, 0.74), Vec3::new(0.0, 0.0, -0.74)];
    let measure = |scale: f32, orientation: Quat| {
        let wrists = positions.map(|position| Pose {
            position: position * scale,
            orientation,
        });
        FlightPose::measure(wrists, shoulders.map(|p| p * scale), heading, span * scale).unwrap()
    };
    // This rotation maps the wrist's -Y palm normal to world -X without using the heading.
    let palms_forward = Quat::from_rotation_z(-FRAC_PI_2);
    let pose = measure(1.0, palms_forward);
    for i in 0..2 {
        assert!((pose.lateral[i] - 0.30).abs() <= f32::EPSILON, "{pose:?}");
        assert!((pose.reach[i] - 0.30).abs() <= f32::EPSILON, "{pose:?}");
        assert_eq!(pose.height[i], 0.0, "{pose:?}");
        // Near zero, acos magnifies quaternion/dot rounding. One epsilon permits only the
        // two representable f32 steps immediately below one in the cosine domain.
        assert!(
            (1.0 - pose.angle_deg[i].to_radians().cos()).abs() <= f32::EPSILON,
            "{pose:?}"
        );
        assert!(
            (pose.palm_forward[i] - 1.0).abs() <= f32::EPSILON,
            "{pose:?}"
        );
    }
    assert_eq!(pose, measure(2.0, palms_forward));
    let palms_down = measure(1.0, Quat::IDENTITY);
    assert!(
        palms_down.palm_forward.into_iter().all(|v| v == 0.0),
        "{palms_down:?}"
    );
    let palms_backward = measure(1.0, Quat::from_rotation_z(FRAC_PI_2));
    assert!(
        palms_backward
            .palm_forward
            .into_iter()
            .all(|v| (v + 1.0).abs() <= f32::EPSILON),
        "{palms_backward:?}"
    );
}

#[test]
fn tuning_rejects_nonfinite_values_and_overlapping_pose_ranges() {
    assert!(CONFIG.valid());
    assert!(
        !WingsuitGesturesConfig {
            flight_dwell_s: f32::NAN,
            ..CONFIG
        }
        .valid()
    );
    assert!(
        !WingsuitGesturesConfig {
            flight_lateral_span: 0.15,
            withdraw_lateral_span: 0.14,
            ..CONFIG
        }
        .valid()
    );
    let mut h = History::new(SUIT);
    h.arm();
    let invalid = WingsuitGesturesConfig {
        close_dwell_s: 0.0,
        ..CONFIG
    };
    assert_eq!(
        h.state.step(
            GestureInput {
                sample: h.sample,
                mode: SUIT,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FOLDED),
                occupied: [false; 2],
                claims: [HandClaims::default(); 2],
            },
            &invalid
        ),
        GestureCommands::default()
    );
    assert!(!h.state.armed);
}
