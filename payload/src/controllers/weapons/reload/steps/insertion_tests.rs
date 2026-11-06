use std::num::NonZeroUsize;

use glam::{Mat4, Quat};

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    body::rig::frames::Frames,
    cockpit::CockpitTransform,
    hands::pose::CockpitHand,
    weapons::reload::{
        ReloadStatus, eject::TapDetector, extraction::Pull, knock::Knocks, plan::WellFrame,
        records::Records, tuning::RELOAD,
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
    rejected: bool,
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
    fn carry(&mut self, _: Vec3, _: Vec3, _: f32, _: &LooseTuning) -> loose::Operation<()> {
        self.carries += 1;
        if self.held {
            self.carry
        } else {
            loose::Operation::Gone
        }
    }
    fn seat(&mut self, _: usize) -> SeatOutcome {
        self.seats += 1;
        if !self.held {
            return SeatOutcome::Gone;
        }
        if self.rejected {
            return SeatOutcome::Rejected {
                rounds: 22,
                capacity: 12,
            };
        }
        match self.seat {
            loose::Operation::Ready(rounds) => {
                self.held = false;
                self.available = None;
                self.transfers.push(rounds);
                SeatOutcome::Loaded
            }
            loose::Operation::Deferred => SeatOutcome::Deferred,
            loose::Operation::Gone => SeatOutcome::Gone,
        }
    }
    fn release(&mut self) {
        self.held = false;
        self.releases += 1;
    }
    fn seat_sound(&mut self, _: usize, _: usize) {}
}

pub(super) fn state(stage: InsertStage) -> State {
    let status = ReloadStatus {
        insert_hands: Some((Hand::Left, Hand::Right)),
        insertion: insert::Insertion {
            stage,
            ..Default::default()
        },
        ..Default::default()
    };
    State {
        status,
        tick: None,
        pouches: [None; 2],
        belt_rings: [None; 2],
        stocked_pouch: None,
        pouch_refusal: None,
        free: Some(Hand::Left),
        loose_seen: Vec::new(),
        frames: Some(Frames {
            cockpit: CockpitTransform {
                head_world: Vec3::ZERO,
                head_cockpit: Vec3::ZERO,
                body: Quat::IDENTITY,
                world_scale: 1.0,
            },
            character: Mat4::IDENTITY,
        }),
        records: Records::new(),
        measured: [None; 2],
        extraction: Pull::new(),
        pending_detach: None,
        pending_ejects: [None; 2],
        detach_generation: 0,
        extraction_blocks_belt: [false; 2],
        extraction_needs_neutral: [false; 2],
        insertion_source: InsertionSource::RetainedMagazine,
        ready_cue: insert::ReadyCue::new(),
        tap: TapDetector::new(),
        flicks: [FlickDetector::new(); 2],
        belts: [BeltPush::new(); 2],
        last_grips: [None; 2],
        drawn: None,
        since_draw_s: 10.0,
        knocks: Knocks::new(),
    }
}

pub(super) fn tick(seating: bool) -> Tick {
    let grip = Pose::IDENTITY;
    let top = insert::held_round(
        frames::held_line(grip, Hand::Left, RELOAD.held_palm_gap_m),
        frames::pointing(grip),
        RELOAD.held_top_m,
        Vec3::ZERO,
    )
    .unwrap();
    let depth = if seating {
        RELOAD.seat_depth_m * 1.1
    } else {
        -0.2
    };
    let well = Well {
        mouth: top.top - top.axis * depth,
        axis: top.axis,
    };
    Tick {
        sequence: Some(7),
        weapons: [
            Some(WeaponTick {
                address: 1,
                ammo_owned: false,
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
                    part_axis: Vec3::Y,
                    spare_top_m: None,
                    launcher: false,
                    spent_round_replaced: false,
                },
                from_table: true,
                reading: engine::Reading {
                    class: 0,
                    name_hash: 1,
                    capacity: 30,
                    magazine: 0,
                    chambers: 0,
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
        reload_button: ButtonControl::default(),
        dt: 0.01,
        game_dt: 0.01,
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
        rejected: false,
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
fn rejected_oversized_seat_keeps_rounds_and_chamber_and_obeys_grip_release() {
    let config = ControllersConfig::default();
    for held in [true, false] {
        let mut state = state(InsertStage::Captured {
            elapsed_s: 0.1,
            depth_m: 0.0,
        });
        let mut adapter = adapter();
        adapter.rejected = true;
        let tick = tick(true);
        let key = tick.weapons[0].unwrap().key;
        state.records.get(key).chambers = [Chamber::Chambered, Chamber::Empty];
        let before = state.records.get(key).chambers;
        let seated_before = state.status.insertion.seated;
        state.insert_with(&tick, &inputs(held), &config, &mut adapter);
        assert_eq!(state.status.insertion.seated, seated_before);
        assert_eq!(adapter.seats, 1);
        assert!(adapter.transfers.is_empty());
        assert_eq!(state.status.counts.seats, 0);
        assert_eq!(state.records.get(key).chambers, before);
        assert_eq!(adapter.held, held);
        assert_eq!(adapter.releases, usize::from(!held));
        assert_eq!(state.status.insert_hands.is_some(), held);
    }
}

#[test]
fn escaped_capture_never_transfers_retained_rounds_or_changes_chamber() {
    let config = ControllersConfig::default();
    for release in [false, true] {
        let mut state = state(InsertStage::Captured {
            elapsed_s: 0.1,
            depth_m: 0.0,
        });
        let mut adapter = adapter();
        let mut escaped = tick(true);
        escaped.hands[Hand::Left.index()]
            .as_mut()
            .unwrap()
            .grip
            .position += Vec3::X * 0.2;
        let weapon = escaped.weapons[0].unwrap();
        let before = state.records.get(weapon.key).chambers;
        state.insert_with(&escaped, &inputs(!release), &config, &mut adapter);
        assert!(adapter.transfers.is_empty());
        assert_eq!(adapter.seats, 0);
        assert_eq!(state.status.counts.seats, 0);
        assert_eq!(state.records.get(weapon.key).chambers, before);
        assert_eq!(adapter.held, !release);
        assert_eq!(adapter.releases, usize::from(release));
    }
}

#[test]
fn first_deferred_world_carry_recovers_and_seats_exactly_once() {
    let config = ControllersConfig::default();
    let (mut state, mut adapter, pickup) = world_pickup();
    let sighting = state.loose_seen[0];
    assert_eq!(state.status.insertion.stage, InsertStage::Idle);
    assert!(state.status.insert_hands.is_none());
    assert!(!adapter.held);
    state.insert_with(&tick(false), &pickup, &config, &mut adapter);
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
    state.insert_with(&tick(false), &inputs(true), &config, &mut adapter);
    assert!(matches!(
        state.status.insertion.stage,
        InsertStage::Holding { .. }
    ));
    assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
    assert!(adapter.held);
    assert_eq!(adapter.carries, 2);
    assert!(adapter.transfers.is_empty());
    let mut capture = tick(true);
    let well = capture.guns[Hand::Right.index()]
        .as_mut()
        .unwrap()
        .well
        .as_mut()
        .unwrap();
    well.mouth += well.axis * RELOAD.seat_depth_m * 1.1;
    state.last_grips[Hand::Left.index()] = Some(-well.axis * 0.001);
    state.insert_with(&capture, &inputs(true), &config, &mut adapter);
    assert!(matches!(
        state.status.insertion.stage,
        InsertStage::Captured { .. }
    ));
    assert_eq!(adapter.seats, 0);
    state.insert_with(&tick(true), &inputs(true), &config, &mut adapter);
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
    state.insert_with(&tick(true), &inputs(false), &config, &mut adapter);
    state.insert_with(&tick(true), &pickup, &config, &mut adapter);
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
    state.insert_with(&tick(false), &pickup, &config, &mut adapter);
    assert_eq!(adapter.takes, vec![(Hand::Left, state.loose_seen[0])]);
    assert_eq!(adapter.carries, 1);
    assert!(adapter.held);
    assert_eq!(
        state.status.insertion.stage,
        InsertStage::Holding { elapsed_s: 0.0 }
    );
    assert_eq!(state.status.insertion.taken, 1);
    assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
    state.insert_with(&tick(false), &inputs(false), &config, &mut adapter);
    assert!(!adapter.held);
    assert_eq!(state.status.insertion.stage, InsertStage::Idle);
    assert!(state.status.insert_hands.is_none());
    assert_eq!(state.status.insertion.dropped, 1);
    assert_eq!(adapter.carries, 1);
    assert_eq!(adapter.releases, 1);
    assert_eq!(adapter.seats, 0);
    assert!(adapter.transfers.is_empty());
    state.insert_with(&tick(true), &inputs(false), &config, &mut adapter);
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
        state.insert_with(&tick(false), &inputs(true), &config, &mut adapter);
        assert_eq!(state.status.insertion, before);
        assert_eq!(
            state.status.insert_hands.map(|(hand, _)| hand),
            Some(Hand::Left)
        );
        assert!(adapter.held);
        if !release {
            adapter.carry = loose::Operation::Ready(());
        }
        state.insert_with(&tick(false), &inputs(!release), &config, &mut adapter);
        assert_eq!(adapter.held, !release);
        assert_eq!(state.status.insert_hands.is_some(), !release);
        assert!(adapter.transfers.is_empty());
        if !release {
            state.insert_with(&tick(false), &inputs(false), &config, &mut adapter);
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
        state.insert_with(&tick(true), &inputs(true), &config, &mut adapter);
        assert_eq!(state.status.insertion, before);
        assert_eq!(state.status.insert_hands, Some((Hand::Left, Hand::Right)));
        assert!(adapter.held);
        assert!(adapter.transfers.is_empty());
        if !release {
            adapter.seat = loose::Operation::Ready(17);
        }
        state.insert_with(&tick(true), &inputs(!release), &config, &mut adapter);
        assert!(!adapter.held);
        assert!(state.status.insert_hands.is_none());
        assert_eq!(adapter.transfers, if release { vec![] } else { vec![17] });
        state.insert_with(&tick(true), &inputs(false), &config, &mut adapter);
        assert_eq!(adapter.transfers.len(), usize::from(!release));
        assert_eq!(state.status.counts.seats, u64::from(!release));
    }
}

#[test]
fn a_seat_readies_a_launcher_at_once_and_a_dry_gun_only_after_its_action() {
    let launcher = ReloadPlan {
        load: LoadKind::Magazine,
        action: ActionKind::None,
        well: WellFrame::Gun {
            mouth: Vec3::ZERO,
            axis: Vec3::Y,
        },
        handle: None,
        travel_m: 0.06,
        part_top_m: Some(0.05),
        part_axis: Vec3::Y,
        spare_top_m: None,
        launcher: true,
        spent_round_replaced: true,
    };
    assert!(ready_after_seat(&launcher, Chamber::Chambered));
    assert!(ready_after_seat(&launcher, Chamber::Empty));
    let pistol = ReloadPlan {
        action: ActionKind::Slide,
        handle: Some(Vec3::new(0.0, 0.03, 0.08)),
        launcher: false,
        spent_round_replaced: false,
        ..launcher
    };
    assert!(!ready_after_seat(&pistol, Chamber::Empty));
    assert!(ready_after_seat(&pistol, Chamber::Chambered));
    let unplaced = ReloadPlan {
        handle: None,
        ..pistol
    };
    assert!(ready_after_seat(&unplaced, Chamber::Empty));
}
