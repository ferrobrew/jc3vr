//! The first half of the weapon handling's input tick: read the wielded weapon, place the pistols,
//! and provide the weapon handling's grip targets ([`crate::controllers::grab`]): the sockets and
//! pouches on the body, the held gun's parts, the weapons and grenades in the world, the grenade's
//! pin, and the carried weapons' sockets.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponBase};

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        body::rig::skeleton,
        frame::{Hand, Pose},
        grab::{self, Candidate, GrabVolume, GripTarget, HolsterSite, Mark, MarkAt, Shown},
        hands::{self, ActiveHands, CockpitHand, ik, pose as hand_pose},
        haptics::{self, Event},
        mode::{AirborneVariant, Mode, OnFootVariant},
        physical_hands::physics::guns,
        weapons::{
            ads,
            begin::place::{CHARGE_MARK_M, Place, held_foregrip, mark_size},
            c4, collide,
            docks::{self, HolsterGun},
            dual_wield,
            engine::{self, HeldWeapon},
            foregrip, grenade, grip_watch, heft,
            hold::{self, second},
            inventory::{self, actions::SocketPoint},
            regions::{
                self, Carry, GunRegion, GunShape, Observed,
                config::RegionsConfig,
                engine::{self as region_engine, DockedLongGun},
            },
            reload::{self, focus::ReloadTarget},
            scope,
            state::{DockTouch, OffTouch, STATE, State},
            types::{TickStart, WieldedReadout},
            volumes::{self, BodyFrame, ChestGrabbers},
        },
    },
};

mod place;

/// The first half of the weapon handling's input tick: read the weapon, place the pistols, and
/// return the weapon handling's grip targets and whether the dual-wield split acts. Called while
/// the bindings inject, before the tables.
pub(crate) fn begin_tick(mode: Mode) -> TickStart {
    let config = Config::lock_query(|c| c.controllers);
    let Some(active) = hands::active_hands_raw() else {
        crate::controllers::weapons::stand_down();
        return TickStart::default();
    };
    let gameplay = gameplay(mode);
    let dual = config.weapons.dual_wield;
    let settings = (dual.split && (dual.independent_draw || dual.place_by_handedness)).then_some(
        dual_wield::attach::Settings {
            dominant: config.acting_hand(),
            independent: dual.independent_draw,
            by_handedness: dual.place_by_handedness,
        },
    );
    let holding = config.weapons.hold;
    // A weapon the game wields on its own goes to the hand without the second weapon.
    let second_hand = second::hand();
    let hold_settings = holding.either_hand.then_some(hold::Settings {
        fallback: second_hand.map_or(config.acting_hand(), Hand::other),
    });
    // SAFETY: the input tick runs on the main thread outside the world update.
    let ticked = unsafe {
        engine::local_player().map_or(Ticked::default(), |owner| {
            if holding.either_hand && holding.second_weapon && gameplay {
                second::validate(owner);
                if holding.promote_second {
                    let wielded = gun::held_weapon(owner).map(|w| w as *const WeaponBase as usize);
                    second::promote_tick(owner, wielded, holding.promote_after_s);
                }
            } else {
                second::stand_down();
            }
            second::show_magazine(Some(owner), gameplay && holding.magazine_pips);
            let pair = dual_wield::attach::input_tick(owner, settings);
            let holder = hold::input_tick(owner, hold_settings, pair.is_some());
            let weapon =
                engine::held_weapon(owner).map(|w| with_holder(with_pair(w, pair), holder));
            let second = second::held_weapon(owner);
            // The guns the hands hold, for their bodies.
            guns::publish(if gameplay {
                guns::read_holds([
                    weapon.map(|w| (w.address, w.class)),
                    second.map(|w| (w.address, w.class)),
                ])
            } else {
                guns::GunHolds::default()
            });
            inventory::input_tick(
                owner,
                &inventory::TickInputs {
                    settings: config.weapons.inventory,
                    dominant: config.acting_hand(),
                    wielded: weapon.map(|w| (w.address, w.holder)),
                    driven: second.and_then(|w| Some((w.address, w.holder?))),
                    separate_pistols: config.weapons.separate_pistols,
                },
            );
            Ticked {
                weapon,
                second,
                holster_guns: config
                    .weapons
                    .holsters
                    .follow_docks
                    .then(|| docks::holster_guns(owner, config.acting_hand())),
                sockets: inventory::actions::socket_points(
                    owner,
                    &config.weapons.inventory,
                    config.acting_hand(),
                    held_by_hand(weapon, second),
                ),
                character: Some(Mat4::from(owner.m_WorldMatrixT1)),
                docked_long: if config.weapons.regions.enabled
                    && config.weapons.holsters.enabled
                    && gameplay
                {
                    region_engine::docked_long_guns(owner, config.acting_hand())
                } else {
                    Vec::new()
                },
            }
        })
    };
    let (weapon, second_weapon) = (ticked.weapon, ticked.second);
    scope::input_tick(weapon.map(|w| w.address));
    // SAFETY: the weapon was read this tick, on the main thread outside the world update.
    if let Some(w) = weapon
        && let Some(base) = unsafe { (w.address as *const WeaponBase).as_ref() }
    {
        unsafe { grip_watch::observe(base, w.class) };
    }
    heft::observe(weapon.filter(|_| gameplay).and_then(|w| {
        Some(heft::HeftTarget {
            hand: w.holder?,
            class: w.class,
            weapon: w.address,
        })
    }));
    let dual_wield = weapon
        .filter(|w| {
            dual.split
                && !dual_wield::single::is(w.address)
                && w.wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD
                && w.wielded.hands.contains(&true)
        })
        .map(|w| w.wielded.hands);
    let second_hand = second::hand();
    let held = weapon
        .filter(|w| hold::owned_holder(w.address).is_some())
        .map(|w| w.wielded.hands)
        .or(second_hand.map(|_| [false; 2]))
        .map(|hands| Hand::BOTH.map(|h| hands[h.index()] || second_hand == Some(h)));

    let mut state = STATE.lock();
    let holder = weapon.and_then(|w| w.holder);
    let gun_side = volumes::gun_side(holder, state.last_holder, config.acting_hand());
    state.last_holder = holder.or(state.last_holder);
    state.gun_side = gun_side;
    let geometry = weapon
        .filter(|w| w.two_handed && gameplay)
        .and_then(|w| state.geometry_for(&w, &config, active.world_scale));
    let regions_config = config.weapons.regions;
    let long = weapon.filter(|w| regions_config.enabled && gameplay && w.two_handed);
    // SAFETY: the weapon was read this tick, on the main thread outside the world update.
    let shape = long.and_then(|w| unsafe {
        region_engine::shape(
            (w.address as *const WeaponBase).as_ref()?,
            w.holder.unwrap_or(config.acting_hand()),
        )
    });
    regions::observe(&Observed {
        weapon: weapon.map(|w| (w.address, w.holder)),
        long: long.is_some(),
        shape,
        now: Instant::now(),
        draw_wait: seconds(regions_config.draw_wait_s),
    });
    // A gun carried by a part other than its grip is not aimed.
    let carried = weapon.and_then(|w| regions::carrying(w.address));
    ads::observe(
        weapon
            .zip(geometry)
            .filter(|_| carried.is_none())
            .and_then(|(w, g)| {
                Some(ads::AdsWeapon {
                    holder: w.holder?,
                    sight: ads::solve::WristSight {
                        origin: g.sight,
                        direction: g.barrel,
                    },
                    scoped: scope::is_scoped(w.address),
                })
            }),
    );
    collide::observe(weapon.zip(geometry).and_then(|(w, g)| {
        Some(collide::CollideWeapon {
            holder: w.holder?,
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
    let hold = foregrip::status().hold.filter(|h| h.engaged);
    if let Some(h) = hold
        && let Some(target) = held_foregrip(&h, &active, &config)
    {
        targets.push(target);
    }
    let gun_hands = weapon.map_or([false; 2], |w| w.wielded.hands);
    let empty_handed = config.weapons.holsters.long_guns_dominant
        && gun_hands == [false; 2]
        && second_hand.is_none();
    if gameplay && let Some(body) = &body {
        if config.weapons.holsters.enabled {
            let mut holsters = volumes::holster_candidates(body, &config.weapons.holsters);
            let mut seated = seat_on_guns(&mut holsters, &ticked, &active);
            state.status.docks_followed = seated.len();
            volumes::reserve_long_guns(&mut holsters, config.acting_hand(), empty_handed);
            volumes::reserve_long_guns(&mut seated, config.acting_hand(), empty_handed);
            targets.extend(holsters.iter().map(|c| place.body(c)));
            targets.extend(seated.iter().map(|c| place.on_skeleton(c)));
        }
        let grabbers = ChestGrabbers::for_guns(weapon.map_or([false; 2], |w| w.wielded.hands));
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
    let free = Hand::BOTH.map(|h| !gun_hands[h.index()] && second_hand != Some(h));
    state.docked_touch = [None; 2];
    if gameplay {
        let mut docked = docked_candidates(&mut state, &ticked, &active, free, &regions_config);
        volumes::reserve_long_guns(&mut docked, config.acting_hand(), empty_handed);
        targets.extend(docked.iter().map(|c| place.touch(c)));
        let sockets = socket_candidates(
            &ticked,
            &active,
            free,
            config.weapons.inventory.socket_radius_m,
        );
        targets.extend(sockets.iter().map(|c| place.on_skeleton(c)));
    }
    if config.weapons.grenade_throw {
        let throwing = grenade::throwing_hand().or(c4::throwing_hand());
        // SAFETY: the input tick runs on the main thread outside the world update.
        let grenades = unsafe {
            grenade::begin_tick(
                &grenade::BeginInputs {
                    active: &active,
                    free: Hand::BOTH.map(|h| free[h.index()] && throwing != Some(h)),
                    acting: gameplay,
                },
                &config.weapons.grenade,
            )
        };
        targets.extend(grenades.iter().map(|c| match (c.volume, c.hand) {
            // The pin is on the grenade in the other hand.
            (GrabVolume::GrenadePin, Some(hand)) => place.on_hand(c, hand.other()),
            _ => place.world(c),
        }));
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    let taken = unsafe {
        hold::take::begin_tick(&hold::take::TakeInputs {
            active: &active,
            settings: &holding,
            free,
            independent_pistols: dual.split && dual.independent_draw,
            gameplay,
        })
    };
    targets.extend(taken.iter().map(|c| place.world(c)));
    // SAFETY: both weapons were read this tick, on the main thread outside the world update.
    let reload_target = unsafe {
        reload::focus(
            weapon.and_then(|w| reload::fill_of(w.address)),
            second_weapon.and_then(|w| reload::fill_of(w.address)),
        )
    };
    let reloaded = match reload_target {
        ReloadTarget::Primary => weapon,
        ReloadTarget::Second => second_weapon,
    };
    let reload_candidates = reload::begin_tick(&reload::BeginInputs {
        weapon: reloaded.as_ref(),
        occupied: free.map(|f| !f),
        active: &active,
        config: &config,
        body: body.as_ref(),
        gun_side,
        gameplay,
    });
    for c in &reload_candidates {
        match (c.volume, c.hand) {
            (GrabVolume::MagazinePouch, _) => {
                targets.extend(place.pouch(c, pouches.reach_margin_m));
            }
            // The action is on the gun in the other hand.
            (GrabVolume::WeaponAction, Some(hand)) => targets.push(place.on_hand(c, hand.other())),
            _ => targets.push(place.world(c)),
        }
    }
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
    // A hand holding the second weapon braces nothing and takes nothing from the other hand.
    let foregrip_hand = weapon
        .and_then(|w| w.holder)
        .map(Hand::other)
        .filter(|&h| second_hand != Some(h));
    let handover_allowed = holding.handover
        && gameplay
        && weapon.is_some_and(|w| {
            hold::owned_holder(w.address).is_some_and(|holder| second_hand != Some(holder.other()))
        });
    let on_gun = match (long, geometry, shape) {
        (Some(w), Some(g), Some(shape)) => w.holder.and_then(|holder| {
            Some(GunOnHand {
                hand: weighted_hands(&active)[holder.index()]?,
                geometry: g,
                shape,
            })
        }),
        _ => None,
    };
    let holder = weapon.and_then(|w| w.holder);
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
                        targets.push(
                            place.on_hand(
                                &Candidate {
                                    volume: GrabVolume::Handle,
                                    centre,
                                    radius: gun_volume_radius(touch.hit.distance)
                                        .max(holding.handle_radius_m),
                                    hand: Some(touch.hand),
                                },
                                holder,
                            ),
                        );
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
            if let Some(h) = hold.filter(|h| weapon.is_some_and(|w| w.address == h.weapon)) {
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
                && let (Some(g), Some(w), Some(off)) = (geometry, weapon, foregrip_hand)
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
            if handover_allowed
                && let Some(w) = weapon
                && let Some(holder) = hold::owned_holder(w.address)
                && let Some(grip) = weighted_hands(&active)[holder.index()]
            {
                targets.push(place.on_hand(
                    &Candidate {
                        volume: GrabVolume::Handle,
                        centre: grip.grip.position,
                        radius: holding.handle_radius_m,
                        hand: Some(holder.other()),
                    },
                    holder,
                ));
            }
        }
    }

    // The weapon handling's own view of where each hand is, for the release's docking, the heft,
    // and the cue on entering a socket.
    let grips = grab::tick::hand_grips(&active);
    let resolved = grab::resolve::resolve(&targets, grips);
    let volumes = resolved.map(|r| r.chosen.map(|c| c.volume));
    let closing_on_foregrip = hold.is_none()
        && foregrip_hand.is_some_and(|h| volumes[h.index()] == Some(GrabVolume::Foregrip));
    heft::freeze(reload::holding_hand().is_some() || closing_on_foregrip);

    state.status.active = true;
    state.status.volumes = volumes;
    state.status.candidates = targets.len();
    state.placed.clone_from(&targets);
    state.points = grips;
    state.status.wielded = weapon.map(|weapon| WieldedReadout { weapon, geometry });
    state.weapon = weapon.map(|w| (w, geometry));
    state.second = second_weapon;
    state.gameplay = gameplay;
    let entered = entered_body_volumes(state.last_volumes, volumes);
    state.last_volumes = volumes;
    drop(state);

    for hand in Hand::BOTH.into_iter().filter(|h| entered[h.index()]) {
        haptics::play(hand, Event::VolumeEnter);
    }
    TickStart {
        targets,
        dual_wield,
        held,
    }
}

/// What the weapon handling read of the local player on the input tick.
#[derive(Default)]
struct Ticked {
    weapon: Option<HeldWeapon>,
    second: Option<HeldWeapon>,
    /// The holsters' guns, at their grips in the world, docked or where they dock; `None` while
    /// the holsters do not sit on their guns.
    holster_guns: Option<Vec<(HolsterSite, Option<Vec3>)>>,
    /// The sockets' grab volumes, in the world.
    sockets: Vec<SocketPoint>,
    /// The character's model-to-world transform.
    character: Option<Mat4>,
    /// The long guns docked on the shoulders, for a draw at any part of one.
    docked_long: Vec<DockedLongGun>,
}

/// The wielded long gun on its holding hand, as the regions place it.
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
            volume: GrabVolume::Holster(gun.site),
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

/// Seat the holsters on their guns ([`crate::controllers::weapons::docks`]), returning the seated
/// ones. Leaves `candidates` untouched while the guns or the frames cannot be read.
fn seat_on_guns(
    candidates: &mut Vec<Candidate>,
    ticked: &Ticked,
    active: &ActiveHands,
) -> Vec<Candidate> {
    let Some(read) = &ticked.holster_guns else {
        return Vec::new();
    };
    let Some(frames) = ticked
        .character
        .and_then(|c| docks::cockpit_frames(active, c))
    else {
        return Vec::new();
    };
    let guns: Vec<HolsterGun> = read
        .iter()
        .map(|&(site, point)| HolsterGun {
            site,
            point: point.map(|p| frames.world_to_cockpit(p)),
        })
        .collect();
    docks::seat(candidates, &guns)
}

/// The sockets' grab volumes in the cockpit frame: a carried weapon for each hand without a gun,
/// and a free socket for the hand holding a weapon that fits it.
fn socket_candidates(
    ticked: &Ticked,
    active: &ActiveHands,
    free: [bool; 2],
    radius: f32,
) -> Vec<Candidate> {
    let Some(frames) = ticked
        .character
        .and_then(|c| docks::cockpit_frames(active, c))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for point in &ticked.sockets {
        let centre = frames.world_to_cockpit(point.point);
        let hands: Vec<Hand> = match point.for_hand {
            None => Hand::BOTH.into_iter().filter(|h| free[h.index()]).collect(),
            Some(hand) => vec![hand],
        };
        out.extend(hands.into_iter().map(|hand| Candidate {
            volume: GrabVolume::Socket(point.socket),
            centre,
            radius: radius.max(0.0),
            hand: Some(hand),
        }));
    }
    out
}

/// The weapon in each hand, by [`Hand::index`]: the wielded weapon in its one hand (not the pair
/// drawn in both), and the second weapon in its.
fn held_by_hand(weapon: Option<HeldWeapon>, second: Option<HeldWeapon>) -> [Option<usize>; 2] {
    let mut held = [None; 2];
    for w in [weapon, second].into_iter().flatten() {
        if w.wielded.hands == [true; 2] {
            continue;
        }
        if let Some(hand) = w.holder {
            held[hand.index()] = Some(w.address);
        }
    }
    held
}

/// Whether the weapon handling acts in `mode`: on the ground and under the parachute or in a fall,
/// where the grips are free. In vehicles and the wingsuit the grips fire their weapons, and
/// swimming holds no weapon.
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

/// The wielded weapon as the dual-wield pair's owned layout has it: a pistol is in a hand only
/// while that hand has it drawn, and the first drawn pistol's hand holds the weapon.
fn with_pair(mut weapon: HeldWeapon, pair: Option<dual_wield::attach::Pair>) -> HeldWeapon {
    if let Some(pair) = pair.filter(|p| p.wielded)
        && weapon.wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD
    {
        weapon.wielded.hands = pair.drawn;
        weapon.holder = pair
            .gun_hands
            .into_iter()
            .find(|hand| pair.drawn[hand.index()]);
    }
    weapon
}

/// The wielded weapon as the hold has it: every gun in the holding hand, which holds the weapon.
/// The table follows on the next placement; this is the hold's current layout.
fn with_holder(mut weapon: HeldWeapon, holder: Option<Hand>) -> HeldWeapon {
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

/// Which hands entered a body volume this tick, by [`Hand::index`]: a hand in a holster or a pouch
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

/// Whether `volume` is placed on the body: a holster or a pouch.
fn is_body_volume(volume: GrabVolume) -> bool {
    matches!(
        volume,
        GrabVolume::Holster(_)
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::C4Pouch
    )
}
