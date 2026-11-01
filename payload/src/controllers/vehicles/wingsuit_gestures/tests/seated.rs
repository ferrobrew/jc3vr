//! Poses sampled from a seated player flying the suit on Index controllers, measured through the
//! live adapter, so the thresholds are checked against arms that flew rather than ideal ones.

use super::*;

/// A tracked pose from `position` and an `[x, y, z, w]` orientation.
fn tracked(position: [f32; 3], orientation: [f32; 4]) -> TrackedPose {
    TrackedPose {
        pose: Pose {
            position: Vec3::from_array(position),
            orientation: Quat::from_array(orientation).normalize(),
        },
        position_valid: true,
        orientation_valid: true,
        position_tracked: true,
        orientation_tracked: true,
        ..Default::default()
    }
}

/// The head and the `[left, right]` grips of one sample, each a position and an orientation.
type Sample = ([[f32; 3]; 3], [[f32; 4]; 3]);

/// Measure `sample` as the live adapter does, for a player of standard proportions with the
/// Index controller's grip offset.
fn measure((positions, orientations): Sample) -> FlightPose {
    let published_at = Instant::now();
    let sample = GestureSample {
        epoch: 1,
        sequence: 1,
        time_ns: 1,
    };
    let frame = ControllerFrame {
        sequence: sample.sequence,
        display_time_ns: sample.time_ns,
        published_at,
        focused: true,
        action_set: ActionSetKind::Wingsuit,
        hands: [1, 2].map(|i| HandFrame {
            active: true,
            grip: tracked(positions[i], orientations[i]),
            ..Default::default()
        }),
    };
    FlightPose::from_tracking(&GestureObservation {
        sample,
        head: GestureHead {
            sample,
            pose: tracked(positions[0], orientations[0]),
            published_at,
        },
        controllers: &frame,
        grip_offset: &GripOffset::INDEX_BEST_GUESS,
        dimensions: &PLAYER,
        torso: None,
        now: published_at,
    })
    .unwrap()
}

const PLAYER: BodyDimensions = BodyDimensions {
    height: 1.80,
    eye_height: 1.68,
    shoulder_height: 1.47,
    shoulder_width: 0.40,
    arm_span: 1.80,
    upper_arm: 0.34,
    forearm: 0.26,
    hand: 0.19,
    seated_eye_height: None,
};

/// Open hands raised in front of the chest at the suit's entry.
const ENTRY: Sample = (
    [
        [-0.0211, -0.0577, -0.0068],
        [-0.2751, -0.1956, -0.2541],
        [0.2606, -0.2005, -0.3662],
    ],
    [
        [-0.2554, 0.0805, 0.0146, 0.9634],
        [-0.7077, 0.0420, 0.6446, -0.2861],
        [-0.6299, 0.0361, -0.7554, -0.1768],
    ],
);

/// The arms spread level, the palms near flat.
const GLIDE: Sample = (
    [
        [-0.0184, -0.0155, 0.0587],
        [-0.8008, -0.0809, 0.1176],
        [0.7492, -0.1062, 0.3818],
    ],
    [
        [-0.0899, 0.0249, 0.0025, 0.9956],
        [0.1336, -0.0888, 0.6585, -0.7353],
        [0.2311, 0.3051, -0.7890, -0.4806],
    ],
);

/// The same glide with the head turned some 60 degrees from the arms.
const GLIDE_LOOKING_ASIDE: Sample = (
    [
        [-0.1077, -0.0153, 0.0760],
        [-0.8041, -0.1742, 0.0607],
        [0.7835, -0.1562, 0.2655],
    ],
    [
        [-0.0643, 0.4841, -0.0060, 0.8726],
        [0.0131, -0.1118, 0.6375, -0.7622],
        [0.0774, 0.3227, -0.7340, -0.5926],
    ],
);

/// Both hands turned to face the wind, the right wrist bent back more than the left.
const BRAKES: [Sample; 2] = [
    (
        [
            [-0.0605, -0.0161, 0.0532],
            [-0.7924, 0.0127, 0.1484],
            [0.7295, 0.0583, 0.2600],
        ],
        [
            [-0.0823, 0.1779, -0.0102, 0.9805],
            [-0.4415, -0.5370, 0.6300, -0.3460],
            [-0.3820, 0.5966, -0.6984, -0.1017],
        ],
    ),
    (
        [
            [-0.0200, -0.0064, 0.0420],
            [-0.8132, -0.1580, 0.0718],
            [0.7819, -0.1376, 0.3108],
        ],
        [
            [-0.0538, 0.0301, -0.0046, 0.9981],
            [-0.4112, -0.5371, 0.4722, -0.5652],
            [-0.3412, 0.7166, -0.5612, -0.2347],
        ],
    ),
];

/// Whether `pose` is a flight pose under `config`, as the recognizer's arming reads it.
fn flight(pose: FlightPose, config: &WingsuitGesturesConfig) -> bool {
    pose.lateral
        .into_iter()
        .all(|v| v >= config.flight_lateral_span)
        && pose
            .angle_deg
            .into_iter()
            .all(|v| v <= config.flight_angle_deg)
        && pose.flat_palms(config.flight_roll_deg)
}

#[test]
fn the_entry_pose_is_withdrawn_and_not_a_flight_pose() {
    let pose = measure(ENTRY);
    assert!(pose.withdrawn(&CONFIG), "{pose:?}");
    assert!(!flight(pose, &CONFIG), "{pose:?}");
}

#[test]
fn a_seated_glide_is_a_flight_pose_wherever_the_head_looks() {
    for sample in [GLIDE, GLIDE_LOOKING_ASIDE] {
        let pose = measure(sample);
        assert!(flight(pose, &CONFIG), "{pose:?}");
        assert!(!pose.braking_palms(&CONFIG, false), "{pose:?}");
        assert!(!pose.withdrawn(&CONFIG), "{pose:?}");
    }
}

#[test]
fn seated_hands_turned_to_the_wind_brake() {
    for sample in BRAKES {
        let pose = measure(sample);
        assert!(pose.braking_palms(&CONFIG, false), "{pose:?}");
        assert!(
            pose.lateral
                .into_iter()
                .all(|v| v >= CONFIG.flight_lateral_span),
            "{pose:?}"
        );
        assert!(!flight(pose, &CONFIG), "{pose:?}");
    }
}

#[test]
fn the_recorded_flight_arms_and_brakes_without_a_neutral() {
    let mut h = History::new(SUIT);
    h.hold(measure(ENTRY), 4);
    assert!(
        h.hold(measure(GLIDE), 6)
            .iter()
            .all(|c| *c == GestureCommands::default())
    );
    assert_eq!(h.state.phase(), GesturePhase::Armed);
    assert!(h.hold(measure(BRAKES[1]), 6).iter().any(|c| c.brake));
    assert_eq!(h.state.phase(), GesturePhase::Braking);
}
