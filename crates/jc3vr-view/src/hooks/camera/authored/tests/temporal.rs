use super::*;
use crate::hooks::camera::authored::{
    composition::InvalidPair,
    policy::{HangingState, LandingState, Owner, Phase, Policy, RecoveryState},
};

#[test]
fn continuous_internal_handoff_and_idle_tail_have_different_lifetimes() {
    let mut policy = Policy::default();
    let entry = policy.observe(frame(1, GraphState::GroundInto, 0.0, 0.2));
    assert_eq!(entry.phase, Phase::Entry);
    assert_eq!(entry.owner(), Owner::NormalTracked);
    assert!(entry.reset);
    let live = policy.observe(frame(2, GraphState::GroundInto, 0.59, 1.0));
    assert_eq!(live.owner(), Owner::Authored);
    let handoff = policy.observe(frame(3, GraphState::GroundFollowOn, 0.61, 1.0));
    assert_eq!(handoff.phase, Phase::Live);
    assert!(!handoff.reset);
    assert_eq!(handoff.owner(), Owner::Authored);
    policy.observe(frame(4, GraphState::GroundFollowOn, 1.466_667, 1.0));
    // The synthetic clock follows the observed idle blend shape, not a runtime timeout.
    for (tick, elapsed, weight) in [(5, 0.25, 0.75), (6, 0.75, 0.25)] {
        let tail = policy.observe(frame(
            tick,
            GraphState::WingsuitIdle,
            1.466_667 + elapsed,
            weight,
        ));
        assert_eq!(tail.phase, Phase::Tail);
        assert_eq!(tail.owner(), Owner::Authored);
    }
    let complete = policy.observe(frame(7, GraphState::WingsuitIdle, 2.466_667, 0.0));
    assert_eq!(complete.owner(), Owner::NormalTracked);
    assert!(complete.reset);
    assert_eq!(complete.phase, Phase::Idle);
    let stale = policy.observe(frame(8, GraphState::WingsuitIdle, 2.5, 0.2));
    assert_eq!(stale.owner(), Owner::NormalTracked);
    assert_eq!(stale.reason, Some(InvalidPair::InvalidTransition));
}

#[test]
fn continuing_absolute_time_does_not_accept_a_restart_at_the_internal_handoff() {
    let mut policy = Policy::default();
    policy.observe(frame(1, GraphState::GroundInto, 0.5, 1.0));
    let restarted = policy.observe(frame(2, GraphState::GroundFollowOn, 0.0, 1.0));
    assert_eq!(restarted.owner(), Owner::NormalTracked);
    assert_eq!(restarted.reason, Some(InvalidPair::ClipTimeReversed));
}

#[test]
fn ordinary_locomotion_and_unknown_states_never_admit_or_extend_a_tail() {
    for state in [
        GraphState::OrdinaryLocomotion,
        GraphState::Other,
        GraphState::WingsuitIdle,
    ] {
        let mut policy = Policy::default();
        assert_eq!(
            policy.observe(frame(1, state, 0.0, 1.0)).owner(),
            Owner::NormalTracked
        );
    }
    let mut policy = Policy::default();
    policy.observe(frame(1, GraphState::GroundInto, 0.0, 1.0));
    policy.observe(frame(2, GraphState::GroundFollowOn, 0.7, 1.0));
    let excluded = policy.observe(frame(3, GraphState::OrdinaryLocomotion, 1.5, 0.5));
    assert_eq!(excluded.owner(), Owner::NormalTracked);
    assert_eq!(excluded.reason, Some(InvalidPair::UnsupportedState));
    assert_eq!(
        policy
            .observe(frame(4, GraphState::WingsuitIdle, 1.6, 0.4))
            .owner(),
        Owner::NormalTracked
    );
}

#[test]
fn tail_requires_current_finalized_weight_for_the_same_accepted_source() {
    for change in 0..5 {
        let mut policy = Policy::default();
        policy.observe(frame(1, GraphState::GroundInto, 0.0, 1.0));
        policy.observe(frame(2, GraphState::GroundFollowOn, 1.4, 1.0));
        let mut tail = frame(3, GraphState::WingsuitIdle, 1.5, 0.5);
        let expected = match change {
            0 => {
                tail.blend.as_mut().unwrap().stamp.finalized -= 1;
                InvalidPair::BlendEpochMismatch
            }
            1 => {
                tail.blend.as_mut().unwrap().source += 1;
                InvalidPair::SourceChanged
            }
            2 => {
                tail.accepted.run += 1;
                InvalidPair::AdmissionMismatch
            }
            3 => {
                tail.blend.as_mut().unwrap().weight = f32::NAN;
                InvalidPair::InvalidBlendWeight
            }
            _ => {
                tail.baseline.sample.local.transform.translation = Vec3::X;
                InvalidPair::BaselineChanged
            }
        };
        let result = policy.observe(tail);
        assert_eq!(result.owner(), Owner::NormalTracked);
        assert_eq!(result.reason, Some(expected));
        assert!(result.reset);
    }
}

#[test]
fn unavailable_interrupt_and_teleport_clear_history_without_freezing_tracking() {
    for invalidation in 0..3 {
        let mut policy = Policy::default();
        policy.observe(frame(1, GraphState::GroundInto, 0.0, 1.0));
        assert_eq!(
            policy
                .observe(frame(2, GraphState::GroundFollowOn, 0.7, 1.0))
                .owner(),
            Owner::Authored
        );
        let result = match invalidation {
            0 => policy.unavailable(),
            1 => policy.interrupt(),
            _ => policy.teleport(),
        };
        assert_eq!(result.owner(), Owner::NormalTracked);
        assert!(result.reset);
        assert_eq!(
            policy
                .observe(frame(3, GraphState::GroundFollowOn, 0.8, 1.0))
                .owner(),
            Owner::NormalTracked
        );
        let reentry = policy.observe(frame(4, GraphState::GroundInto, 0.0, 1.0));
        assert_eq!(reentry.phase, Phase::Entry);
        assert_eq!(reentry.owner(), Owner::NormalTracked);
        assert_eq!(
            policy
                .observe(frame(5, GraphState::GroundInto, 0.1, 1.0))
                .owner(),
            Owner::Authored
        );
    }
}

#[test]
fn character_and_continuity_changes_clear_the_temporal_pair() {
    for character_change in [true, false] {
        let mut policy = Policy::default();
        policy.observe(frame(1, GraphState::GroundInto, 0.0, 1.0));
        let mut next = frame(2, GraphState::GroundInto, 0.1, 1.0);
        if character_change {
            next.accepted.character += 1;
            next.sample.root.stamp.character += 1;
            next.baseline.sample.root.stamp.character += 1;
        } else {
            next.accepted.continuity += 1;
            next.sample.root.stamp.continuity += 1;
            next.baseline.sample.root.stamp.continuity += 1;
        }
        next.sample.local.stamp = next.sample.root.stamp;
        next.blend.as_mut().unwrap().stamp = next.sample.root.stamp;
        next.baseline.sample.local.stamp = next.baseline.sample.root.stamp;
        let result = policy.observe(next);
        assert_eq!(result.owner(), Owner::NormalTracked);
        assert_eq!(
            result.reason,
            Some(if character_change {
                InvalidPair::CharacterChanged
            } else {
                InvalidPair::ContinuityChanged
            })
        );
    }
}

#[test]
fn exact_state_capture_does_not_fabricate_a_blend_tail() {
    let mut policy = Policy::new();
    let exact = |tick, state| {
        let mut value = frame(tick, state, 0.0, 1.0);
        value.blend = None;
        value.clip_time = None;
        value
    };
    assert_eq!(
        policy.observe(exact(1, GraphState::GroundInto)).phase,
        Phase::Entry
    );
    assert_eq!(
        policy.observe(exact(2, GraphState::GroundFollowOn)).owner(),
        Owner::Authored
    );
    let tail = policy.observe(exact(3, GraphState::WingsuitIdle));
    assert_eq!(tail.owner(), Owner::NormalTracked);
    assert_eq!(tail.reason, Some(InvalidPair::MissingSample));
}

#[test]
fn named_cases_admit_only_their_own_state_identity() {
    for (state, case) in [
        (GraphState::Landing(LandingState::Hard), NamedCase::Landing),
        (
            GraphState::Recovery(RecoveryState::GetUp),
            NamedCase::Recovery,
        ),
        (GraphState::FloorPickup, NamedCase::FloorPickup),
        (
            GraphState::Hanging(HangingState::GrappleAttached),
            NamedCase::Hanging,
        ),
    ] {
        let mut policy = Policy::new();
        let exact = |tick| {
            let mut value = frame(tick, state, 0.0, 1.0);
            value.accepted.case = case;
            value.blend = None;
            value.clip_time = None;
            value
        };
        assert_eq!(policy.observe(exact(1)).phase, Phase::Entry);
        assert_eq!(policy.observe(exact(2)).owner(), Owner::Authored);
        let mut mismatched = exact(3);
        mismatched.accepted.case = NamedCase::BoostedGroundTakeoff;
        let decision = policy.observe(mismatched);
        assert_eq!(decision.owner(), Owner::NormalTracked);
        assert_eq!(decision.reason, Some(InvalidPair::AdmissionMismatch));
    }
}

#[test]
fn exact_motion_continuity_does_not_bridge_distinct_landing_or_hanging_states() {
    for (first, second) in [
        (
            GraphState::Landing(LandingState::Hard),
            GraphState::Landing(LandingState::FastFall),
        ),
        (
            GraphState::Hanging(HangingState::GrappleAttached),
            GraphState::Hanging(HangingState::StuntIdle),
        ),
    ] {
        assert!(!first.continues(second));
        let mut policy = Policy::new();
        let exact = |tick, state: GraphState| {
            let mut value = frame(tick, state, 0.0, 1.0);
            value.accepted.case = state.named_case().unwrap();
            value.blend = None;
            value.clip_time = None;
            value
        };
        policy.observe(exact(1, first));
        let changed = policy.observe(exact(2, second));
        assert_eq!(changed.owner(), Owner::NormalTracked);
        assert_eq!(changed.reason, Some(InvalidPair::InvalidTransition));
    }
    assert!(
        GraphState::Recovery(RecoveryState::PoseMatching)
            .continues(GraphState::Recovery(RecoveryState::BlendIn))
    );
    assert!(
        GraphState::Recovery(RecoveryState::BlendIn)
            .continues(GraphState::Recovery(RecoveryState::GetUp))
    );
    assert!(
        !GraphState::Recovery(RecoveryState::GetUp)
            .continues(GraphState::Recovery(RecoveryState::PoseMatching))
    );
}

#[test]
fn missing_local_history_and_unknown_root_treatment_cannot_own_the_view() {
    let mut policy = Policy::default();
    policy.observe(frame(1, GraphState::GroundInto, 0.0, 1.0));
    let mut mixed = frame(2, GraphState::GroundInto, 0.1, 1.0);
    mixed.sample.local.stamp.finalized = 1;
    assert_eq!(
        policy.observe(mixed).reason,
        Some(InvalidPair::MixedRootLocalTime)
    );
    let mut unknown = frame(3, GraphState::GroundInto, 0.2, 1.0);
    unknown.root_treatment = RootTreatment::Unknown;
    let rejected = policy.observe(unknown);
    assert_eq!(rejected.owner(), Owner::NormalTracked);
    assert_eq!(rejected.reason, Some(InvalidPair::RootTreatmentUnknown));
}
