use std::num::NonZeroUsize;

use super::*;
use crate::controllers::{
    aim::raycast::RayHit,
    body::{procedural::ground::terrain_foothold, rig::legs::swing::FootPose},
};

struct Hits {
    height: f32,
    slope: f32,
    body: Option<u32>,
    missing: bool,
    queries: usize,
}

impl Default for Hits {
    fn default() -> Self {
        Self {
            height: 0.0,
            slope: 0.0,
            body: None,
            missing: false,
            queries: 0,
        }
    }
}

impl Terrain for Hits {
    fn floor_height(&self, _support: Support) -> f32 {
        self.height
    }

    fn foothold(
        &mut self,
        support: Support,
        point: Vec3,
        heading: f32,
        ankle_height: f32,
    ) -> Option<FootPose> {
        self.queries += 1;
        if self.missing {
            return None;
        }
        let hit = RayHit {
            position: Vec3::new(point.x, self.height + point.x * self.slope, point.z),
            normal: Vec3::new(-self.slope, 1.0, 0.0).normalize(),
            distance: 0.3,
            object: self.body.and_then(|_| NonZeroUsize::new(1)),
            body: self.body.unwrap_or(1),
            material: None,
        };
        terrain_foothold(support, point, heading, ankle_height, &hit, self.body)
    }
}

fn sample(now: Instant, identity: u64, transform: Mat4) -> SupportSample {
    SupportSample {
        identity,
        generation: 1,
        transform,
        at: now,
    }
}

fn commit(
    runtime: &mut Runtime,
    rig: &Rig,
    now: Instant,
    sequence: u64,
    root: Mat4,
    support: Option<SupportSample>,
    terrain: &mut Hits,
) -> Snapshot {
    let mut gate = input(rig, now);
    gate.root = root;
    gate.previous_root = Mat4::from_translation(Vec3::splat(20.0));
    gate.head_world = root.transform_point3(Vec3::new(0.0, 1.63, -0.085));
    gate.view = root.to_scale_rotation_translation().1;
    gate.support = support;
    runtime
        .commit_terrain(Some(tracking(now, sequence)), gate, terrain)
        .unwrap()
}

#[test]
fn native_hit_adapter_plants_on_sloped_terrain_and_reaches_the_pose_writer() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let mut terrain = Hits {
        slope: 0.08,
        ..Hits::default()
    };
    let fit = commit(
        &mut runtime,
        &rig,
        now,
        1,
        Mat4::IDENTITY,
        Some(sample(now, 0, Mat4::IDENTITY)),
        &mut terrain,
    );
    assert!(terrain.queries >= 2);
    assert!(
        fit.contacts
            .iter()
            .any(|contact| matches!(contact, ContactState::Planted { .. }))
    );
    for foot in fit.feet {
        assert!((foot.position.y - (0.03 + foot.position.x * terrain.slope)).abs() < 0.04);
        assert!(
            (foot.rotation * Vec3::Y)
                .abs_diff_eq(Vec3::new(-terrain.slope, 1.0, 0.0).normalize(), 0.01)
        );
    }
    let pose = realize(
        &rig,
        &RigPose {
            bones: rig.reference,
        },
        &fit,
        Mat4::IDENTITY,
        Quat::IDENTITY,
    )
    .unwrap();
    for (side, foot) in [RigBone::LeftFoot, RigBone::RightFoot]
        .into_iter()
        .enumerate()
    {
        assert!(pose.position(foot).distance(fit.feet[side].position) < 0.01);
    }
}

#[test]
fn moving_support_uses_the_current_frame_once_without_creating_gait_travel() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let mut terrain = Hits {
        body: Some(7),
        ..Hits::default()
    };
    let first = commit(
        &mut runtime,
        &rig,
        now,
        1,
        Mat4::IDENTITY,
        Some(sample(now, 8, Mat4::IDENTITY)),
        &mut terrain,
    );
    let next = now + Duration::from_millis(33);
    let root =
        Mat4::from_rotation_translation(Quat::from_rotation_y(0.08), Vec3::new(0.04, 0.0, 0.02));
    let second = commit(
        &mut runtime,
        &rig,
        next,
        2,
        root,
        Some(sample(next, 8, root)),
        &mut terrain,
    );
    assert!(second.gait.completed_across_support.length() < 1.0e-5);
    assert_eq!(second.gait.distance, first.gait.distance);
    for (before, after) in first.feet.into_iter().zip(second.feet) {
        assert!(
            root.transform_point3(before.position)
                .distance(after.position)
                < 0.04
        );
    }
}

#[test]
fn missing_terrain_and_future_dated_samples_release_instead_of_inventing_a_floor() {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let mut terrain = Hits::default();
    commit(
        &mut runtime,
        &rig,
        now,
        1,
        Mat4::IDENTITY,
        Some(sample(now, 0, Mat4::IDENTITY)),
        &mut terrain,
    );
    let next = now + Duration::from_millis(33);
    terrain.missing = true;
    let missing = commit(
        &mut runtime,
        &rig,
        next,
        2,
        Mat4::IDENTITY,
        Some(sample(next, 0, Mat4::IDENTITY)),
        &mut terrain,
    );
    assert!(
        missing
            .contacts
            .iter()
            .all(|contact| matches!(contact, ContactState::Released(_)))
    );
    terrain.missing = false;
    let future = next + Duration::from_millis(33);
    let released = commit(
        &mut runtime,
        &rig,
        future,
        3,
        Mat4::IDENTITY,
        Some(sample(future + Duration::from_millis(1), 0, Mat4::IDENTITY)),
        &mut terrain,
    );
    assert_eq!(released.provenance.support, None);
    assert!(
        released
            .contacts
            .iter()
            .all(|contact| matches!(contact, ContactState::Released(_)))
    );
}

#[test]
fn observed_terrain_stable_turning_completes_a_checked_swing_and_plant() {
    turning_contacts(1);
}

#[test]
fn observed_terrain_identity_recovery_completes_a_checked_swing_and_plant() {
    turning_contacts(2);
}

fn turning_contacts(identity: u64) {
    let rig = rig();
    let now = Instant::now();
    let mut runtime = Runtime::default();
    let mut terrain = Hits::default();
    commit(
        &mut runtime,
        &rig,
        now,
        1,
        Mat4::IDENTITY,
        Some(sample(now, 1, Mat4::IDENTITY)),
        &mut terrain,
    );
    let mut swung = [false; 2];
    let mut planted_after_swing = false;
    let mut states = Vec::new();
    for sequence in 2..28 {
        let next = now + Duration::from_millis(sequence * 33);
        let turn = (sequence as f32 * 0.025).min(0.45);
        let root = Mat4::from_rotation_translation(
            Quat::from_rotation_y(turn),
            Vec3::new(0.0, 0.0, -(sequence as f32) * 0.004),
        );
        let fit = commit(
            &mut runtime,
            &rig,
            next,
            sequence,
            root,
            Some(sample(next, identity, Mat4::IDENTITY)),
            &mut terrain,
        );
        states.push((
            sequence,
            fit.contacts,
            fit.stance,
            fit.body.hips,
            fit.feet,
            fit.quality.temporal,
        ));
        for (side, contact) in fit.contacts.into_iter().enumerate() {
            match contact {
                ContactState::Swinging { support, .. } => {
                    assert_eq!(support, (identity, 1));
                    swung[side] = true;
                }
                ContactState::Planted { support, .. } => {
                    assert_eq!(support, (identity, 1));
                    planted_after_swing |= swung[side];
                }
                ContactState::Released(_) => {}
            }
        }
    }
    assert!(
        planted_after_swing,
        "swung={swung:?}, planted_after_swing={planted_after_swing}; states={states:#?}"
    );
    assert!(terrain.queries > 28);
}
