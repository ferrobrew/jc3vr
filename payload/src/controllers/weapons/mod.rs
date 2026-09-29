//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads", and
//! "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the body's grab volumes (the holsters and the grenade pouch), placed from the
//!   body dimensions;
//! - [`holster`]: what a grab at a holster does: draw its slot or holster the held weapon;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`config`]: the holster and foregrip settings.
//!
//! The weapon handling runs on the input tick, in two halves around the bindings' tables
//! ([`crate::controllers::bindings::input_tick`]):
//!
//! 1. [`begin_tick`] reads the wielded weapon, places the grab volumes, and answers which volume
//!    each hand's grab point (the controller's grip, in the cockpit frame) is in, which the tables
//!    latch at the grip's press edge. A hand holding the foregrip is always in it, so that its next
//!    grip press lets go rather than firing the grapple.
//! 2. [`finish_tick`] acts on the grabs the tables report: a holster grab draws or holsters, a
//!    foregrip grab engages or releases the two-handed grip, and a grenade-pouch grab does nothing
//!    until the physical throw exists. It also holds the fire inputs back while a reload, a draw, or
//!    a holster plays (the action-runs hand tier).
//!
//! Guns are sticky: nothing here draws or holsters except a grab. The grab-volume highlights are
//! submitted to the overlay's [`Source::GrabVolumes`] slot. Everything acts only while the bindings
//! inject, which is only under the controller scheme with a controller active.

pub mod config;
mod engine;
pub mod foregrip;
pub mod holster;
pub mod volumes;

use std::time::Duration;

use glam::{Quat, Vec3};
use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        bindings::{
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
        mode::{AirborneVariant, Mode, OnFootVariant},
        overlay::{
            primitive::{Anchor, DepthMode, Facing, Primitive, Rgba, Shape, Source, SpriteSize},
            store,
        },
        weapons::{
            engine::HeldWeapon,
            foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
            holster::HolsterAction,
        },
    },
    headpose,
};

/// What a grab did, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GrabOutcome {
    Holster(HolsterAction),
    /// A holster grab with no local player to act on.
    NoCharacter,
    Foregrip(Transition),
    /// A foregrip grab that could not engage: the weapon's geometry is not measured yet.
    ForegripUnmeasured,
    /// A grenade-pouch grab: the physical throw does not exist yet.
    Pouch,
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
            GrabOutcome::Pouch => "grenade pouch (the throw is not built)".to_owned(),
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
}

pub fn status() -> WeaponsStatus {
    STATE.lock().status.clone()
}

/// The first half of the weapon handling's input tick: read the weapon, place the volumes, and
/// return the volume each hand is in. Called while the bindings inject, before the tables.
pub(crate) fn begin_tick(mode: Mode) -> [Option<GrabVolume>; 2] {
    let config = Config::lock_query(|c| c.controllers);
    let Some(active) = hands::active_hands_raw() else {
        stand_down();
        return [None; 2];
    };
    let gameplay = gameplay(mode);
    // SAFETY: the input tick runs on the main thread outside the world update.
    let weapon = unsafe { engine::local_player().and_then(|owner| engine::held_weapon(owner)) };

    let mut state = STATE.lock();
    let geometry = weapon
        .filter(|w| w.two_handed && gameplay)
        .and_then(|w| state.geometry_for(&w, &config, active.world_scale));

    let mut candidates = Vec::new();
    if gameplay && config.weapons.holsters.enabled {
        let origin = volumes::body_origin(
            active.head.position,
            config.weapons.holsters.crouch_deadband_m,
        );
        let dims = body_dimensions(&config);
        candidates = volumes::holster_candidates(
            origin,
            &dims,
            &config.weapons.holsters,
            config.dominant_hand,
        );
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
    drop(state);

    submit_highlights(&config, &active, &candidates, volumes);
    volumes
}

/// The second half of the weapon handling's input tick: act on the tick's grabs and hold the fire
/// inputs back during a weapon action. `desired` is the tables' output, which a draw adds its
/// slot-select click to.
pub(crate) fn finish_tick(grabs: [Option<GrabVolume>; 2], desired: &mut Vec<Desired>, dt: f32) {
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
            GrabVolume::VehicleControl => continue,
        };
        tracing::debug!(target: "controllers", ?hand, volume = volume.label(), outcome = %outcome.label(), "grab");
        events.push(GrabEvent {
            hand,
            volume,
            outcome,
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
    let mut state = STATE.lock();
    state.weapon = None;
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
    },
    weapon: None,
    gameplay: false,
    measured: None,
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
        && status.tiers[dominant.index()].effective() == HandTier::ActionRuns
}

/// Submit a highlight for the volume each hand is in, and a marker on a held foregrip.
fn submit_highlights(
    config: &ControllersConfig,
    active: &ActiveHands,
    candidates: &[Candidate],
    volumes: [Option<GrabVolume>; 2],
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
        GrabVolume::GrenadePouch | GrabVolume::VehicleControl => POUCH,
    }
}

const HOLSTER: Rgba = [1.0, 0.75, 0.25, 0.8];
const FOREGRIP: Rgba = [0.35, 0.85, 1.0, 0.8];
const POUCH: Rgba = [0.5, 1.0, 0.45, 0.8];

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
