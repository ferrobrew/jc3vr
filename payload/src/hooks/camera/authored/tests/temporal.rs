use super::*;
use crate::hooks::camera::authored::{
    composition::InvalidPair,
    policy::{Owner, Phase, Policy},
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
                tail.blend.stamp.finalized -= 1;
                InvalidPair::BlendEpochMismatch
            }
            1 => {
                tail.blend.source += 1;
                InvalidPair::SourceChanged
            }
            2 => {
                tail.accepted.run += 1;
                InvalidPair::AdmissionMismatch
            }
            3 => {
                tail.blend.weight = f32::NAN;
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
        next.blend.stamp = next.sample.root.stamp;
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
