//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads",
//! "Grenades", "Melee", and "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the body's grab volumes (the holsters, the grenade pouch, and the magazine
//!   pouch), placed from the body dimensions;
//! - [`holster`]: what a grab at a holster does: draw its slot or holster the held weapon;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`recoil`]: the recoil offset between the controller and the gun, with its return spring;
//! - [`fire`]: the per-shot seam that kicks the recoil and marks the shot's hand;
//! - [`haptics`]: the game's rumbles and the grapple's pulses routed to the hands;
//! - [`dual_wield`]: each dual-wielded pistol fired by its own hand's trigger;
//! - [`camera_kick`]: the game's camera recoil, suppressed while the guns aim by hand;
//! - [`grenade`]: the physical grenade throw;
//! - [`melee`]: melee attacks from physical strikes;
//! - [`magazine`]: the magazine reload gesture;
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
//!    for the throw, and a magazine-pouch grab takes a magazine for the reload gesture. It runs the
//!    throw, the strike detector, and the magazine gesture, and holds the fire inputs back while a
//!    reload, a draw, or a holster plays (the action-runs hand tier).
//!
//! Guns are sticky: nothing here draws or holsters except a grab. The grab-volume highlights are
//! submitted to the overlay's [`Source::GrabVolumes`] slot. Everything acts only while the bindings
//! inject, which is only under the controller scheme with a controller active.

mod camera_kick;
pub mod config;
pub mod dual_wield;
mod engine;
mod fire;
pub mod foregrip;
pub mod grenade;
pub mod haptics;
pub mod holster;
pub mod magazine;
pub mod melee;
pub mod recoil;
pub mod volumes;

use std::time::Duration;

use glam::{Quat, Vec3};
use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};
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
        weapons::{
            engine::HeldWeapon,
            foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
            holster::HolsterAction,
            magazine::{GestureEvent, GestureInputs, MagazineGesture},
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
}

/// What the first half of the weapon handling's input tick hands the bindings.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TickStart {
    /// The grab volume each hand is in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// Whether the dual-wield split acts: a pistol in each hand, on foot, with the split on.
    pub dual_wield: bool,
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
    /// A magazine-pouch grab: a magazine is taken for the reload gesture.
    Magazine,
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
            GrabOutcome::Magazine => "magazine taken".to_owned(),
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
    pub wielded: Option<WieldedReadout>,
    /// Whether the fire inputs were held back for a weapon action on the last tick.
    pub fire_blocked: bool,
    /// The most recent grabs, newest last.
    pub events: Vec<GrabEvent>,
    pub grabs: u64,
    /// The magazine reload gesture.
    pub magazine: MagazineGesture,
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
    // SAFETY: the input tick runs on the main thread outside the world update.
    let weapon = unsafe {
        engine::local_player().and_then(|owner| {
            dual_wield::place(
                owner,
                config.dominant_hand,
                dual.split && dual.place_by_handedness,
            );
            engine::held_weapon(owner)
        })
    };
    let dual_wield = dual.split
        && mode == Mode::OnFoot(OnFootVariant::Ground)
        && weapon.is_some_and(|w| {
            w.wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD && w.wielded.hands == [true; 2]
        });

    let mut state = STATE.lock();
    let geometry = weapon
        .filter(|w| w.two_handed && gameplay)
        .and_then(|w| state.geometry_for(&w, &config, active.world_scale));

    let mut candidates = Vec::new();
    let origin = volumes::body_origin(
        active.head.position,
        config.weapons.holsters.crouch_deadband_m,
    );
    let dims = body_dimensions(&config);
    if gameplay && (config.weapons.holsters.enabled || config.weapons.grenade_throw) {
        candidates = volumes::holster_candidates(
            origin,
            &dims,
            &config.weapons.holsters,
            config.dominant_hand,
            config.weapons.grenade_throw,
        );
        if !config.weapons.holsters.enabled {
            candidates.retain(|c| c.volume == GrabVolume::GrenadePouch);
        }
    }
    let off = config.dominant_hand.other();
    let well = weapon
        .filter(|w| {
            gameplay
                && config.weapons.reload_gesture
                && w.holder == Some(config.dominant_hand)
                && !w.wielded.hands[off.index()]
        })
        .and_then(|w| {
            let holder = w.holder?;
            let wrist = active.hands[holder.index()]?.wrist;
            Some(magazine::well_point(
                wrist,
                holder,
                config.weapons.magazine.well_offset_m,
            ))
        });
    if well.is_some() {
        candidates.push(volumes::magazine_pouch(
            origin,
            &dims,
            &config.weapons.magazine.pouch,
            off,
        ));
    }
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
    state.status.wielded = weapon.map(|weapon| WieldedReadout { weapon, geometry });
    state.weapon = weapon.map(|w| (w, geometry));
    state.gameplay = gameplay;
    state.well = well;
    let holding_magazine = state.status.magazine.stage != magazine::MagazineStage::Idle;
    drop(state);

    let well_ring = well
        .filter(|_| holding_magazine)
        .map(|centre| (centre, config.weapons.magazine.well_radius_m));
    submit_highlights(&config, &active, &candidates, volumes, well_ring);
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
    pub detection: Detection,
    /// The engine's tick delta, seconds.
    pub dt: f32,
}

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw, the strike detector, and the magazine gesture, and hold the fire inputs back during a
/// weapon action. `desired` is the tables' output, which a draw adds its slot-select click to, a
/// strike its melee click, and an inserted magazine its reload click.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let FinishInputs {
        grabs,
        grips,
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
                match holster::decide(site, hand, config.dominant_hand, wielded) {
                    Some(action) => act(action, desired),
                    None => continue,
                }
            }
            GrabVolume::Foregrip => match transition {
                Some(t) => GrabOutcome::Foregrip(t),
                None => GrabOutcome::ForegripUnmeasured,
            },
            GrabVolume::GrenadePouch => GrabOutcome::Pouch,
            GrabVolume::MagazinePouch => GrabOutcome::Magazine,
            GrabVolume::VehicleControl(_) => continue,
        };
        tracing::debug!(target: "controllers", ?hand, volume = volume.label(), outcome = %outcome.label(), "grab");
        events.push(GrabEvent {
            hand,
            volume,
            outcome,
        });
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
    match &active {
        Some(active) if config.weapons.melee => melee::input_tick(
            &melee::TickInputs {
                acting: true,
                excluded: grenade::throwing_hand(),
                active,
                detection,
                dt,
            },
            &config,
            desired,
        ),
        _ => melee::stand_down(),
    }
    let magazine = magazine_tick(&config, active.as_ref(), grabs, dt);
    if magazine == Some(GestureEvent::Inserted) {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: Action::RELOAD,
            },
            want: Want::Click,
        });
    }

    let fire_blocked =
        config.weapons.block_fire_during_actions && weapon_action_running(config.dominant_hand);
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
    recoil::clear();
    grenade::stand_down();
    melee::stand_down();
    dual_wield::observe_triggers(false, [dual_wield::TriggerInput::default(); 2]);
    // SAFETY: the bindings stand down on the main thread outside the world update.
    if let Some(owner) = unsafe { engine::local_player() } {
        unsafe { dual_wield::place(owner, Hand::Right, false) };
    }
    let mut state = STATE.lock();
    state.weapon = None;
    state.well = None;
    state.status.magazine.reset();
    state.status.active = false;
    state.status.volumes = [None; 2];
    state.status.fire_blocked = false;
    store::clear(Source::GrabVolumes);
}

/// The fire inputs a weapon action holds back.
const FIRE_ACTIONS: [Action; 2] = [Action::FIRE_RIGHT, Action::R2];

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
    /// The held weapon's magazine well in the cockpit frame, while the magazine gesture can reload
    /// it.
    well: Option<Vec3>,
}

impl State {
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
        magazine: MagazineGesture::new(),
    },
    weapon: None,
    gameplay: false,
    measured: None,
    well: None,
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

/// Step the magazine gesture: `grabs` says whether the off-hand took a magazine this tick.
fn magazine_tick(
    config: &ControllersConfig,
    active: Option<&ActiveHands>,
    grabs: [Option<GrabVolume>; 2],
    dt: f32,
) -> Option<GestureEvent> {
    let off = config.dominant_hand.other();
    let mut state = STATE.lock();
    if !config.weapons.reload_gesture {
        state.status.magazine.reset();
        return None;
    }
    let inputs = GestureInputs {
        taken: grabs[off.index()] == Some(GrabVolume::MagazinePouch),
        hand: active
            .and_then(|a| a.hands[off.index()])
            .map(|h| h.grip.position),
        well: state.well,
        dt,
    };
    let event = state.status.magazine.step(inputs, &config.weapons.magazine);
    drop(state);
    if let Some(event) = event {
        tracing::debug!(target: "controllers", ?event, "magazine gesture");
        if event != GestureEvent::Dropped {
            haptics::pulse(
                off,
                config.weapons.magazine.pulse,
                haptics::PulseSource::Magazine,
            );
        }
    }
    event
}

/// Carry out a holster decision.
fn act(action: HolsterAction, desired: &mut Vec<Desired>) -> GrabOutcome {
    match action {
        HolsterAction::Draw { action: select, .. } => {
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
    well: Option<(Vec3, f32)>,
) {
    if !(config.overlay.enabled && config.overlay.grab_volumes) {
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
    for volume in volumes.iter().flatten() {
        if let Some(c) = candidates.iter().find(|c| c.volume == *volume) {
            prims.push(ring(
                to_world(c.centre),
                c.radius * active.world_scale,
                colour(*volume),
            ));
        }
    }
    if let Some((centre, radius)) = well {
        prims.push(ring(
            to_world(centre),
            radius * active.world_scale,
            MAGAZINE,
        ));
    }
    if let Some(hold) = foregrip::status().hold.filter(|h| h.engaged) {
        let mut posed = active.hands;
        foregrip::apply(&mut posed, &config.weapons.foregrip);
        if let Some(off) = posed[hold.holder.other().index()] {
            prims.push(marker(to_world(off.grip.position), active.world_scale));
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

fn marker(at: Vec3, world_scale: f32) -> Primitive {
    Primitive::Sprite {
        at: Anchor::World(at),
        size: SpriteSize::World(0.03 * world_scale),
        facing: Facing::Billboard,
        shape: Shape::Disc,
        color: FOREGRIP,
        depth: DepthMode::XRay {
            occluded_alpha: 0.4,
        },
    }
}

fn colour(volume: GrabVolume) -> Rgba {
    match volume {
        GrabVolume::Holster(_) => HOLSTER,
        GrabVolume::Foregrip => FOREGRIP,
        GrabVolume::GrenadePouch | GrabVolume::VehicleControl(_) => POUCH,
        GrabVolume::MagazinePouch => MAGAZINE,
    }
}

const HOLSTER: Rgba = [1.0, 0.75, 0.25, 0.8];
const FOREGRIP: Rgba = [0.35, 0.85, 1.0, 0.8];
const POUCH: Rgba = [0.5, 1.0, 0.45, 0.8];
const MAGAZINE: Rgba = [0.85, 0.55, 1.0, 0.8];

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
