use super::*;

/// A wanted velocity held in memory, which counts its writes.
#[derive(Debug, Default)]
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

fn store(wanted: Vec3) -> Memory {
    Memory { wanted, writes: 0 }
}

#[test]
fn the_contributors_are_declared_in_their_order() {
    let mut sorted = Contributor::ORDER;
    sorted.sort();
    assert_eq!(sorted, Contributor::ORDER);
}

#[test]
fn each_contribution_reads_what_the_one_before_left() {
    let mut composition = Composition::new(store(Vec3::new(1.0, -2.0, 3.0)));
    composition.apply(Contributor::Locomotion, |_, wanted| {
        assert_eq!(wanted, Vec3::new(1.0, -2.0, 3.0));
        Some(with_horizontal(wanted, Vec2::new(4.0, 5.0)))
    });
    composition.apply(Contributor::PalmPush, |_, wanted| {
        assert_eq!(wanted, Vec3::new(4.0, -2.0, 5.0));
        pushed(wanted, Some(Vec2::new(0.5, -0.5)))
    });
    assert_eq!(composition.read(), Vec3::new(4.5, -2.0, 4.5));
    assert_eq!(composition.store().writes, 2);
}

#[test]
fn a_contribution_that_returns_nothing_writes_nothing() {
    let mut composition = Composition::new(store(Vec3::new(1.0, 2.0, 3.0)));
    composition.apply(Contributor::PalmPush, |_, wanted| pushed(wanted, None));
    composition.apply(Contributor::StepDrive, |_, _| None);
    assert_eq!(composition.read(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(composition.store().writes, 0);
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
