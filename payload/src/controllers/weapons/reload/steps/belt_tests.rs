use super::*;
use crate::controllers::{
    bindings::tables::describe::{Control, Effect, Gesture, ModAction, Row},
    weapons::{
        capabilities::{GunHand, GunOwner, NativePair},
        reload::{BeltEligibility, belt::PushStage, steps::insertion_tests},
    },
};

const SEQUENCE: u64 = 7;

fn fixture() -> (ControllersConfig, State, Tick, [Option<GunOwner>; 2]) {
    let config = ControllersConfig::default();
    let mut state = insertion_tests::state(InsertStage::Idle);
    state.status.insert_hands = None;
    state.pouch = Some((Vec3::ZERO, 0.1));
    let mut tick = insertion_tests::tick(false, &config);
    let hand = config.acting_hand();
    if hand != tick.dominant {
        tick.guns.swap(0, 1);
    }
    tick.dominant = hand;
    let guns = Hand::BOTH.map(|h| {
        (h == hand).then_some(GunOwner {
            weapon: 1,
            role: GunHand::Grip,
        })
    });
    (config, state, tick, guns)
}

fn offer(
    state: &State,
    tick: &Tick,
    config: &ControllersConfig,
    guns: [Option<GunOwner>; 2],
) -> Option<BeltEligibility> {
    state
        .belt_finish(tick)
        .eligibility_with(SEQUENCE, config, guns, guns, |address| {
            let weapon = tick.weapon(address)?;
            Some((weapon.reading, weapon.reserve))
        })
}

pub(crate) fn eligible_belt_for_test(
    config: &ControllersConfig,
) -> (BeltEligibility, [Option<GunOwner>; 2]) {
    let (_, state, mut tick, _) = fixture();
    if tick.dominant != config.acting_hand() {
        tick.guns.swap(0, 1);
    }
    tick.dominant = config.acting_hand();
    let guns = Hand::BOTH.map(|hand| {
        (hand == tick.dominant).then_some(GunOwner {
            weapon: 1,
            role: GunHand::Grip,
        })
    });
    (offer(&state, &tick, config, guns).unwrap(), guns)
}

#[test]
fn actual_belt_gate_requires_ammo_space_delay_and_no_other_reload_claim() {
    let (config, mut state, mut tick, guns) = fixture();
    let hand = tick.dominant;
    let weapon_slot = tick.guns[hand.index()].unwrap().weapon;
    assert!(state.belt_gate(&tick, hand).allows(&config.weapons.reload));
    assert!(offer(&state, &tick, &config, guns).is_some());

    state.since_draw_s = config.weapons.reload.belt.after_draw_s;
    assert!(offer(&state, &tick, &config, guns).is_some());
    state.since_draw_s = config.weapons.reload.belt.after_draw_s - 0.001;
    assert!(!state.belt_gate(&tick, hand).allows(&config.weapons.reload));
    assert!(offer(&state, &tick, &config, guns).is_none());
    state.since_draw_s = 10.0;
    state.status.insert_hands = Some((hand.other(), hand));
    assert!(offer(&state, &tick, &config, guns).is_none());
    state.status.insert_hands = None;
    state.extraction_blocks_belt[hand.index()] = true;
    assert!(offer(&state, &tick, &config, guns).is_none());
    state.extraction_blocks_belt[hand.index()] = false;

    tick.weapons[weapon_slot].as_mut().unwrap().reserve = 0;
    assert!(offer(&state, &tick, &config, guns).is_none());
    tick.weapons[weapon_slot]
        .as_mut()
        .unwrap()
        .reading
        .unlimited = true;
    assert!(offer(&state, &tick, &config, guns).is_some());
    let weapon = tick.weapons[weapon_slot].as_mut().unwrap();
    weapon.reading.out = false;
    weapon.reading.magazine = weapon.reading.capacity;
    assert!(offer(&state, &tick, &config, guns).is_none());
    tick.weapons[weapon_slot].as_mut().unwrap().reading.out = true;
    assert!(offer(&state, &tick, &config, guns).is_some());

    for disabled in [false, true] {
        let mut config = config;
        if disabled {
            config.weapons.reload.enabled = false;
        } else {
            config.weapons.reload.belt.enabled = false;
        }
        assert!(offer(&state, &tick, &config, guns).is_none());
    }
    let mut gate = state.belt_gate(&tick, hand);
    gate.extracting = true;
    assert!(!gate.allows(&config.weapons.reload));
}

#[test]
fn shell_pouch_and_belt_gates_count_magazine_rounds_without_the_chamber() {
    let (config, state, mut tick, guns) = fixture();
    let slot = tick.guns[tick.dominant.index()].unwrap().weapon;
    let weapon = tick.weapons[slot].as_mut().unwrap();
    weapon.plan.load = LoadKind::Shell;
    weapon.reading.out = false;
    weapon.reading.chambers = 1;
    weapon.reading.magazine = weapon.reading.capacity;
    assert_eq!(
        weapon.reading.magazine_rounds(),
        weapon.reading.capacity - 1
    );
    assert!(weapon.reading.has_space());
    assert!(state.can_take(weapon));
    assert!(state.can_belt(weapon));
    assert!(
        state
            .belt_gate(&tick, tick.dominant)
            .allows(&config.weapons.reload)
    );
    assert!(offer(&state, &tick, &config, guns).is_some());

    let finish = state.belt_finish(&tick);
    tick.weapons[slot].as_mut().unwrap().reading.magazine += 1;
    let weapon = tick.weapons[slot].as_ref().unwrap();
    assert_eq!(weapon.reading.magazine_rounds(), weapon.reading.capacity);
    assert!(!weapon.reading.has_space());
    assert!(!state.can_take(weapon));
    assert!(!state.can_belt(weapon));
    assert!(
        !state
            .belt_gate(&tick, tick.dominant)
            .allows(&config.weapons.reload)
    );
    assert!(offer(&state, &tick, &config, guns).is_none());
    assert!(
        finish
            .eligibility_with(SEQUENCE, &config, guns, guns, |_| {
                Some((weapon.reading, weapon.reserve))
            })
            .is_none()
    );

    let weapon = tick.weapons[slot].as_mut().unwrap();
    weapon.reading.chambers = 0;
    weapon.reading.magazine = weapon.reading.capacity;
    assert!(!state.can_take(weapon));
    assert!(!state.can_belt(weapon));
}

#[test]
fn post_finish_evidence_uses_final_ammo_not_the_tick_or_debug_status() {
    let (config, state, tick, guns) = fixture();
    let finish = state.belt_finish(&tick);
    let mut reading = tick.weapon(1).unwrap().reading;
    let allowed = |reading, reserve| {
        finish
            .eligibility_with(SEQUENCE, &config, guns, guns, |_| Some((reading, reserve)))
            .is_some()
    };
    reading.out = false;
    reading.magazine = reading.capacity;
    assert!(!allowed(reading, 70));
    reading.magazine -= 1;
    assert!(!allowed(reading, 0));
    assert!(allowed(reading, 70));
    assert!(
        finish
            .eligibility_with(SEQUENCE, &config, guns, guns, |_| None)
            .is_none()
    );
    reading.out = true;
    assert!(allowed(reading, 70));
}

#[test]
fn absent_stale_or_noncurrent_guns_never_authorize_an_engine_read() {
    let (config, state, tick, guns) = fixture();
    let finish = state.belt_finish(&tick);
    let no_read = |_| panic!("no current belt owner may be read");
    for role in [GunHand::Empty, GunHand::Carry, GunHand::Support] {
        let mut changed = guns;
        changed[tick.dominant.index()].as_mut().unwrap().role = role;
        assert!(
            finish
                .eligibility_with(SEQUENCE, &config, changed, changed, no_read)
                .is_none()
        );
    }
    let mut changed = guns;
    changed[tick.dominant.index()].as_mut().unwrap().weapon = 2;
    for (source, current) in [(changed, changed), (guns, changed), (guns, [None; 2])] {
        assert!(
            finish
                .eligibility_with(SEQUENCE, &config, source, current, no_read)
                .is_none()
        );
    }
    assert!(
        finish
            .eligibility_with(SEQUENCE + 1, &config, guns, guns, no_read)
            .is_none()
    );
    for missing in 0..5 {
        let mut tick = tick;
        let mut state = insertion_tests::state(InsertStage::Idle);
        state.status.insert_hands = None;
        state.pouch = Some((Vec3::ZERO, 0.1));
        match missing {
            0 => tick.sequence = None,
            1 => tick.guns[tick.dominant.index()] = None,
            2 => tick.guns[tick.dominant.index()].as_mut().unwrap().well = None,
            3 => state.pouch = None,
            _ => tick.dominant = tick.dominant.other(),
        }
        assert!(
            state
                .belt_finish(&tick)
                .eligibility_with(SEQUENCE, &config, guns, guns, no_read)
                .is_none()
        );
    }
}

#[test]
fn final_claims_readiness_and_sequence_revoke_the_exact_physical_row() {
    let (config, state, tick, guns) = fixture();
    let hand = tick.dominant;
    let lease = offer(&state, &tick, &config, guns).unwrap();
    let row = Row {
        control: Control::Motion(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::BeltReload),
    };
    assert_eq!(lease.row(SEQUENCE, hand, guns, [true; 2]), Some(row));
    assert!(lease.row(SEQUENCE + 1, hand, guns, [true; 2]).is_none());
    assert!(lease.row(SEQUENCE, hand.other(), guns, [true; 2]).is_none());
    assert!(lease.row(SEQUENCE, hand, [None; 2], [true; 2]).is_none());
    let mut unready = [true; 2];
    unready[hand.index()] = false;
    assert!(lease.row(SEQUENCE, hand, guns, unready).is_none());
    for role in [GunHand::Carry, GunHand::Support] {
        let mut changed = guns;
        changed[hand.index()].as_mut().unwrap().role = role;
        assert!(lease.row(SEQUENCE, hand, changed, [true; 2]).is_none());
    }
    let mut other_changed = guns;
    other_changed[hand.other().index()] = Some(GunOwner {
        weapon: 2,
        role: GunHand::Grip,
    });
    assert!(
        lease
            .row(SEQUENCE, hand, other_changed, [true; 2])
            .is_none()
    );
    let mut pair = guns;
    pair[hand.index()].as_mut().unwrap().role =
        GunHand::NativePair(NativePair { first_hand: hand });
    assert!(offer(&state, &tick, &config, pair).is_some());
}

#[test]
fn hint_eligibility_does_not_require_an_already_completed_push() {
    let (config, mut state, mut tick, guns) = fixture();
    let hand = tick.dominant;
    tick.guns[hand.index()]
        .as_mut()
        .unwrap()
        .well
        .as_mut()
        .unwrap()
        .axis = Vec3::Z;
    state.last_mouths[hand.index()] = None;
    assert!(offer(&state, &tick, &config, guns).is_some());
    assert_eq!(state.belts[hand.index()].stage, PushStage::Outside);
}
