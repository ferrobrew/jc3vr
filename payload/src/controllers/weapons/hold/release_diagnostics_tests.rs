use std::{mem, time::Instant};

use super::*;
use crate::controllers::frame::{ActionSetKind, HandFrame, TrackedPose};

fn record(hand: Hand, weapon: usize) -> ReleaseRecord {
    ReleaseRecord {
        sequence: 0,
        hand,
        weapon: Some(weapon),
        route: ReleaseRoute::Wielded,
        max_speed_mps: 15.0,
        controller: None,
        stages: ReleaseStages::default(),
        drop: None,
        refusal: None,
        before_disarm_sample: Unavailable::NoExistingObservation,
        native_readback: NativeReadback::UNAVAILABLE,
    }
}

fn observation(stage: SampleStage, motion: Option<(Vec3, Vec3)>) -> DropObservation {
    DropObservation::sampled(
        42,
        Some(WeaponSlot::E_SLOT_TWO_HANDED),
        stage,
        motion,
        Some(7),
    )
}

#[test]
fn cached_motion_retains_each_vectors_validity_without_substituting_zero() {
    let cases = [
        (Vec3::ZERO, Vec3::ZERO, true, true),
        (Vec3::X, Vec3::Y, true, true),
        (Vec3::splat(f32::NAN), Vec3::Y, false, true),
        (Vec3::X, Vec3::splat(f32::INFINITY), true, false),
    ];
    for (linear, angular, linear_valid, angular_valid) in cases {
        let sample = observation(SampleStage::Wielded, Some((linear, angular)))
            .cached_world_motion
            .unwrap();
        assert_eq!(sample.linear_is_finite(), linear_valid);
        assert_eq!(sample.angular_is_finite(), angular_valid);
        if linear_valid {
            assert_eq!(sample.linear, linear);
        }
        if angular_valid {
            assert_eq!(sample.angular, angular);
        }
    }
    assert!(
        observation(SampleStage::Wielded, None)
            .cached_world_motion
            .is_none()
    );
}

#[test]
fn a_recent_controller_publication_does_not_date_the_cached_gun_sample() {
    let published_at = Instant::now() - Duration::from_millis(20);
    let mut hands: [HandFrame; 2] = Default::default();
    hands[Hand::Left.index()].active = true;
    hands[Hand::Left.index()].grip = TrackedPose {
        position_valid: true,
        orientation_valid: true,
        position_tracked: false,
        orientation_tracked: true,
        linear_velocity: Some(Vec3::Y),
        angular_velocity: None,
        ..TrackedPose::default()
    };
    let frame = ControllerFrame {
        sequence: 13,
        display_time_ns: 100,
        published_at,
        focused: true,
        action_set: ActionSetKind::OnFoot,
        hands,
    };
    let context = ControllerContext::from_frame(&frame, Hand::Left);
    assert_eq!(context.frame_sequence, 13);
    assert_eq!(context.display_time_ns, 100);
    assert!(context.publication_age >= Duration::from_millis(20));
    assert!(context.active && context.position_valid && context.orientation_valid);
    assert!(!context.position_tracked);
    assert!(context.orientation_tracked);
    assert_eq!(context.linear_cockpit_mps, Some(Vec3::Y));
    assert_eq!(context.angular_cockpit_rad_s, None);
    assert!(!ControllerContext::from_frame(&frame, Hand::Right).active);

    let sample = observation(SampleStage::AfterDisarm, Some((Vec3::X, Vec3::ZERO)));
    assert_eq!(sample.sample_age, Unavailable::ProducerTimeUnknown);
    assert_eq!(
        sample.sample_source,
        SampleSource::CachedWeaponDataWorldMatrices
    );
    assert_eq!(sample.cached_world_motion.unwrap().linear, Vec3::X);
    assert_eq!(sample.requested_world_motion, None);
}

#[test]
fn initial_stages_do_not_claim_disarm_drop_or_readback() {
    let attempt = ReleaseRecord::new(Hand::Right, 0.0);
    assert_eq!(attempt.hand, Hand::Right);
    assert_eq!(attempt.weapon, None);
    assert_eq!(attempt.route, ReleaseRoute::Unresolved);
    assert_eq!(attempt.stages, ReleaseStages::default());
    assert_eq!(attempt.drop, None);
    assert_eq!(
        attempt.before_disarm_sample,
        Unavailable::NoExistingObservation
    );
    assert_eq!(attempt.native_readback, NativeReadback::UNAVAILABLE);

    for stage in [SampleStage::Wielded, SampleStage::AfterDisarm] {
        let sample = observation(stage, None);
        assert_eq!(sample.sample_stage, stage);
        assert!(!sample.on_drop_returned);
        assert!(!sample.slot_clear_returned);
        assert_eq!(sample.write, NativeWrite::NotAttempted);
    }
}

#[test]
fn setter_completion_preserves_requested_values_without_claiming_readback() {
    let mut attempt = record(Hand::Right, 42);
    let mut sample = observation(
        SampleStage::Wielded,
        Some((Vec3::splat(f32::INFINITY), Vec3::Y)),
    );
    sample.requested_world_motion = Some(WorldMotion {
        linear: Vec3::ZERO,
        angular: Vec3::Y,
    });
    sample.write = NativeWrite::SetterReturned;
    sample.on_drop_returned = true;
    sample.slot_clear_returned = true;
    attempt.drop = Some(sample);
    attempt.stages.slot_clear_returned = true;
    attempt.stages.reconcile_returned = true;
    let mut status = ReleaseStatus::EMPTY;
    status.record(attempt);
    let saved = status.latest.unwrap();
    let drop = saved.drop.unwrap();
    assert!(!drop.cached_world_motion.unwrap().linear_is_finite());
    assert_eq!(drop.requested_world_motion.unwrap().linear, Vec3::ZERO);
    assert_eq!(drop.write, NativeWrite::SetterReturned);
    assert!(saved.stages.slot_clear_returned && saved.stages.reconcile_returned);
    assert_eq!(
        saved.native_readback.after_setter,
        Unavailable::ReadbackContractUnproven
    );
    assert_eq!(
        saved.native_readback.after_slot_clear,
        Unavailable::LifetimeAndPhaseUnproven
    );
    assert_eq!(
        saved.native_readback.next_step,
        Unavailable::LifetimeAndPhaseUnproven
    );
}

#[test]
fn a_refused_other_hand_attempt_replaces_the_previous_complete_release() {
    let mut status = ReleaseStatus::EMPTY;
    let mut first = record(Hand::Left, 42);
    first.drop = Some(observation(SampleStage::Wielded, None));
    first.stages.slot_clear_returned = true;
    first.stages.reconcile_returned = true;
    status.record(first);
    let previous_copy = status.latest.unwrap();

    let mut second = record(Hand::Right, 99);
    second.route = ReleaseRoute::ArmedInHand;
    second.stages.before_disarm = true;
    second.stages.disarm_returned = true;
    second.refusal = Some(DockRefusal::NotHolstered);
    status.record(second);
    let latest = status.latest.unwrap();
    assert_eq!(latest.sequence, 2);
    assert_eq!(latest.hand, Hand::Right);
    assert_eq!(latest.weapon, Some(99));
    assert_eq!(latest.route, ReleaseRoute::ArmedInHand);
    assert_eq!(latest.refusal, Some(DockRefusal::NotHolstered));
    assert!(latest.stages.before_disarm && latest.stages.disarm_returned);
    assert!(!latest.stages.slot_clear_returned && !latest.stages.reconcile_returned);
    assert_eq!(latest.drop, None);
    assert_eq!(previous_copy.weapon, Some(42));
    assert_eq!(previous_copy.hand, Hand::Left);
    assert_eq!(previous_copy.sequence, 1);
}

#[test]
fn copied_native_and_loadout_identities_remain_distinct() {
    let mut attempt = record(Hand::Left, 99);
    attempt.drop = Some(observation(SampleStage::Wielded, None));
    let mut status = ReleaseStatus::EMPTY;
    status.record(attempt);
    let latest = status.latest.unwrap();
    assert_eq!(latest.weapon, Some(99));
    assert_eq!(latest.drop.unwrap().weapon, 42);
    assert_eq!(latest.drop.unwrap().native_body_id, Some(7));
}

#[test]
fn latest_record_storage_is_fixed_size_across_many_attempts() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<ReleaseStatus>();
    assert!(mem::size_of::<ReleaseStatus>() <= 512);
    assert!(!mem::needs_drop::<ReleaseStatus>());
    let mut status = ReleaseStatus::EMPTY;
    for weapon in 1..=10_000 {
        status.record(record(Hand::Left, weapon));
    }
    assert_eq!(status.attempts, 10_000);
    assert_eq!(status.latest.unwrap().weapon, Some(10_000));
    assert_eq!(status.latest.unwrap().sequence, 10_000);
}

#[test]
fn delegation_and_write_rejection_remain_distinct_from_a_zero_request() {
    let mut status = ReleaseStatus::EMPTY;
    let mut attempt = record(Hand::Left, 42);
    let mut sample = observation(SampleStage::Wielded, Some((Vec3::ZERO, Vec3::ZERO)));
    sample.branch = DropBranch::HeavySlotNativeDrop;
    sample.slot = Some(WeaponSlot::E_SLOT_HEAVY);
    sample.write = NativeWrite::HeavySlotDelegated;
    sample.slot_clear_returned = true;
    attempt.drop = Some(sample);
    status.record(attempt);
    let delegated = status.latest.unwrap().drop.unwrap();
    assert_eq!(delegated.write, NativeWrite::HeavySlotDelegated);
    assert_eq!(delegated.requested_world_motion, None);
    assert!(!delegated.on_drop_returned);

    sample.branch = DropBranch::OnDropThenVelocityWrite;
    sample.requested_world_motion = Some(WorldMotion {
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
    });
    sample.write = NativeWrite::Rejected(BodyError::Reowned);
    sample.on_drop_returned = true;
    attempt.drop = Some(sample);
    status.record(attempt);
    let rejected = status.latest.unwrap().drop.unwrap();
    assert_eq!(rejected.write, NativeWrite::Rejected(BodyError::Reowned));
    assert_eq!(rejected.requested_world_motion.unwrap().linear, Vec3::ZERO);
    assert_eq!(
        status.latest.unwrap().native_readback,
        NativeReadback::UNAVAILABLE
    );
}

#[test]
fn the_attempt_counter_saturates_without_preventing_replacement() {
    let mut status = ReleaseStatus {
        attempts: u64::MAX,
        latest: None,
    };
    status.record(record(Hand::Right, 42));
    status.record(record(Hand::Left, 99));
    assert_eq!(status.attempts, u64::MAX);
    assert_eq!(status.latest.unwrap().sequence, u64::MAX);
    assert_eq!(status.latest.unwrap().weapon, Some(99));
}
