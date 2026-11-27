//! Registers the systems above the hands with the hands: the hand composition's stages in their
//! order, and the observers of each world weapon at its render update. The weapon
//! handling fills its held-item source and its held guns' targets itself
//! ([`jc3vr_weapons::register`]), and the grapple its pump of the hang swing
//! ([`jc3vr_grapple::register`]).
//!
//! Each fills a field of the body's contributions, which reports any still empty to the install's
//! registration check ([`jc3vr_body::contributions::register`]). The tables here declare the
//! stages' and the grip sources' order ([`GRIP_ORDER`]), once.

use jc3vr_body::{
    contributions::BodyContributions,
    controllers::{
        fingers::{grip, grip::GripSource},
        hands::{
            anchor,
            late::ObserveWeapon,
            stages,
            stages::{HandComposition, HandStageFn},
        },
        held::guns,
    },
};
use jc3vr_physical::controllers::{physical_hands, physical_hands::grip_sources as physical_grips};
use jc3vr_vehicles::{vehicles, vehicles::grip_sources as vehicle_grips};
use jc3vr_weapons::controllers::weapons::{
    ads, collide, grenade, grip_sources as weapon_grips, hold::rack, recoil,
};

pub(super) fn register(body: &mut BodyContributions) {
    body.target_stages = &TARGET_STAGES;
    body.hand_stages = &HAND_STAGES;
    body.weapon_observers = &WEAPON_OBSERVERS;
    body.grip_order = &GRIP_ORDER;
}

/// Every grip source, in the order their declarations win for one hand: what the hand holds in its
/// fingers (the weapon handling's items, then what the physical hands take hold of) over what it
/// holds on to (a climb, the vehicle controls and handles, the grapple line), and a palm pressed on
/// a surface last.
static GRIP_ORDER: [GripSource; 19] = [
    weapon_grips::MAGAZINE,
    weapon_grips::GRENADE,
    weapon_grips::C4,
    weapon_grips::GUN,
    weapon_grips::FOREGRIP,
    physical_grips::CHARACTER,
    physical_grips::DOOR,
    physical_grips::ARTICULATED,
    physical_grips::PICKUP,
    physical_grips::STUNT,
    physical_grips::CLIMB,
    grip::WHEEL,
    grip::HANDLEBARS,
    grip::STICK,
    grip::LEVER,
    vehicle_grips::PARACHUTE_HANDLE,
    vehicle_grips::MOUNTED_GUN,
    grip::GRAPPLE_LINE,
    grip::SURFACE,
];

/// The hand composition's target stages, in the order they run: a hand on a physical vehicle
/// control or a mounted gun's handle holds it, then the shared support and weight targets of a held
/// gun pose it (the two-handed pose while the off-hand holds a foregrip, and the sag, trail, and
/// sway of a long gun in one hand), recording which hands hold a gun body first. The hand bodies'
/// drive applies the same ones.
static TARGET_STAGES: [HandStageFn; 2] = [vehicle_controls, gun_targets];

/// The rest of the hand composition's stages, in the order they run after the target stages: aiming
/// down the sights lifts the gun to the eye and the hands as the sights leave them are recorded;
/// the synthetic recoil and the wall probe displace a kinematic gun hand; a hand whose gun is a
/// body gets its sighted pose back, since the poses between are the gun body's to give; the hand
/// bodies hold a pressed palm at its surface and draw a body's hand at its body; and an anchored
/// grapple hand holds the line, last, over all of them.
static HAND_STAGES: [HandStageFn; 7] = [
    aim_down_sights,
    stages::record_sighted,
    synthetic_recoil,
    wall_probe,
    stages::keep_physical,
    hand_bodies,
    grapple_anchor,
];

/// The observers of each world weapon at its render update: the grenade handling's world weapons,
/// the remote rack members, and the mounted guns. Their order has no effect.
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
