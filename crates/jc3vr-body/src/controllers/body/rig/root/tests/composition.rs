//! The on-foot composition of the wanted velocity against the writes the locomotion, the palms'
//! push, and the step drive made one after another, each reading the character's velocity back:
//! the same velocity, the same reads between them, and the same movement for every combination.

use glam::{Vec2, Vec3};

use crate::controllers::{
    body::rig::root::{
        DriveTick, MAX_WANTED_MPS, OnFoot, OnFootSplit, compose_on_foot, horizontal,
        ledger::Movement,
    },
    physics::wanted::WantedStore,
};

/// A wanted velocity held in memory, which counts its writes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Memory {
    wanted: Vec3,
    writes: u32,
}

impl WantedStore for Memory {
    fn read(&self) -> Vec3 {
        self.wanted
    }

    fn write(&mut self, wanted: Vec3) {
        self.wanted = wanted;
        self.writes += 1;
    }
}

/// One tick's inputs: what the task wrote, the stick's velocity (`None` while the animation moves
/// the body), the palms' push, and the drive's step (`None` while the player does not own the
/// root).
#[derive(Debug, Clone, Copy)]
struct Case {
    task: Vec3,
    stick: Option<Vec2>,
    push: Option<Vec2>,
    step: Option<Vec2>,
    dt: f32,
}

/// The writes as the three writers made them in turn, each reading the velocity back first.
fn writers_in_turn(case: Case) -> (Memory, OnFootSplit) {
    let mut store = Memory {
        wanted: case.task,
        writes: 0,
    };
    let task = horizontal(store.read());
    if let Some(stick) = case.stick {
        let read = store.read();
        store.write(Vec3::new(stick.x, read.y, stick.y));
    }
    let stick = horizontal(store.read());
    if let Some(push) = case.push {
        let [x, y, z] = store.read().to_array();
        store.write(Vec3::new(x + push.x, y, z + push.y));
    }
    let palms = horizontal(store.read()) - stick;
    let drive = case.step.map(|step| {
        let [x, y, z] = store.read().to_array();
        let task = Vec2::new(x, z);
        let movement = Movement::new(task, step, case.dt);
        if step != Vec2::ZERO {
            let sum = movement.velocity(case.dt).clamp_length_max(MAX_WANTED_MPS);
            store.write(Vec3::new(sum.x, y, sum.y));
        }
        DriveTick {
            task,
            step,
            movement,
        }
    });
    (
        store,
        OnFootSplit {
            task,
            stick,
            palms,
            drive,
        },
    )
}

/// The writes as the composition makes them.
fn composed(case: Case) -> (Memory, OnFootSplit) {
    let mut store = Memory {
        wanted: case.task,
        writes: 0,
    };
    let split = compose_on_foot(
        &mut store,
        OnFoot {
            locomotion: |_: &mut &mut Memory, wanted: Vec3| {
                case.stick
                    .map(|stick| Vec3::new(stick.x, wanted.y, stick.y))
            },
            push: || case.push,
            step: || case.step,
            dt: case.dt,
        },
    );
    (store, split)
}

/// A composition borrows the store, so the test reads it back afterwards.
impl WantedStore for &mut Memory {
    fn read(&self) -> Vec3 {
        (**self).read()
    }

    fn write(&mut self, wanted: Vec3) {
        (**self).write(wanted)
    }
}

#[test]
fn the_composition_writes_what_the_writers_in_turn_wrote_for_every_combination() {
    let tasks = [
        Vec3::ZERO,
        Vec3::new(1.5, -0.3, -2.0),
        Vec3::new(-7.0, 4.0, 9.5),
    ];
    let sticks = [None, Some(Vec2::ZERO), Some(Vec2::new(2.0, -3.5))];
    let pushes = [None, Some(Vec2::new(0.4, 0.1)), Some(Vec2::new(-11.0, 6.0))];
    let steps = [
        None,
        Some(Vec2::ZERO),
        Some(Vec2::new(0.8, -0.6)),
        Some(Vec2::new(10.0, 9.0)),
    ];
    let dts = [1.0 / 30.0, 0.0, 0.1];
    let mut cases = 0;
    for task in tasks {
        for stick in sticks {
            for push in pushes {
                for step in steps {
                    for dt in dts {
                        let case = Case {
                            task,
                            stick,
                            push,
                            step,
                            dt,
                        };
                        assert_eq!(composed(case), writers_in_turn(case), "{case:?}");
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 3 * 3 * 3 * 4 * 3);
}

#[test]
fn the_sum_is_capped_and_the_vertical_velocity_kept() {
    let case = Case {
        task: Vec3::new(0.0, -3.0, 0.0),
        stick: Some(Vec2::new(6.0, 0.0)),
        push: Some(Vec2::new(4.0, 0.0)),
        step: Some(Vec2::new(8.0, 0.0)),
        dt: 1.0 / 30.0,
    };
    let (store, split) = composed(case);
    assert!((store.wanted.length_squared() - (MAX_WANTED_MPS.powi(2) + 9.0)).abs() < 1e-3);
    assert_eq!(store.wanted.y, -3.0);
    assert_eq!(split.stick, Vec2::new(6.0, 0.0));
    assert_eq!(split.palms, Vec2::new(4.0, 0.0));
    assert_eq!(split.drive.map(|d| d.task), Some(Vec2::new(10.0, 0.0)));
}

#[test]
fn a_step_that_does_not_move_submits_its_movement_and_writes_nothing() {
    let case = Case {
        task: Vec3::new(1.0, 0.0, 2.0),
        stick: None,
        push: None,
        step: Some(Vec2::ZERO),
        dt: 1.0 / 30.0,
    };
    let (store, split) = composed(case);
    assert_eq!(store.writes, 0);
    assert_eq!(
        split.drive.map(|d| d.movement),
        Some(Movement::new(Vec2::new(1.0, 2.0), Vec2::ZERO, 1.0 / 30.0))
    );
}
