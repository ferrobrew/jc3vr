//! The first half of the weapon handling's input tick: read the weapons in the hands, and provide
//! the weapon handling's grip targets ([`crate::controllers::grab`]): the sockets and pouches on
//! the body, the held gun's parts, the weapons and grenades in the world, and the grenade's pin.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::equipment::weapon::WeaponBase;

use crate::{
    config::Config,
    controllers::{
        body::rig::skeleton,
        config::ControllersConfig,
        frame::{Hand, Pose},
        grab::{self, Candidate, GrabVolume, GripTarget, Mark, MarkAt, Shown},
        hands::{self, ActiveHands, CockpitHand, ik, pose as hand_pose},
        haptics::{self, Event},
        loadout::{LoadoutSnapshot, grab::GrabRules, live},
        mode::{AirborneVariant, Mode, OnFootVariant},
        physical_hands::physics::guns,
        weapons::{
            ads,
            begin::place::{CHARGE_MARK_M, Place, held_foregrip, mark_size},
            c4,
            capabilities::{Capabilities, gun_hands_now, tracked_hands, trigger_claims},
            collide,
            docks::{self, SocketInputs, SocketUse, attachments::Sample},
            engine::{self, HeldWeapon},
            foregrip, grenade, grip_watch, heft,
            hold::{self, armed},
            inventory,
            pouch_models::{self, Prop, ShownPouch},
            regions::{
                self, Carry, GunRegion, GunShape, Observed,
                config::RegionsConfig,
                engine::{self as region_engine, DockedLongGun},
            },
            reload, scope,
            state::{DockTouch, OffTouch, STATE, State},
            types::{TickStart, WieldedReadout},
            volumes::{self, BodyFrame, ChestGrabbers},
        },
    },
};

mod place;
mod socket_candidates;

/// The first half of the weapon handling's input tick: read the weapons in the hands and return
/// the weapon handling's grip targets. Called while the bindings inject, before the tables.
pub(crate) fn begin_tick(mode: Mode) -> TickStart {
    let config = Config::lock_query(|c| c.controllers);
    docks::publish_targets(None);
    let Some(active) = hands::active_hands_raw() else {
        crate::controllers::weapons::stand_down();
        return TickStart::default();
    };
    let gameplay = gameplay(mode);
    let capabilities = Capabilities::of(mode);
    let holding = config.weapons.hold;
    // SAFETY: the input tick runs on the main thread outside the world update.
    let ticked = unsafe {
        engine::local_player().map_or(Ticked::default(), |owner| {
            armed::show_magazines(Some(owner), capabilities.reload && holding.magazine_pips);
            // The table lands the wielded weapon in a hand first; the hold then follows it there.
            let wielded = hold::wielded(owner);
            let arrival = inventory::input_tick(
                owner,
                &inventory::TickInputs {
                    settings: config.weapons.inventory,
                    dominant: config.acting_hand(),
                    wielded,
                    separate_pistols: config.weapons.separate_pistols,
                },
            );
            let holder = hold::input_tick(wielded, holding.either_hand, arrival);
            hold::tick::input_tick(owner, wielded, holding.either_hand && holding.per_hand_tick);
            let weapon = engine::held_weapon(owner).map(|w| with_holder(w, holder));
            let guns = engine::in_hands(weapon, Hand::BOTH.map(|h| armed::held_weapon(owner, h)));
            // The gun a hand holds beside the wielded weapon, armed there.
            let beside = Hand::BOTH
                .into_iter()
                .filter_map(|h| guns[h.index()])
                .find(|w| weapon.is_none_or(|wielded| wielded.address != w.address));
            // The guns the hands hold, for their bodies.
            guns::publish(if capabilities.support {
                guns::read_holds([
                    weapon.map(|w| (w.address, w.class)),
                    beside.map(|w| (w.address, w.class)),
                ])
            } else {
                guns::GunHolds::default()
            });
            let snapshot = live::snapshot_now();
            let table_hands = snapshot.as_ref().map_or([false; 2], hands_holding);
            let gun_hands = weapon.map_or([false; 2], |w| w.wielded.hands);
            let free = Hand::BOTH.map(|h| !table_hands[h.index()] && !gun_hands[h.index()]);
            let sockets = snapshot
                .as_ref()
                .filter(|_| capabilities.dock && config.weapons.holsters.enabled)
                .map(|snapshot| {
                    docks::socket_points(
                        owner,
                        &SocketInputs {
                            snapshot,
                            free,
                            rules: GrabRules {
                                both_hands: holding.either_hand && holding.second_weapon,
                            },
                            offsets: &config.weapons.inventory.offsets,
                        },
                    )
                });
            let docked_long = match &snapshot {
                Some(snapshot)
                    if config.weapons.regions.enabled
                        && config.weapons.holsters.enabled
                        && capabilities.dock =>
                {
                    region_engine::docked_long_guns(owner, snapshot, config.acting_hand())
                }
                _ => Vec::new(),
            };
            Ticked {
                weapon,
                guns,
                beside,
                table_hands,
                sockets,
                character: Some(Mat4::from(owner.m_WorldMatrixT1)),
                docked_long,
            }
        })
    };
    let (weapon, beside) = (ticked.weapon, ticked.beside);
    scope::input_tick(ticked.guns.map(|g| g.map(|w| w.address)));
    // SAFETY: the weapon was read this tick, on the main thread outside the world update.
    if let Some(w) = weapon
        && let Some(base) = unsafe { (w.address as *const WeaponBase).as_ref() }
    {
        unsafe { grip_watch::observe(base, w.class) };
    }
    let guns = ticked.guns;
    heft::observe(Hand::BOTH.map(|hand| {
        guns[hand.index()]
            .filter(|_| capabilities.support)
            .map(|w| heft::HeftTarget {
                hand,
                class: w.class,
                weapon: w.address,
            })
    }));
    let armed_hands = Hand::BOTH.map(|h| armed::in_hand(h).is_some());
    let gun_hands = weapon.map_or([false; 2], |w| w.wielded.hands);
    let free = Hand::BOTH.map(|h| {
        !gun_hands[h.index()] && !armed_hands[h.index()] && !ticked.table_hands[h.index()]
    });
    let claims = trigger_claims(capabilities, gun_hands_now(), tracked_hands());
    let held = claims
        .iter()
        .any(|claim| claim.reserved)
        .then(|| claims.map(|claim| claim.reserved));

    let mut state = STATE.lock();
    let holder = weapon.and_then(|w| w.holder);
    let gun_side = volumes::gun_side(holder, state.last_holder, config.acting_hand());
    state.last_holder = holder.or(state.last_holder);
    state.gun_side = gun_side;
    // Each hand's long gun is measured on its own wrist.
    let geometries = Hand::BOTH.map(|hand| {
        guns[hand.index()]
            .filter(|w| w.two_handed && capabilities.support)
            .and_then(|w| state.geometry_for(hand, &w, &config, active.world_scale))
    });
    let geometry = weapon
        .and_then(|w| w.holder)
        .and_then(|hand| geometries[hand.index()]);
    // The gun held in one hand while the other is free, wielded or armed: everything the free hand
    // does to it (the handover, and on a long gun the foregrip and a touch along it) acts on it.
    let lone =
        engine::lone_gun(guns, free).filter(|_| capabilities.transfer || capabilities.support);
    let braced = lone.filter(|w| w.two_handed);
    let braced_geometry = braced
        .and_then(|w| w.holder)
        .and_then(|hand| geometries[hand.index()]);
    let regions_config = config.weapons.regions;
    let long = braced.filter(|_| regions_config.enabled);
    // SAFETY: the weapon was read this tick, on the main thread outside the world update.
    let shape = long.and_then(|w| unsafe {
        region_engine::shape(
            (w.address as *const WeaponBase).as_ref()?,
            w.holder.unwrap_or(config.acting_hand()),
        )
    });
    regions::observe(&Observed {
        guns: guns.map(|g| {
            g.filter(|w| regions_config.enabled && capabilities.support && w.two_handed)
                .map(|w| w.address)
        }),
        lone: long.map(|w| (w.address, shape)),
        now: Instant::now(),
        draw_wait: seconds(regions_config.draw_wait_s),
    });
    let carried = braced.and_then(|w| regions::carrying(w.address));
    // A gun carried by a part other than its grip is not aimed.
    ads::observe(Hand::BOTH.map(|hand| {
        let w = guns[hand.index()]?;
        let g = geometries[hand.index()]?;
        regions::carrying(w.address)
            .is_none()
            .then(|| ads::AdsWeapon {
                holder: hand,
                sight: ads::solve::WristSight {
                    origin: g.sight,
                    direction: g.barrel,
                },
                scoped: scope::is_scoped(w.address),
            })
    }));
    collide::observe(Hand::BOTH.map(|hand| {
        geometries[hand.index()].map(|g| collide::CollideWeapon {
            holder: hand,
            muzzle: g.muzzle,
            barrel: g.barrel,
        })
    }));

    let body = skeleton::skeleton_in_cockpit(active.head.position, active.world_scale)
        .and_then(BodyFrame::on);
    skeleton::record_volume_anchor(if body.is_some() {
        "the character's skeleton"
    } else {
        "nothing: no skeleton"
    });
    let pouches = &config.weapons.pouches;
    let place = Place::new(&active);
    // A held foregrip goes first, so that the bracing hand keeps it over anything as near.
    let mut targets = Vec::new();
    if capabilities.transfer && holding.handover {
        for holder in Hand::BOTH {
            let taker = holder.other();
            if grenade::safe::in_hand(holder).is_some()
                && free[taker.index()]
                && let Some(held) = active.hands[holder.index()]
            {
                targets.push(place.on_hand(
                    &Candidate {
                        volume: GrabVolume::Handle,
                        centre: held.grip.position,
                        radius: holding.handle_radius_m.max(0.0),
                        hand: Some(taker),
                    },
                    holder,
                ));
            }
        }
    }
    let hold = foregrip::status().hold.filter(|h| h.engaged);
    if let Some(h) = hold
        && let Some(target) = held_foregrip(&h, &active, &config)
    {
        targets.push(target);
    }
    let empty_handed = config.weapons.holsters.long_guns_dominant
        && gun_hands == [false; 2]
        && ticked.table_hands == [false; 2];
    if capabilities.dock {
        let draw = socket_candidates::DrawContext {
            dominant: config.acting_hand(),
            occupied: Hand::BOTH
                .map(|hand| gun_hands[hand.index()] || ticked.table_hands[hand.index()]),
        };
        let (draws, others) = socket_candidates(&ticked, &active, body.as_ref(), &config, draw);
        state.status.docks_followed = if config.weapons.holsters.follow_docks {
            draws.len()
        } else {
            0
        };
        targets.extend(draws.iter().map(|c| place.on_skeleton(c)));
        targets.extend(others.iter().map(|(c, usage)| match usage {
            SocketUse::NoFit => place.no_fit(c),
            SocketUse::Draw | SocketUse::Dock => place.on_skeleton(c),
        }));
    }
    if gameplay && let Some(body) = &body {
        // A hand holding a gun, a prop, or an explosive takes nothing from the chest.
        let grabbers = ChestGrabbers::for_guns(Hand::BOTH.map(|h| {
            gun_hands[h.index()] || armed_hands[h.index()] || ticked.table_hands[h.index()]
        }));
        // A pouch with nothing to give is neither offered nor drawn.
        if config.weapons.grenade_throw
            && grenade::in_stock()
            && let Some(pouch) = volumes::grenade_pouch(body, pouches, gun_side, grabbers)
        {
            targets.extend(place.pouch(&pouch, pouches.reach_margin_m));
        }
        if config.weapons.c4_throw
            && c4::in_stock()
            && let Some(pouch) = volumes::c4_pouch(body, pouches, gun_side, grabbers)
        {
            targets.extend(place.pouch(&pouch, pouches.reach_margin_m));
        }
    }
    state.docked_touch = [None; 2];
    if capabilities.dock {
        let mut docked = docked_candidates(&mut state, &ticked, &active, free, &regions_config);
        volumes::reserve_long_guns(&mut docked, config.acting_hand(), empty_handed);
        targets.extend(docked.iter().map(|c| place.touch(c)));
    }
    if config.weapons.grenade_throw {
        let charge = c4::throwing_hand();
        // SAFETY: the input tick runs on the main thread outside the world update.
        let grenades = unsafe {
            grenade::begin_tick(
                &grenade::BeginInputs {
                    active: &active,
                    free: Hand::BOTH
                        .map(|h| free[h.index()] && !grenade::holds(h) && charge != Some(h)),
                    acting: gameplay,
                },
                &config.weapons.grenade,
            )
        };
        targets.extend(grenades.iter().map(|c| place.world(c)));
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let taken = unsafe {
        hold::take::begin_tick(&hold::take::TakeInputs {
            active: &active,
            settings: &holding,
            free,
            gameplay,
        })
    };
    targets.extend(taken.iter().map(|c| place.world(c)));
    let reload_candidates = reload::begin_tick(&reload::BeginInputs {
        weapons: [weapon.as_ref(), beside.as_ref()],
        occupied: free.map(|f| !f),
        active: &active,
        config: &config,
        body: body.as_ref(),
        gun_side,
        gameplay: capabilities.reload,
    });
    for c in &reload_candidates {
        match (c.volume, c.hand) {
            (GrabVolume::MagazinePouch, _) => {
                targets.extend(place.pouch(c, pouches.reach_margin_m));
            }
            // The action is on the gun in the other hand.
            (GrabVolume::WeaponAction | GrabVolume::InsertedMagazine, Some(hand)) => {
                targets.push(place.on_hand(c, hand.other()));
            }
            _ => targets.push(place.world(c)),
        }
    }
    let frames = ticked
        .character
        .and_then(|c| docks::cockpit_frames(&active, c));
    let shown = body
        .as_ref()
        .filter(|_| capabilities.reload)
        .map_or_else(Vec::new, |body| {
            let mut shown = shown_props(body, &config, gun_side);
            if !gameplay {
                shown.retain(|prop| matches!(prop.prop, Prop::Magazine));
            }
            shown
        });
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { pouch_models::input_tick(&pouches.models, frames, &shown) };
    if let Some((gun_hand, mouth, radius)) = reload::well(&config.weapons.reload) {
        targets.push(GripTarget::sphere(
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
            .showing(Shown::Always),
        ));
    }
    if let Some((hand, contact)) = c4::preview() {
        targets.push(GripTarget::measured(
            GrabVolume::ChargeSurface,
            hand,
            contact.distance,
            Mark::ring(MarkAt::World(contact.point), CHARGE_MARK_M)
                .on_surface(contact.normal)
                .showing(Shown::Always),
        ));
    }
    // A hand holding something of its own braces nothing and takes nothing from the other hand, so
    // only the lone gun's free hand does.
    let foregrip_hand = braced.and_then(|w| w.holder).map(Hand::other);
    let handover_allowed = capabilities.transfer
        && holding.handover
        && lone.is_some_and(|w| hold::owned_holder(w.address).is_some());
    let on_gun = match (long, braced_geometry, shape) {
        (Some(w), Some(g), Some(shape)) => w.holder.and_then(|holder| {
            Some(GunOnHand {
                hand: weighted_hands(&active)[holder.index()]?,
                geometry: g,
                shape,
            })
        }),
        _ => None,
    };
    let holder = braced.and_then(|w| w.holder);
    state.touch = None;
    state.slide = None;
    state.promote = None;
    match on_gun {
        Some(gun) => {
            let touch = foregrip_hand
                .filter(|_| hold.is_none())
                .and_then(|off| off_touch(&gun, off, &active, &regions_config));
            state.touch = touch.map(|(touch, _)| touch);
            match (touch, holder) {
                (Some((touch, centre)), Some(holder)) if touch.hit.region == GunRegion::Grip => {
                    if handover_allowed {
                        targets.push(place.on_hand(
                            &Candidate {
                                volume: GrabVolume::Handle,
                                centre,
                                radius: gun_volume_radius(touch.hit.distance),
                                hand: Some(touch.hand),
                            },
                            holder,
                        ));
                    }
                }
                (Some((touch, centre)), Some(holder)) if config.weapons.foregrip.enabled => {
                    targets.push(place.on_hand(
                        &Candidate {
                            volume: GrabVolume::Foregrip,
                            centre,
                            radius: gun_volume_radius(touch.hit.distance),
                            hand: Some(touch.hand),
                        },
                        holder,
                    ));
                }
                _ => {}
            }
            if let Some(h) = hold.filter(|h| braced.is_some_and(|w| w.address == h.weapon)) {
                let carrying = carried.is_some();
                match brace_moves(&gun, h.holder.other(), &active, &regions_config, carrying) {
                    Some(BraceMove::OntoGrip) => state.promote = Some(h.holder.other()),
                    // A pump the reload has out sets where the hand is along the barrel.
                    Some(BraceMove::Slide(point, region)) if !foregrip::pumping(h.weapon) => {
                        state.slide = Some((point, region));
                    }
                    Some(BraceMove::Slide(..)) | None => {}
                }
            }
        }
        None => {
            if config.weapons.foregrip.enabled
                && hold.is_none()
                && let (Some(g), Some(w), Some(off)) = (braced_geometry, braced, foregrip_hand)
                && let Some(holder) = w.holder
                && let Some(holding_hand) = weighted_hands(&active)[holder.index()]
            {
                targets.push(place.on_hand(
                    &Candidate {
                        volume: GrabVolume::Foregrip,
                        centre: foregrip::foregrip_point(&holding_hand, &g),
                        radius: config.weapons.foregrip.radius_m,
                        hand: Some(off),
                    },
                    holder,
                ));
            }
        }
    }
    if handover_allowed
        && hold.is_none()
        && let Some(w) = lone
        && let Some(holder) = hold::owned_holder(w.address)
        && let Some(current) = weighted_hands(&active)[holder.index()]
    {
        // SAFETY: The owned held weapon is read on the main thread outside the world update.
        let grip = unsafe {
            hold::take::measured_handle_point(w.address, holder, &current, active.world_scale)
        }
        .unwrap_or(current.grip.position);
        if let Some(candidate) =
            hold::world::broad_handover_candidate(hold::world::HandoverPolicy {
                allowed: handover_allowed,
                engaged_foregrip: hold.is_some(),
                guns,
                offer: hold::world::Handover {
                    holder,
                    grip,
                    radius: holding.handle_radius_m,
                    free,
                    points: active.hands.map(|hand| hand.map(|hand| hand.grip.position)),
                    contact: state.touch.as_ref().map(|touch| touch.hit.region),
                },
            })
        {
            targets.push(place.on_hand(&candidate, holder));
        }
    }

    // The weapon handling's own view of where each hand is, for the release's docking, the heft,
    // and the cue on entering a socket.
    let grips = grab::tick::hand_grips(&active);
    reload::prioritize_extraction(&mut targets, grips);
    let resolved = grab::resolve::resolve(&targets, grips);
    let volumes = resolved.map(|r| r.chosen.map(|c| c.volume));
    let closing_on_foregrip = hold.is_none()
        && foregrip_hand.is_some_and(|h| volumes[h.index()] == Some(GrabVolume::Foregrip));
    // A hand carrying a round holds still the gun in the other hand, whose well it reaches for, and
    // a hand closing on a foregrip holds still the gun it closes on.
    let carrying_round = reload::holding_hand();
    heft::freeze(Hand::BOTH.map(|hand| {
        carrying_round == Some(hand.other())
            || (closing_on_foregrip && foregrip_hand == Some(hand.other()))
    }));

    state.status.active = true;
    state.status.volumes = volumes;
    state.status.candidates = targets.len();
    state.placed.clone_from(&targets);
    state.points = grips;
    state.status.wielded = weapon.map(|weapon| WieldedReadout { weapon, geometry });
    state.weapon = weapon.map(|w| (w, geometry));
    state.lone = lone.map(|w| (w, braced_geometry));
    state.beside = beside;
    state.gameplay = gameplay;
    let entered = entered_body_volumes(state.last_volumes, volumes);
    state.last_volumes = volumes;
    drop(state);

    for hand in Hand::BOTH.into_iter().filter(|h| entered[h.index()]) {
        haptics::play(hand, Event::VolumeEnter);
    }
    TickStart { targets, held }
}

/// What the weapon handling read of the local player on the input tick.
#[derive(Default)]
struct Ticked {
    weapon: Option<HeldWeapon>,
    /// The gun in each hand, by [`Hand::index`]: the wielded weapon and the ones armed beside it.
    guns: [Option<HeldWeapon>; 2],
    /// The gun a hand holds beside the wielded weapon.
    beside: Option<HeldWeapon>,
    /// Which hands the loadout table has something in, by [`Hand::index`].
    table_hands: [bool; 2],
    /// Native socket observations and hand offers from this input tick.
    sockets: Option<Sample>,
    /// The character's model-to-world transform.
    character: Option<Mat4>,
    /// The long guns docked on the shoulders, for a draw at any part of one.
    docked_long: Vec<DockedLongGun>,
}

/// The lone long gun on its holding hand, as the regions place it.
struct GunOnHand {
    /// The holding hand as the gun is shown on it.
    hand: CockpitHand,
    geometry: foregrip::GripGeometry,
    shape: GunShape,
}

impl GunOnHand {
    /// The gun's own frame in the cockpit frame, with the position in cockpit metres.
    fn pose(&self) -> Pose {
        hand_pose::mul(self.hand.wrist, self.geometry.gun)
    }
}

/// Where `hand` (the hand without the gun) touches the gun, and the centre of its volume in the
/// cockpit frame.
fn off_touch(
    gun: &GunOnHand,
    hand: Hand,
    active: &ActiveHands,
    config: &RegionsConfig,
) -> Option<(OffTouch, Vec3)> {
    let point = active.hands[hand.index()]?.grip.position;
    let scale = active.world_scale;
    let pose = gun.pose();
    let hit = regions::classify(
        &gun.shape,
        regions::into_gun(pose, point, scale),
        config.grip_radius_m,
        config.reach_m,
    )?;
    let touch = OffTouch {
        hand,
        hit,
        anchor_in_wrist: regions::out_of_gun(gun.geometry.gun, hit.anchor, scale),
    };
    Some((touch, regions::out_of_gun(pose, hit.anchor, scale)))
}

/// How a bracing hand moved along the gun.
enum BraceMove {
    /// Onto the grip of a gun the other hand carries.
    OntoGrip,
    /// Along the handguard or the barrel, to this point in the holding wrist's frame.
    Slide(Vec3, GunRegion),
}

/// How the bracing hand `hand` has moved along the gun, from where its controller is: a hand that
/// reaches the grip of a gun the holding hand carries takes it, and one on the handguard or the
/// barrel slides along them.
fn brace_moves(
    gun: &GunOnHand,
    hand: Hand,
    active: &ActiveHands,
    config: &RegionsConfig,
    carrying: bool,
) -> Option<BraceMove> {
    let point = active.hands[hand.index()]?.grip.position;
    let hit = regions::classify(
        &gun.shape,
        regions::into_gun(gun.pose(), point, active.world_scale),
        config.grip_radius_m,
        config.reach_m,
    )?;
    if hit.region == GunRegion::Grip {
        return carrying.then_some(BraceMove::OntoGrip);
    }
    hit.region.slides().then(|| {
        BraceMove::Slide(
            regions::out_of_gun(gun.geometry.gun, hit.anchor, active.world_scale),
            hit.region,
        )
    })
}

/// The radius of a volume on a gun centred where a hand `distance` away closes on it: the hand is
/// just inside it, so any volume the hand is well inside of wins over the gun (the action's, while
/// the chamber waits on it).
fn gun_volume_radius(distance: f32) -> f32 {
    (distance.max(0.0) + GUN_VOLUME_MARGIN_M) / GUN_VOLUME_DEPTH
}

/// How far past the hand a volume on a gun reaches, metres.
const GUN_VOLUME_MARGIN_M: f32 = 0.01;
/// How deep in a volume on a gun the hand is, as a share of its radius.
const GUN_VOLUME_DEPTH: f32 = 0.95;

/// The volumes on the long guns docked on the shoulders, for each free hand that touches one at a
/// part other than its grip, as both are drawn: a grab there draws the gun from its shoulder as the holster does, and
/// the hand takes it where it touched it ([`regions::pend_draw`]). Notes each touch in `state`.
fn docked_candidates(
    state: &mut State,
    ticked: &Ticked,
    active: &ActiveHands,
    free: [bool; 2],
    config: &RegionsConfig,
) -> Vec<Candidate> {
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
    let targets = ik::tick_records().current;
    for hand in Hand::BOTH.into_iter().filter(|h| free[h.index()]) {
        let Some(cockpit) = active.hands[hand.index()] else {
            continue;
        };
        let world = frames.cockpit_to_world(cockpit.grip.position);
        let wrist = targets[hand.index()].map_or(frames.view * cockpit.wrist.orientation, |t| {
            t.wrist.orientation
        });
        let touched = ticked
            .docked_long
            .iter()
            .filter_map(|gun| {
                let hit = regions::classify(
                    &gun.shape,
                    regions::into_gun(gun.pose, world, 1.0),
                    config.grip_radius_m,
                    config.reach_m,
                )?;
                Some((gun, hit))
            })
            .filter(|(_, hit)| hit.region != GunRegion::Grip)
            .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance));
        let Some((gun, hit)) = touched else {
            continue;
        };
        state.docked_touch[hand.index()] = Some(DockTouch {
            site: gun.site,
            weapon: gun.weapon,
            carry: Carry {
                region: hit.region,
                anchor: hit.anchor,
                turn: regions::turn_on_wrist(wrist, gun.pose.orientation),
            },
        });
        out.push(Candidate {
            volume: GrabVolume::Socket(gun.site),
            centre: frames.world_to_cockpit(regions::out_of_gun(gun.pose, hit.anchor, 1.0)),
            radius: gun_volume_radius(hit.distance / active.world_scale.max(f32::EPSILON)),
            hand: Some(hand),
        });
    }
    out
}

/// A duration of `s` seconds, zero for a value that is not a finite positive number.
fn seconds(s: f32) -> Duration {
    Duration::try_from_secs_f32(s).unwrap_or(Duration::ZERO)
}

/// The pouches to draw their models on this tick: each pouch with something to give, wherever it
/// is placed, whichever hands are free to take from it.
fn shown_props(body: &BodyFrame, config: &ControllersConfig, gun_side: Hand) -> Vec<ShownPouch> {
    let pouches = &config.weapons.pouches;
    let sides = volumes::chest_sides(pouches, gun_side);
    let on_chest = |prop: Prop, pouch: Candidate, side: Hand| ShownPouch {
        prop,
        pouch: Pose {
            position: pouch.centre,
            orientation: body.chest,
        },
        side,
        weapon: None,
    };
    let mut out = Vec::new();
    if config.weapons.grenade_throw
        && grenade::in_stock()
        && let Some(pouch) = volumes::grenade_pouch(body, pouches, gun_side, ChestGrabbers::Either)
    {
        out.push(on_chest(Prop::Grenade, pouch, sides.grenade));
    }
    if config.weapons.c4_throw
        && c4::in_stock()
        && let Some(pouch) = volumes::c4_pouch(body, pouches, gun_side, ChestGrabbers::Either)
    {
        out.push(on_chest(Prop::Charge, pouch, sides.c4));
    }
    if let Some(pouch) = reload::stocked_pouch() {
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

/// Which hands hold an item of `snapshot`'s, by [`Hand::index`].
fn hands_holding(snapshot: &LoadoutSnapshot) -> [bool; 2] {
    Hand::BOTH.map(|hand| snapshot.held(hand).is_some())
}

/// The sockets' grab volumes in the cockpit frame, each its socket's size: the weapons on the
/// sockets for the hands that may draw them, and apart from those, the free sockets that take a
/// held weapon and the ones that do not, for the hand holding it. Off
/// [`HolstersConfig::follow_docks`](crate::controllers::weapons::config::HolstersConfig::follow_docks),
/// a socket with a landmark on the body (the hips, the shoulders, and the chest) is reached there
/// rather than where its weapon is seen.
fn socket_candidates(
    ticked: &Ticked,
    active: &ActiveHands,
    body: Option<&BodyFrame>,
    config: &ControllersConfig,
    draw: socket_candidates::DrawContext,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    socket_candidates::socket_candidates(
        ticked.sockets.as_ref(),
        active,
        ticked.character,
        body,
        &config.weapons.holsters,
        draw,
    )
}

/// Whether the explosive pouches and world-weapon pickup act in `mode`. Held-gun interactions use
/// [`Capabilities`] independently, including in the wingsuit, vehicle seats, and swimming.
pub(super) fn gameplay(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::OnFoot(OnFootVariant::Ground)
            | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute)
    )
}

/// The hands as the gun is shown on them before its recoil: the two-handed pose of a held
/// foregrip, then the weight ([`crate::controllers::weapons::heft::apply`]). The volumes on a held
/// gun (its foregrip, its well, and its action) are placed from these, so that they are where the
/// player sees them.
pub(super) fn weighted_hands(active: &ActiveHands) -> [Option<CockpitHand>; 2] {
    let mut hands = active.hands;
    foregrip::apply(&mut hands, &active.config.weapons.foregrip);
    heft::apply(
        &mut hands,
        active.head.position,
        &active.config.weapons.heft,
    );
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
        now[i].is_some_and(|v| is_body_volume(v) && previous[i] != Some(v))
    })
}

/// Whether `volume` is placed on the body: a socket or a pouch.
fn is_body_volume(volume: GrabVolume) -> bool {
    matches!(
        volume,
        GrabVolume::Socket(_)
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::C4Pouch
    )
}
