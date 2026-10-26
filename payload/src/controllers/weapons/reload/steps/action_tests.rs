use glam::Quat;

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    hands::pose::CockpitHand,
    weapons::reload::{action::ActionEngine, chamber::WorkStage},
};

struct FakeAction {
    rounds: i32,
    magazine_in: bool,
    pump: Option<Hand>,
    discarded: usize,
    cycles: usize,
    reads: usize,
}

impl ActionEngine for FakeAction {
    fn pump_hand(&self, _: usize) -> Option<Hand> {
        self.pump
    }

    fn ammo(&mut self, _: usize) -> Option<(i32, bool)> {
        self.reads += 1;
        Some((self.rounds, self.magazine_in))
    }

    fn eject_round(&mut self, _: usize) -> Option<(i32, bool)> {
        if self.rounds > 0 && self.magazine_in {
            self.rounds -= 1;
            self.discarded += 1;
        }
        self.ammo(1)
    }

    fn cycled(&mut self, _: usize, _: ActionKind, clip: bool) {
        assert!(
            !clip,
            "physical travel must not replay a native action clip"
        );
        self.cycles += 1;
    }
}

fn fixture(
    kind: ActionKind,
    hand: Hand,
    rounds: i32,
) -> (ControllersConfig, State, Tick, FakeAction) {
    let config = ControllersConfig::default();
    let mut state = insertion_tests::state(InsertStage::Idle);
    state.status.insert_hands = None;
    state.free = Some(hand.other());
    let mut tick = insertion_tests::tick(false, &config);
    if hand == Hand::Left {
        tick.guns.swap(0, 1);
        tick.hands.swap(0, 1);
    }
    let weapon = tick.weapons[0].as_mut().unwrap();
    weapon.plan.action = kind;
    weapon.plan.handle = Some(Vec3::ZERO);
    weapon.reading.magazine = rounds;
    weapon.reading.out = false;
    let gun = tick.guns[hand.index()].as_mut().unwrap();
    gun.handle = Some((Vec3::ZERO, Vec3::Z));
    gun.frame = Some(GunFrame {
        pose: Pose::IDENTITY,
        world_scale: 1.0,
    });
    state.records.get(weapon.key).chambers[gun.entry()] = if rounds > 0 {
        Chamber::Chambered
    } else {
        Chamber::Empty
    };
    let engine = FakeAction {
        rounds,
        magazine_in: true,
        pump: (kind == ActionKind::Pump).then_some(hand.other()),
        discarded: 0,
        cycles: 0,
        reads: 0,
    };
    (config, state, tick, engine)
}

fn move_hand(tick: &mut Tick, worker: Hand, position: Vec3) {
    let pose = Pose {
        position,
        orientation: Quat::IDENTITY,
    };
    tick.hands[worker.index()] = Some(CockpitHand {
        wrist: pose,
        grip: pose,
        aim: pose,
    });
}

fn step(
    state: &mut State,
    tick: &mut Tick,
    engine: &mut FakeAction,
    config: &ControllersConfig,
    position: Vec3,
    grip: (bool, bool),
) {
    let worker = state.free.unwrap();
    move_hand(tick, worker, position);
    let (pressed, down) = grip;
    let mut inputs = FinishInputs {
        grabs: [None; 2],
        grips: [ButtonControl::default(); 2],
        triggers: [ButtonControl::default(); 2],
        reload_button: ButtonControl::default(),
        dt: 1.0,
    };
    inputs.grabs[worker.index()] = pressed.then_some(GrabVolume::WeaponAction);
    inputs.grips[worker.index()] = ButtonControl {
        down,
        pressed,
        released: !down,
    };
    state.work_travel(tick, &inputs, &config.weapons.reload, engine);
}

fn chamber(state: &mut State, tick: &Tick, hand: Hand) -> Chamber {
    let (weapon, gun) = tick.gun(hand).unwrap();
    state.records.get(weapon.key).chambers[gun.entry()]
}

#[test]
fn runtime_candidates_offer_ready_slide_and_bolt_to_either_free_hand() {
    for hand in Hand::BOTH {
        for kind in [ActionKind::Slide, ActionKind::Bolt] {
            let (config, mut state, tick, _) = fixture(kind, hand, 5);
            let candidate = state
                .handle_candidate(&tick, Some(hand.other()), &config.weapons.reload)
                .unwrap();
            assert_eq!(candidate.volume, GrabVolume::WeaponAction);
            assert_eq!(candidate.hand, Some(hand.other()));
            assert!(
                state
                    .handle_candidate(&tick, None, &config.weapons.reload)
                    .is_none()
            );
            state.status.insertion.stage = InsertStage::Holding { elapsed_s: 0.0 };
            assert!(
                state
                    .handle_candidate(&tick, Some(hand.other()), &config.weapons.reload)
                    .is_none()
            );
        }
    }
}

#[test]
fn action_candidates_preserve_magazine_claims_settings_and_geometry_requirements() {
    let (mut config, mut state, mut tick, _) = fixture(ActionKind::Slide, Hand::Right, 5);
    state.extraction_blocks_belt[Hand::Right.index()] = true;
    assert!(
        state
            .handle_candidate(&tick, Some(Hand::Left), &config.weapons.reload)
            .is_none()
    );
    state.extraction_blocks_belt[Hand::Right.index()] = false;
    config.weapons.reload.chamber = false;
    config.weapons.reload.manual_action = false;
    assert!(
        state
            .handle_candidate(&tick, Some(Hand::Left), &config.weapons.reload)
            .is_none()
    );
    config.weapons.reload.chamber = true;
    tick.guns[Hand::Right.index()].as_mut().unwrap().handle = None;
    assert!(
        state
            .handle_candidate(&tick, Some(Hand::Left), &config.weapons.reload)
            .is_none()
    );
}

#[test]
fn ready_charging_handles_and_pumps_do_not_gain_a_handle_candidate() {
    for kind in [
        ActionKind::ChargingHandle,
        ActionKind::Pump,
        ActionKind::None,
    ] {
        let (config, mut state, tick, _) = fixture(kind, Hand::Right, 5);
        assert!(
            state
                .handle_candidate(&tick, Some(Hand::Left), &config.weapons.reload)
                .is_none()
        );
    }
    let (mut config, mut state, tick, _) = fixture(ActionKind::Slide, Hand::Right, 5);
    config.weapons.reload.action.physical = false;
    assert!(
        state
            .handle_candidate(&tick, Some(Hand::Left), &config.weapons.reload)
            .is_none()
    );
    assert_eq!(state.status.work.stage, WorkStage::Idle);
}

#[test]
fn runtime_ready_slide_discards_once_at_the_rear_and_feeds_without_adding_ammo() {
    for hand in Hand::BOTH {
        let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, hand, 5);
        let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            Vec3::ZERO,
            (true, true),
        );
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            rear,
            (false, true),
        );
        assert_eq!(engine.rounds, 4);
        assert_eq!(chamber(&mut state, &tick, hand), Chamber::Empty);
        assert!(state.fire_hold(&tick, hand, &config.weapons.reload));
        for _ in 0..4 {
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                rear,
                (false, true),
            );
        }
        assert_eq!(engine.discarded, 1);
        assert_eq!(engine.cycles, 0);
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            rear,
            (false, false),
        );
        assert_eq!(engine.cycles, 1);
        assert_eq!(engine.rounds, 4);
        assert_eq!(chamber(&mut state, &tick, hand), Chamber::Chambered);
        assert!(!state.fire_hold(&tick, hand, &config.weapons.reload));
        assert_eq!(state.status.counts.racks, 1);
        assert!(engine.reads > 0);
    }
}

#[test]
fn repeated_ready_cycles_discard_one_round_each_without_refund_or_duplication() {
    let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 5);
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    for remaining in (1..5).rev() {
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            Vec3::ZERO,
            (true, true),
        );
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            rear,
            (false, true),
        );
        step(
            &mut state,
            &mut tick,
            &mut engine,
            &config,
            rear,
            (false, false),
        );
        assert_eq!(engine.rounds, remaining);
        assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Chambered);
    }
    assert_eq!(engine.discarded, 4);
    assert_eq!(engine.cycles, 4);
}

#[test]
fn partial_ready_slide_pull_preserves_the_round_and_dry_cycle_only_feeds() {
    let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 5);
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear * 0.4,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear * 0.4,
        (false, false),
    );
    assert_eq!(engine.rounds, 5);
    assert_eq!(engine.cycles, 0);
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Chambered);
    state.records.get(tick.weapons[0].unwrap().key).chambers[0] = Chamber::Empty;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, false),
    );
    assert_eq!(
        engine.rounds, 5,
        "a dry chamber has no live round to discard"
    );
    assert_eq!(engine.discarded, 0);
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Chambered);
}

#[test]
fn last_live_round_is_discarded_without_refill_and_an_empty_slide_locks_back() {
    let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 1);
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, false),
    );
    assert_eq!(engine.rounds, 0);
    assert_eq!(engine.discarded, 1);
    assert_eq!(engine.cycles, 0);
    assert!(state.records.get(tick.weapons[0].unwrap().key).travels[0].locked_back);
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Empty);
}

#[test]
fn pump_and_bolt_hold_open_without_repeated_debits_and_close_after_reacquisition() {
    for hand in Hand::BOTH {
        for kind in [ActionKind::Pump, ActionKind::Bolt] {
            let (config, mut state, mut tick, mut engine) = fixture(kind, hand, 3);
            let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
            let lifted = Vec3::new(
                -config.weapons.reload.action.bolt_radius_m,
                config.weapons.reload.action.bolt_radius_m,
                0.0,
            );
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                Vec3::ZERO,
                (true, true),
            );
            if kind == ActionKind::Bolt {
                step(
                    &mut state,
                    &mut tick,
                    &mut engine,
                    &config,
                    lifted,
                    (false, true),
                );
            }
            let start = if kind == ActionKind::Bolt {
                lifted
            } else {
                Vec3::ZERO
            };
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                start + rear,
                (false, true),
            );
            assert_eq!(engine.rounds, 2);
            engine.pump = None;
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                start + rear,
                (false, false),
            );
            for _ in 0..3 {
                step(
                    &mut state,
                    &mut tick,
                    &mut engine,
                    &config,
                    start + rear,
                    (false, false),
                );
            }
            assert_eq!(engine.discarded, 1);
            assert_eq!(engine.cycles, 0);
            assert_eq!(chamber(&mut state, &tick, hand), Chamber::Empty);
            engine.pump = (kind == ActionKind::Pump).then_some(hand.other());
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                start + rear,
                (true, true),
            );
            step(
                &mut state,
                &mut tick,
                &mut engine,
                &config,
                start,
                (false, true),
            );
            if kind == ActionKind::Bolt {
                step(
                    &mut state,
                    &mut tick,
                    &mut engine,
                    &config,
                    Vec3::ZERO,
                    (false, true),
                );
            }
            assert_eq!(engine.cycles, 1);
            assert_eq!(engine.rounds, 2);
            assert_eq!(chamber(&mut state, &tick, hand), Chamber::Chambered);
        }
    }
}

#[test]
fn a_magazine_out_cycle_neither_discards_refunds_nor_feeds() {
    let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 4);
    engine.magazine_in = false;
    tick.weapons[0].as_mut().unwrap().reading.out = true;
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, false),
    );
    assert_eq!(
        engine.rounds, 4,
        "a detached magazine owns the remaining count"
    );
    assert_eq!(engine.discarded, 0);
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Empty);
    assert!(state.fire_hold(&tick, Hand::Right, &config.weapons.reload));
}

#[test]
fn an_initially_empty_gun_cycles_without_producing_a_round() {
    let (mut config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 0);
    config.weapons.reload.action.lock_back = false;
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, false),
    );
    assert_eq!(engine.cycles, 1);
    assert_eq!(engine.rounds, 0);
    assert_eq!(engine.discarded, 0);
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Empty);
}

#[test]
fn action_return_uses_the_current_native_ammo_not_the_old_tick_snapshot() {
    let (config, mut state, mut tick, mut engine) = fixture(ActionKind::Slide, Hand::Right, 0);
    engine.rounds = 8;
    let rear = Vec3::Z * tick.weapons[0].unwrap().plan.travel_m;
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        Vec3::ZERO,
        (true, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, true),
    );
    step(
        &mut state,
        &mut tick,
        &mut engine,
        &config,
        rear,
        (false, false),
    );
    assert_eq!(chamber(&mut state, &tick, Hand::Right), Chamber::Chambered);
    assert_eq!(engine.rounds, 8);
    assert_eq!(engine.discarded, 0);
}
