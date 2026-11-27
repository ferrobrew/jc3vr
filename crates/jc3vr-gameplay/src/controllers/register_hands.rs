//! Registers the systems above the hands with the hands: the hand composition's stages in their
//! order, the observers of each world weapon at its render update in theirs, and the grapple's pump
//! of the hang swing the anchor drives. The weapon handling fills its held-item source and its held
//! guns' targets in its own `register_body` (`register_below`).
//!
//! Each fills a field of the body's contributions, which reports any still empty to the install's
//! registration check ([`jc3vr_body::contributions::register`]). The tables here declare the
//! stages' and the observers' order, once.

use jc3vr_body::{
    contributions::BodyContributions,
    controllers::{
        hands::{
            anchor,
            late::ObserveWeapon,
            stages,
            stages::{HandComposition, HandStageFn},
        },
        held::guns,
    },
};
use jc3vr_grapple::grapple::physical::swing;
use jc3vr_physical::controllers::physical_hands;
use jc3vr_vehicles::controllers::vehicles;
use jc3vr_weapons::controllers::weapons::{ads, collide, grenade, hold::rack, recoil};

pub(super) fn register(body: &mut BodyContributions) {
    body.target_stages = &TARGET_STAGES;
    body.hand_stages = &HAND_STAGES;
    body.weapon_observers = &WEAPON_OBSERVERS;
    body.hang_pump = Some(swing::HANG_PUMP);
}

/// The hand composition's target stages, in the order they run: a hand on a physical vehicle
/// control or a mounted gun's handle holds it, then the shared support and weight targets of a held
/// gun pose it (the two-handed pose while the off-hand holds a foregrip, and the sag, trail, and
/// sway of a long gun in one hand), recording which hands hold a gun body first. The hand bodies'
/// drive applies the same ones.
static TARGET_STAGES: [HandStageFn; 2] = [vehicle_controls, gun_targets];

/// The rest of the hand composition's stages, in the order they run after the target stages:
/// aiming down the sights lifts the gun to the eye and the hands as the sights leave them are
/// recorded; the synthetic recoil and the wall probe displace a kinematic gun hand; a hand whose gun
/// is a body gets its sighted pose back, since the poses between are the gun body's to give; the
/// hand bodies hold a pressed palm at its surface and draw a body's hand at its body; and an
/// anchored grapple hand holds the line, last, over all of them.
static HAND_STAGES: [HandStageFn; 7] = [
    aim_down_sights,
    stages::record_sighted,
    synthetic_recoil,
    wall_probe,
    stages::keep_physical,
    hand_bodies,
    grapple_anchor,
];

/// The observers of each world weapon at its render update, in the order they run: the grenade
/// handling's world weapons, the remote rack members, and the mounted guns.
static WEAPON_OBSERVERS: [ObserveWeapon; 3] = [
    grenade::observe_world_weapon,
    rack::remote::observe,
    vehicles::mounted::observe,
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
