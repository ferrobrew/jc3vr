mod current;
mod review;
mod seated;

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
const GROUND: Mode = Mode::OnFoot(OnFootVariant::Ground);
const FLIGHT: FlightPose = FlightPose {
    lateral: [0.30; 2],
    reach: [0.32; 2],
    height: [0.0; 2],
    angle_deg: [5.0; 2],
    palm_roll_deg: [0.0; 2],
};
const FOLDED: FlightPose = FlightPose {
    lateral: [0.03; 2],
    reach: [0.20; 2],
    height: [-0.19; 2],
    angle_deg: [80.0; 2],
    palm_roll_deg: [0.0; 2],
};
const BRAKE: FlightPose = FlightPose {
    palm_roll_deg: [90.0; 2],
    ..FLIGHT
};

struct History {
    state: WingsuitGestures,
    sample: GestureSample,
    mode: Mode,
    airborne: bool,
    gameplay: bool,
    canopy: bool,
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
            airborne: false,
            gameplay: true,
            canopy: false,
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
                airborne: self.airborne,
                gameplay: self.gameplay,
                canopy_requested: self.canopy,
                pose,
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

fn deploys(commands: &[GestureCommands]) -> usize {
    commands
        .iter()
        .filter(|c| c.transition == Some(SuitTransition::Deploy))
        .count()
}

#[test]
fn a_flight_pose_held_from_the_start_of_a_fall_deploys_after_the_dwell() {
    let mut h = History::new(FALLING);
    // The dwell starts on the first sample and is met on the sixth, past 0.2 s later.
    assert_eq!(deploys(&h.hold(FLIGHT, 5)), 0);
    assert_eq!(h.state.phase(), GesturePhase::AwaitingFlight);
    assert_eq!(deploys(&h.hold(FLIGHT, 1)), 1);
}

#[test]
fn a_refused_deploy_is_asked_again_only_after_the_retry_pause() {
    let mut h = History::new(FALLING);
    let commands = h.hold(FLIGHT, 30);
    let at: Vec<usize> = commands
        .iter()
        .enumerate()
        .filter(|(_, c)| c.transition == Some(SuitTransition::Deploy))
        .map(|(i, _)| i)
        .collect();
    // Samples are 50 ms apart: the first request on the sixth, the next 0.5 s later.
    assert_eq!(at, [5, 15, 25]);
}

#[test]
fn arms_spread_under_a_canopy_deploy_the_suit_once_it_is_cut() {
    let mut h = History::new(CANOPY);
    assert_eq!(deploys(&h.hold(FLIGHT, 20)), 0);
    h.mode = FALLING;
    assert_eq!(deploys(&h.hold(FLIGHT, 5)), 0);
    assert_eq!(deploys(&h.hold(FLIGHT, 1)), 1);
}

#[test]
fn the_recognizer_watches_through_the_jump_and_deploys_during_it() {
    // Arms spread on stepping off: the dwell is met in the jump, and the deploy goes in there.
    let mut h = History::new(GROUND);
    assert_eq!(deploys(&h.hold(FLIGHT, 10)), 0);
    assert_eq!(h.state.phase(), GesturePhase::Unavailable);
    h.airborne = true;
    assert_eq!(deploys(&h.hold(FLIGHT, 5)), 0);
    assert_eq!(deploys(&h.hold(FLIGHT, 1)), 1);
    // The game's fall continues the jump's: the dwell carries over rather than restarting.
    let mut h = History::new(GROUND);
    h.airborne = true;
    assert_eq!(deploys(&h.hold(FLIGHT, 3)), 0);
    h.mode = FALLING;
    assert_eq!(deploys(&h.hold(FLIGHT, 2)), 0);
    assert_eq!(deploys(&h.hold(FLIGHT, 1)), 1);
}

#[test]
fn a_fall_out_of_the_suit_needs_the_neutral_before_it_deploys() {
    let mut h = History::new(SUIT);
    h.hold(FLIGHT, 6);
    h.mode = FALLING;
    assert_eq!(deploys(&h.hold(FLIGHT, 20)), 0);
    assert_eq!(h.state.phase(), GesturePhase::AwaitingNeutral);
    h.hold(FOLDED, 5);
    assert_eq!(deploys(&h.hold(FLIGHT, 6)), 1);
    // A menu over the fall leaves the fall's origin as it was.
    let mut h = History::new(SUIT);
    h.hold(FLIGHT, 2);
    h.mode = Mode::Ui;
    h.observe(Some(FLIGHT));
    h.mode = FALLING;
    assert_eq!(deploys(&h.hold(FLIGHT, 20)), 0);
}

#[test]
fn a_claim_in_the_middle_of_a_fall_does_not_ask_for_the_neutral() {
    let mut h = History::new(FALLING);
    h.hold(FOLDED, 2);
    h.claims[Hand::Left.index()].grip = true;
    assert_eq!(deploys(&h.hold(FLIGHT, 10)), 0);
    h.claims = [HandClaims::default(); 2];
    assert_eq!(deploys(&h.hold(FLIGHT, 6)), 1);
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
        palm_roll_deg: [40.0; 2],
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
fn each_transient_claim_suspends_and_requires_a_fresh_pose() {
    for hand in Hand::BOTH {
        for claim in [
            HandClaims {
                grip: true,
                ..Default::default()
            },
            HandClaims {
                slot: true,
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
    // Cutting the canopy starts a fall afresh: the arms still spread open the suit.
    assert_eq!(deploys(&h.hold(FLIGHT, 20)), 2);
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
        if mode == Mode::Ui {
            // A menu hides nothing of the fall's origin: out of the suit, it needs the neutral.
            assert_eq!(deploys(&h.hold(FLIGHT, 12)), 0);
            h.hold(FOLDED, 5);
        }
        // The flight pose's dwell restarts in the fall rather than carrying over.
        assert_eq!(deploys(&h.hold(FLIGHT, 5)), 0);
        assert_eq!(deploys(&h.hold(FLIGHT, 1)), 1);
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
                airborne: false,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FOLDED),
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

/// A wrist frame with the fingers out along `hand`'s arm and the palm rolled `roll_deg` from flat
/// toward facing forward, for a player facing `facing` (the rotation from facing -Z).
fn spread_wrist(hand: Hand, roll_deg: f32, facing: Quat) -> Quat {
    let side = if hand == Hand::Left { -1.0 } else { 1.0 };
    facing * Quat::from_rotation_x(roll_deg.to_radians()) * Quat::from_rotation_y(-side * FRAC_PI_2)
}

#[test]
fn calibrated_measurement_uses_each_shoulders_side_and_the_wrists_roll() {
    let span = 1.8;
    let heading = Quat::from_rotation_y(FRAC_PI_2);
    // The player faces -X, with the left shoulder and wrist on +Z and the right on -Z.
    let shoulders = [Vec3::new(0.0, 0.0, 0.2), Vec3::new(0.0, 0.0, -0.2)];
    let positions = [Vec3::new(0.0, 0.0, 0.74), Vec3::new(0.0, 0.0, -0.74)];
    let measure = |scale: f32, roll_deg: f32| {
        let wrists = Hand::BOTH.map(|hand| Pose {
            position: positions[hand.index()] * scale,
            orientation: spread_wrist(hand, roll_deg, heading),
        });
        FlightPose::measure(wrists, shoulders.map(|p| p * scale), heading, span * scale).unwrap()
    };
    let pose = measure(1.0, 90.0);
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
        assert!((pose.palm_roll_deg[i] - 90.0).abs() < 1e-3, "{pose:?}");
    }
    assert_eq!(pose, measure(2.0, 90.0));
    for roll_deg in [0.0, 35.0, -90.0, 150.0] {
        let rolled = measure(1.0, roll_deg);
        assert!(
            rolled
                .palm_roll_deg
                .into_iter()
                .all(|v| (v - roll_deg).abs() < 1e-3),
            "{roll_deg}: {rolled:?}"
        );
    }
}

#[test]
fn the_palm_roll_ignores_raising_and_sweeping_the_arms() {
    let heading = Quat::IDENTITY;
    let shoulders = [Vec3::new(-0.2, 0.0, 0.0), Vec3::new(0.2, 0.0, 0.0)];
    // Each arm raised 30 degrees and swept 30 degrees forward, the hand carried with it.
    let carry = |hand: Hand| {
        let side = if hand == Hand::Left { -1.0 } else { 1.0 };
        Quat::from_rotation_y(side * 30_f32.to_radians())
            * Quat::from_axis_angle(Vec3::Z, side * 30_f32.to_radians())
    };
    let wrists = Hand::BOTH.map(|hand| {
        let carry = carry(hand);
        let out = if hand == Hand::Left {
            Vec3::NEG_X
        } else {
            Vec3::X
        };
        Pose {
            position: shoulders[hand.index()] + carry * out * 0.6,
            orientation: carry * spread_wrist(hand, 80.0, heading),
        }
    });
    let pose = FlightPose::measure(wrists, shoulders, heading, 1.8).unwrap();
    assert!(
        pose.palm_roll_deg
            .into_iter()
            .all(|v| (v - 80.0).abs() < 1e-2),
        "{pose:?}"
    );
    assert!(pose.braking_palms(&CONFIG, false), "{pose:?}");
}

#[test]
fn the_arms_face_the_body_while_the_head_looks_around() {
    let head = |yaw_deg: f32| Pose {
        position: Vec3::new(0.0, 1.2, 0.0),
        orientation: Quat::from_rotation_y(yaw_deg.to_radians()),
    };
    let wrists = [Vec3::new(-0.8, 0.95, 0.1), Vec3::new(0.8, 0.95, 0.1)].map(|position| Pose {
        position,
        orientation: Quat::IDENTITY,
    });
    for yaw_deg in [-70.0, -30.0, 0.0, 45.0, 70.0] {
        let heading = flight_heading(wrists, head(yaw_deg));
        assert!(
            heading.angle_between(Quat::IDENTITY) < 1e-3,
            "{yaw_deg}: {heading:?}"
        );
    }
    // Past a head's turn, and with the wrists together, the arms say nothing about the body.
    let behind = flight_heading(wrists, head(120.0));
    assert!(behind.angle_between(Quat::from_rotation_y(120_f32.to_radians())) < 1e-3);
    let together = [Vec3::new(-0.05, 0.9, -0.4), Vec3::new(0.05, 0.9, -0.4)].map(|position| Pose {
        position,
        orientation: Quat::IDENTITY,
    });
    let alone = flight_heading(together, head(40.0));
    assert!(alone.angle_between(Quat::from_rotation_y(40_f32.to_radians())) < 1e-3);
    // A player turned to face the cockpit's back still stands upright.
    let turned = wrists.map(|wrist| Pose {
        position: wrist.position * Vec3::new(-1.0, 1.0, -1.0),
        ..wrist
    });
    let heading = flight_heading(turned, head(180.0));
    assert!(
        (heading * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5),
        "{heading:?}"
    );
    assert!(
        (heading * Vec3::NEG_Z).abs_diff_eq(Vec3::Z, 1e-5),
        "{heading:?}"
    );
}

#[test]
fn a_spread_flight_pose_arms_the_suit_without_a_neutral_first() {
    let mut h = History::new(SUIT);
    assert!(h.hold(FLIGHT, 6).iter().all(|c| c.transition.is_none()));
    assert!(h.state.armed);
    assert_eq!(h.state.phase(), GesturePhase::Armed);
    assert!(h.hold(BRAKE, 6).iter().any(|c| c.brake));
    assert_eq!(h.state.phase(), GesturePhase::Braking);
    // A fall's start needs no neutral either.
    let mut h = History::new(FALLING);
    assert_eq!(deploys(&h.hold(FLIGHT, 6)), 1);
}

#[test]
fn palms_past_the_brake_band_toward_palm_up_do_not_brake() {
    let mut h = History::new(SUIT);
    h.arm();
    let palm_up = FlightPose {
        palm_roll_deg: [CONFIG.brake_roll_max_deg + 1.0; 2],
        ..FLIGHT
    };
    assert!(h.hold(palm_up, 12).iter().all(|c| !c.brake));
    let one_hand = FlightPose {
        palm_roll_deg: [90.0, 20.0],
        ..FLIGHT
    };
    assert!(h.hold(one_hand, 12).iter().all(|c| !c.brake));
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
                airborne: false,
                gameplay: true,
                canopy_requested: false,
                pose: Some(FOLDED),
                claims: [HandClaims::default(); 2],
            },
            &invalid
        ),
        GestureCommands::default()
    );
    assert!(!h.state.armed);
}
