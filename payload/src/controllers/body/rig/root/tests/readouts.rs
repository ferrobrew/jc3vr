use std::time::Instant;

use glam::{Vec2, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    body::rig::{
        collision::probe::{ProbeResult, TimedProbe},
        lean::StandingHeight,
        root::{
            FrameReadout, FrameState, RootStatus, Sim, SimStatus,
            chase::HeadVelocity,
            ledger::{LedgerCounts, Movement},
            owner::RootOwner,
            pushback::Pushback,
            shift::CockpitShift,
            status_from, try_frame_readout_from, try_sim_readout_from,
        },
    },
    roomscale::SeatPhase,
};

#[test]
fn blocking_and_try_readouts_project_current_counts_without_mutating_the_simulation() {
    let frame_status = FrameReadout {
        at: Some(Instant::now()),
        torso_yaw: 0.75,
        world_scale: 2.0,
        shift: Vec3::new(1.0, 2.0, 3.0),
        teleport_rebases: 4,
        exit_rebases: 5,
        debt_m: 0.25,
        pushbacks: 6,
        ..FrameReadout::default()
    };
    let frame = local_frame(frame_status);
    let sim = local_sim();
    let stored_status = sim.lock().status;
    let movement = sim.lock().movement;
    let expected = RootStatus {
        frame: frame_status,
        sim: SimStatus {
            ledger: LedgerCounts {
                exact: 1,
                projected: 1,
            },
            ..stored_status
        },
    };

    for _ in 0..3 {
        assert_eq!(status_from(&frame, &sim), expected);
        assert_eq!(try_frame_readout_from(&frame), Some(expected.frame));
        assert_eq!(try_sim_readout_from(&sim), Some(expected.sim));
        let stored = sim.lock();
        assert_eq!(stored.status, stored_status);
        assert_eq!(stored.movement, movement);
    }
    assert_eq!(frame.lock().status, frame_status);
}

#[test]
fn a_held_frame_lock_returns_busy_without_blocking_the_simulation_readout() {
    let frame = local_frame(FrameReadout::default());
    let sim = local_sim();
    let held = frame.lock();
    assert_eq!(try_frame_readout_from(&frame), None);
    assert_eq!(try_sim_readout_from(&sim).unwrap().ledger.projected, 1);
    drop(held);
    assert_eq!(
        try_frame_readout_from(&frame),
        Some(FrameReadout::default())
    );
}

#[test]
fn a_held_simulation_lock_returns_busy_without_blocking_the_frame_readout() {
    let expected = FrameReadout {
        pushbacks: 7,
        ..FrameReadout::default()
    };
    let frame = local_frame(expected);
    let sim = local_sim();
    let held = sim.lock();
    assert_eq!(try_sim_readout_from(&sim), None);
    assert_eq!(try_frame_readout_from(&frame), Some(expected));
    drop(held);
    assert_eq!(try_sim_readout_from(&sim).unwrap().ledger.projected, 1);
}

fn local_frame(status: FrameReadout) -> Mutex<FrameState> {
    Mutex::new(FrameState {
        seen_epoch: 0,
        shift: CockpitShift::new(),
        head_velocity: HeadVelocity::new(),
        standing: StandingHeight::new(),
        last_seat: SeatPhase::Out,
        pushback: Pushback::new(),
        published: None,
        status,
    })
}

fn local_sim() -> Mutex<Sim> {
    let now = Instant::now();
    let mut sim = Sim::new();
    sim.status = SimStatus {
        at: Some(now),
        owner: Some(RootOwner::Player),
        effective_gap: Vec2::new(1.0, 2.0),
        chase_velocity: Vec2::new(3.0, 4.0),
        task_velocity: Vec2::new(5.0, 6.0),
        ledger: LedgerCounts {
            exact: 11,
            projected: 13,
        },
        probe: Some(TimedProbe {
            result: ProbeResult::Obstructed { depth: 0.25 },
            at: now,
        }),
        capsule_owned: true,
        capsule_blocked: true,
    };
    let movement = Movement {
        task: Vec2::ZERO,
        chase: Vec2::X,
    };
    sim.movement.submit_owned(movement, RootOwner::Player);
    sim.movement.complete(Vec3::ZERO, Vec3::X);
    sim.movement.submit_owned(movement, RootOwner::Player);
    sim.movement.complete(Vec3::ZERO, Vec3::ZERO);
    sim.movement.submit_owned(movement, RootOwner::Player);
    Mutex::new(sim)
}
