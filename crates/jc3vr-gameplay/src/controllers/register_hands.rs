//! Registers the systems above the hands with the hands: the hand composition's stages in their
//! order, the observers of each world weapon at its render update, the hand bodies' post-step
//! sample taken before each rendered frame's composition, the weapon handling's held-item source
//! ([`jc3vr_body::controllers::held`]) and its held guns' support and weight targets, which the
//! hand bodies' drive applies ([`jc3vr_body::controllers::held::guns`]), and the grapple's pump of
//! the hang swing the anchor drives.
//!
//! The hands do real work in each of these every frame, so the install refuses to finish with one
//! missing: [`register`] panics naming it.

use std::fmt::Write as _;

use jc3vr_body::controllers::{
    hands,
    hands::{
        anchor, late,
        late::WeaponObserver,
        stages,
        stages::{HandComposition, HandStage, HandStageFn},
    },
    held,
    held::guns,
};
use jc3vr_grapple::grapple::physical::swing;
use jc3vr_physical::controllers::{physical_hands, physical_hands::physics};
use jc3vr_vehicles::controllers::vehicles;
use jc3vr_weapons::controllers::{
    weapons,
    weapons::{ads, collide, grenade, gun_bodies, hold::rack, recoil},
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
    guns::register_targets(gun_bodies::apply);
    anchor::register_hang_pump(swing::HANG_PUMP);
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
    guns::apply_targets(&mut c.hands, c.head.position);
}

fn aim_down_sights(c: &mut HandComposition) {
    ads::apply(&mut c.hands, c.head, c.shoulders, c.torso);
}

fn synthetic_recoil(c: &mut HandComposition) {
    recoil::apply(&mut c.hands);
}

fn wall_probe(c: &mut HandComposition) {
    collide::apply(&mut c.hands, c.head, c.world_scale);
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
    if !guns::targets_registered() {
        missing.push_str("\n- the held guns' support and weight targets");
    }
    if !anchor::hang_pump_registered() {
        missing.push_str("\n- the grapple's hang pump");
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
