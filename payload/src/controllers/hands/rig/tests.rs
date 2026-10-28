use jc3gi::animation::skeleton::{HkaSkeleton, Skeleton};

use super::*;

fn key(
    controller: &mut AnimationController,
    skeleton: &mut Skeleton,
    havok: &mut HkaSkeleton,
) -> SkeletonKey {
    havok.m_Bones.size = 2;
    skeleton.m_HavokSkeleton = havok;
    controller.m_Skeleton = skeleton;
    unsafe { SkeletonKey::of(controller) }.unwrap()
}

#[test]
fn replacement_failure_expiry_and_view_changes_do_not_reuse_authoritative_old_arms() {
    let mut controller: AnimationController = unsafe { std::mem::zeroed() };
    let mut skeleton: Skeleton = unsafe { std::mem::zeroed() };
    let mut havok: HkaSkeleton = unsafe { std::mem::zeroed() };
    let initial_key = key(&mut controller, &mut skeleton, &mut havok);
    let now = Instant::now();
    let sample = ArmSample {
        shoulder: Vec3::new(0.2, 1.4, 0.0),
        elbow: Vec3::new(0.5, 1.4, 0.0),
        wrist: Vec3::new(0.8, 1.4, 0.0),
        eyes: Vec3::new(0.0, 1.7, 0.0),
        to_body: Quat::IDENTITY,
    };
    let mut state = State::EMPTY;
    state.prepare((initial_key, 1), Quat::IDENTITY);
    state.last = Some(now);
    state.resolved = [true; 2];
    for tracker in &mut state.trackers {
        tracker.update(sample, 0.0, 0.1);
    }
    let original = state.arms(now, Quat::IDENTITY)[0].unwrap();
    let turn = Quat::from_rotation_y(0.7);
    let transported = state.arms(now, turn)[0].unwrap();
    assert!(
        transported
            .eye_to_shoulder
            .abs_diff_eq(turn.inverse() * original.eye_to_shoulder, 1.0e-6)
    );
    state.resolved[0] = false;
    assert!(state.arms(now, turn)[0].is_none());
    assert!(
        state
            .arms(now + Duration::from_secs(1), turn)
            .iter()
            .all(Option::is_none)
    );
    state.prepare((initial_key, 2), Quat::IDENTITY);
    assert!(state.trackers.iter().all(|tracker| tracker.arm().is_none()));
    assert!(state.arms(now, turn).iter().all(Option::is_none));
    for tracker in &mut state.trackers {
        tracker.update(sample, 0.0, 0.1);
    }
    let mut replacement: Skeleton = unsafe { std::mem::zeroed() };
    let replacement_key = key(&mut controller, &mut replacement, &mut havok);
    state.prepare((replacement_key, 2), Quat::IDENTITY);
    assert!(state.trackers.iter().all(|tracker| tracker.arm().is_none()));
}
