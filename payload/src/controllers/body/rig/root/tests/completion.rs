use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::{
    body::rig::root::{
        CompletionSnapshot, CompletionSnapshotAvailability, CompletionUnavailable,
        MovementCompletion, Sim,
        ledger::{Attribution, CompletedMovement, Movement},
        owner::{self, OwnerInput, RootOwner},
        shift::{CockpitBasis, CockpitShift, ShiftPair},
        stand_down,
    },
    roomscale::{SeatPhase, posture::EffectivePosture},
};

#[test]
fn two_completions_capture_the_post_completion_pair_and_replace_one_record() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
    let first = completed(&sim);
    assert_eq!(first.sequence, 1);
    assert_eq!(first.previous_root, root(Vec3::ZERO));
    assert_eq!(first.current_root, root(Vec3::X));
    assert_eq!(first.completed_at, now);
    assert_eq!(first.ledger.movement, sim.movement.completed());
    assert_eq!(first.ledger.counts, sim.movement.counts());
    assert_eq!(first.ledger.movement.current, Vec2::X);
    assert_eq!(first.ledger.request.unwrap().credited, Vec2::X);
    assert_eq!(
        first.ledger.request.unwrap().attribution,
        Attribution::Exact
    );
    assert!(!sim.movement.pending());

    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Player);
    sim.complete_movement(
        root(Vec3::X),
        root(Vec3::X + Vec3::Z),
        now + Duration::from_millis(1),
        0,
    );
    let second = completed(&sim);
    assert_eq!(second.sequence, 2);
    assert_eq!(
        second.ledger.movement,
        CompletedMovement {
            previous: Vec2::X,
            current: Vec2::ONE,
        }
    );
    assert_eq!(second.ledger.counts.exact, 2);
    assert_eq!(first.ledger.movement.current, Vec2::X);
    assert_eq!(first.sequence, 1);

    sim.complete_movement(root(Vec3::X), root(Vec3::X + Vec3::Z), now, 0);
    let idle = completed(&sim);
    assert_eq!(idle.sequence, 3);
    assert_eq!(idle.ledger.request, None);
    assert_eq!(idle.ledger.movement.previous, Vec2::ONE);
    assert_eq!(idle.ledger.movement.current, Vec2::ONE);
    assert_eq!(idle.ledger.counts.exact, 2);
}

#[test]
fn consumed_owner_survives_an_intervening_selection_and_status_change() {
    let now = Instant::now();
    let mut sim = Sim::new();
    let input = OwnerInput {
        task_wrote: true,
        climbing: false,
        authored: false,
        posture: EffectivePosture::Standing,
        seat: SeatPhase::Out,
    };
    let submitted_owner = owner::select(&input);
    sim.movement
        .submit_owned(movement(Vec2::X), submitted_owner);
    let later_owner = owner::select(&OwnerInput {
        authored: true,
        ..input
    });
    stand_down(&mut sim.status, later_owner);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
    assert_eq!(sim.status.owner, Some(RootOwner::Authored));
    assert_eq!(
        completed(&sim).ledger.request.unwrap().owner,
        Some(RootOwner::Player)
    );

    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Seated);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::Z), now, 0);
    let replacement = completed(&sim).ledger.request.unwrap();
    assert_eq!(replacement.movement, movement(Vec2::Y));
    assert_eq!(replacement.owner, Some(RootOwner::Seated));
    assert_eq!(replacement.credited, Vec2::Y);
}

#[test]
fn every_clear_reason_drops_pending_credit_and_invalidates_the_latest_record() {
    let now = Instant::now();
    for reason in [
        CompletionUnavailable::Cleared,
        CompletionUnavailable::SourceUnavailable,
        CompletionUnavailable::Teleported,
    ] {
        let mut sim = Sim::new();
        sim.movement
            .submit_owned(movement(Vec2::X), RootOwner::Player);
        sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
        let held = completed(&sim);
        sim.movement
            .submit_owned(movement(Vec2::Y), RootOwner::Player);
        sim.clear_movement(reason);
        assert_eq!(sim.completion, CompletionSnapshot::unavailable(reason));
        assert_eq!(
            sim.completion.availability(),
            CompletionSnapshotAvailability::Unavailable(reason),
        );
        assert_eq!(sim.completion.completion(), None);
        assert!(!sim.movement.pending());
        assert_eq!(sim.movement.completed().previous, Vec2::X);
        sim.complete_movement(root(Vec3::ZERO), root(Vec3::Z), now, 0);
        let after = completed(&sim);
        assert_eq!(after.sequence, 2);
        assert_eq!(after.ledger.request, None);
        assert_eq!(after.ledger.movement.previous, Vec2::X);
        assert_eq!(after.ledger.movement.current, Vec2::X);
        assert_eq!(after.ledger.counts.exact, 1);
        assert_eq!(held.ledger.movement.previous, Vec2::ZERO);
    }
}

#[test]
fn teleport_at_completion_discards_pending_credit_without_resetting_sequence() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::Z), now, 1);
    assert_eq!(sim.completion_sequence, 2);
    assert_eq!(
        sim.completion,
        CompletionSnapshot::unavailable(CompletionUnavailable::Teleported)
    );
    assert!(!sim.movement.pending());
    assert_eq!(sim.movement.completed().current, Vec2::X);
    assert_eq!(sim.movement.counts().exact, 1);

    sim.seen_epoch = 1;
    sim.clear_movement(CompletionUnavailable::Teleported);
    sim.complete_movement(root(Vec3::Z), root(Vec3::Z), now, 1);
    assert_eq!(completed(&sim).sequence, 3);
    assert_eq!(completed(&sim).ledger.request, None);
}

#[test]
fn sequence_exhaustion_does_not_wrap_or_change_movement_completion() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.completion_sequence = u64::MAX;
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
    assert_eq!(sim.completion_sequence, u64::MAX);
    assert_eq!(
        sim.completion,
        CompletionSnapshot::unavailable(CompletionUnavailable::SequenceExhausted)
    );
    assert!(!sim.movement.pending());
    assert_eq!(sim.movement.completed().current, Vec2::X);
    assert_eq!(sim.movement.counts().exact, 1);
}

#[test]
fn frame_boundary_copies_token_and_totals_together_and_keeps_the_exact_final_shift() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.status.at = Some(now);
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    let first = sim
        .complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0)
        .unwrap();
    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Player);
    let second = sim
        .complete_movement(root(Vec3::X), root(Vec3::X + Vec3::Z), now, 0)
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(second.sequence(), 2);
    let boundary = sim.frame_movement(now, 0);
    let basis = CockpitBasis {
        body: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        world_scale: 2.0,
    };
    let mut shift = CockpitShift::new();
    shift.observe(&basis, boundary.movement());
    shift.slide_view(Vec3::Z * 0.25);
    let exact_pair = shift.pair();
    let source = boundary.compose(basis, &shift);
    let record = source.completion().completion().unwrap();
    assert_eq!(record.token(), second);
    assert_eq!(source.movement(), record.ledger.movement);
    assert_eq!(source.current_basis(), basis);
    assert_eq!(source.shift(), exact_pair);
    assert!((exact_pair.current - exact_pair.previous).distance(-Vec3::X * 0.5) < 1e-6);
    let raw = Vec3::new(0.2, 1.7, -0.3);
    let (previous, current) = shift.apply(raw);
    assert_eq!(raw - source.shift().previous, previous);
    assert_eq!(raw - source.shift().current, current);
    for _ in 0..3 {
        let repeated = sim.frame_movement(now, 0);
        shift.observe(&basis, repeated.movement());
        assert_eq!(repeated.compose(basis, &shift), source);
    }
    let third = sim
        .complete_movement(root(Vec3::ZERO), root(Vec3::ZERO), now, 0)
        .unwrap();
    assert_ne!(third, second);
    assert_eq!(source.completion().completion().unwrap().token(), second);
    assert_eq!(source.movement().previous, Vec2::X);
}

#[test]
fn invalidated_or_unpublishable_completions_never_return_a_token() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.status.at = Some(now);
    assert!(
        sim.complete_movement(root(Vec3::ZERO), root(Vec3::ZERO), now, 0)
            .is_some()
    );
    for reason in [
        CompletionUnavailable::Cleared,
        CompletionUnavailable::SourceUnavailable,
    ] {
        sim.clear_movement(reason);
        let source = sim.frame_movement(now, 0).compose(
            CockpitBasis {
                body: Quat::IDENTITY,
                world_scale: 1.0,
            },
            &CockpitShift::new(),
        );
        assert_eq!(source.completion(), CompletionSnapshot::unavailable(reason));
    }
    assert_eq!(
        sim.complete_movement(root(Vec3::ZERO), root(Vec3::ZERO), now, 1),
        None
    );
    assert_eq!(sim.completion.completion(), None);
    sim.seen_epoch = 1;
    sim.completion_sequence = u64::MAX;
    assert_eq!(
        sim.complete_movement(root(Vec3::ZERO), root(Vec3::ZERO), now, 1),
        None
    );
    assert_eq!(sim.completion.completion(), None);
}

#[test]
fn stale_frame_boundary_clears_completion_and_settles_the_running_totals() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.status.at = Some(now);
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    let _ = sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), now, 0);
    let boundary = sim.frame_movement(now + Duration::from_secs(1), 0);
    let source = boundary.compose(
        CockpitBasis {
            body: Quat::IDENTITY,
            world_scale: 1.0,
        },
        &CockpitShift::new(),
    );
    assert_eq!(
        source.completion(),
        CompletionSnapshot::unavailable(CompletionUnavailable::SourceUnavailable)
    );
    assert_eq!(source.movement().previous, source.movement().current);
    assert_eq!(
        source.shift(),
        ShiftPair {
            previous: Vec3::ZERO,
            current: Vec3::ZERO
        }
    );
}

fn completed(sim: &Sim) -> MovementCompletion {
    assert_eq!(
        sim.completion.availability(),
        CompletionSnapshotAvailability::Completed,
    );
    sim.completion
        .completion()
        .expect("a completed snapshot retains its owned record")
}

fn movement(chase: Vec2) -> Movement {
    Movement::new(Vec2::ZERO, chase, 1.0)
}

fn root(position: Vec3) -> Mat4 {
    Mat4::from_translation(position)
}
