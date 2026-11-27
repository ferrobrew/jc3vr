//! The begin half's target stages: the body the volumes ride, and the grip targets on the hands,
//! the body, and in the world.

use std::time::Instant;

use jc3vr_body::controllers::{
    body::rig::{frames::SkeletonLandmarks, skeleton},
    body_frame,
    grab::{Candidate, GrabVolume, GripTarget, Mark, MarkAt, MarkStyle, Shown},
    hands::ActiveHands,
};
use jc3vr_core::{game_tick::GameTick, published::Stamp};
use jc3vr_input::frame::{Hand, Pose};
use parking_lot::Mutex;

use crate::controllers::{
    loadout::{grab::swaps, live},
    weapons::{
        begin::{
            Begin,
            lone::gun_volume_radius,
            place::{held_foregrip, mark_size},
            read::Ticked,
            socket_candidates as candidates,
        },
        c4,
        config::WeaponSettings,
        docks,
        docks::{SocketUse, attract},
        foregrip, grenade,
        grenade::tuning::GRENADE,
        hold,
        hold::tuning::HOLD,
        pouch_models::{self as models, Prop, ShownPouch},
        pouch_return as returns, regions,
        regions::GunRegion,
        reload as reloads, volumes,
        volumes::{BodyFrame, ChestGrabbers, StrideFilter},
    },
};

/// Place the body the volumes ride: the character's skeleton in the cockpit frame, its belt held
/// steady through a stride while Rico moves, looking where the head does.
pub(super) fn body(_game: &GameTick<'_>, b: &mut Begin) {
    let active = &b.active;
    let moving = {
        let locomotion = body_frame::locomotion();
        locomotion.fresh(Stamp::now()) && !locomotion.standing_still()
    };
    let placed = skeleton::skeleton_in_cockpit(active.head.position, active.world_scale)
        .map(|(landmarks, source)| (stride_steady(landmarks, moving), source))
        .and_then(|(landmarks, source)| Some((BodyFrame::on(landmarks)?, source)));
    skeleton::record_volume_anchor(
        placed.map_or("nothing: no skeleton", |(_, source)| source.anchor()),
    );
    b.body = placed.map(|(body, _)| body.looking(active.head.orientation));
}

/// Offer a held grenade or charge to the other, free hand, and the engaged foregrip to its hand: a
/// held foregrip goes first, so that the bracing hand keeps it over anything as near.
pub(super) fn hand_offers(_game: &GameTick<'_>, b: &mut Begin) {
    let active = &b.active;
    if b.context.modes.guns {
        for holder in Hand::BOTH {
            let taker = holder.other();
            let explosive = grenade::safe::in_hand(holder).is_some()
                || c4::held().is_some_and(|(hand, _)| hand == holder);
            if explosive
                && b.free[taker.index()]
                && let Some(held) = active.hands[holder.index()]
            {
                b.targets.push(b.place.on_hand(
                    &Candidate {
                        volume: GrabVolume::Handle,
                        centre: held.grip.position,
                        radius: HOLD.get().hold.handle_radius_m,
                        hand: Some(taker),
                    },
                    holder,
                ));
            }
        }
    }
    b.hold = foregrip::engaged_hold();
    if let Some(h) = b.hold
        && let Some(target) = held_foregrip(&h, active)
    {
        b.targets.push(target);
    }
}

/// The sockets: the weapons on them for the hands that may draw them, and the free sockets that
/// take a held weapon, with their attractions.
pub(super) fn sockets(_game: &GameTick<'_>, b: &mut Begin) {
    if !b.context.modes.guns {
        return;
    }
    let active = &b.active;
    let place = &b.place;
    let draw = candidates::DrawContext {
        dominant: b.context.settings.dominant,
        occupied: b.free.map(|free| !free),
    };
    let (draws, others) = socket_candidates(&b.ticked, active, draw);
    b.targets.extend(draws.iter().map(|c| place.on_skeleton(c)));
    // A held weapon's reach to the back's free sockets moves off their place; a draw stays on
    // the weapon the hand sees, and the attraction below keeps the place, as it travels the
    // weapon there.
    let snapshot = live::snapshot_now();
    // A taken socket the held weapon swaps onto is marked as a place to put it, not as one
    // that refuses it.
    let swapping = |c: &Candidate| match (c.volume, c.hand, snapshot.as_ref()) {
        (GrabVolume::Socket(socket), Some(hand), Some(snapshot)) => swaps(snapshot, hand, socket),
        _ => false,
    };
    let body = &b.body;
    b.targets.extend(others.iter().map(|(c, usage)| {
        let mut reach = [*c];
        if *usage != SocketUse::Draw
            && let Some(body) = body
        {
            volumes::reach_back_sockets(&mut reach, body);
        }
        match usage {
            SocketUse::NoFit if swapping(c) => place.swap_onto(&reach[0]),
            SocketUse::NoFit => place.no_fit(&reach[0]),
            SocketUse::Draw | SocketUse::Dock => place.on_skeleton(&reach[0]),
        }
    }));
    b.attractors = attract::attractors(
        &others,
        body.as_ref(),
        |p| place.xf.point_to_world(p),
        &HOLD.get().docks.attraction,
    );
    if let Some(snapshot) = &snapshot {
        b.attractors.extend(attract::swap_attractors(
            &others,
            |hand, socket| swaps(snapshot, hand, socket),
            body.as_ref(),
            |p| place.xf.point_to_world(p),
            &HOLD.get().docks.attraction,
        ));
    }
}

/// The grenade and C4 pouches on the chest, each while it has something to give and a hand free
/// to take from it.
pub(super) fn chest_pouches(_game: &GameTick<'_>, b: &mut Begin) {
    let Some(body) = b.body.as_ref().filter(|_| b.context.modes.pouches) else {
        return;
    };
    let pouches = &HOLD.get().volumes.pouches;
    let dominant = b.context.settings.dominant;
    // A hand holding anything takes nothing from the chest.
    let grabbers = ChestGrabbers::for_guns(b.free.map(|free| !free));
    // A pouch with nothing to give is neither offered nor drawn.
    if grenade::stock::in_stock()
        && let Some(pouch) = volumes::grenade_pouch(body, pouches, dominant, grabbers)
    {
        b.targets
            .extend(b.place.pouch(&pouch, pouches.reach_margin_m));
    }
    if c4::pouch_offered()
        && let Some(pouch) = volumes::c4_pouch(body, pouches, dominant, grabbers)
    {
        b.targets
            .extend(b.place.pouch(&pouch, pouches.reach_margin_m));
    }
}

/// Watch for a held grenade or charge pressed back onto its pouch, whatever is in stock and
/// whichever hands may take from the pouches.
pub(super) fn pouch_return(_game: &GameTick<'_>, b: &mut Begin) {
    let dominant = b.context.settings.dominant;
    let return_body = b.body.as_ref().filter(|_| b.context.modes.pouches);
    let as_pouch = |pouch: Candidate| returns::Pouch {
        centre: pouch.centre,
        radius: pouch.radius,
    };
    returns::observe(&returns::Inputs {
        grenade_pouch: return_body
            .and_then(|body| {
                volumes::grenade_pouch(
                    body,
                    &HOLD.get().volumes.pouches,
                    dominant,
                    ChestGrabbers::Either,
                )
            })
            .map(as_pouch),
        charge_pouch: return_body
            .and_then(|body| {
                volumes::c4_pouch(
                    body,
                    &HOLD.get().volumes.pouches,
                    dominant,
                    ChestGrabbers::Either,
                )
            })
            .map(as_pouch),
        grips: b
            .active
            .hands
            .map(|hand| hand.map(|hand| hand.grip.position)),
        grenades: Hand::BOTH.map(grenade::returnable),
        charges: c4::returnable(),
    });
}

/// The long guns docked on the shoulders, for a free hand touching one away from its grip.
pub(super) fn docked_guns(_game: &GameTick<'_>, b: &mut Begin) {
    if !b.context.modes.guns {
        return;
    }
    let empty_handed = b.free == [true; 2];
    let mut docked = docked_candidates(&b.ticked, &b.active, b.free);
    volumes::reserve_long_guns(&mut docked, b.context.settings.dominant, empty_handed);
    b.targets.extend(docked.iter().map(|c| b.place.touch(c)));
}

/// The grenades in the world and the pins of the grenades in the hands.
pub(super) fn grenades(game: &GameTick<'_>, b: &mut Begin) {
    let inputs = grenade::BeginInputs {
        active: &b.active,
        free: b.free,
        acting: b.context.modes.pouches,
    };
    let grenades = grenade::begin_tick(game, &inputs, GRENADE.get());
    b.targets.extend(grenades.iter().map(|c| b.place.world(c)));
    b.targets.extend(
        grenade::pin_candidates(&inputs, GRENADE.get())
            .iter()
            .map(|(holder, c)| b.place.grenade_pin(c, *holder)),
    );
}

/// The planted charges.
pub(super) fn placed_charges(game: &GameTick<'_>, b: &mut Begin) {
    let placed = c4::placed::begin_tick(
        game,
        &c4::placed::BeginInputs {
            active: &b.active,
            free: b.free,
            acting: b.context.modes.pouches,
        },
    );
    b.targets.extend(placed.iter().map(|c| b.place.world(c)));
}

/// The weapons in the world without a body: in characters' hands, on racks, and in lockers.
pub(super) fn world_weapons(game: &GameTick<'_>, b: &mut Begin) {
    let taken = hold::take::begin_tick(
        game,
        &hold::take::TakeInputs {
            active: &b.active,
            free: b.free,
            gameplay: b.context.modes.world,
        },
    );
    b.targets.extend(taken.iter().map(|c| b.place.world(c)));
}

/// The reload's targets: the magazine pouch, the magazines in the world, the inserted magazine and
/// the action on the gun in the other hand, and the belt pouches the gun is pushed onto.
pub(super) fn reload(_game: &GameTick<'_>, b: &mut Begin) {
    let pouches = &HOLD.get().volumes.pouches;
    let reload_candidates = reloads::begin_tick(&reloads::BeginInputs {
        weapons: [b.ticked.weapon.as_ref(), b.ticked.beside.as_ref()],
        // The reload's own round leaves the hand free to the reload, which carries it on.
        occupied: b.ticked.contents.map(|hand| !hand.free_but_for_round()),
        active: &b.active,
        config: &b.context.settings,
        body: b.body.as_ref(),
        gun_side: b.gun_side,
        gameplay: b.context.modes.guns,
    });
    for c in &reload_candidates {
        match (c.volume, c.hand) {
            (GrabVolume::MagazinePouch, _) => {
                b.targets.extend(b.place.pouch(c, pouches.reach_margin_m));
            }
            // The action is on the gun in the other hand.
            (GrabVolume::WeaponAction | GrabVolume::InsertedMagazine, Some(hand)) => {
                b.targets.push(b.place.on_hand(c, hand.other()));
            }
            _ => b.targets.push(b.place.world(c)),
        }
    }
    b.targets
        .extend(reloads::belt_marks().iter().map(|c| b.place.belt_push(c)));
}

/// Draw the models of what the pouches hold.
pub(super) fn pouch_models(game: &GameTick<'_>, b: &mut Begin) {
    let frames = b
        .ticked
        .character
        .and_then(|c| docks::cockpit_frames(&b.active, c));
    let shown = b
        .body
        .as_ref()
        .filter(|_| b.context.modes.guns)
        .map_or_else(Vec::new, |body| {
            let mut shown = shown_props(body, &b.context.settings);
            if !b.context.modes.pouches {
                shown.retain(|prop| matches!(prop.prop, Prop::Magazine));
            }
            shown
        });
    models::input_tick(game, frames, &shown);
}

/// The magazine well's ring and the surface a held charge would be planted on.
pub(super) fn marks(_game: &GameTick<'_>, b: &mut Begin) {
    if let Some((gun_hand, mouth, radius, occupied)) = reloads::well(&reloads::tuning::Reload::of(
        &b.context.settings.weapons.reload,
    )) {
        // The well's ring turns red while its round has to leave before another seats: the hand
        // holding a round sees that the gun will not take it before pushing it home.
        b.targets.push(GripTarget::sphere(
            GrabVolume::MagazineWell,
            Some(gun_hand.other()),
            mouth,
            radius,
            Mark::ring(
                MarkAt::OnHand {
                    hand: gun_hand,
                    point: mouth,
                },
                mark_size(radius),
            )
            .styled(if occupied {
                MarkStyle::NoFitRing
            } else {
                MarkStyle::Ring
            })
            .showing(Shown::Always),
        ));
    }
    if let Some((hand, contact)) = c4::preview() {
        b.targets.push(GripTarget::measured(
            GrabVolume::ChargeSurface,
            hand,
            contact.distance,
            Mark::ring(
                MarkAt::World(contact.point),
                HOLD.get().begin.marks.charge_m,
            )
            .on_surface(contact.normal)
            .showing(Shown::Always),
        ));
    }
}

/// The skeleton the belt rides this tick, its hip joints held steady through a stride while Rico
/// moves ([`StrideFilter`]).
fn stride_steady(landmarks: SkeletonLandmarks, moving: bool) -> SkeletonLandmarks {
    let mut stride = STRIDE.lock();
    let now = Instant::now();
    let dt = stride
        .1
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
    stride.1 = Some(now);
    stride.0.step(landmarks, moving, dt)
}

/// The belt's stride filter and when it last stepped.
static STRIDE: Mutex<(StrideFilter, Option<Instant>)> = Mutex::new((StrideFilter::new(), None));

/// The sockets' grab volumes in the cockpit frame, each its socket's size: the weapons on the
/// sockets for the hands that may draw them, and apart from those, the free sockets that take a
/// held weapon and the ones that do not, for the hand holding it. Each socket is reached where its
/// weapon is seen on the character's body, or where a held weapon would sit on it, as drawn (see
/// [`crate::controllers::weapons::docks`]).
fn socket_candidates(
    ticked: &Ticked,
    active: &ActiveHands,
    draw: candidates::DrawContext,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    candidates::socket_candidates(
        ticked.sockets.as_ref(),
        active,
        ticked.character,
        &HOLD.get().volumes.sockets,
        draw,
    )
}

/// The volumes on the long guns docked on the shoulders, for each free hand that touches one at a
/// part other than its grip, as both are drawn: a grab there draws the gun from its shoulder as the
/// holster does, and the gun slides into the hand by its grip, aligned to shoot.
fn docked_candidates(ticked: &Ticked, active: &ActiveHands, free: [bool; 2]) -> Vec<Candidate> {
    let mut out = Vec::new();
    if ticked.docked_long.is_empty() {
        return out;
    }
    let Some(frames) = ticked
        .character
        .and_then(|c| docks::cockpit_frames(active, c))
    else {
        return out;
    };
    for hand in Hand::BOTH.into_iter().filter(|h| free[h.index()]) {
        let Some(cockpit) = active.hands[hand.index()] else {
            continue;
        };
        let world = frames.cockpit.point_to_world(cockpit.grip.position);
        let touched = ticked
            .docked_long
            .iter()
            .filter_map(|gun| {
                let hit = regions::classify(
                    &gun.shape,
                    regions::into_gun(gun.pose, world, 1.0),
                    HOLD.get().regions.grip_radius_m,
                    HOLD.get().regions.reach_m,
                )?;
                Some((gun, hit))
            })
            .filter(|(_, hit)| hit.region != GunRegion::Grip)
            .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance));
        let Some((gun, hit)) = touched else {
            continue;
        };
        out.push(Candidate {
            volume: GrabVolume::Socket(gun.site),
            centre: frames
                .cockpit
                .point_to_cockpit(regions::out_of_gun(gun.pose, hit.anchor, 1.0)),
            radius: gun_volume_radius(hit.distance / active.world_scale.max(f32::EPSILON)),
            hand: Some(hand),
        });
    }
    out
}

/// The pouches to draw their models on this tick: each pouch with something to give, wherever it
/// is placed, whichever hands are free to take from it.
fn shown_props(body: &BodyFrame, config: &WeaponSettings) -> Vec<ShownPouch> {
    let pouches = &HOLD.get().volumes.pouches;
    let dominant = config.dominant;
    let sides = volumes::chest_sides(dominant);
    let on_chest = |prop: Prop, pouch: Candidate, side: Hand| ShownPouch {
        prop,
        pouch: Pose {
            position: pouch.centre,
            orientation: body.pouch_chest(),
        },
        side,
        weapon: None,
    };
    let mut out = Vec::new();
    if grenade::stock::in_stock()
        && let Some(pouch) = volumes::grenade_pouch(body, pouches, dominant, ChestGrabbers::Either)
    {
        out.push(on_chest(Prop::Grenade, pouch, sides.grenade));
    }
    if c4::in_stock()
        && let Some(pouch) = volumes::c4_pouch(body, pouches, dominant, ChestGrabbers::Either)
    {
        out.push(on_chest(Prop::Charge, pouch, sides.c4));
    } else if !c4::in_stock()
        && c4::detonator_hand().is_none()
        && let Some(pouch) = volumes::c4_pouch(body, pouches, dominant, ChestGrabbers::Either)
    {
        // With no charge left to give, the pouch gives the detonator, so it shows that.
        out.push(on_chest(Prop::Detonator, pouch, sides.c4));
    }
    if let Some(pouch) = reloads::stocked_pouch() {
        out.push(ShownPouch {
            prop: Prop::Magazine,
            pouch: Pose {
                position: pouch.centre,
                orientation: body.pelvis,
            },
            side: pouch.side,
            weapon: Some(pouch.weapon),
        });
    }
    out
}
