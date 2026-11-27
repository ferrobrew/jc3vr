use std::{cell::Cell, rc::Rc};

use super::*;

/// A wanted velocity held in memory, which counts its writes; the clone the test keeps sees them.
#[derive(Debug, Default, Clone)]
struct Memory(Rc<Cell<(Vec3, u32)>>);

impl Memory {
    fn writes(&self) -> u32 {
        self.0.get().1
    }
}

impl WantedStore for Memory {
    fn read(&self) -> Vec3 {
        self.0.get().0
    }

    fn write(&mut self, wanted: Vec3) {
        self.0.set((wanted, self.writes() + 1));
    }
}

fn store(wanted: Vec3) -> Memory {
    Memory(Rc::new(Cell::new((wanted, 0))))
}

#[test]
fn a_frames_contributors_apply_in_order_each_on_the_last_ones_result() {
    let memory = store(Vec3::ZERO);
    let mut composition = Composition::new(memory.clone());
    let sequence = [
        Contributor::Rail,
        Contributor::GrappleLock,
        Contributor::Locomotion,
        Contributor::PalmPush,
        Contributor::StepDrive,
        Contributor::Strokes,
    ];
    for (n, contributor) in sequence.into_iter().enumerate() {
        composition.apply(contributor, |_, wanted| {
            assert_eq!(wanted, Vec3::X * n as f32, "{}", contributor.label());
            Some(wanted + Vec3::X)
        });
    }
    assert_eq!(composition.read(), Vec3::X * 6.0);
    assert_eq!(memory.writes(), 6);
}

#[test]
fn each_contribution_reads_what_the_one_before_left() {
    let memory = store(Vec3::new(1.0, -2.0, 3.0));
    let mut composition = Composition::new(memory.clone());
    composition.apply(Contributor::Locomotion, |_, wanted| {
        assert_eq!(wanted, Vec3::new(1.0, -2.0, 3.0));
        Some(with_horizontal(wanted, Vec2::new(4.0, 5.0)))
    });
    composition.apply(Contributor::PalmPush, |_, wanted| {
        assert_eq!(wanted, Vec3::new(4.0, -2.0, 5.0));
        pushed(wanted, Some(Vec2::new(0.5, -0.5)))
    });
    assert_eq!(composition.read(), Vec3::new(4.5, -2.0, 4.5));
    assert_eq!(memory.writes(), 2);
}

#[test]
fn a_contribution_that_returns_nothing_writes_nothing() {
    let memory = store(Vec3::new(1.0, 2.0, 3.0));
    let mut composition = Composition::new(memory.clone());
    composition.apply(Contributor::PalmPush, |_, wanted| pushed(wanted, None));
    composition.apply(Contributor::StepDrive, |_, _| None);
    assert_eq!(composition.read(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(memory.writes(), 0);
}

#[test]
fn the_locomotion_keeps_the_tasks_vertical_velocity() {
    assert_eq!(
        with_horizontal(Vec3::new(9.0, -4.0, 9.0), Vec2::new(1.0, 2.0)),
        Vec3::new(1.0, -4.0, 2.0)
    );
}

#[test]
fn the_push_adds_to_the_horizontal_velocity_only() {
    assert_eq!(
        pushed(Vec3::new(1.0, -4.0, 2.0), Some(Vec2::new(0.25, -1.0))),
        Some(Vec3::new(1.25, -4.0, 1.0))
    );
    assert_eq!(pushed(Vec3::ONE, None), None);
}

#[test]
#[should_panic(expected = "wanted velocity: locomotion applied after palm push")]
fn a_contributor_out_of_order_is_refused() {
    let mut composition = Composition::new(store(Vec3::ZERO));
    composition.apply(Contributor::PalmPush, |_, _| None);
    composition.apply(Contributor::Locomotion, |_, _| None);
}
