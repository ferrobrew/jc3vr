//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads",
//! "Grenades", "Melee", and "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the body's grab volumes (the holsters, the grenade pouch, the reload pouch, and
//!   the C4 pouch), placed from the body dimensions;
//! - [`holster`]: what a grab at a holster does: draw its slot or holster the held weapon;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`recoil`]: the recoil offset between the controller and the gun, with its return spring;
//! - [`heft`]: the one-handed weight of a long gun: sag, lag, sway, a stronger kick, and a wider
//!   spread, gone while it is braced on the foregrip;
//! - [`fire`]: the per-shot seam that kicks the recoil and marks the shot's hand;
//! - [`grip_watch`]: a diagnostic watch on how each held gun sits in its hand;
//! - [`haptics`]: the game's rumbles and the grapple's pulses routed to the hands;
//! - [`dual_wield`]: each dual-wielded pistol drawn by its own hand and fired by its own hand's
//!   trigger;
//! - [`camera_kick`]: the game's camera recoil, suppressed while the guns aim by hand;
//! - [`grenade`]: the physical grenade throw;
//! - [`c4`]: the C4 throw from the chest pouch and the wrist detonator;
//! - [`melee`]: melee attacks from physical strikes;
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
//!    latch at the grip's press edge. A hand holding the foregrip is always in it, so that its next
//!    grip press lets go rather than firing the grapple.
//! 2. [`finish_tick`] acts on the grabs the tables report: a holster grab draws or holsters, a
//!    foregrip grab engages or releases the two-handed grip, a grenade-pouch grab primes a grenade
//!    for the throw, a C4-pouch grab primes a charge, and a reload-pouch grab takes a round for the
//!    reload. It runs the throws, the wrist detonator, the strike detector, and the reload, and
//!    holds the fire inputs back while a reload, a draw, or a holster plays (the action-runs hand
//!    tier) and while the reload holds the gun from firing.
//!
//! Guns are sticky: nothing here draws or holsters except a grab. The grab-volume highlights (and,
//! with `holsters.show_volumes`, every body volume) are submitted to the overlay's
//! [`Source::GrabVolumes`] slot. A grip press that grabs nothing near a holster is logged with its
//! distance from the nearest one. Everything acts only while the bindings inject, which is only
//! under the controller scheme with a controller active.

pub mod c4;
mod camera_kick;
pub mod config;
pub mod dual_wield;
pub mod empty;
mod engine;
mod fire;
pub mod foregrip;
pub mod grenade;
pub mod grip_watch;
pub mod haptics;
pub mod heft;
pub mod holster;
pub mod melee;
pub mod recoil;
pub mod reload;
pub mod scope;
pub mod unarmed_fire;
pub mod volumes;
pub mod wheel;

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
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
            grab::{self, Candidate, GrabVolume},
            hold::{Desired, MapKind, Target, Want},
        },
        body::{self, BodyDimensions},
        config::ControllersConfig,
        frame::{Hand, Pose},
        hands::{
            self, ActiveHands, ik, pose as hand_pose,
            tiers::{self, HandTier, TierState},
        },
        mode::{AirborneVariant, Detection, Mode, OnFootVariant},
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        roomscale::{self, posture::EffectivePosture},
        weapons::{
            engine::HeldWeapon,
            foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
            holster::HolsterAction,
            volumes::{NearMiss, Stance},
        },
    },
    headpose,
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
        .with_hook_library(dual_wield::hook_library())
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
}

/// What a grab did, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GrabOutcome {
    Holster(HolsterAction),
    /// A holster grab with no local player to act on.
    NoCharacter,
    Foregrip(Transition),
    /// A foregrip grab that could not engage: the weapon's geometry is not measured yet.
    ForegripUnmeasured,
    /// A grenade-pouch grab: a grenade is asked for.
    Pouch,
    /// A reload-pouch grab: a round is taken for the reload.
    Magazine,
    /// A grab of a gun's slide, charging handle, or bolt.
    Action,
    /// A C4-pouch grab: a charge is asked for.
    C4,
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
            GrabOutcome::ForegripUnmeasured => "foregrip not measured yet".to_owned(),
            GrabOutcome::Pouch => "grenade asked for".to_owned(),
            GrabOutcome::Magazine => "reload pouch".to_owned(),
            GrabOutcome::Action => "action grabbed".to_owned(),
            GrabOutcome::C4 => "charge asked for".to_owned(),
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
            dominant: config.dominant_hand,
            independent: dual.independent_draw,
            by_handedness: dual.place_by_handedness,
        },
    );
    // SAFETY: the input tick runs on the main thread outside the world update.
    let weapon = unsafe {
        engine::local_player().and_then(|owner| {
            let pair = dual_wield::attach::input_tick(owner, settings);
            engine::held_weapon(owner).map(|w| with_pair(w, pair))
        })
    };
    scope::input_tick(weapon.map(|w| w.address));
    // SAFETY: the weapon was read this tick, on the main thread outside the world update.
    if let Some(base) = weapon.and_then(|w| unsafe { (w.address as *const WeaponBase).as_ref() }) {
        unsafe { grip_watch::observe(base) };
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

    let mut state = STATE.lock();
    let geometry = weapon
        .filter(|w| w.two_handed && gameplay)
        .and_then(|w| state.geometry_for(&w, &config, active.world_scale));

    let mut candidates = Vec::new();
    let stance = state.step_stance();
    let body_frame = volumes::body_frame(
        active.head.position,
        stance,
        config.weapons.holsters.crouch_deadband_m,
    );
    let dims = body_dimensions(&config);
    if gameplay && (config.weapons.holsters.enabled || config.weapons.grenade_throw) {
        candidates = volumes::holster_candidates(
            &body_frame,
            &dims,
            &config.weapons.holsters,
            config.dominant_hand,
            config.weapons.grenade_throw,
        );
        if !config.weapons.holsters.enabled {
            candidates.retain(|c| c.volume == GrabVolume::GrenadePouch);
        }
    }
    if gameplay && config.weapons.c4_throw {
        candidates.push(volumes::c4_pouch(
            &body_frame,
            &dims,
            &config.weapons.c4.pouch,
            config.dominant_hand,
        ));
    }
    candidates.extend(reload::begin_tick(&reload::BeginInputs {
        weapon: weapon.as_ref(),
        active: &active,
        config: &config,
        body_frame: &body_frame,
        dims: &dims,
        gameplay,
    }));
    let hold = foregrip::status().hold.filter(|h| h.engaged);
    let foregrip_hand = weapon.and_then(|w| w.holder).map(Hand::other);
    if config.weapons.foregrip.enabled
        && hold.is_none()
        && let (Some(g), Some(w), Some(off)) = (geometry, weapon, foregrip_hand)
        && let Some(holder) = w.holder.and_then(|h| active.hands[h.index()])
    {
        candidates.push(Candidate {
            volume: GrabVolume::Foregrip,
            centre: foregrip::foregrip_point(&holder, &g),
            radius: config.weapons.foregrip.radius_m,
            hand: Some(off),
        });
    }

    let volumes = Hand::BOTH.map(|hand| {
        if hold.is_some_and(|h| h.holder.other() == hand) {
            return Some(GrabVolume::Foregrip);
        }
        let point = active.hands[hand.index()]?.grip.position;
        grab::grab_volume(point, hand, &candidates)
    });

    state.status.active = true;
    state.status.volumes = volumes;
    state.status.candidates = candidates.len();
    state.status.stance = Some(stance);
    state.placed.clone_from(&candidates);
    state.grab_points = active.hands.map(|h| h.map(|h| h.grip.position));
    state.status.wielded = weapon.map(|weapon| WieldedReadout { weapon, geometry });
    state.weapon = weapon.map(|w| (w, geometry));
    state.gameplay = gameplay;
    drop(state);

    submit_highlights(&config, &active, &candidates, volumes);
    TickStart {
        volumes,
        dual_wield,
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
    pub detection: Detection,
    /// The engine's tick delta, seconds.
    pub dt: f32,
}

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw, the strike detector, and the reload, and hold the fire inputs back during a weapon
/// action. `desired` is the tables' output, which a draw adds its slot-select click to and a strike
/// its melee click.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let FinishInputs {
        grabs,
        grips,
        triggers,
        reload_button,
        detection,
        dt,
    } = *inputs;
    let config = Config::lock_query(|c| c.controllers);
    let active = hands::active_hands_raw();
    let (weapon, gameplay) = {
        let state = STATE.lock();
        (state.weapon, state.gameplay)
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
    };
    let transition = foregrip::tick(&inputs, dt, &config.weapons.foregrip);

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
                match holster::decide(site, hand, config.dominant_hand, wielded, independent) {
                    Some(action) => act(action, hand, independent, desired),
                    None => continue,
                }
            }
            GrabVolume::Foregrip => match transition {
                Some(t) => GrabOutcome::Foregrip(t),
                None => GrabOutcome::ForegripUnmeasured,
            },
            GrabVolume::GrenadePouch => GrabOutcome::Pouch,
            GrabVolume::MagazinePouch => GrabOutcome::Magazine,
            GrabVolume::WeaponAction => GrabOutcome::Action,
            GrabVolume::C4Pouch => GrabOutcome::C4,
            GrabVolume::VehicleControl(_) | GrabVolume::ParachuteHandle(_) | GrabVolume::Pickup => {
                continue;
            }
        };
        tracing::debug!(target: "controllers", ?hand, volume = volume.label(), outcome = %outcome.label(), "grab");
        events.push(GrabEvent {
            hand,
            volume,
            outcome,
        });
    }
    if gameplay {
        log_near_misses(&grabs, &grips);
    }

    grenade::input_tick(
        &grenade::TickInputs {
            acting: config.weapons.grenade_throw && gameplay && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePouch)),
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
            dt,
        },
        &config,
    );
    match &active {
        Some(active) if config.weapons.melee => melee::input_tick(
            &melee::TickInputs {
                acting: true,
                excluded: grenade::throwing_hand().or_else(c4::throwing_hand),
                active,
                detection,
                dt,
            },
            &config,
            desired,
        ),
        _ => melee::stand_down(),
    }
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

    let fire_blocked = reload_blocks
        || (config.weapons.block_fire_during_actions
            && weapon_action_running(config.dominant_hand));
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
    grip_watch::clear();
    recoil::clear();
    grenade::stand_down();
    c4::stand_down();
    c4::wrist::stand_down();
    melee::stand_down();
    dual_wield::observe_triggers(None);
    dual_wield::attach::clear();
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

/// The highlights are submitted at the input tick's rate, so they live a little past one tick.
const HIGHLIGHT_TTL: Duration = Duration::from_millis(150);

struct State {
    status: WeaponsStatus,
    /// The tick's wielded weapon and its two-handed geometry.
    weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// Whether the tick's mode is one the weapon handling acts in.
    gameplay: bool,
    /// The last geometry measured, and the weapon it belongs to: a measurement needs the wrist at
    /// full weight, so a tick without one keeps the last.
    measured: Option<(usize, GripGeometry)>,
    /// The tick's placed volumes and each hand's grab point, for the near-miss log.
    placed: Vec<Candidate>,
    grab_points: [Option<Vec3>; 2],
    last_miss_log: Option<Instant>,
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
    },
    weapon: None,
    gameplay: false,
    measured: None,
    placed: Vec::new(),
    grab_points: [None; 2],
    last_miss_log: None,
});

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

/// The dimensions the volumes are placed with: the character's while the hands are remapped onto
/// them, the player's otherwise.
fn body_dimensions(config: &ControllersConfig) -> BodyDimensions {
    if config.body.remap_hands {
        BodyDimensions::CHARACTER_ESTIMATE
    } else {
        body::player()
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

/// Carry out `hand`'s holster decision. With `independent` pistol draws, a draw of the dual-wield
/// slot brings out only `hand`'s pistol.
fn act(
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
        HolsterAction::AlreadyDrawn { .. } => {}
    }
    GrabOutcome::Holster(action)
}

/// Drop the fire inputs from the tables' output.
fn hold_fire_back(desired: &mut Vec<Desired>) {
    desired
        .retain(|d| !(d.target.map == MapKind::Player && FIRE_ACTIONS.contains(&d.target.action)));
}

/// Whether a reload, a draw, or a holster is playing with the dominant hand in the action-runs
/// tier.
fn weapon_action_running(dominant: Hand) -> bool {
    let status = tiers::status();
    status.state == Some(TierState::WeaponAction)
        && status.effective[dominant.index()] == HandTier::ActionRuns
}

/// Submit a highlight for the volume each hand is in, and a marker on a held foregrip.
fn submit_highlights(
    config: &ControllersConfig,
    active: &ActiveHands,
    candidates: &[Candidate],
    volumes: [Option<GrabVolume>; 2],
) {
    let show_all = config.weapons.holsters.show_volumes;
    if !(config.overlay.enabled && (config.overlay.grab_volumes || show_all)) {
        store::clear(Source::GrabVolumes);
        return;
    }
    let head = headpose::query();
    let body = headpose::xr::body_rotation();
    let to_world = |point: Vec3| {
        hand_pose::compose_world(
            head.position,
            body,
            active.head.position,
            Pose {
                position: point,
                orientation: Quat::IDENTITY,
            },
            active.world_scale,
        )
        .position
    };
    let mut prims = Vec::new();
    if show_all {
        for c in candidates
            .iter()
            .filter(|c| is_body_volume(c.volume) && !volumes.contains(&Some(c.volume)))
        {
            let [r, g, b, _] = colour(c.volume);
            prims.push(ring(
                to_world(c.centre),
                c.radius * active.world_scale,
                [r, g, b, SHOWN_ALPHA],
            ));
        }
    }
    for volume in volumes.iter().flatten() {
        if let Some(c) = candidates.iter().find(|c| c.volume == *volume) {
            prims.push(ring(
                to_world(c.centre),
                c.radius * active.world_scale,
                colour(*volume),
            ));
        }
    }
    if let Some((centre, radius)) = reload::well_ring(&config.weapons.reload) {
        prims.push(ring(
            to_world(centre),
            radius * active.world_scale,
            MAGAZINE,
        ));
    }
    if let Some(hand) = reload::holding_hand()
        && let Some(held) = active.hands[hand.index()]
    {
        prims.push(marker(
            to_world(held.grip.position),
            active.world_scale,
            MAGAZINE,
        ));
    }
    if let Some(hold) = foregrip::status().hold.filter(|h| h.engaged) {
        let mut posed = active.hands;
        foregrip::apply(&mut posed, &config.weapons.foregrip);
        if let Some(off) = posed[hold.holder.other().index()] {
            prims.push(marker(
                to_world(off.grip.position),
                active.world_scale,
                FOREGRIP,
            ));
        }
    }
    store::submit(Source::GrabVolumes, &mut prims, HIGHLIGHT_TTL);
}

fn ring(centre: Vec3, radius: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(centre),
        size: SpriteSize::World(radius * 2.0),
        facing: Facing::Billboard,
        shape: Shape::Ring { thickness: 0.15 },
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.4,
        },
    }
}

fn marker(at: Vec3, world_scale: f32, color: Rgba) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(at),
        size: SpriteSize::World(0.03 * world_scale),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.4,
        },
    }
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

fn colour(volume: GrabVolume) -> Rgba {
    match volume {
        GrabVolume::Holster(_) => HOLSTER,
        GrabVolume::Foregrip => FOREGRIP,
        GrabVolume::GrenadePouch
        | GrabVolume::VehicleControl(_)
        | GrabVolume::ParachuteHandle(_)
        | GrabVolume::Pickup => POUCH,
        GrabVolume::MagazinePouch | GrabVolume::WeaponAction => MAGAZINE,
        GrabVolume::C4Pouch => C4,
    }
}

const HOLSTER: Rgba = [1.0, 0.75, 0.25, 0.8];
const FOREGRIP: Rgba = [0.35, 0.85, 1.0, 0.8];
const POUCH: Rgba = [0.5, 1.0, 0.45, 0.8];
const MAGAZINE: Rgba = [0.85, 0.55, 1.0, 0.8];
const C4: Rgba = [1.0, 0.4, 0.3, 0.8];
/// The opacity of a body volume drawn with `show_volumes` that no hand is in, fainter than a
/// highlighted one.
const SHOWN_ALPHA: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grips_are_free_only_on_foot_and_under_the_parachute() {
        assert!(gameplay(Mode::OnFoot(OnFootVariant::Ground)));
        assert!(gameplay(Mode::Airborne(AirborneVariant::Parachute)));
        assert!(!gameplay(Mode::Airborne(AirborneVariant::Wingsuit)));
        assert!(!gameplay(Mode::OnFoot(OnFootVariant::Swimming)));
        assert!(!gameplay(Mode::Ui));
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
