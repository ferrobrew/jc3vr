use glam::Quat;

use super::*;

fn tick() -> HingeTick {
    HingeTick {
        held: false,
        lid: false,
        angle_rad: 1.0,
        range_rad: 1.2,
        rate_rad_s: Some(0.0),
        rested_s: 0.0,
        dt: 1.0 / 30.0,
    }
}

const CONFIG: HingeConfig = HingeConfig::new();

#[test]
fn damping_preserves_vehicle_translation_and_rotation_at_the_door_com() {
    for (linear, angular, at_door) in [
        (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
        (
            Vec3::new(12.0, -3.0, 5.0),
            Vec3::ZERO,
            Vec3::new(12.0, -3.0, 5.0),
        ),
        (
            Vec3::ZERO,
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(-2.0, 0.0, -6.0),
        ),
        (
            Vec3::new(12.0, -3.0, 5.0),
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 7.0, 1.0),
        ),
    ] {
        let chassis = state(Vec3::new(10.0, -4.0, 8.0), Velocities { linear, angular });
        let carrier = Velocities {
            linear: at_door,
            angular,
        };
        let door = state(Vec3::new(13.0, -2.0, 7.0), carrier);
        for factor in [0.0, 0.25, 1.0] {
            let result =
                hinge_velocities(&door, &chassis, Swing::Damped(factor), 0.1, 9.81).unwrap();
            assert_velocities(result, carrier);
        }
    }
}

#[test]
fn damping_scales_only_the_relative_swing() {
    let relative = Velocities {
        linear: Vec3::new(2.0, -4.0, 6.0),
        angular: Vec3::new(-2.0, 4.0, 1.0),
    };
    for moving in [false, true] {
        let chassis = state(
            Vec3::new(10.0, -4.0, 8.0),
            if moving {
                Velocities {
                    linear: Vec3::new(12.0, -3.0, 5.0),
                    angular: Vec3::new(1.0, 2.0, 3.0),
                }
            } else {
                Velocities::default()
            },
        );
        let carrier = Velocities {
            linear: if moving {
                Vec3::new(4.0, 7.0, 1.0)
            } else {
                Vec3::ZERO
            },
            angular: chassis.velocities.angular,
        };
        let door = state(
            Vec3::new(13.0, -2.0, 7.0),
            Velocities {
                linear: carrier.linear + relative.linear,
                angular: carrier.angular + relative.angular,
            },
        );
        for factor in [0.0, 0.25, 1.0] {
            let result =
                hinge_velocities(&door, &chassis, Swing::Damped(factor), 0.1, 9.81).unwrap();
            assert_velocities(
                result,
                Velocities {
                    linear: carrier.linear + relative.linear * factor,
                    angular: carrier.angular + relative.angular * factor,
                },
            );
        }
    }
}

#[test]
fn strut_preserves_carrier_motion_and_cancels_world_gravity_when_tilted() {
    for orientation in [
        Quat::IDENTITY,
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        Quat::from_rotation_x(std::f32::consts::PI),
    ] {
        let mut chassis = state(
            Vec3::new(10.0, -4.0, 8.0),
            Velocities {
                linear: Vec3::new(12.0, -3.0, 5.0),
                angular: Vec3::new(1.0, 2.0, 3.0),
            },
        );
        chassis.pose.orientation = orientation;
        let mut door = state(
            Vec3::new(13.0, -2.0, 7.0),
            Velocities {
                linear: Vec3::new(-10.0, 20.0, 4.0),
                angular: Vec3::new(4.0, -2.0, 8.0),
            },
        );
        door.pose.orientation = orientation * Quat::from_rotation_x(0.8);
        let result = hinge_velocities(&door, &chassis, Swing::Strut, 0.1, 9.81).unwrap();
        assert_velocities(
            result,
            Velocities {
                linear: Vec3::new(4.0, 7.981, 1.0),
                angular: chassis.velocities.angular,
            },
        );
        assert_velocities(
            Velocities {
                linear: result.linear + Vec3::NEG_Y * 9.81 * 0.1,
                ..result
            },
            Velocities {
                linear: Vec3::new(4.0, 7.0, 1.0),
                angular: chassis.velocities.angular,
            },
        );
    }
}

#[test]
fn a_stationary_strut_only_compensates_positive_gravity_and_time() {
    let chassis = state(Vec3::ZERO, Velocities::default());
    let door = state(
        Vec3::new(3.0, 2.0, -1.0),
        Velocities {
            linear: Vec3::ONE,
            angular: Vec3::ONE,
        },
    );
    for (dt, g, lift) in [
        (0.1, 10.0, 1.0),
        (0.0, 10.0, 0.0),
        (-0.1, 10.0, 0.0),
        (0.1, -10.0, 0.0),
    ] {
        assert_velocities(
            hinge_velocities(&door, &chassis, Swing::Strut, dt, g).unwrap(),
            Velocities {
                linear: Vec3::Y * lift,
                angular: Vec3::ZERO,
            },
        );
    }
}

#[test]
fn held_free_and_latching_doors_do_not_request_velocity_writes() {
    let body = state(Vec3::ZERO, Velocities::default());
    for swing in [
        Swing::Held,
        Swing::Free,
        Swing::Latch(LatchCause::Shoved),
        Swing::Latch(LatchCause::Settled),
    ] {
        assert_eq!(hinge_velocities(&body, &body, swing, 0.1, 9.81), None);
    }
}

fn state(centre: Vec3, velocities: Velocities) -> BodyState {
    BodyState {
        pose: BodyPose {
            centre,
            orientation: Quat::IDENTITY,
        },
        velocities,
        mass_kg: Some(10.0),
        inverse_inertia: Vec3::ONE,
    }
}

fn assert_velocities(actual: Velocities, expected: Velocities) {
    assert!(
        actual.linear.abs_diff_eq(expected.linear, 1e-5),
        "{actual:?} != {expected:?}"
    );
    assert!(
        actual.angular.abs_diff_eq(expected.angular, 1e-5),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn a_held_door_never_latches() {
    let held = HingeTick {
        held: true,
        angle_rad: 0.0,
        rate_rad_s: Some(-10.0),
        rested_s: 10.0,
        ..tick()
    };
    assert_eq!(swing(&held, &CONFIG), Swing::Held);
}

#[test]
fn a_rebound_does_not_latch_but_a_shove_does() {
    // The controller's own latch would take a door closing at a tenth of this.
    let rebound = HingeTick {
        angle_rad: 0.01,
        rate_rad_s: Some(-(CONFIG.latch_speed_deg_s * 0.5).to_radians()),
        ..tick()
    };
    assert!(matches!(swing(&rebound, &CONFIG), Swing::Damped(_)));
    let shove = HingeTick {
        rate_rad_s: Some(-(CONFIG.latch_speed_deg_s * 2.0).to_radians()),
        ..rebound
    };
    assert_eq!(swing(&shove, &CONFIG), Swing::Latch(LatchCause::Shoved));
    // Fast but well open is just a swing.
    let swinging = HingeTick {
        angle_rad: 0.5,
        ..shove
    };
    assert!(matches!(swing(&swinging, &CONFIG), Swing::Damped(_)));
}

#[test]
fn a_door_resting_shut_latches_after_a_while() {
    let resting = HingeTick {
        angle_rad: 0.01,
        rested_s: CONFIG.settle_s * 0.5,
        ..tick()
    };
    assert!(matches!(swing(&resting, &CONFIG), Swing::Damped(_)));
    let settled = HingeTick {
        rested_s: CONFIG.settle_s,
        ..resting
    };
    assert_eq!(swing(&settled, &CONFIG), Swing::Latch(LatchCause::Settled));
}

#[test]
fn a_side_door_is_damped() {
    let Swing::Damped(factor) = swing(&tick(), &CONFIG) else {
        panic!("a free side door is damped");
    };
    assert!(factor > 0.9 && factor < 1.0, "{factor}");
}

#[test]
fn a_lid_stays_up_above_the_strut_and_falls_below_it() {
    let lid = HingeTick {
        lid: true,
        ..tick()
    };
    assert_eq!(swing(&lid, &CONFIG), Swing::Strut);
    let falling = HingeTick {
        rate_rad_s: Some(-1.0),
        ..lid
    };
    assert_eq!(swing(&falling, &CONFIG), Swing::Strut);
    let thrown = HingeTick {
        rate_rad_s: Some(2.0),
        ..lid
    };
    assert!(matches!(swing(&thrown, &CONFIG), Swing::Damped(_)));
    let low = HingeTick {
        angle_rad: CONFIG.strut_fraction * lid.range_rad * 0.5,
        ..lid
    };
    assert_eq!(swing(&low, &CONFIG), Swing::Free);
}

#[test]
fn a_known_canopy_in_the_front_left_slot_gets_lid_support() {
    let canopy = table::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/door_handles/door_handles.json"
    )))
    .unwrap()
    .into_iter()
    .find(|entry| entry.part == "fighterjet_door_f")
    .unwrap();
    let lid = HingeTick {
        lid: is_lid(DoorSlot::FRONT_LEFT, Some(canopy.part_hash)),
        ..tick()
    };
    assert_eq!(swing(&lid, &CONFIG), Swing::Strut);
    let side = HingeTick {
        lid: is_lid(DoorSlot::FRONT_LEFT, None),
        ..tick()
    };
    assert!(matches!(swing(&side, &CONFIG), Swing::Damped(_)));
}

#[test]
fn canopy_support_does_not_drop_out_at_the_angle_or_rising_threshold() {
    let mut support = CanopySupport::Free;
    let mut canopy = HingeTick {
        lid: true,
        ..tick()
    };
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
    for (angle, rate) in [(0.35, 0.3), (0.37, -0.03), (0.34, 0.0)] {
        canopy.angle_rad = angle;
        canopy.rate_rad_s = Some(rate);
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
    }
}

#[test]
fn a_palm_impulse_yields_support_until_closing_has_stopped() {
    let mut support = CanopySupport::Free;
    let mut canopy = HingeTick {
        lid: true,
        ..tick()
    };
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
    support.yield_for_input();
    for _ in 0..60 {
        canopy.rate_rad_s = Some(-0.4);
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    }
    canopy.rate_rad_s = Some(0.0);
    for _ in 0..3 {
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    }
    canopy.rate_rad_s = Some(-0.1);
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    canopy.rate_rad_s = None;
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    canopy.rate_rad_s = Some(0.0);
    for _ in 0..4 {
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    }
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
}

#[test]
fn renewed_palm_input_extends_the_minimum_yield_time() {
    let mut support = CanopySupport::Free;
    let canopy = HingeTick {
        lid: true,
        ..tick()
    };
    for _ in 0..30 {
        support.yield_for_input();
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    }
    for _ in 0..6 {
        assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    }
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
}

#[test]
fn a_closing_release_or_hold_does_not_lock_the_canopy_open() {
    let mut support = CanopySupport::Free;
    let canopy = HingeTick {
        lid: true,
        rate_rad_s: Some(-0.5),
        ..tick()
    };
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
    let closed = HingeTick {
        angle_rad: 0.01,
        rested_s: CONFIG.settle_s,
        ..canopy
    };
    assert_eq!(
        support.swing(&closed, &CONFIG),
        Swing::Latch(LatchCause::Settled)
    );
    let held = HingeTick {
        held: true,
        ..canopy
    };
    support = CanopySupport::Supported { pose: None };
    assert_eq!(support.swing(&held, &CONFIG), Swing::Held);
    assert_eq!(support, CanopySupport::Free);
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Free);
}

#[test]
fn supported_canopy_yields_to_physical_closing_but_retains_small_drift() {
    let mut support = CanopySupport::Supported { pose: None };
    let canopy = HingeTick {
        lid: true,
        rate_rad_s: Some(-0.02),
        ..tick()
    };
    assert_eq!(support.swing(&canopy, &CONFIG), Swing::Strut);
    let closing = HingeTick {
        rate_rad_s: Some(-0.5),
        ..canopy
    };
    assert_eq!(support.swing(&closing, &CONFIG), Swing::Free);
    assert!(matches!(support, CanopySupport::Yielding { .. }));
    for _ in 0..30 {
        assert_eq!(support.swing(&closing, &CONFIG), Swing::Free);
    }
    let closed = HingeTick {
        angle_rad: 0.01,
        rested_s: CONFIG.settle_s,
        ..closing
    };
    assert_eq!(
        support.swing(&closed, &CONFIG),
        Swing::Latch(LatchCause::Settled)
    );
}

#[test]
fn canopy_pose_correction_is_carrier_relative_bounded_and_cleared_by_input() {
    let chassis = state(Vec3::ZERO, Velocities::default());
    let mut door = state(Vec3::Y, Velocities::default());
    let mut support = CanopySupport::Supported { pose: None };
    assert_velocities(
        support.correction(&door, &chassis, 0.1),
        Velocities::default(),
    );
    let mut moved = chassis;
    moved.pose.centre = Vec3::new(100.0, 20.0, -50.0);
    moved.pose.orientation = Quat::from_rotation_z(1.0);
    door.pose.centre = moved.pose.centre + moved.pose.orientation * Vec3::Y;
    door.pose.orientation = moved.pose.orientation;
    assert_velocities(
        support.correction(&door, &moved, 0.1),
        Velocities::default(),
    );
    door.pose.centre += Vec3::X * 10.0;
    door.pose.orientation *= Quat::from_rotation_x(1.0);
    let correction = support.correction(&door, &moved, 0.1);
    assert!((correction.linear.length() - CANOPY_RESTORE_MPS).abs() < 1e-5);
    assert!((correction.angular.length() - CANOPY_RESTORE_RAD_S).abs() < 1e-5);
    assert!(correction.linear.x < 0.0);
    for dt in [0.0, -0.1] {
        assert_velocities(support.correction(&door, &moved, dt), Velocities::default());
    }
    support.yield_for_input();
    assert_velocities(
        support.correction(&door, &moved, 0.1),
        Velocities::default(),
    );
}

#[test]
fn canopy_target_bounds_accumulated_closing_drift_while_the_carrier_turns() {
    for hz in [30, 60, 120] {
        let dt = 1.0 / hz as f32;
        let mut chassis = state(Vec3::ZERO, Velocities::default());
        let mut door = state(Vec3::Y, Velocities::default());
        let mut support = CanopySupport::Supported { pose: None };
        support.correction(&door, &chassis, dt);
        let mut offset = Vec3::ZERO;
        let mut angle = 0.0;
        for step in 0..hz * 20 {
            chassis.pose.centre = Vec3::new(step as f32 * dt * 20.0, 10.0, 0.0);
            chassis.pose.orientation = Quat::from_rotation_y(step as f32 * dt);
            door.pose.centre = chassis.pose.centre + chassis.pose.orientation * (Vec3::Y + offset);
            door.pose.orientation = chassis.pose.orientation * Quat::from_rotation_x(angle);
            let correction = support.correction(&door, &chassis, dt);
            offset += chassis.pose.orientation.inverse() * correction.linear * dt;
            offset.y -= 0.01 * dt;
            angle += (chassis.pose.orientation.inverse() * correction.angular).x * dt;
            angle -= 0.01 * dt;
        }
        assert!(offset.length() < 0.003, "{hz}: {offset:?}");
        assert!(angle.abs() < 0.003, "{hz}: {angle}");
    }
}

#[test]
fn yielded_support_does_not_reengage_below_its_opening_threshold() {
    let mut support = CanopySupport::Supported { pose: None };
    support.yield_for_input();
    let low = HingeTick {
        lid: true,
        angle_rad: CONFIG.strut_fraction * tick().range_rad * 0.5,
        ..tick()
    };
    for _ in 0..30 {
        assert_eq!(support.swing(&low, &CONFIG), Swing::Free);
    }
    assert_eq!(support, CanopySupport::Free);
    assert_eq!(
        support.swing(
            &HingeTick {
                angle_rad: 1.0,
                ..low
            },
            &CONFIG
        ),
        Swing::Strut
    );
}

#[test]
fn canopy_restore_uses_the_shortest_rotation_for_both_quaternion_signs() {
    let chassis = state(Vec3::ZERO, Velocities::default());
    for angle in [0.0, 0.1, -0.1, 3.5, -3.5] {
        let mut support = CanopySupport::Supported { pose: None };
        let mut door = state(Vec3::Y, Velocities::default());
        support.correction(&door, &chassis, 0.1);
        door.pose.orientation = Quat::from_rotation_x(angle);
        let positive = support.correction(&door, &chassis, 0.1);
        door.pose.orientation = -door.pose.orientation;
        assert_velocities(support.correction(&door, &chassis, 0.1), positive);
        let shortest = if angle > std::f32::consts::PI {
            angle - std::f32::consts::TAU
        } else if angle < -std::f32::consts::PI {
            angle + std::f32::consts::TAU
        } else {
            angle
        };
        assert!(
            (positive.angular.x
                + (shortest * CANOPY_RESTORE_PER_S)
                    .clamp(-CANOPY_RESTORE_RAD_S, CANOPY_RESTORE_RAD_S))
            .abs()
                < 1e-5
        );
    }
}

#[test]
fn invalid_or_replaced_bodies_discard_the_cached_support_target() {
    let body = state(Vec3::Y, Velocities::default());
    for (door, chassis) in [(None, Some(&body)), (Some(&body), None), (None, None)] {
        let mut support = CanopySupport::Supported {
            pose: Some(body.pose),
        };
        assert_eq!(support.velocities(door, chassis, 0.1, 9.81), None);
        assert_eq!(support, CanopySupport::Free);
    }
    let original = (NonZeroUsize::new(1).unwrap(), 10);
    for replacement in [
        None,
        Some((original.0, 11)),
        Some((NonZeroUsize::new(2).unwrap(), 10)),
    ] {
        let mut support = CanopySupport::Supported {
            pose: Some(body.pose),
        };
        let mut previous = Some(original);
        support.observe_body(&mut previous, Some(original));
        assert!(matches!(support, CanopySupport::Supported { .. }));
        support.observe_body(&mut previous, replacement);
        assert_eq!(support, CanopySupport::Free);
        assert_eq!(previous, replacement);
    }
}

#[test]
fn the_lids_are_the_slots_hinged_across_the_vehicle() {
    assert!(is_lid(DoorSlot::HOOD, None));
    assert!(is_lid(DoorSlot::TRUNK, None));
    assert!(!is_lid(DoorSlot::FRONT_LEFT, None));
    assert!(!is_lid(DoorSlot::REAR_RIGHT, None));
}
