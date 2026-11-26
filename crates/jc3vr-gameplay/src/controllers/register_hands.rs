//! Registers the systems above the hands with the hands: the hand composition's stages in their
//! order, the observers of each world weapon at its render update, the hand bodies' post-step
//! sample taken before each rendered frame's composition, and the weapon handling's held-item
//! source ([`crate::controllers::held`]).
//!
//! The hands do real work in each of these every frame, so the install refuses to finish with one
//! missing: [`register`] panics naming it.

use std::fmt::Write as _;

use crate::controllers::{
    hands::{
        self, anchor,
        late::{self, WeaponObserver},
        stages::{self, HandComposition, HandStage, HandStageFn},
    },
    held,
    physical_hands::{self, physics, physics::guns},
    vehicles,
    weapons::{self, ads, collide, config::WEAPONS_CONFIG, grenade, hold::rack, recoil},
};

pub(super) fn register() {
    for (stage, run) in STAGES {
        stages::register_hand_stage(stage, run);
    }
    late::register_weapon_observer(WeaponObserver::Grenade, grenade::observe_world_weapon);
    late::register_weapon_observer(WeaponObserver::RackRemote, rack::remote::observe);
    late::register_weapon_observer(WeaponObserver::Mounted, vehicles::mounted::observe);
    late::register_post_step_sample(post_step_sample);
    weapons::register_held::register();
    let missing = missing();
    assert!(
        missing.is_empty(),
        "hands: the install left contributions unregistered:{missing}"
    );
}

/// The hand composition's stages, in the order they run.
const STAGES: [(HandStage, HandStageFn); 8] = [
    (HandStage::Vehicles, vehicle_controls),
    (HandStage::GunTargets, gun_targets),
    (HandStage::Ads, aim_down_sights),
    (HandStage::Recoil, synthetic_recoil),
    (HandStage::Collide, wall_probe),
    (HandStage::KeepPhysical, stages::keep_physical),
    (HandStage::PhysicalHands, hand_bodies),
    (HandStage::Anchor, grapple_anchor),
];

fn vehicle_controls(c: &mut HandComposition) {
    vehicles::apply(&mut c.hands);
}

fn gun_targets(c: &mut HandComposition) {
    c.physical = guns::physical_hands();
    let heft = WEAPONS_CONFIG.query(|w| w.heft);
    guns::targets::apply(&mut c.hands, c.head.position, &heft);
}

fn aim_down_sights(c: &mut HandComposition) {
    let config = WEAPONS_CONFIG.query(|w| w.ads);
    ads::apply(&mut c.hands, c.head, c.shoulders, c.torso, &config);
}

fn synthetic_recoil(c: &mut HandComposition) {
    let config = WEAPONS_CONFIG.query(|w| w.recoil);
    recoil::apply(&mut c.hands, &config);
}

fn wall_probe(c: &mut HandComposition) {
    let config = WEAPONS_CONFIG.query(|w| w.collide);
    collide::apply(&mut c.hands, c.head, c.world_scale, &config);
}

fn hand_bodies(c: &mut HandComposition) {
    physical_hands::apply(&mut c.hands, c.at);
}

fn grapple_anchor(c: &mut HandComposition) {
    anchor::apply(&mut c.hands, c.head, c.world_scale);
}

fn post_step_sample() {
    // SAFETY: the late update's frame begins on the game thread from `Game::UpdateRender`, after
    // the simulation and outside its step.
    unsafe { physics::sample_render_offsets() };
}

/// Every contribution still unregistered, one per line, or empty.
fn missing() -> String {
    let mut missing = String::new();
    for stage in stages::missing_hand_stages() {
        let _ = write!(missing, "\n- hand stage {stage:?}");
    }
    for observer in late::missing_weapon_observers() {
        let _ = write!(missing, "\n- weapon observer {observer:?}");
    }
    if !hands::late::post_step_sample_registered() {
        missing.push_str("\n- the hand bodies' post-step sample");
    }
    if !held::is_registered() {
        missing.push_str("\n- the held-item source");
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_table_keeps_the_composition_order() {
        let order = STAGES.map(|(stage, _)| stage);
        assert_eq!(order, HandStage::ALL);
        assert_eq!(
            order,
            [
                HandStage::Vehicles,
                HandStage::GunTargets,
                HandStage::Ads,
                HandStage::Recoil,
                HandStage::Collide,
                HandStage::KeepPhysical,
                HandStage::PhysicalHands,
                HandStage::Anchor,
            ]
        );
    }

    #[test]
    fn the_registration_leaves_nothing_missing() {
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        REGISTERED.call_once(register);
        assert_eq!(missing(), "");
    }
}
