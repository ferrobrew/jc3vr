//! The first half of the weapon handling's input tick: read the weapons in the hands, and provide
//! the weapon handling's grip targets ([`jc3vr_body::controllers::grab`]): the sockets and pouches on
//! the body, the held gun's parts, and the weapons and grenades in the world.
//!
//! The half is a table of stages ([`STAGES`]) run in order over one [`Begin`] context: the tick's
//! settings and mode table, the hands, and what each stage leaves for the ones after it. The
//! stages read the local player first, then measure the guns, then gather every grip target, and
//! resolve and publish last; a stage reads only what an earlier one left.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{GrabVolume, GripTarget},
    hands,
    hands::{ActiveHands, CockpitHand},
    mode::Mode,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;

use crate::controllers::weapons::{
    begin::{place::Place, read::Ticked},
    context::TickContext,
    docks,
    docks::attract::Attractor,
    engine::HeldWeapon,
    foregrip,
    foregrip::GripGeometry,
    handling_tuning::HANDLING,
    heft, hold,
    hud_feed::hand_ammo,
    regions::GunShape,
    stand_down::{StandDown, stand_down},
    types::TickStart,
    volumes::BodyFrame,
};

mod hint;
mod lone;
mod place;
mod read;
mod resolve;
mod socket_candidates;
mod targets;

/// The first half of the weapon handling's input tick: read the weapons in the hands and return
/// the weapon handling's grip targets. Called while the bindings inject, before the tables.
pub fn begin_tick(game: &GameTick<'_>, mode: Mode) -> TickStart {
    let context = TickContext::read(mode);
    docks::publish_targets(None);
    let Some(active) = hands::active_hands_raw() else {
        stand_down(game, StandDown::HandsInactive);
        hand_ammo::clear();
        hint::clear();
        return TickStart::default();
    };
    let mut begin = Begin::new(context, active);
    for stage in &STAGES {
        (stage.run)(game, &mut begin);
    }
    TickStart {
        targets: begin.targets,
        held: begin.held,
    }
}

/// The begin half's context: what the tick starts from, and what each stage leaves for the ones
/// after it.
pub(super) struct Begin {
    /// The tick's settings and mode table, and the handoff to the finish half, which the publish
    /// stage completes.
    context: TickContext,
    active: ActiveHands,
    /// How the candidates become grip targets, for the tick's head pose.
    place: Place,
    /// What the player stage read of the local player.
    ticked: Ticked,
    /// Each hand free to take, by [`Hand::index`].
    free: [bool; 2],
    /// The physical hands reserving a trigger ([`TickStart::held`]).
    held: Option<[bool; 2]>,
    /// The hand the pouches are laid out around.
    gun_side: Hand,
    /// Each hand's long gun's two-handed geometry, measured on its own wrist.
    geometries: [Option<GripGeometry>; 2],
    /// The wielded weapon's two-handed geometry.
    geometry: Option<GripGeometry>,
    /// The gun held in one hand while the other is free.
    lone: Option<HeldWeapon>,
    /// The lone gun when it is a long gun, which the free hand braces.
    braced: Option<HeldWeapon>,
    braced_geometry: Option<GripGeometry>,
    /// The braced gun's shape.
    shape: Option<GunShape>,
    /// The body the volumes ride.
    body: Option<BodyFrame>,
    /// The engaged foregrip hold.
    hold: Option<foregrip::Hold>,
    /// The free sockets' attractions.
    attractors: Vec<Attractor>,
    targets: Vec<GripTarget>,
    /// The hand that would brace the lone long gun.
    foregrip_hand: Option<Hand>,
    /// Each hand's grip as the weapon handling sees it.
    grips: [Option<Vec3>; 2],
    /// The volume each hand's resolved target is in.
    volumes: [Option<GrabVolume>; 2],
}

impl Begin {
    fn new(context: TickContext, active: ActiveHands) -> Self {
        Self {
            context,
            place: Place::new(&active),
            active,
            ticked: Ticked::default(),
            free: [false; 2],
            held: None,
            gun_side: Hand::Right,
            geometries: [None; 2],
            geometry: None,
            lone: None,
            braced: None,
            braced_geometry: None,
            shape: None,
            body: None,
            hold: None,
            attractors: Vec::new(),
            targets: Vec::new(),
            foregrip_hand: None,
            grips: [None; 2],
            volumes: [None; 2],
        }
    }
}

/// One stage of the begin half.
struct Stage {
    /// The stage, for the order's test.
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the order's test"))]
    name: &'static str,
    run: fn(&GameTick<'_>, &mut Begin),
}

/// The begin half's stages, in the order they run.
const STAGES: [Stage; 19] = [
    // Read the local player: the table's and the hold's tick, the weapons in the hands, the hand
    // bodies' guns, the sockets, and the long guns on the shoulders.
    Stage {
        name: "player",
        run: read::player,
    },
    // Observe the guns in the hands (the scope, the ammo readout, the grip watch, the heft), and
    // reserve the triggers of the hands on a gun.
    Stage {
        name: "guns",
        run: read::guns,
    },
    // Measure each long gun on its wrist, and pick the lone gun and the braced long gun.
    Stage {
        name: "geometry",
        run: read::geometry,
    },
    // Hand the guns to the regions, the sights, and the collision.
    Stage {
        name: "gun observers",
        run: read::gun_observers,
    },
    // Place the body the volumes ride.
    Stage {
        name: "body",
        run: targets::body,
    },
    // Offer a held explosive to the other hand, and the engaged foregrip to its hand.
    Stage {
        name: "hand offers",
        run: targets::hand_offers,
    },
    Stage {
        name: "sockets",
        run: targets::sockets,
    },
    Stage {
        name: "chest pouches",
        run: targets::chest_pouches,
    },
    Stage {
        name: "pouch return",
        run: targets::pouch_return,
    },
    Stage {
        name: "docked guns",
        run: targets::docked_guns,
    },
    Stage {
        name: "grenades",
        run: targets::grenades,
    },
    Stage {
        name: "placed charges",
        run: targets::placed_charges,
    },
    Stage {
        name: "world weapons",
        run: targets::world_weapons,
    },
    Stage {
        name: "reload",
        run: targets::reload,
    },
    Stage {
        name: "pouch models",
        run: targets::pouch_models,
    },
    // The magazine well and the charge's surface.
    Stage {
        name: "marks",
        run: targets::marks,
    },
    // The lone gun's handle and foregrip, where the free hand touches it, and the handover.
    Stage {
        name: "lone gun",
        run: lone::lone_gun,
    },
    // Resolve the targets each hand would take.
    Stage {
        name: "resolve",
        run: resolve::resolve,
    },
    // Publish the tick for the finish half and the readout, and cue the hands.
    Stage {
        name: "publish",
        run: resolve::publish,
    },
];

/// The hands as the gun is shown on them before its recoil: the two-handed pose of a held
/// foregrip, then the weight ([`crate::controllers::weapons::heft::apply`]). The volumes on a held
/// gun (its foregrip, its well, and its action) are placed from these, so that they are where the
/// player sees them.
pub(super) fn weighted_hands(active: &ActiveHands) -> [Option<CockpitHand>; 2] {
    let mut hands = active.hands;
    let tuning = HANDLING.get();
    foregrip::apply(&mut hands, &tuning.foregrip);
    heft::apply(&mut hands, active.head.position, &tuning.heft);
    hands
}

/// The wielded weapon as the hold has it: every gun in the holding hand, which holds the weapon.
/// The table follows on the next placement; this is the hold's current layout.
pub(super) fn with_holder(mut weapon: HeldWeapon, holder: Option<Hand>) -> HeldWeapon {
    if let Some(holder) = holder.filter(|_| hold::owned_holder(weapon.address).is_some())
        && weapon.wielded.hands.contains(&true)
    {
        // A weapon with a gun in each hand keeps both; the layout only mirrors it.
        if weapon.wielded.hands != [true; 2] {
            weapon.wielded.hands = Hand::BOTH.map(|h| h == holder);
        }
        weapon.holder = Some(holder);
    }
    weapon
}

/// Which hands entered a body volume this tick, by [`Hand::index`]: a hand in a socket or a pouch
/// that it was not in on the last tick. Moving straight from one body volume to another counts as
/// entering the new one.
pub(super) fn entered_body_volumes(
    previous: [Option<GrabVolume>; 2],
    now: [Option<GrabVolume>; 2],
) -> [bool; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        now[i].is_some_and(|v| v.on_body() && previous[i] != Some(v))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stages_run_in_their_documented_order() {
        assert_eq!(
            STAGES.map(|stage| stage.name),
            [
                "player",
                "guns",
                "geometry",
                "gun observers",
                "body",
                "hand offers",
                "sockets",
                "chest pouches",
                "pouch return",
                "docked guns",
                "grenades",
                "placed charges",
                "world weapons",
                "reload",
                "pouch models",
                "marks",
                "lone gun",
                "resolve",
                "publish",
            ]
        );
    }
}
