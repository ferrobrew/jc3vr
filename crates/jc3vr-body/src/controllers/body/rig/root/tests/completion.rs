use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, Vec3};

use crate::controllers::{
    body::rig::root::{
        Sim,
        ledger::{Attribution, CompletedMovement, Movement},
        owner::{self, OwnerInput, RootOwner},
        stand_down,
    },
    roomscale::{SeatPhase, posture::EffectivePosture},
};

#[test]
fn completions_credit_the_pending_request_once_and_roll_the_totals() {
    let mut sim = Sim::new();
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    let first = sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), 0);
    assert_eq!(first.movement, sim.movement.completed());
    assert_eq!(first.counts, sim.movement.counts());
    assert_eq!(first.movement.current, Vec2::X);
    assert_eq!(first.request.unwrap().credited, Vec2::X);
    assert_eq!(first.request.unwrap().attribution, Attribution::Exact);
    assert!(!sim.movement.pending());

    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Player);
    let second = sim.complete_movement(root(Vec3::X), root(Vec3::X + Vec3::Z), 0);
    assert_eq!(
        second.movement,
        CompletedMovement {
            previous: Vec2::X,
            current: Vec2::ONE,
        }
    );
    assert_eq!(second.counts.exact, 2);

    let idle = sim.complete_movement(root(Vec3::X), root(Vec3::X + Vec3::Z), 0);
    assert_eq!(idle.request, None);
    assert_eq!(idle.movement.previous, Vec2::ONE);
    assert_eq!(idle.movement.current, Vec2::ONE);
    assert_eq!(idle.counts.exact, 2);
}

#[test]
fn consumed_owner_survives_an_intervening_selection_and_status_change() {
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
    let completion = sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), 0);
    assert_eq!(sim.status.owner, Some(RootOwner::Authored));
    assert_eq!(completion.request.unwrap().owner, Some(RootOwner::Player));

    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Seated);
    let replacement = sim
        .complete_movement(root(Vec3::ZERO), root(Vec3::Z), 0)
        .request
        .unwrap();
    assert_eq!(replacement.movement, movement(Vec2::Y));
    assert_eq!(replacement.owner, Some(RootOwner::Seated));
    assert_eq!(replacement.credited, Vec2::Y);
}

#[test]
fn teleport_at_completion_discards_pending_credit() {
    let mut sim = Sim::new();
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), 0);
    sim.movement
        .submit_owned(movement(Vec2::Y), RootOwner::Player);
    let teleported = sim.complete_movement(root(Vec3::ZERO), root(Vec3::Z), 1);
    assert_eq!(teleported.request, None);
    assert!(!sim.movement.pending());
    assert_eq!(sim.movement.completed().current, Vec2::X);
    assert_eq!(sim.movement.counts().exact, 1);
}

#[test]
fn stale_frame_boundary_settles_the_running_totals() {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.status.at = Some(now);
    sim.movement
        .submit_owned(movement(Vec2::X), RootOwner::Player);
    sim.complete_movement(root(Vec3::ZERO), root(Vec3::X), 0);
    assert_eq!(sim.frame_movement(now).current, Vec2::X);
    let settled = sim.frame_movement(now + Duration::from_secs(1));
    assert_eq!(settled.previous, settled.current);
}

fn movement(drive: Vec2) -> Movement {
    Movement::new(Vec2::ZERO, drive, 1.0)
}

fn root(position: Vec3) -> Mat4 {
    Mat4::from_translation(position)
}
