use std::num::NonZeroUsize;

use glam::{Mat4, Quat};

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    body::rig::frames::Frames,
    hands::pose::CockpitHand,
    weapons::reload::{
        ReloadStatus, eject::TapDetector, extraction::Pull, plan::WellFrame, records::Records,
    },
};

struct FakeMagazine {
    held: bool,
    available: Option<loose::Sighting>,
    takes: Vec<(Hand, loose::Sighting)>,
    carry: loose::Operation<()>,
    carries: usize,
    seat: loose::Operation<i32>,
    seats: usize,
    transfers: Vec<i32>,
    releases: usize,
}

impl MagazineInsertion for FakeMagazine {
    fn held(&self) -> bool {
        self.held
    }
    fn take(&mut self, hand: Hand, sighting: loose::Sighting) -> bool {
        if self.held || self.available != Some(sighting) {
            return false;
        }
        self.held = true;
        self.takes.push((hand, sighting));
        true
    }
    fn carry(&mut self, _: Vec3, _: Vec3, _: f32, _: &LooseMagazineConfig) -> loose::Operation<()> {
        self.carries += 1;
        if self.held {
            self.carry
        } else {
            loose::Operation::Gone
        }
    }
    fn seat(&mut self) -> loose::Operation<i32> {
        self.seats += 1;
        if !self.held {
            return loose::Operation::Gone;
        }
        if matches!(self.seat, loose::Operation::Ready(_)) {
            self.held = false;
            self.available = None;
        }
        self.seat
    }
    fn release(&mut self) {
        self.held = false;
        self.releases += 1;
    }
    fn load_rounds(&mut self, _: usize, rounds: i32) {
        self.transfers.push(rounds);
    }
}

fn state(stage: InsertStage) -> State {
    let status = ReloadStatus {
        insert_hands: Some((Hand::Left, Hand::Right)),
        insertion: insert::Insertion { stage, ..Default::default() },
        ..Default::default()
    };
    State {
        status,
        tick: None,
        pouch: None,
        stocked_pouch: None,
        free: Some(Hand::Left),
        loose_seen: Vec::new(),
        frames: Some(Frames {
            head_world: Vec3::ZERO,
            head_cockpit: Vec3::ZERO,
            view: Quat::IDENTITY,
            world_scale: 1.0,
            character: Mat4::IDENTITY,
        }),
        records: Records::new(),
        measured: [None; 2],
        extraction: Pull::new(),
        pending_detach: None,
        detach_generation: 0,
        extraction_blocks_belt: [false; 2],
        extraction_needs_neutral: [false; 2],
        insertion_source: InsertionSource::RetainedMagazine,
        tap: TapDetector::new(),
        flicks: [FlickDetector::new(); 2],
        belts: [BeltPush::new(); 2],
        last_grips: [None; 2],
        last_mouths: [None; 2],
        drawn: None,
        since_draw_s: 10.0,
    }
}

fn tick(seating: bool, config: &ControllersConfig) -> Tick {
    let grip = Pose::IDENTITY;
    let top = insert::held_round(
        grip.position,
        frames::pointing(grip),
        config.weapons.reload.held_top_m,
        Vec3::ZERO,
    )
    .unwrap();
    let depth = if seating {
        config.weapons.reload.seat_depth_m * 1.1
    } else {
        -0.2
    };
    let well = Well {
        mouth: top.top - top.axis * depth,
        axis: top.axis,
    };
    Tick {
        weapons: [
            Some(WeaponTick {
                address: 1,
                key: RecordKey::Object(1),
                plan: ReloadPlan {
                    load: LoadKind::Magazine,
                    action: ActionKind::Slide,
                    well: WellFrame::Gun {
                        mouth: Vec3::ZERO,
                        axis: Vec3::Y,
                    },
                    handle: None,
                    travel_m: 0.06,
                    part_top_m: None,
                },
                from_table: true,
                reading: engine::Reading {
                    class: 0,
                    name_hash: 1,
                    capacity: 30,
                    magazine: 0,
                    out: true,
                    exempt: false,
                    unlimited: false,
                },
                reserve: 70,
            }),
            None,
        ],
        dominant: Hand::Right,
        guns: [
            None,
            Some(GunTick {
                weapon: 0,
                index: 0,
                magazine: None,
                wrist: Pose::IDENTITY,
                frame: None,
                well: Some(well),
                handle: None,
            }),
        ],
        hands: [
            Some(CockpitHand {
                wrist: grip,
                grip,
                aim: grip,
            }),
            None,
        ],
        world_scale: 1.0,
    }
}

fn inputs(held: bool) -> FinishInputs {
    FinishInputs {
        grabs: [None; 2],
        grips: [
            ButtonControl {
                down: held,
                pressed: false,
                released: !held,
            },
            ButtonControl::default(),
        ],
        triggers: [ButtonControl::default(); 2],
        reload_button: ButtonControl::default(),
        dt: 0.01,
    }
}

fn adapter() -> FakeMagazine {
    FakeMagazine {
        held: true,
        available: None,
        takes: Vec::new(),
        carry: loose::Operation::Ready(()),
        carries: 0,
        seat: loose::Operation::Ready(17),
        seats: 0,
        transfers: Vec::new(),
        releases: 0,
    }
}

fn world_pickup() -> (State, FakeMagazine, FinishInputs) {
    let sighting = loose::Sighting {
        magazine: loose::Loose::test(
            NonZeroUsize::new(1).unwrap(),
            3,
            NonZeroUsize::new(1).unwrap(),
            1,
            17,
        ),
        cockpit: Vec3::ZERO,
    };
    let mut state = state(InsertStage::Idle);
    state.status.insert_hands = None;
    state.insertion_source = InsertionSource::Pouch;
    state.loose_seen.push(sighting);
    let mut adapter = adapter();
    adapter.held = false;
    adapter.available = Some(sighting);
    adapter.carry = loose::Operation::Deferred;
    let mut inputs = inputs(true);
    inputs.grabs[Hand::Left.index()] = Some(GrabVolume::WorldMagazine);
    inputs.grips[Hand::Left.index()].pressed = true;
    (state, adapter, inputs)
}

#[test]
fn first_deferred_world_carry_recovers_and_seats_exactly_once() {
    let config = ControllersConfig::default();
    let (mut state, mut adapter, pickup) = world_pickup();
    let sighting = state.loose_seen[0];
    assert_eq!(state.status.insertion.stage, InsertStage::Idle);
    assert!(state.status.insert_hands.is_none());
    assert!(!adapter.held);
    state.insert_with(&tick(false, &config), &pickup, &config, &mut adapter);
    assert_eq!(adapter.takes, vec![(Hand::Left, sighting)]);
    assert_eq!(adapter.carries, 1);
    assert!(adapter.held);
    assert_eq!(
        state.status.insertion.stage,
        InsertStage::Holding { elapsed_s: 0.0 }
    );
    assert_eq!(state.status.insertion.taken, 1);
    assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
    assert_eq!(state.insertion_source, InsertionSource::RetainedMagazine);
    assert_eq!(adapter.seats, 0);
    assert!(adapter.transfers.is_empty());

    adapter.carry = loose::Operation::Ready(());
    state.insert_with(&tick(false, &config), &inputs(true), &config, &mut adapter);
    assert!(matches!(
        state.status.insertion.stage,
        InsertStage::Holding { .. }
    ));
    assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
    assert!(adapter.held);
    assert_eq!(adapter.carries, 2);
    assert!(adapter.transfers.is_empty());
    let mut capture = tick(true, &config);
    let well = capture.guns[Hand::Right.index()]
        .as_mut()
        .unwrap()
        .well
        .as_mut()
        .unwrap();
    well.mouth += well.axis * config.weapons.reload.seat_depth_m * 1.1;
    state.last_grips[Hand::Left.index()] = Some(-well.axis * 0.001);
    state.insert_with(&capture, &inputs(true), &config, &mut adapter);
    assert!(matches!(
        state.status.insertion.stage,
        InsertStage::Captured { .. }
    ));
    assert_eq!(adapter.seats, 0);
    state.insert_with(&tick(true, &config), &inputs(true), &config, &mut adapter);
    assert!(!adapter.held);
    assert!(adapter.available.is_none());
    assert_eq!(state.status.insertion.stage, InsertStage::Idle);
    assert!(state.status.insert_hands.is_none());
    assert_eq!(adapter.transfers, vec![sighting.magazine.rounds]);
    assert_eq!(adapter.seats, 1);
    assert_eq!(adapter.releases, 0);
    assert_eq!(state.status.insertion.taken, 1);
    assert_eq!(state.status.insertion.seated, 1);
    assert_eq!(state.status.counts.seats, 1);
    state.insert_with(&tick(true, &config), &inputs(false), &config, &mut adapter);
    state.insert_with(&tick(true, &config), &pickup, &config, &mut adapter);
    assert_eq!(adapter.takes.len(), 1);
    assert_eq!(adapter.seats, 1);
    assert_eq!(adapter.transfers, vec![sighting.magazine.rounds]);
    assert_eq!(state.status.insertion.seated, 1);
    assert_eq!(state.status.counts.seats, 1);
}

#[test]
fn first_deferred_world_carry_releases_without_transfer() {
    let config = ControllersConfig::default();
    let (mut state, mut adapter, pickup) = world_pickup();
    state.insert_with(&tick(false, &config), &pickup, &config, &mut adapter);
    assert_eq!(adapter.takes, vec![(Hand::Left, state.loose_seen[0])]);
    assert_eq!(adapter.carries, 1);
    assert!(adapter.held);
    assert_eq!(
        state.status.insertion.stage,
        InsertStage::Holding { elapsed_s: 0.0 }
    );
    assert_eq!(state.status.insertion.taken, 1);
    assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
    state.insert_with(&tick(false, &config), &inputs(false), &config, &mut adapter);
    assert!(!adapter.held);
    assert_eq!(state.status.insertion.stage, InsertStage::Idle);
    assert!(state.status.insert_hands.is_none());
    assert_eq!(state.status.insertion.dropped, 1);
    assert_eq!(adapter.carries, 1);
    assert_eq!(adapter.releases, 1);
    assert_eq!(adapter.seats, 0);
    assert!(adapter.transfers.is_empty());
    state.insert_with(&tick(true, &config), &inputs(false), &config, &mut adapter);
    assert_eq!(adapter.takes.len(), 1);
    assert_eq!(adapter.releases, 1);
    assert_eq!(adapter.seats, 0);
    assert!(adapter.transfers.is_empty());
    assert_eq!(state.status.insertion.taken, 1);
    assert_eq!(state.status.insertion.dropped, 1);
    assert_eq!(state.status.insertion.seated, 0);
    assert_eq!(state.status.counts.seats, 0);
}

#[test]
fn unavailable_carry_preserves_hand_occupancy_then_recovers_or_releases() {
    let config = ControllersConfig::default();
    for release in [false, true] {
        let mut state = state(InsertStage::Holding { elapsed_s: 0.0 });
        let mut adapter = adapter();
        adapter.carry = loose::Operation::Deferred;
        let before = state.status.insertion;
        state.insert_with(&tick(false, &config), &inputs(true), &config, &mut adapter);
        assert_eq!(state.status.insertion, before);
        assert_eq!(
            state.status.insert_hands.map(|(hand, _)| hand),
            Some(Hand::Left)
        );
        assert!(adapter.held);
        if !release {
            adapter.carry = loose::Operation::Ready(());
        }
        state.insert_with(
            &tick(false, &config),
            &inputs(!release),
            &config,
            &mut adapter,
        );
        assert_eq!(adapter.held, !release);
        assert_eq!(state.status.insert_hands.is_some(), !release);
        assert!(adapter.transfers.is_empty());
        if !release {
            state.insert_with(&tick(false, &config), &inputs(false), &config, &mut adapter);
            assert!(!adapter.held);
            assert!(state.status.insert_hands.is_none());
        }
    }
}

#[test]
fn unavailable_seat_rolls_back_idle_and_transfers_rounds_only_once_after_recovery() {
    let config = ControllersConfig::default();
    for release in [false, true] {
        let mut state = state(InsertStage::Captured {
            elapsed_s: 0.0,
            depth_m: 0.01,
        });
        let mut adapter = adapter();
        adapter.seat = loose::Operation::Deferred;
        let before = state.status.insertion;
        state.insert_with(&tick(true, &config), &inputs(true), &config, &mut adapter);
        assert_eq!(state.status.insertion, before);
        assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
        assert!(adapter.held);
        assert!(adapter.transfers.is_empty());
        if !release {
            adapter.seat = loose::Operation::Ready(17);
        }
        state.insert_with(
            &tick(true, &config),
            &inputs(!release),
            &config,
            &mut adapter,
        );
        assert!(!adapter.held);
        assert!(state.status.insert_hands.is_none());
        assert_eq!(adapter.transfers, if release { vec![] } else { vec![17] });
        state.insert_with(&tick(true, &config), &inputs(false), &config, &mut adapter);
        assert_eq!(adapter.transfers.len(), usize::from(!release));
        assert_eq!(state.status.counts.seats, u64::from(!release));
    }
}
