use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec2, Vec3};
use proptest::prelude::*;

use super::*;
use crate::controllers::{
    body::{
        BodyDimensions,
        procedural::{
            rig::{ArmRig, Rig, RigBone},
            skeleton::Transform,
        },
        rig::{
            estimate::{
                contacts::Contact,
                fit::{self, Context, Parameters},
            },
            observation::{
                CompositionPair, CompositionSide, Epochs, RawPose, RawTracking, SampleStamp,
                StepStamp,
            },
            root::{owner::RootOwner, shift::CockpitBasis},
            torso::EYES_TO_NECK_PIVOT,
        },
    },
    frame::{Pose, TrackedPose},
};

pub(crate) mod cost;
mod final_invariants;
pub(crate) mod profile;
mod recovery;
mod regressions;
pub(crate) mod retained;
mod trajectories;
mod transitions;

#[derive(Clone)]
struct Fixture {
    now: Instant,
    anatomy: Anatomy,
    epochs: Epochs,
}

impl Fixture {
    fn new() -> Self {
        Self {
            now: Instant::now(),
            anatomy: Anatomy::from_rig(BodyDimensions::GENERIC_ADULT, &reference_rig(), 1).unwrap(),
            epochs: Epochs {
                origin: 1,
                calibration: 1,
                world_scale: 1,
                authority: 1,
            },
        }
    }

    fn observation(&self, sample: Sample) -> Observation {
        let stamp = SampleStamp {
            sequence: sample.sequence,
            display_time_ns: (sample.time * 1.0e9) as i64,
            published_at: self.now,
            epochs: self.epochs,
        };
        let tracked = |pose: Pose| TrackedPose {
            pose,
            position_valid: true,
            orientation_valid: true,
            position_tracked: true,
            orientation_tracked: true,
            linear_velocity: None,
            angular_velocity: None,
        };
        let to_cockpit = |pose: Pose| Pose {
            position: sample.basis.inverse() * pose.position,
            orientation: sample.basis.inverse() * pose.orientation,
        };
        let raw = RawTracking {
            head: RawPose {
                stamp,
                tracked: tracked(to_cockpit(sample.head)),
            },
            grips: sample.hands.map(|hand| {
                hand.map(|hand| RawPose {
                    stamp,
                    tracked: tracked(to_cockpit(hand)),
                })
            }),
            focused: true,
        };
        let side = |step, root| CompositionSide {
            stamp: StepStamp {
                step,
                epochs: self.epochs,
            },
            root,
            anchor_world: Vec3::ZERO,
            basis: CockpitBasis {
                body: sample.basis,
                world_scale: 1.0,
            },
            shift_cockpit: Vec3::ZERO,
            owner: RootOwner::Player,
            native_rig: None,
        };
        Observation::new(
            raw,
            CompositionPair::new(
                side(sample.previous_step, sample.previous_root),
                side(sample.step, sample.root),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn support(&self, previous_step: u64, step: u64) -> Support {
        Support {
            identity: 3,
            generation: 1,
            previous_step: StepStamp {
                step: previous_step,
                epochs: self.epochs,
            },
            current_step: StepStamp {
                step,
                epochs: self.epochs,
            },
            previous: Mat4::IDENTITY,
            current: Mat4::IDENTITY,
            half_extent: Vec2::splat(100.0),
        }
    }

    fn tick(
        &self,
        estimator: &mut Estimator,
        sample: Sample,
        transition: Transition,
    ) -> Result<Snapshot, EstimateError> {
        let observation = self.observation(sample);
        estimator.update(Update {
            observation: &observation,
            now: self.now,
            max_age: Duration::from_millis(100),
            dt: sample.dt,
            anatomy: &self.anatomy,
            support: Some(self.support(sample.previous_step, sample.step)),
            transition,
        })
    }
}

#[derive(Copy, Clone)]
struct Sample {
    sequence: u64,
    previous_step: u64,
    step: u64,
    time: f32,
    dt: f32,
    head: Pose,
    hands: [Option<Pose>; 2],
    root: Mat4,
    previous_root: Mat4,
    basis: Quat,
}

impl Sample {
    fn standing(step: u64, dt: f32) -> Self {
        Self {
            sequence: step,
            previous_step: step.saturating_sub(1),
            step,
            time: step as f32 * dt,
            dt,
            head: Pose {
                position: Vec3::new(0.0, 1.63, -0.085),
                orientation: Quat::IDENTITY,
            },
            hands: [None; 2],
            root: Mat4::IDENTITY,
            previous_root: Mat4::IDENTITY,
            basis: Quat::IDENTITY,
        }
    }
}

fn reference_rig() -> Rig {
    let mut reference = [Transform {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    }; RigBone::COUNT];
    let mut set = |bone: RigBone, point| reference[bone.index()].translation = point;
    set(RigBone::Hips, Vec3::new(0.0, 0.93, 0.0));
    set(RigBone::Pelvis, Vec3::new(0.0, 0.93, 0.0));
    set(RigBone::LeftUpLeg, Vec3::new(-0.09, 0.93, 0.0));
    set(RigBone::RightUpLeg, Vec3::new(0.09, 0.93, 0.0));
    set(RigBone::LeftLeg, Vec3::new(-0.09, 0.48, -0.02));
    set(RigBone::RightLeg, Vec3::new(0.09, 0.48, -0.02));
    set(RigBone::LeftFoot, Vec3::new(-0.09, 0.03, 0.0));
    set(RigBone::RightFoot, Vec3::new(0.09, 0.03, 0.0));
    set(RigBone::LeftShoulder, Vec3::new(-0.20, 1.44, 0.0));
    set(RigBone::RightShoulder, Vec3::new(0.20, 1.44, 0.0));
    set(RigBone::Neck, Vec3::new(0.0, 1.555, 0.0));
    set(RigBone::Head, Vec3::new(0.0, 1.63, -0.085));
    Rig {
        bones: [0; RigBone::COUNT],
        reference,
        arms: [ArmRig {
            shoulder_local: Vec3::ZERO,
            length: 0.65,
        }; 2],
    }
}

fn assert_anatomy(snapshot: Snapshot, anatomy: Anatomy) {
    let body = snapshot.body;
    assert!(body.pelvis.is_finite() && body.neck.is_finite());
    assert!((body.chest.distance(body.pelvis) - anatomy.character.lower_torso).abs() < 1.0e-4);
    assert!((body.neck.distance(body.chest) - anatomy.character.upper_torso).abs() < 1.0e-4);
    for side in 0..2 {
        assert!(body.ankles[side].abs_diff_eq(snapshot.feet[side].position, 1.0e-6));
        assert!(
            (body.knees[side].distance(body.hips[side]) - anatomy.character.thighs[side]).abs()
                < 1.0e-4
        );
        assert!(
            (body.ankles[side].distance(body.knees[side]) - anatomy.character.shins[side]).abs()
                < 1.0e-4
        );
        assert!(body.knee_flexion[side] <= 2.65 + 1.0e-4);
        assert!(body.ankle_flexion[side].abs() <= 0.95 + 1.0e-4);
    }
    assert!(snapshot.quality.evaluations <= MAX_EVALUATIONS);
    assert_eq!(snapshot.quality.lower_body_confidence, Confidence::Low);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn finite_bounded_length_preserving_fit(
        x in -0.12_f32..0.12, z in -0.25_f32..0.10, drop in 0.0_f32..0.25,
        pitch in -0.6_f32..0.4,
    ) {
        let fixture = Fixture::new();
        let mut sample = Sample::standing(1, 1.0 / 90.0);
        sample.head.position += Vec3::new(x, -drop, z);
        sample.head.orientation = Quat::from_rotation_x(pitch);
        let mut estimator = Estimator::new();
        let snapshot = fixture.tick(&mut estimator, sample, Transition::Continue).unwrap();
        assert_anatomy(snapshot, fixture.anatomy);
        prop_assert_eq!(snapshot.gait.distance, 0.0);
        prop_assert!(snapshot.quality.yaw_ambiguity >= 0.0 && snapshot.quality.yaw_ambiguity <= 1.0);
    }
}

#[test]
fn invalid_reference_and_player_proportions_are_rejected() {
    let mut rig = reference_rig();
    rig.reference[RigBone::LeftLeg.index()].translation = Vec3::NAN;
    assert_eq!(
        Anatomy::from_rig(BodyDimensions::GENERIC_ADULT, &rig, 1),
        Err(EstimateError::InvalidAnatomy)
    );
    let dimensions = BodyDimensions {
        eye_height: f32::NAN,
        ..BodyDimensions::GENERIC_ADULT
    };
    assert_eq!(
        Anatomy::from_rig(dimensions, &reference_rig(), 1),
        Err(EstimateError::InvalidAnatomy)
    );
}

#[test]
fn candidate_reach_is_evaluated_from_its_shoulders_once() {
    let fixture = Fixture::new();
    let sample = Sample::standing(1, 1.0 / 90.0);
    let observation = fixture.observation(sample);
    let stance = Stance {
        origin: Vec3::ZERO,
        yaw: 0.0,
    };
    let parameters = Parameters::neutral(stance, &fixture.anatomy);
    let feet = crate::controllers::body::rig::estimate::contacts::rest_feet(
        stance,
        fixture.anatomy.character,
        fixture.anatomy.ankle_height,
    );
    let contacts = feet.map(|pose| {
        Contact::seed(
            pose,
            Some(fixture.support(0, 1)),
            fixture.anatomy.ankle_height,
        )
    });
    let mut context = Context {
        anatomy: &fixture.anatomy,
        neck: sample.head.position + EYES_TO_NECK_PIVOT,
        head_yaw: 0.0,
        root_yaw: 0.0,
        hands: [None; 2],
        tracking_floor: Vec3::ZERO,
        world_scale: 1.0,
        predicted: parameters,
        previous: parameters,
        rates: [0.0; fit::PARAMETERS],
        dt: sample.dt,
        continuity: false,
        floor_y: 0.0,
        contacts,
    };
    let neutral = fit::evaluate(parameters, &context).unwrap();
    context.hands[1] = Some(Pose {
        position: neutral.player_shoulders[1] + Vec3::X * 0.9,
        orientation: Quat::IDENTITY,
    });
    let straight = fit::evaluate(parameters, &context).unwrap();
    let mut rotated = parameters;
    rotated.0[8] = 0.6;
    let turned = fit::evaluate(rotated, &context).unwrap();
    assert!(straight.arm_errors[1].unwrap() > 0.1);
    assert_ne!(straight.arm_errors[1], turned.arm_errors[1]);
    assert_eq!(observation.raw().grips, [None; 2]);
    let delta = context.hands[1].unwrap().position - straight.player_shoulders[1];
    let expected = straight.body.shoulders[1]
        + delta * (fixture.anatomy.character.arms[1] / fixture.anatomy.player_segments.arms[1]);
    assert!(
        straight.remapped_hands[1]
            .unwrap()
            .abs_diff_eq(expected, 1.0e-5)
    );
}
