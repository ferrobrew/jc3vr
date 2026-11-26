use glam::{Vec2, Vec3};
use jc3vr_core::published::Stamp;
use parking_lot::Mutex;

use crate::controllers::{
    body::rig::root::{
        FrameReadout, FrameState, RootStatus, Sim, SimStatus,
        ledger::{LedgerCounts, Movement},
        owner::RootOwner,
        pushback::Pushback,
        shift::CockpitShift,
        status_from,
        trace::RootTrace,
    },
    roomscale::SeatPhase,
};

#[test]
fn the_readout_projects_current_counts_without_mutating_the_simulation() {
    let frame_status = FrameReadout {
        at: Some(Stamp::now()),
        snaps: 3,
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
        let stored = sim.lock();
        assert_eq!(stored.status, stored_status);
        assert_eq!(stored.movement, movement);
    }
    assert_eq!(frame.lock().status, frame_status);
}

fn local_frame(status: FrameReadout) -> Mutex<FrameState> {
    Mutex::new(FrameState {
        seen_epoch: 0,
        shift: CockpitShift::new(),
        last_seat: SeatPhase::Out,
        pushback: Pushback::new(),
        published: None,
        status,
    })
}

fn local_sim() -> Mutex<Sim> {
    let now = Stamp::now();
    let mut sim = Sim::new();
    sim.status = SimStatus {
        at: Some(now),
        owner: Some(RootOwner::Player),
        effective_gap: Vec2::new(1.0, 2.0),
        step_velocity: Vec2::new(3.0, 4.0),
        task_velocity: Vec2::new(5.0, 6.0),
        ledger: LedgerCounts {
            exact: 11,
            projected: 13,
        },
        capsule_owned: true,
        capsule_blocked: true,
        trace: Some(RootTrace {
            tick: 17,
            position: Vec3::new(1.0, 2.0, 3.0),
            jumps: 2,
            ..RootTrace::default()
        }),
    };
    let movement = Movement {
        task: Vec2::ZERO,
        drive: Vec2::X,
    };
    sim.movement.submit_owned(movement, RootOwner::Player);
    sim.movement.complete(Vec3::ZERO, Vec3::X);
    sim.movement.submit_owned(movement, RootOwner::Player);
    sim.movement.complete(Vec3::ZERO, Vec3::ZERO);
    sim.movement.submit_owned(movement, RootOwner::Player);
    Mutex::new(sim)
}
