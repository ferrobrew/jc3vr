//! The first half of the weapon handling's input tick: read the wielded weapon, place the grab
//! volumes and the pistols, and report which volume each hand's grab point is in.

use glam::{Mat4, Vec3};
use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponBase};

use crate::{
    config::Config,
    controllers::{
        bindings::grab::{Candidate, GrabVolume, HolsterSite},
        body::{self, BodyDimensions, alignment},
        config::ControllersConfig,
        frame::Hand,
        hands::{self, ActiveHands, CockpitHand},
        haptics::{self, Event},
        mode::{AirborneVariant, Mode, OnFootVariant},
        weapons::{
            ads, c4, collide,
            config::HolsterAnchor,
            docks::{self, DockedGun},
            dual_wield,
            engine::{self, HeldWeapon},
            foregrip, grenade, grip_watch, heft, highlights,
            hold::{self, second},
            inventory,
            reload::{self, focus::ReloadTarget},
            scope,
            state::STATE,
            types::{TickStart, WieldedReadout},
            volumes::{self, Anchor, BodyFrame, ChestGrabbers, HeadDrop},
        },
    },
};

/// The first half of the weapon handling's input tick: read the weapon, place the volumes and the
/// pistols, and return the volume each hand is in and whether the dual-wield split acts. Called
/// while the bindings inject, before the tables.
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
            } else {
                second::stand_down();
            }
            second::show_magazine(Some(owner), gameplay && holding.magazine_pips);
            let pair = dual_wield::attach::input_tick(owner, settings);
            let holder = hold::input_tick(owner, hold_settings, pair.is_some());
            let weapon =
                engine::held_weapon(owner).map(|w| with_holder(with_pair(w, pair), holder));
            let second = second::held_weapon(owner);
            inventory::input_tick(
                owner,
                &inventory::TickInputs {
                    settings: config.weapons.inventory,
                    wielded: weapon.map(|w| (w.address, w.holder)),
                    driven: second.and_then(|w| Some((w.address, w.holder?))),
                },
            );
            Ticked {
                weapon,
                second,
                docked: if config.weapons.holsters.follow_docks {
                    docks::docked_guns(owner, config.acting_hand())
                } else {
                    Vec::new()
                },
                character: Some(Mat4::from(owner.m_WorldMatrixT1)),
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
    ads::observe(weapon.zip(geometry).and_then(|(w, g)| {
        Some(ads::AdsWeapon {
            holder: w.holder?,
            sight: ads::solve::WristSight {
                origin: g.sight,
                direction: g.barrel,
            },
            scoped: scope::is_scoped(w.address),
        })
    }));
    collide::observe(weapon.zip(geometry).and_then(|(w, g)| {
        Some(collide::CollideWeapon {
            holder: w.holder?,
            muzzle: g.muzzle,
            barrel: g.barrel,
        })
    }));

    let mut candidates = Vec::new();
    let stance = state.step_stance();
    let looking = active.head.orientation * Vec3::NEG_Z;
    let body_frame = volumes::body_frame(
        HeadDrop {
            position: active.head.position,
            pitch_down: (-looking.y).clamp(-1.0, 1.0).asin(),
            previous_y: state.body_y,
        },
        stance,
        config.weapons.holsters.crouch_deadband_m,
        volume_anchor(&config, &active),
    );
    state.body_y = Some(body_frame.origin.y);
    let dims = body_dimensions(&config, &body_frame.anchor);
    let pouches = &config.weapons.pouches;
    if gameplay {
        if config.weapons.holsters.enabled {
            candidates = volumes::holster_candidates(&body_frame, &dims, &config.weapons.holsters);
            state.status.docks_followed =
                follow_docks(&mut candidates, &ticked, &body_frame, &config, &active);
        }
        let grabbers = ChestGrabbers::for_guns(weapon.map_or([false; 2], |w| w.wielded.hands));
        if config.weapons.grenade_throw {
            candidates.extend(volumes::grenade_pouch(
                &body_frame,
                &dims,
                pouches,
                gun_side,
                grabbers,
            ));
        }
        if config.weapons.c4_throw {
            candidates.extend(volumes::c4_pouch(
                &body_frame,
                &dims,
                pouches,
                gun_side,
                grabbers,
            ));
        }
    }
    let body_points = body_points(&active, &body_frame.anchor);
    let gun_hands = weapon.map_or([false; 2], |w| w.wielded.hands);
    let free = Hand::BOTH.map(|h| !gun_hands[h.index()] && second_hand != Some(h));
    if config.weapons.grenade_throw {
        let throwing = grenade::throwing_hand().or(c4::throwing_hand());
        // SAFETY: the input tick runs on the main thread outside the world update.
        candidates.extend(unsafe {
            grenade::begin_tick(
                &grenade::BeginInputs {
                    active: &active,
                    free: Hand::BOTH.map(|h| free[h.index()] && throwing != Some(h)),
                    acting: gameplay,
                },
                &config.weapons.grenade,
            )
        });
    }
    // SAFETY: the input tick runs on the main thread outside the world update.
    candidates.extend(unsafe {
        hold::take::begin_tick(&hold::take::TakeInputs {
            active: &active,
            settings: &holding,
            free,
            independent_pistols: dual.split && dual.independent_draw,
            gameplay,
        })
    });
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
    candidates.extend(reload::begin_tick(&reload::BeginInputs {
        weapon: reloaded.as_ref(),
        occupied: free.map(|f| !f),
        active: &active,
        config: &config,
        body_frame: &body_frame,
        dims: &dims,
        gun_side,
        body_points,
        gameplay,
    }));
    let hold = foregrip::status().hold.filter(|h| h.engaged);
    // A hand holding the second weapon braces nothing and takes nothing from the other hand.
    let foregrip_hand = weapon
        .and_then(|w| w.holder)
        .map(Hand::other)
        .filter(|&h| second_hand != Some(h));
    if config.weapons.foregrip.enabled
        && hold.is_none()
        && let (Some(g), Some(w), Some(off)) = (geometry, weapon, foregrip_hand)
        && let Some(holder) = w.holder.and_then(|h| weighted_hands(&active)[h.index()])
    {
        candidates.push(Candidate {
            volume: GrabVolume::Foregrip,
            centre: foregrip::foregrip_point(&holder, &g),
            radius: config.weapons.foregrip.radius_m,
            hand: Some(off),
        });
    }
    if holding.handover
        && gameplay
        && let Some(w) = weapon
        && let Some(holder) = hold::owned_holder(w.address)
        && second_hand != Some(holder.other())
        && let Some(grip) = weighted_hands(&active)[holder.index()]
    {
        candidates.push(Candidate {
            volume: GrabVolume::Handle,
            centre: grip.grip.position,
            radius: holding.handle_radius_m,
            hand: Some(holder.other()),
        });
    }

    let volumes = Hand::BOTH.map(|hand| {
        if hold.is_some_and(|h| h.holder.other() == hand) {
            return Some(GrabVolume::Foregrip);
        }
        let held = active.hands[hand.index()]?.grip.position;
        let body = body_points[hand.index()].unwrap_or(held);
        volumes::grab_volume_by(hand, &candidates, |c| {
            if is_body_volume(c.volume) { body } else { held }
        })
        .or_else(|| volumes::pouch_in_reach(body, hand, &candidates, pouches.reach_margin_m))
    });
    let closing_on_foregrip = hold.is_none()
        && foregrip_hand.is_some_and(|h| volumes[h.index()] == Some(GrabVolume::Foregrip));
    heft::freeze(reload::holding_hand().is_some() || closing_on_foregrip);

    state.status.active = true;
    state.status.volumes = volumes;
    state.status.candidates = candidates.len();
    state.status.stance = Some(stance);
    state.placed.clone_from(&candidates);
    state.grab_points = body_points;
    state.status.wielded = weapon.map(|weapon| WieldedReadout { weapon, geometry });
    state.weapon = weapon.map(|w| (w, geometry));
    state.gameplay = gameplay;
    let entered = entered_body_volumes(state.last_volumes, volumes);
    state.last_volumes = volumes;
    drop(state);

    for hand in Hand::BOTH.into_iter().filter(|h| entered[h.index()]) {
        haptics::play(hand, Event::VolumeEnter);
    }
    highlights::submit(
        &config,
        &active,
        &highlights::Reach {
            candidates: &candidates,
            volumes,
            body_points,
        },
    );
    TickStart {
        volumes,
        dual_wield,
        held,
    }
}

/// What the weapon handling read of the local player on the input tick.
#[derive(Default)]
struct Ticked {
    weapon: Option<HeldWeapon>,
    second: Option<HeldWeapon>,
    /// The guns docked on the body, at their points in the world.
    docked: Vec<(HolsterSite, Vec3)>,
    /// The character's model-to-world transform.
    character: Option<Mat4>,
}

/// Move the holsters with guns docked in them onto the guns ([`crate::controllers::weapons::docks`]),
/// by the guns' displacement from the holsters placed on the character's skeleton. Returns how many
/// moved.
fn follow_docks(
    candidates: &mut [Candidate],
    ticked: &Ticked,
    body_frame: &BodyFrame,
    config: &ControllersConfig,
    active: &ActiveHands,
) -> usize {
    if ticked.docked.is_empty() {
        return 0;
    }
    let Some(frames) = ticked
        .character
        .and_then(|c| docks::cockpit_frames(active, c))
    else {
        return 0;
    };
    let Some(skeleton) = alignment::skeleton_in_cockpit(active.head.position, active.world_scale)
    else {
        return 0;
    };
    let anchor = Anchor::Skeleton(skeleton);
    let on_character = volumes::holster_candidates(
        &BodyFrame {
            anchor,
            ..*body_frame
        },
        &body_dimensions(config, &anchor),
        &config.weapons.holsters,
    );
    let guns: Vec<DockedGun> = ticked
        .docked
        .iter()
        .map(|&(site, point)| DockedGun {
            site,
            point: frames.world_to_cockpit(point),
        })
        .collect();
    docks::follow(candidates, &on_character, &guns, docks::MAX_SHIFT_M)
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

/// The body the volumes ride: the character's skeleton when the setting asks for it and the
/// skeleton is fresh and upright, and the player's body turned to the estimated torso otherwise
/// (facing the cockpit frame's forward without an estimate).
fn volume_anchor(config: &ControllersConfig, active: &ActiveHands) -> Anchor {
    let skeleton = (config.weapons.holsters.anchor == HolsterAnchor::Rico)
        .then(|| alignment::skeleton_in_cockpit(active.head.position, active.world_scale))
        .flatten();
    let anchor = match skeleton {
        Some(skeleton) => Anchor::Skeleton(skeleton),
        None => Anchor::Body {
            yaw: alignment::estimate().map_or(0.0, |e| e.yaw),
        },
    };
    alignment::record_volume_anchor(anchor.label());
    anchor
}

/// The dimensions the volumes are placed with: the player's on the player's body; on the
/// character's skeleton, the character's while the hands are remapped onto them, and the player's
/// otherwise.
fn body_dimensions(config: &ControllersConfig, anchor: &Anchor) -> BodyDimensions {
    match anchor {
        Anchor::Skeleton(_) if config.body.remap_hands => BodyDimensions::CHARACTER_ESTIMATE,
        _ => body::player(),
    }
}

/// Each hand's point for the body's volumes: the player's own grip, before the body-scale remap,
/// on the player's body, so that a hand on the player's own belt finds the pouch there; the grip
/// of the hand the player sees on the character's skeleton, whose landmarks are the character's.
fn body_points(active: &ActiveHands, anchor: &Anchor) -> [Option<Vec3>; 2] {
    match anchor {
        Anchor::Body { .. } => active.player_grips,
        Anchor::Skeleton(_) => active.hands.map(|h| h.map(|h| h.grip.position)),
    }
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
pub(super) fn is_body_volume(volume: GrabVolume) -> bool {
    matches!(
        volume,
        GrabVolume::Holster(_)
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::C4Pouch
    )
}
