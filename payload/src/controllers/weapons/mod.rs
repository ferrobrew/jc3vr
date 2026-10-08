//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads",
//! "Grenades", and "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the body's grab volumes (the holsters, the grenade pouch, the reload pouch, and
//!   the C4 pouch), placed from the player's body dimensions;
//! - [`holster`]: what a grab at a holster does: draw its slot or holster the held weapon;
//! - [`docks`]: the holsters moved onto the guns docked in them;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`recoil`]: the recoil offset between the controller and the gun, with its return spring;
//! - [`heft`]: the one-handed weight of a long gun: sag, lag, sway, a stronger kick, and a wider
//!   spread, gone while it is braced on the foregrip;
//! - [`fire`]: the per-shot seam that kicks the recoil and marks the shot's hand;
//! - [`grip_watch`]: a diagnostic watch on how each held gun sits in its hand;
//! - [`haptics`]: the game's rumbles and the grapple's pulses routed to the hands;
//! - [`highlights`]: the grab-volume highlights on the overlay;
//! - [`dual_wield`]: each dual-wielded pistol drawn by its own hand and fired by its own hand's
//!   trigger;
//! - [`hold`]: which hand holds the wielded weapon (the hand that drew it, took it off the
//!   ground, or was handed it), the attach bones that put it there, and the direct pickup;
//! - [`camera_kick`]: the game's camera recoil, suppressed while the guns aim by hand;
//! - [`grenade`]: the physical grenade throw;
//! - [`c4`]: the C4 throw from the chest pouch and the wrist detonator;
//! - [`reload`]: the true reload: eject, load from the belt pouch, and chamber by hand;
//! - [`empty`]: the game's automatic switch away from an empty weapon suppressed, and the dry fire
//!   pulsed;
//! - [`unarmed_fire`]: the fire input with nothing in the hands kept from drawing a weapon;
//! - [`scope`]: scoped weapons: the game's precision aim suppressed, and the lens on the scope;
//! - [`wheel`]: the game's weapon selection cross, opened with a held input and steered by the
//!   hand;
//! - [`config`]: the weapon-handling settings.
//!
//! The weapon handling runs on the input tick, in two halves around the bindings' tables
//! ([`crate::controllers::bindings::input_tick`]):
//!
//! 1. [`begin_tick`] reads the wielded weapon, places the grab volumes, and answers which volume
//!    each hand's grab point (the controller's grip, in the cockpit frame) is in, which the tables
//!    latch at the grip's press edge. The body's volumes are tested against the player's own grip,
//!    before the body-scale remap, while they ride the player's body, and the weapon's volumes
//!    against the grip of the hand on the weapon. A hand just outside a pouch reaches it
//!    ([`volumes::pouch_in_reach`]), so a press there takes from the pouch rather than firing the
//!    grapple. A hand holding the foregrip is always in it, so that its next grip press lets go
//!    rather than firing the grapple.
//! 2. [`finish_tick`] acts on the grabs the tables report: a holster grab draws or holsters, a
//!    grab of the held gun's grip by the free hand hands the gun over, a foregrip grab engages or
//!    releases the two-handed grip, a grenade-pouch grab primes a grenade
//!    for the throw, a C4-pouch grab primes a charge, and a reload-pouch grab takes a round for the
//!    reload. It runs the throws, the wrist detonator, and the reload, and
//!    holds the fire inputs back while a reload, a draw, or a holster plays (the action-runs hand
//!    tier) and while the reload holds the gun from firing.
//!
//! Guns are sticky: nothing here draws or holsters except a grab. The grab-volume highlights, each
//! placed pouch (faint, and brighter while a hand is within reach, with `pouches.show`), and, with
//! slot ([`highlights`]). A grip press that grabs nothing near a holster is logged with its
//! slot. A grip press that grabs nothing near a holster is logged with its
//! distance from the nearest one. Everything acts only while the bindings inject, which is only
//! under the controller scheme with a controller active.

pub mod c4;
mod camera_kick;
pub mod config;
pub mod docks;
pub mod dual_wield;
pub mod empty;
mod engine;
mod fire;
pub mod foregrip;
pub mod grenade;
pub mod grip_watch;
pub mod haptics;
pub mod heft;
mod highlights;
pub mod hold;
pub mod holster;
pub mod recoil;
pub mod reload;
pub mod scope;
pub mod unarmed_fire;
pub mod volumes;
pub mod wheel;

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::{
    character::inventory::WeaponSlot, equipment::weapon::WeaponBase,
    input::input_action_map::Action,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        bindings::{
            controls::ButtonControl,
            grab::{Candidate, GrabVolume, HolsterSite},
            hold::{Desired, MapKind, Target, Want},
        },
        body::{self, BodyDimensions, alignment},
        config::ControllersConfig,
        frame::Hand,
        hands::{
            self, ActiveHands, CockpitHand, ik,
            tiers::{self, HandTier, TierState},
        },
        mode::{AirborneVariant, Mode, OnFootVariant},
        overlay::{primitive::Source, store},
        roomscale::{self, posture::EffectivePosture},
        weapons::{
            config::HolsterAnchor,
            docks::DockedGun,
            engine::HeldWeapon,
            foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
            haptics::PulseSource,
            hold::{
                dock::{self, Release},
                holder::Cause,
                second,
            },
            holster::HolsterAction,
            reload::focus::ReloadTarget,
            volumes::{BodyFrame, ChestGrabbers, HeadDrop, NearMiss, Stance},
        },
    },
};

/// The weapon handling's hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(fire::hook_library())
        .with_hook_library(haptics::hook_library())
        .with_hook_library(camera_kick::hook_library())
        .with_hook_library(grenade::hook_library())
        .with_hook_library(c4::hook_library())
        .with_hook_library(scope::hook_library())
        .with_hook_library(hold::hook_library())
        .with_hook_library(empty::hook_library())
        .with_hook_library(heft::hook_library())
        .with_hook_library(reload::hook_library())
        .with_hook_library(wheel::hook_library())
}

/// What the first half of the weapon handling's input tick hands the bindings.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TickStart {
    /// The grab volume each hand is in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// The hands holding a pistol of the dual-wield slot while the dual-wield split acts (with the
    /// split on, a pistol in at least one hand, in any mode), indexed by [`Hand::index`].
    pub dual_wield: Option<[bool; 2]>,
    /// The hands holding a gun of the wielded weapon while the hold puts it in either hand,
    /// indexed by [`Hand::index`]: a hand's trigger fires the gun it holds, and only that hand's.
    pub held: Option<[bool; 2]>,
}

/// What a grab did, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GrabOutcome {
    Holster(HolsterAction),
    /// A holster grab with no local player to act on.
    NoCharacter,
    Foregrip(Transition),
    /// A grab of the held gun's grip by the free hand, which took the gun when `moved`.
    Handover {
        moved: bool,
    },
    /// A foregrip grab that could not engage: the weapon's geometry is not measured yet.
    ForegripUnmeasured,
    /// A grenade-pouch grab: a grenade is asked for.
    Pouch,
    /// A grab of the held grenade's pin.
    Pin,
    /// A grab of a live grenade, to catch it.
    Catch,
    /// A reload-pouch grab: a round is taken for the reload.
    Magazine,
    /// A grab of a gun's slide, charging handle, or bolt.
    Action,
    /// A C4-pouch grab: a charge is asked for.
    C4,
    /// A grab of a weapon without a body in the world (in a character's hands, or on a rack),
    /// which went into the hand when `taken`.
    Take {
        taken: bool,
    },
}

impl GrabOutcome {
    pub fn label(self) -> String {
        match self {
            GrabOutcome::Holster(action) => action.label(),
            GrabOutcome::NoCharacter => "no character".to_owned(),
            GrabOutcome::Foregrip(Transition::Grabbed) => "foregrip grabbed".to_owned(),
            GrabOutcome::Foregrip(Transition::Released(r)) => {
                format!("foregrip released ({})", r.label())
            }
            GrabOutcome::Handover { moved: true } => "gun handed over".to_owned(),
            GrabOutcome::Handover { moved: false } => "gun not handed over".to_owned(),
            GrabOutcome::ForegripUnmeasured => "foregrip not measured yet".to_owned(),
            GrabOutcome::Pouch => "grenade asked for".to_owned(),
            GrabOutcome::Pin => "grenade pin grabbed".to_owned(),
            GrabOutcome::Catch => "live grenade grabbed".to_owned(),
            GrabOutcome::Magazine => "reload pouch".to_owned(),
            GrabOutcome::Action => "action grabbed".to_owned(),
            GrabOutcome::C4 => "charge asked for".to_owned(),
            GrabOutcome::Take { taken: true } => "weapon taken".to_owned(),
            GrabOutcome::Take { taken: false } => "weapon not taken".to_owned(),
        }
    }
}

/// One grab, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrabEvent {
    pub hand: Hand,
    pub volume: GrabVolume,
    pub outcome: GrabOutcome,
}

/// The wielded weapon as the weapon handling saw it on the last tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WieldedReadout {
    pub weapon: HeldWeapon,
    /// The two-handed geometry in use, when measured.
    pub geometry: Option<GripGeometry>,
}

/// The weapon handling's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeaponsStatus {
    /// Whether the last input tick ran the weapon handling.
    pub active: bool,
    /// The grab volume each hand was in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// How many volumes were placed.
    pub candidates: usize,
    /// The stance the body volumes were placed for.
    pub stance: Option<Stance>,
    pub wielded: Option<WieldedReadout>,
    /// Whether the fire inputs were held back for a weapon action on the last tick.
    pub fire_blocked: bool,
    /// The most recent grabs, newest last.
    pub events: Vec<GrabEvent>,
    pub grabs: u64,
    /// The last grip press near but outside a holster.
    pub last_miss: Option<(Hand, NearMiss)>,
    /// How many holsters were moved onto their docked guns on the last tick.
    pub docks_followed: usize,
}

pub fn status() -> WeaponsStatus {
    STATE.lock().status.clone()
}

/// The first half of the weapon handling's input tick: read the weapon, place the volumes and the
/// pistols, and return the volume each hand is in and whether the dual-wield split acts. Called
/// while the bindings inject, before the tables.
pub(crate) fn begin_tick(mode: Mode) -> TickStart {
    let config = Config::lock_query(|c| c.controllers);
    let Some(active) = hands::active_hands_raw() else {
        stand_down();
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
            Ticked {
                weapon: engine::held_weapon(owner).map(|w| with_holder(with_pair(w, pair), holder)),
                second: second::held_weapon(owner),
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

    let haptics = &config.weapons.haptics;
    if haptics.volume_cues {
        for hand in Hand::BOTH.into_iter().filter(|h| entered[h.index()]) {
            haptics::pulse(hand, haptics.volume_enter, PulseSource::GrabVolume);
        }
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

/// What the bindings hand the second half of the weapon handling's input tick.
pub(crate) struct FinishInputs {
    /// The grab volume each hand grabbed this tick, indexed by [`Hand::index`].
    pub grabs: [Option<GrabVolume>; 2],
    /// Each hand's grip over the tick.
    pub grips: [ButtonControl; 2],
    /// Each hand's trigger press over the tick.
    pub triggers: [ButtonControl; 2],
    /// The reload action's press over the tick.
    pub reload_button: ButtonControl,
    /// The engine's tick delta, seconds.
    pub dt: f32,
}

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw and the reload, and hold the fire inputs back during a weapon action. `desired` is the
/// tables' output, which a draw adds its slot-select click to.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let FinishInputs {
        grabs,
        grips,
        triggers,
        reload_button,
        dt,
    } = *inputs;
    let config = Config::lock_query(|c| c.controllers);
    let active = hands::active_hands_raw();
    let (weapon, gameplay, gun_side) = {
        let state = STATE.lock();
        (state.weapon, state.gameplay, state.gun_side)
    };

    let foregrip_grab = grabs
        .iter()
        .zip(Hand::BOTH)
        .find(|(g, _)| **g == Some(GrabVolume::Foregrip))
        .map(|(_, hand)| hand);
    let two_handed = weapon
        .filter(|(w, _)| w.two_handed && gameplay)
        .and_then(|(w, geometry)| {
            Some(TwoHandedWeapon {
                holder: w.holder?,
                address: w.address,
                geometry,
            })
        });
    let inputs = Inputs {
        active: config.weapons.foregrip.enabled && active.is_some(),
        hands: active.as_ref().map_or([None; 2], |a| a.hands),
        weapon: two_handed,
        grab_pressed: foregrip_grab
            .is_some_and(|hand| two_handed.is_some_and(|w| w.holder.other() == hand)),
        grip_held: two_handed.is_some_and(|w| grips[w.holder.other().index()].held()),
    };
    let transition = foregrip::tick(&inputs, dt, &config.weapons.foregrip);
    // A release pulses the off-hand whatever ended the hold: a press, a move away, or the weapon
    // leaving the hands.
    if let (Some(Transition::Released(_)), Some(w)) = (transition, two_handed)
        && config.weapons.haptics.volume_cues
    {
        haptics::pulse(
            w.holder.other(),
            config.weapons.haptics.grab,
            PulseSource::GrabVolume,
        );
    }

    let mut events = Vec::new();
    for hand in Hand::BOTH {
        let Some(volume) = grabs[hand.index()] else {
            continue;
        };
        let outcome = match volume {
            GrabVolume::Holster(site) => {
                let wielded = weapon.map(|(w, _)| w.wielded);
                let independent =
                    config.weapons.dual_wield.split && config.weapons.dual_wield.independent_draw;
                let second_weapon = holster::Second {
                    enabled: config.weapons.hold.either_hand && config.weapons.hold.second_weapon,
                    held: second::hand().zip(second::slot()),
                };
                match holster::decide(
                    site,
                    hand,
                    config.acting_hand(),
                    wielded,
                    independent,
                    second_weapon,
                ) {
                    Some(action) => act(action, hand, &config, desired),
                    None => continue,
                }
            }
            GrabVolume::Handle => {
                let moved = hold::hand_over(hand);
                if moved {
                    // The hand that took the gun let go of the foregrip to take it.
                    foregrip::clear();
                }
                GrabOutcome::Handover { moved }
            }
            GrabVolume::Foregrip => match transition {
                Some(t) => GrabOutcome::Foregrip(t),
                None => GrabOutcome::ForegripUnmeasured,
            },
            GrabVolume::GrenadePouch => GrabOutcome::Pouch,
            GrabVolume::GrenadePin => GrabOutcome::Pin,
            GrabVolume::LiveGrenade => GrabOutcome::Catch,
            GrabVolume::MagazinePouch => GrabOutcome::Magazine,
            GrabVolume::WeaponAction => GrabOutcome::Action,
            GrabVolume::C4Pouch => GrabOutcome::C4,
            GrabVolume::WorldWeapon => {
                let Some(active) = active.as_ref() else {
                    continue;
                };
                let dual = config.weapons.dual_wield;
                let inputs = hold::take::TakeInputs {
                    active,
                    settings: &config.weapons.hold,
                    free: [true; 2],
                    independent_pistols: dual.split && dual.independent_draw,
                    gameplay,
                };
                // SAFETY: the input tick runs on the main thread outside the world update.
                let taken = unsafe { hold::take::finish_grab(hand, &inputs) };
                GrabOutcome::Take { taken }
            }
            GrabVolume::VehicleControl(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::Pickup
            | GrabVolume::Climb => {
                continue;
            }
        };
        tracing::debug!(target: "controllers", ?hand, volume = volume.label(), outcome = %outcome.label(), "grab");
        if config.weapons.haptics.volume_cues && cues_grab(outcome) {
            haptics::pulse(hand, config.weapons.haptics.grab, PulseSource::GrabVolume);
        }
        events.push(GrabEvent {
            hand,
            volume,
            outcome,
        });
    }
    if gameplay {
        log_near_misses(&grabs, &grips);
    }
    let holder = weapon.and_then(|(w, _)| w.holder);
    let released = hold::act::release_tick(holder, grips.map(ButtonControl::held))
        .filter(|_| config.weapons.hold.grip_to_hold && gameplay);
    if let (Some(hand), Some((w, _))) = (released, weapon) {
        let volumes = STATE.lock().status.volumes;
        let at_own_holster = matches!(
            volumes[hand.index()],
            Some(GrabVolume::Holster(site))
                if holster::site_slot(site, config.acting_hand()) == w.wielded.slot
        );
        let droppable = w.wielded.slot != WeaponSlot::E_SLOT_DUAL_WIELD;
        release(
            hand,
            dock::on_release(at_own_holster, droppable),
            w.wielded,
            &config,
        );
    }

    grenade::input_tick(
        &grenade::TickInputs {
            acting: config.weapons.grenade_throw && gameplay && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePouch)),
            pin_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePin)),
            catch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::LiveGrenade)),
            cockpit_grips: active
                .as_ref()
                .map_or([None; 2], |a| a.hands.map(|h| h.map(|h| h.grip.position))),
            triggers,
            dominant: config.acting_hand(),
            grips,
            world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
            dt,
        },
        &config,
        desired,
    );
    c4::input_tick(
        &c4::TickInputs {
            acting: config.weapons.c4_throw && gameplay && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::C4Pouch)),
            grips,
            world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
        },
        &config.weapons.c4,
    );
    c4::wrist::input_tick(
        &c4::wrist::TickInputs {
            acting: config.weapons.wrist_detonator && active.is_some(),
            presser: gun_side,
            dt,
        },
        &config,
    );
    let reload_held = reload::finish_tick(
        &reload::FinishInputs {
            grabs,
            grips,
            triggers,
            reload_button,
            dt,
        },
        &config,
    );
    // A gun the reload holds cannot fire. The dual-wield split keeps a held pistol's trigger from
    // the game itself, so the fire input is held back only when no drawn gun may fire.
    let gun_hands = weapon.map_or([false; 2], |(w, _)| w.wielded.hands);
    let reload_blocks = gun_hands.contains(&true)
        && Hand::BOTH
            .iter()
            .all(|h| !gun_hands[h.index()] || reload_held[h.index()]);

    second::set_trigger(
        gameplay && second::hand().is_some_and(|hand| triggers[hand.index()].held()),
    );
    let fire_blocked = reload_blocks
        || hold::act::holstering()
        || primary_trigger_idle(weapon.map(|(w, _)| w), &triggers)
        || (config.weapons.block_fire_during_actions
            && weapon_action_running(weapon.and_then(|(w, _)| w.holder).unwrap_or(gun_side)));
    if fire_blocked {
        hold_fire_back(desired);
    }

    let mut state = STATE.lock();
    state.status.fire_blocked = fire_blocked;
    state.status.grabs += events.len() as u64;
    state.status.events.extend(events);
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

/// Stop acting: drop the foregrip hold and forget the tick's weapon. Called whenever the bindings
/// do not inject.
pub(crate) fn stand_down() {
    foregrip::clear();
    heft::observe(None);
    heft::freeze(false);
    grip_watch::clear();
    recoil::clear();
    grenade::stand_down();
    c4::stand_down();
    c4::wrist::stand_down();
    dual_wield::observe_triggers(None);
    dual_wield::attach::clear();
    hold::clear();
    hold::act::clear();
    hold::take::stand_down();
    hold::pips::submit(None);
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { second::stand_down() };
    reload::stand_down();
    wheel::stand_down();
    let mut state = STATE.lock();
    state.weapon = None;
    state.status.active = false;
    state.status.volumes = [None; 2];
    state.status.fire_blocked = false;
    state.status.stance = None;
    state.placed.clear();
    state.grab_points = [None; 2];
    state.body_y = None;
    state.last_volumes = [None; 2];
    store::clear(Source::GrabVolumes);
}

/// The fire inputs a weapon action holds back.
const FIRE_ACTIONS: [Action; 2] = [Action::FIRE_RIGHT, Action::R2];

/// How far outside a holster's surface a grip press counts as a near miss, metres.
const NEAR_MISS_MARGIN_M: f32 = 0.3;

/// The shortest time between two near-miss log lines.
const NEAR_MISS_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// How many grabs the readout keeps.
const MAX_EVENTS: usize = 6;

struct State {
    status: WeaponsStatus,
    /// The tick's wielded weapon and its two-handed geometry.
    weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// Whether the tick's mode is one the weapon handling acts in.
    gameplay: bool,
    /// The last geometry measured, and the weapon it belongs to: a measurement needs the wrist at
    /// full weight, so a tick without one keeps the last.
    measured: Option<(usize, GripGeometry)>,
    /// The tick's placed volumes and each hand's grab point for the body's volumes, for the
    /// near-miss log.
    placed: Vec<Candidate>,
    grab_points: [Option<Vec3>; 2],
    last_miss_log: Option<Instant>,
    /// The hand that last held a gun, kept while none is drawn.
    last_holder: Option<Hand>,
    /// The body frame's height on the last tick ([`volumes::body_frame`]).
    body_y: Option<f32>,
    /// The volume each hand was in on the last tick, for the cue on entering one.
    last_volumes: [Option<GrabVolume>; 2],
    /// The tick's hand the pouches and the wrist detonator are laid out around
    /// ([`volumes::gun_side`]).
    gun_side: Hand,
}

impl State {
    /// The stance for this tick, from the roomscale posture (the single source of truth for
    /// seated versus standing): seated for the seated posture, standing for the standing and
    /// roomscale postures, and standing without a recent roomscale frame (the roomscale
    /// subsystems off, or not yet run). Logs a change of stance.
    fn step_stance(&mut self) -> Stance {
        let now = Instant::now();
        let posture = roomscale::frame_shared()
            .filter(|f| f.fresh(now))
            .map(|f| f.posture);
        let stance = match posture {
            Some(EffectivePosture::Seated) => Stance::Seated,
            _ => Stance::Standing,
        };
        let previous = self.status.stance;
        if previous.map(Stance::label) != Some(stance.label()) {
            tracing::debug!(target: "controllers", stance = stance.label(), "holsters: placed for a new stance");
        }
        stance
    }

    /// The two-handed geometry of `weapon`, measured afresh when possible.
    fn geometry_for(
        &mut self,
        weapon: &HeldWeapon,
        config: &ControllersConfig,
        world_scale: f32,
    ) -> Option<GripGeometry> {
        let target = weapon
            .holder
            .and_then(|holder| ik::tick_records().current[holder.index()]);
        // SAFETY: the weapon was read this tick on the main thread outside the world update.
        let fresh = target.and_then(|target| unsafe {
            engine::measure_geometry(
                weapon,
                target,
                &config.aim,
                &config.weapons.foregrip,
                world_scale,
            )
        });
        match fresh {
            Some(geometry) => {
                self.measured = Some((weapon.address, geometry));
                Some(geometry)
            }
            None => self
                .measured
                .filter(|(address, _)| *address == weapon.address)
                .map(|(_, geometry)| geometry),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    status: WeaponsStatus {
        active: false,
        volumes: [None; 2],
        candidates: 0,
        wielded: None,
        fire_blocked: false,
        events: Vec::new(),
        grabs: 0,
        last_miss: None,
        stance: None,
        docks_followed: 0,
    },
    weapon: None,
    gameplay: false,
    measured: None,
    placed: Vec::new(),
    grab_points: [None; 2],
    last_miss_log: None,
    last_holder: None,
    body_y: None,
    last_volumes: [None; 2],
    gun_side: Hand::Right,
});

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

/// Move the holsters with guns docked in them onto the guns ([`docks`]), by the guns'
/// displacement from the holsters placed on the character's skeleton. Returns how many moved.
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
    let anchor = volumes::Anchor::Skeleton(skeleton);
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
fn gameplay(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::OnFoot(OnFootVariant::Ground)
            | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute)
    )
}

/// The body the volumes ride: the character's skeleton when the setting asks for it and the
/// skeleton is fresh and upright, and the player's body turned to the estimated torso otherwise
/// (facing the cockpit frame's forward without an estimate).
fn volume_anchor(config: &ControllersConfig, active: &ActiveHands) -> volumes::Anchor {
    let skeleton = (config.weapons.holsters.anchor == HolsterAnchor::Rico)
        .then(|| alignment::skeleton_in_cockpit(active.head.position, active.world_scale))
        .flatten();
    let anchor = match skeleton {
        Some(skeleton) => volumes::Anchor::Skeleton(skeleton),
        None => volumes::Anchor::Body {
            yaw: alignment::estimate().map_or(0.0, |e| e.yaw),
        },
    };
    alignment::record_volume_anchor(anchor.label());
    anchor
}

/// The dimensions the volumes are placed with: the player's on the player's body; on the
/// character's skeleton, the character's while the hands are remapped onto them, and the player's
/// otherwise.
fn body_dimensions(config: &ControllersConfig, anchor: &volumes::Anchor) -> BodyDimensions {
    match anchor {
        volumes::Anchor::Skeleton(_) if config.body.remap_hands => {
            BodyDimensions::CHARACTER_ESTIMATE
        }
        _ => body::player(),
    }
}

/// Each hand's point for the body's volumes: the player's own grip, before the body-scale remap,
/// on the player's body, so that a hand on the player's own belt finds the pouch there; the grip
/// of the hand the player sees on the character's skeleton, whose landmarks are the character's.
fn body_points(active: &ActiveHands, anchor: &volumes::Anchor) -> [Option<Vec3>; 2] {
    match anchor {
        volumes::Anchor::Body { .. } => active.player_grips,
        volumes::Anchor::Skeleton(_) => active.hands.map(|h| h.map(|h| h.grip.position)),
    }
}

/// Record, and log at most once per [`NEAR_MISS_LOG_INTERVAL`], a grip press this tick that grabbed
/// nothing within [`NEAR_MISS_MARGIN_M`] of a holster, so that a reach that falls short in the
/// headset can be read from the log.
fn log_near_misses(grabs: &[Option<GrabVolume>; 2], grips: &[ButtonControl; 2]) {
    let mut state = STATE.lock();
    for hand in Hand::BOTH {
        let i = hand.index();
        if !grips[i].pressed || grabs[i].is_some() {
            continue;
        }
        let Some(point) = state.grab_points[i] else {
            continue;
        };
        let Some(miss) = volumes::near_miss(point, hand, &state.placed, NEAR_MISS_MARGIN_M) else {
            continue;
        };
        state.status.last_miss = Some((hand, miss));
        let now = Instant::now();
        if state
            .last_miss_log
            .is_some_and(|t| now.saturating_duration_since(t) < NEAR_MISS_LOG_INTERVAL)
        {
            continue;
        }
        state.last_miss_log = Some(now);
        tracing::debug!(
            target: "controllers",
            ?hand,
            nearest = miss.volume.label(),
            outside_m = miss.outside_m,
            offset_from_centre = ?miss.offset,
            stance = state.status.stance.map_or("none", Stance::label),
            "holsters: grip press outside the nearest holster"
        );
    }
}

/// The hands as the gun is shown on them before its recoil: the two-handed pose of a held
/// foregrip, then the weight ([`heft::apply`]). The volumes on a held gun (its foregrip, its well,
/// and its action) are placed from these, so that they are where the player sees them.
fn weighted_hands(active: &ActiveHands) -> [Option<CockpitHand>; 2] {
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

/// Whether the fire input is held back because no hand holding the wielded weapon pulls its
/// trigger while another hand's does: the one fire action reaches the weapon from either trigger,
/// and only a hand holding the weapon may fire it. The other hand is empty, or holds the second
/// weapon, which its trigger fires instead.
fn primary_trigger_idle(weapon: Option<HeldWeapon>, triggers: &[ButtonControl; 2]) -> bool {
    weapon.is_some_and(|w| {
        let hands = w.wielded.hands;
        let pulled = |h: Hand| triggers[h.index()].held();
        hands.contains(&true)
            && Hand::BOTH.into_iter().any(pulled)
            && !Hand::BOTH
                .into_iter()
                .any(|h| hands[h.index()] && pulled(h))
    })
}

/// Carry out `hand`'s holster decision. With independent pistol draws, a draw of the dual-wield
/// slot brings out only `hand`'s pistol. With `hold.instant_dock`, a long gun is drawn and
/// holstered without the game's acts ([`hold::act`]), falling back to the game's when that is
/// refused.
fn act(
    action: HolsterAction,
    hand: Hand,
    config: &ControllersConfig,
    desired: &mut Vec<Desired>,
) -> GrabOutcome {
    let independent = config.weapons.dual_wield.split && config.weapons.dual_wield.independent_draw;
    let settings = &config.weapons.hold;
    let pistols = docks_pistols(config);
    match action {
        HolsterAction::Draw { slot, .. } if docks_without_act(config, slot) => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::draw(hand, slot, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the draw falls back to the game's");
                    return act_with_game(action, hand, independent, desired);
                }
            }
        }
        HolsterAction::TakeSecond { slot } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            if let Err(refusal) = unsafe { second::take(hand, slot) } {
                tracing::debug!(target: "controllers", %refusal, "hold: no second weapon taken");
            }
            return GrabOutcome::Holster(action);
        }
        HolsterAction::PutBackSecond { .. } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            unsafe { second::put_back() };
            return GrabOutcome::Holster(action);
        }
        HolsterAction::DrawPistol { hand } if pistols => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::draw_pistol(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the pistol's draw falls back to the layout's");
                }
            }
        }
        HolsterAction::HolsterPistol { hand } if pistols => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::holster_pistol(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the pistol's holster falls back to the layout's");
                }
            }
        }
        HolsterAction::Holster { slot } if docks_without_act(config, slot) => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::holster(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the holster falls back to the game's");
                }
            }
        }
        _ => {}
    }
    act_with_game(action, hand, independent, desired)
}

/// Whether the pistols are drawn and holstered without the game's acts, each sliding between its hip
/// and its hand: with `hold.pistol_dock`, which needs each hand to draw its own pistol.
fn docks_pistols(config: &ControllersConfig) -> bool {
    let dual = config.weapons.dual_wield;
    let hold = config.weapons.hold;
    hold.either_hand && hold.pistol_dock && dual.split && dual.independent_draw
}

/// Whether `slot` is drawn and holstered without the game's act: the long guns with
/// `hold.instant_dock`, and the pistols as [`docks_pistols`] says.
fn docks_without_act(config: &ControllersConfig, slot: WeaponSlot) -> bool {
    let hold = config.weapons.hold;
    let long_guns = hold.either_hand && hold.instant_dock && hold::equip::handles(slot);
    long_guns || (slot == WeaponSlot::E_SLOT_DUAL_WIELD && docks_pistols(config))
}

/// Carry out `hand`'s holster decision through the game's own draw and holster.
fn act_with_game(
    action: HolsterAction,
    hand: Hand,
    independent: bool,
    desired: &mut Vec<Desired>,
) -> GrabOutcome {
    match action {
        HolsterAction::Draw {
            slot,
            action: select,
        } => {
            if independent && slot == WeaponSlot::E_SLOT_DUAL_WIELD {
                dual_wield::attach::begin_draw(hand);
            }
            hold::begin(hand, Cause::Draw, None);
            // The game acts on a slot select when its input is released, so a one-tick click
            // selects.
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: select,
                },
                want: Want::Click,
            });
        }
        HolsterAction::Holster { .. } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { engine::local_player() } {
                Some(owner) => unsafe { engine::holster(owner) },
                None => return GrabOutcome::NoCharacter,
            }
        }
        HolsterAction::DrawPistol { hand } => {
            dual_wield::attach::draw(hand);
        }
        HolsterAction::HolsterPistol { hand } => {
            dual_wield::attach::holster(hand);
        }
        HolsterAction::AlreadyDrawn { .. }
        | HolsterAction::Refused { .. }
        | HolsterAction::HandFull { .. }
        | HolsterAction::TakeSecond { .. }
        | HolsterAction::PutBackSecond { .. } => {}
    }
    GrabOutcome::Holster(action)
}

/// Carry out what opening `hand`'s grip on its gun does: holster it (without the game's act with
/// `hold.instant_dock`, through the game's holster otherwise), or let it fall.
fn release(hand: Hand, what: Release, wielded: holster::Wielded, config: &ControllersConfig) {
    let settings = &config.weapons.hold;
    let pair_held = wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD && wielded.hands == [true; 2];
    match what {
        // One pistol of a pair held in both hands goes back alone.
        Release::Holster if pair_held => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            let slid = docks_pistols(config)
                && unsafe { hold::act::holster_pistol(hand, settings.snap_s) }.is_ok();
            if !slid {
                dual_wield::attach::holster(hand);
            }
        }
        Release::Holster => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            let instant = docks_without_act(config, wielded.slot)
                && unsafe { hold::act::holster(hand, settings.snap_s) }.is_ok();
            if !instant && let Some(owner) = unsafe { engine::local_player() } {
                unsafe { engine::holster(owner) };
            }
        }
        Release::Drop => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            if let Err(refusal) = unsafe { hold::act::drop_held(hand, settings.max_throw_mps) } {
                tracing::debug!(target: "controllers", %refusal, "hold: the gun was not let go of");
            }
        }
        Release::Keep => {}
    }
}

/// Drop the fire inputs from the tables' output.
fn hold_fire_back(desired: &mut Vec<Desired>) {
    desired
        .retain(|d| !(d.target.map == MapKind::Player && FIRE_ACTIONS.contains(&d.target.action)));
}

/// Whether a reload, a draw, or a holster is playing with `gun_hand`, the hand holding the weapon
/// (or the hand the last one rode, during a draw), in the action-runs tier.
fn weapon_action_running(gun_hand: Hand) -> bool {
    let status = tiers::status();
    status.state == Some(TierState::WeaponAction)
        && status.effective[gun_hand.index()] == HandTier::ActionRuns
}

/// Which hands entered a body volume this tick, by [`Hand::index`]: a hand in a holster or a pouch
/// that it was not in on the last tick. Moving straight from one body volume to another counts as
/// entering the new one.
fn entered_body_volumes(
    previous: [Option<GrabVolume>; 2],
    now: [Option<GrabVolume>; 2],
) -> [bool; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        now[i].is_some_and(|v| is_body_volume(v) && previous[i] != Some(v))
    })
}

/// Whether a grab's outcome is one the grab cue pulses: a draw, a holster, or a foregrip grab.
/// The pouches pulse their own outcomes, and a release pulses from the transition, whatever
/// caused it.
fn cues_grab(outcome: GrabOutcome) -> bool {
    matches!(
        outcome,
        GrabOutcome::Holster(
            HolsterAction::Draw { .. }
                | HolsterAction::Holster { .. }
                | HolsterAction::DrawPistol { .. }
                | HolsterAction::HolsterPistol { .. }
                | HolsterAction::TakeSecond { .. }
                | HolsterAction::PutBackSecond { .. }
        ) | GrabOutcome::Foregrip(Transition::Grabbed)
            | GrabOutcome::Handover { moved: true }
            | GrabOutcome::Take { taken: true }
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::bindings::grab::HolsterSite;

    #[test]
    fn the_grips_are_free_only_on_foot_and_under_the_parachute() {
        assert!(gameplay(Mode::OnFoot(OnFootVariant::Ground)));
        assert!(gameplay(Mode::Airborne(AirborneVariant::Parachute)));
        assert!(!gameplay(Mode::Airborne(AirborneVariant::Wingsuit)));
        assert!(!gameplay(Mode::OnFoot(OnFootVariant::Swimming)));
        assert!(!gameplay(Mode::Ui));
    }

    #[test]
    fn a_hand_entering_a_holster_or_a_pouch_is_cued_once() {
        let hip = Some(GrabVolume::Holster(HolsterSite::Hip(Hand::Right)));
        let pouch = Some(GrabVolume::MagazinePouch);
        let foregrip = Some(GrabVolume::Foregrip);
        assert_eq!(
            entered_body_volumes([None, None], [pouch, hip]),
            [true, true]
        );
        // Staying in a volume is not entering it again.
        assert_eq!(
            entered_body_volumes([pouch, hip], [pouch, hip]),
            [false, false]
        );
        // Moving from one body volume straight into another is.
        assert_eq!(
            entered_body_volumes([hip, None], [pouch, None]),
            [true, false]
        );
        // The foregrip is not on the body; its grab is cued instead.
        assert_eq!(
            entered_body_volumes([None; 2], [foregrip, None]),
            [false, false]
        );
        assert!(cues_grab(GrabOutcome::Foregrip(Transition::Grabbed)));
        assert!(cues_grab(GrabOutcome::Holster(HolsterAction::Holster {
            slot: WeaponSlot::E_SLOT_TWO_HANDED
        })));
        assert!(!cues_grab(GrabOutcome::Holster(HolsterAction::Refused {
            slot: WeaponSlot::E_SLOT_TWO_HANDED
        })));
        assert!(!cues_grab(GrabOutcome::Magazine));
    }

    fn rifle_in(hand: Hand) -> HeldWeapon {
        HeldWeapon {
            address: 0x1000,
            wielded: holster::Wielded {
                slot: WeaponSlot::E_SLOT_TWO_HANDED,
                hands: Hand::BOTH.map(|h| h == hand),
            },
            holder: Some(hand),
            two_handed: true,
            class: heft::HeftClass::of(0, true),
        }
    }

    fn triggers(left: bool, right: bool) -> [ButtonControl; 2] {
        let pulled = |down| ButtonControl {
            down,
            ..ButtonControl::default()
        };
        [pulled(left), pulled(right)]
    }

    #[test]
    fn only_a_hand_holding_the_gun_fires_it() {
        let left_rifle = Some(rifle_in(Hand::Left));
        // The other trigger alone fires nothing while the gun is in this hand.
        assert!(primary_trigger_idle(left_rifle, &triggers(false, true)));
        // The holding hand's trigger fires it, with or without the other trigger.
        assert!(!primary_trigger_idle(left_rifle, &triggers(true, false)));
        assert!(!primary_trigger_idle(left_rifle, &triggers(true, true)));
        // No trigger pulled holds nothing back.
        assert!(!primary_trigger_idle(left_rifle, &triggers(false, false)));
        // Mirrored: a second weapon's trigger in the left hand does not fire the right-held gun.
        let right_rifle = Some(rifle_in(Hand::Right));
        assert!(primary_trigger_idle(right_rifle, &triggers(true, false)));
        assert!(!primary_trigger_idle(right_rifle, &triggers(false, true)));
        // Pistols in both hands fire from either trigger, and no weapon is the game's as before.
        let mut pair = rifle_in(Hand::Right);
        pair.wielded.hands = [true; 2];
        assert!(!primary_trigger_idle(Some(pair), &triggers(true, false)));
        assert!(!primary_trigger_idle(None, &triggers(true, true)));
    }

    #[test]
    fn pistols_dock_only_when_each_hand_draws_its_own() {
        let mut config = ControllersConfig::new();
        let pistols = WeaponSlot::E_SLOT_DUAL_WIELD;
        let rifle = WeaponSlot::E_SLOT_TWO_HANDED;
        assert!(docks_without_act(&config, pistols));
        assert!(docks_without_act(&config, rifle));
        config.weapons.dual_wield.independent_draw = false;
        assert!(!docks_without_act(&config, pistols));
        assert!(docks_without_act(&config, rifle));
        config.weapons.dual_wield.independent_draw = true;
        config.weapons.hold.pistol_dock = false;
        assert!(!docks_without_act(&config, pistols));
        config.weapons.hold.pistol_dock = true;
        config.weapons.hold.instant_dock = false;
        assert!(
            docks_without_act(&config, pistols),
            "the switches are separate"
        );
        assert!(!docks_without_act(&config, rifle));
        assert!(!docks_without_act(&config, WeaponSlot::E_SLOT_HEAVY));
    }

    #[test]
    fn a_weapon_action_holds_only_the_fire_inputs_back() {
        let want = |action| Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Hold(1.0),
        };
        let mut desired = vec![
            want(Action::FIRE_RIGHT),
            want(Action::R2),
            want(Action::JUMP),
        ];
        hold_fire_back(&mut desired);
        assert_eq!(desired, vec![want(Action::JUMP)]);
    }
}
