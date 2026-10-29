use super::*;
use crate::controllers::weapons::reload::{
    steps::insertion_tests::{state as make_state, tick as insertion_tick},
    tuning::{RELOAD, Reload},
};

fn tick(seating: bool) -> Tick {
    let mut tick = insertion_tick(seating);
    tick.weapons[0].as_mut().unwrap().plan.handle = Some(Vec3::ZERO);
    tick
}

fn state() -> State {
    make_state(InsertStage::Idle)
}

fn shot(hand: Hand, magazine: i32) -> gate::Shot {
    gate::Shot {
        weapon: 1,
        hand,
        magazine,
    }
}

#[test]
fn delayed_pair_shots_preserve_owned_readiness_and_still_lock_back() {
    let config = ControllersConfig::default();
    let mut tick = tick(false);
    let mut state = state();
    let weapon = tick.weapons[0].as_mut().unwrap();
    weapon.ammo_owned = true;
    let key = weapon.key;
    let mut left = tick.guns[1].unwrap();
    left.index = 1;
    tick.guns[0] = Some(left);
    let chambers = [Chamber::Empty, Chamber::Chambered];
    state.records.get(key).chambers = chambers;
    state.apply_shots(
        &tick,
        &Reload::of(&config.weapons.reload),
        vec![shot(Hand::Right, 3), shot(Hand::Left, 0)],
    );
    assert_eq!(state.records.get(key).chambers, chambers);
    assert_eq!(state.status.counts.lock_backs, 1);
    assert!(!state.records.get(key).travels[1].in_battery(&RELOAD.action));
}

#[test]
fn delayed_unowned_shots_keep_the_native_readiness_fallback() {
    let config = ControllersConfig::default();
    let tick = tick(false);
    let mut state = state();
    let key = tick.weapons[0].unwrap().key;
    state.records.get(key).chambers = [Chamber::Empty; 2];
    state.apply_shots(
        &tick,
        &Reload::of(&config.weapons.reload),
        vec![shot(Hand::Right, 2)],
    );
    assert_eq!(state.records.get(key).chambers[0], Chamber::Chambered);
    state.apply_shots(
        &tick,
        &Reload::of(&config.weapons.reload),
        vec![shot(Hand::Right, 0)],
    );
    assert_eq!(state.records.get(key).chambers[0], Chamber::Empty);
    assert_eq!(state.status.counts.lock_backs, 1);
}

#[test]
fn first_empty_observation_preserves_owned_chambers_and_seeds_lockback() {
    let config = ControllersConfig::default();
    for owned in [false, true] {
        let mut tick = tick(false);
        let weapon = tick.weapons[0].as_mut().unwrap();
        weapon.ammo_owned = owned;
        let key = weapon.key;
        let mut state = state();
        state.records.get(key).chambers = [Chamber::Chambered; 2];
        state.sync(&tick, &Reload::of(&config.weapons.reload));
        assert_eq!(
            state.records.get(key).chambers,
            [if owned {
                Chamber::Chambered
            } else {
                Chamber::Empty
            }; 2]
        );
        assert!(!state.records.get(key).travels[0].in_battery(&RELOAD.action));
    }
}
