//! Picking things up with an empty hand. The design is in `docs/mod/input/physical-hands.md`, the
//! engine side in `docs/engine/gameplay/item-pickup.md`.
//!
//! Each simulation tick, from the palms' tick ([`super::after_aim`]), every empty hand that holds
//! nothing looks for what it could take: short rays out of the palm, and the surface the palm
//! presses into ([`reach`]). The nearest object it reaches that a hand takes is the hand's target:
//! a loose rigid object light enough to move, or a weapon lying in the world.
//!
//! The grip decides, on the input tick: a hand with a target and no other grab volume reports the
//! [`GrabVolume::Pickup`] volume to the bindings, which latch the grip press to it, so the press
//! neither grapples nor reaches the game. A pickup grab of a weapon takes that weapon straight into
//! the grabbing hand ([`crate::controllers::weapons::hold::pickup`]), or, with `direct_pickup` off
//! or a weapon the direct pickup refuses, clicks the game's use-item action, so that the game picks
//! a weapon up its own way ([`finish_tick`]). A pickup grab of an
//! object asks the next simulation tick to take hold of it; letting the grip go asks it to let go.
//!
//! A held object is held at the point the hand touched, in one of two ways. A hand its body drives
//! holds it, with the hand bodies' `joint_grabs`, through a joint: a fixed constraint between the
//! hand's body and the object's, which the physics step makes and ends
//! ([`crate::controllers::physical_hands::physics`]), so the object has mass, the hand's own
//! strength carries it, and it leaves with the velocity the hand gave it. Otherwise the object is
//! carried, not attached: each tick its body is given the velocities that carry the grab point to
//! where the hand holds it and turn the body as the hand turns, within the hands' strength
//! ([`crate::controllers::physical_hands::hold`]), and it is let go with the hand's velocity, so
//! that it is thrown. One hand carries what weighs less than [`PickupConfig::hand_force_n`], and
//! two hands twice that; a hand that holds what it cannot lift drags it, and a body hinged to the
//! world swings about its hinge. A hand that holds something does not probe, push off surfaces, or
//! strike.

mod grips;

use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;
use jc3gi::{
    equipment::weapon::WeaponBase, input::input_action_map::Action, vehicle::vehicle::DoorSlot,
};

use crate::{
    config::Config,
    controllers::{
        bindings::{
            grab::GrabVolume,
            hold::{Desired, MapKind, Target, Want},
        },
        fingers::grip::GripSource,
        frame::Hand,
        physical_hands::{
            STALE_AFTER, STATE, articulated, bodies,
            characters::{self, CharactersConfig, Regrip, Seize},
            config::PickupConfig,
            contact::{Surface, SurfaceHit},
            doors::{self, DoorsConfig, Latch},
            grasp::{self, Grip, PalmMotion, Reach, TargetKind},
            hold::{self, HandHold, HoldTuning, PlayerKeepOut},
            palm::PalmBox,
            physics::HoldTarget,
            pickup::grips::{keep_latch, keep_regrip, keep_seize, shove_on_release},
            pulse,
            rigid::Velocities,
        },
        weapons::{
            self, FinishInputs,
            haptics::PulseSource,
            hold::{
                holder::Cause,
                pickup::{Refusal, TakeOptions},
            },
        },
    },
};

/// A hand's hold, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeldReport {
    pub mass_kg: f32,
    /// Whether the hands holding the object can lift it.
    pub liftable: bool,
    /// Whether the other hand holds the same object.
    pub shared: bool,
    /// The held body's centre of mass, world space.
    pub centre: Vec3,
    /// The grab point on the body, world space.
    pub anchor: Vec3,
    /// Where the hand would have the grab point, world space.
    pub target: Vec3,
    /// How far the grab point is from where the hand would have it, metres.
    pub lag_m: f32,
    /// The share of the hand's strongest pull used, `0..=1`.
    pub load: f32,
    /// The share of the hand's strongest twist used, `0..=1`.
    pub twist: f32,
    /// Whether the hand holds the object through a joint rather than carrying it.
    pub joined: bool,
}

/// What the hand bodies tell a pickup tick about the joints.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct JointInputs {
    /// Which hands take what they grab through a joint: those their bodies drive, with
    /// `joint_grabs`.
    pub join: [bool; 2],
    /// Which hands lost the joint their grip held.
    pub lost: [bool; 2],
    /// The share of each hand body's strongest pull and twist the last step used.
    pub loads: [(f32, f32); 2],
}

/// The pickups' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PickupStatus {
    /// Objects taken hold of.
    pub holds: u64,
    /// Weapon grabs: taken straight into the hand, or through the game's use-item action.
    pub weapons: u64,
    /// Objects let go of.
    pub releases: u64,
    /// The speed of the last object let go of, metres per second.
    pub last_throw_mps: Option<f32>,
    /// Holds lost: the body was freed, changed hands, or was torn away.
    pub lost: u64,
    /// Grips taken on a latched door.
    pub door_grips: u64,
    /// Latches a hand undid.
    pub unlatches: u64,
    /// The slot of the last door a hand took.
    pub last_door: Option<DoorSlot>,
    /// Grips taken on characters.
    pub character_grips: u64,
    /// Characters a hand pulled out of a vehicle seat.
    pub hijacks: u64,
    /// Articulated objects a hand took hold of: bodies joined to the world or another body.
    pub articulated_grips: u64,
    /// Characters on foot a hand yanked.
    pub yanks: u64,
    /// Fallen characters' ragdolls a hand took hold of after a yank.
    pub regrips: u64,
    pub last_error: Option<String>,
}

/// The pickup volume `hand` is in: [`GrabVolume::Pickup`] when the hand has a target and holds
/// nothing. Called from the input tick.
pub(crate) fn volume(hand: Hand) -> Option<GrabVolume> {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    if !(config.enabled && config.pickup.enabled) {
        return None;
    }
    let state = STATE.lock();
    let i = hand.index();
    let available = state.fresh(Instant::now())
        && state.pickup.held[i].is_none()
        && state.status.hands[i]
            .reach
            .is_some_and(|r| config.pickup.weapons || r.kind != TargetKind::Weapon);
    available.then_some(GrabVolume::Pickup)
}

/// Act on the input tick's pickup grabs and grip releases: a grab of a weapon clicks the game's
/// use-item action into `desired`, a grab of an object asks the next simulation tick to take hold
/// of it, and a released grip asks it to let go. Called from the input tick after the tables.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let mut state = STATE.lock();
    state.pickup.input_at = Some(Instant::now());
    let fresh = state.fresh(Instant::now());
    let mut pulses = Vec::new();
    let mut weapons = Vec::new();
    // A hand that yanked a seated character out through an open door enters the vehicle the
    // game's own way, which hijacks the seat.
    for clicked in std::mem::take(&mut state.pickup.hijack_clicks) {
        if clicked {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::ENTER_VEHICLE,
                },
                want: Want::Click,
            });
        }
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        if !inputs.grips[i].held() {
            if state.pickup.held[i].is_some()
                || matches!(state.pickup.requests[i], Some(Request::Grab(_)))
            {
                state.pickup.requests[i] = Some(Request::Release);
            }
            continue;
        }
        if inputs.grabs[i] != Some(GrabVolume::Pickup) {
            continue;
        }
        let Some(reach) = state.status.hands[i].reach.filter(|_| fresh) else {
            continue;
        };
        match reach.kind {
            TargetKind::Weapon => {
                weapons.push((hand, reach));
                state.status.pickup.weapons += 1;
                pulses.push(hand);
            }
            TargetKind::Prop { .. }
            | TargetKind::Articulated { .. }
            | TargetKind::Door(_)
            | TargetKind::Character(_) => {
                state.pickup.requests[i] = Some(Request::Grab(reach));
            }
        }
    }
    drop(state);
    let (holding, dual) =
        Config::lock_query(|c| (c.controllers.weapons.hold, c.controllers.weapons.dual_wield));
    for (hand, reach) in weapons {
        let refusal = if holding.direct_pickup {
            let options = TakeOptions {
                independent_pistols: dual.split && dual.independent_draw,
                drop_replaced: holding.drop_replaced,
                slide_s: if holding.take_slide {
                    holding.snap_s
                } else {
                    0.0
                },
                cause: Cause::Pickup,
            };
            // SAFETY: the input tick runs on the main thread outside the world update.
            let refusal = unsafe { take_weapon(hand, reach, options) }.err();
            weapons::hold::record_pickup(refusal.map(|r| r.to_string()));
            if let Some(refusal) = refusal {
                tracing::debug!(target: "controllers", ?hand, %refusal, "physical hands: direct weapon pickup refused");
            }
            refusal
        } else {
            None
        };
        if !holding.direct_pickup || refusal.is_some_and(Refusal::falls_back) {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::USE_ITEM,
                },
                want: Want::Click,
            });
            tracing::debug!(target: "controllers", ?hand, "physical hands: weapon pickup through the use-item action");
        }
    }
    for hand in pulses {
        pulse(
            hand,
            config.pickup.grab_pulse,
            1.0,
            &config,
            PulseSource::Pickup,
        );
    }
}

/// Take the weapon `reach` found straight into `hand` ([`weapons::hold::pickup::take`]), once its
/// body is checked to still belong to it.
///
/// # Safety
///
/// Main thread, outside the world update.
unsafe fn take_weapon(hand: Hand, reach: Reach, options: TakeOptions) -> Result<(), Refusal> {
    // A weapon's body leaves the world when it is picked up or despawned, so a body that still
    // belongs to the reached object proves the object is alive.
    if unsafe { bodies::read(reach.body, reach.object) }.is_err() {
        return Err(Refusal::NotDropped);
    }
    let weapon = unsafe { &mut *(reach.object.get() as *mut WeaponBase) };
    unsafe { weapons::hold::pickup::take(hand, weapon, options) }.map(|_| ())
}

/// Keep what the hands hold through a game menu: the grips are not read there, and a release is
/// acted on when gameplay resumes. Called from the input tick in place of [`finish_tick`].
pub(crate) fn pause() {
    STATE.lock().pickup.input_at = Some(Instant::now());
}

/// The pickups' state across ticks.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PickupState {
    held: [Option<Held>; 2],
    /// Each hand's grip on a latched door, before the latch gives.
    latches: [Option<Latch>; 2],
    /// Each hand's grip on a character.
    seizes: [Option<Seize>; 2],
    /// Each hand waiting to take hold of a character it yanked off its feet.
    regrips: [Option<Regrip>; 2],
    /// The hands that yanked a seated character, for the input tick to click the vehicle entry.
    hijack_clicks: [bool; 2],
    requests: [Option<Request>; 2],
    /// When the input tick last acted on the grips.
    input_at: Option<Instant>,
}

impl PickupState {
    pub(super) const fn new() -> Self {
        Self {
            held: [None; 2],
            latches: [None; 2],
            seizes: [None; 2],
            regrips: [None; 2],
            hijack_clicks: [false; 2],
            requests: [None; 2],
            input_at: None,
        }
    }

    pub(super) fn holding(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some()
    }

    /// Whether `hand` grips a latched door.
    pub(super) fn latched(&self, hand: Hand) -> bool {
        self.latches[hand.index()].is_some()
    }

    /// Whether `hand` carries what it holds, rather than holding it through a joint.
    pub(super) fn carrying(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some_and(|h| !h.joined)
    }

    /// What `hand` holds, for its finger pose: the grip's source, and the held body when the hand
    /// holds one rather than gripping a latched door or a character.
    pub(super) fn grip(&self, hand: Hand) -> Option<(GripSource, Option<u32>)> {
        let i = hand.index();
        if let Some(held) = self.held[i] {
            return Some((held.kind.source(), Some(held.body)));
        }
        if self.latches[i].is_some() {
            return Some((GripSource::Door, None));
        }
        (self.seizes[i].is_some() || self.regrips[i].is_some())
            .then_some((GripSource::Character, None))
    }

    /// The objects the hands hold, as addresses, zero for none.
    pub(super) fn held_objects(&self) -> [usize; 2] {
        self.held.map(|h| h.map_or(0, |h| h.object.get()))
    }

    /// Let go of everything without throwing, and forget the requests.
    pub(super) fn drop_all(&mut self) {
        self.held = [None; 2];
        self.latches = [None; 2];
        self.seizes = [None; 2];
        self.regrips = [None; 2];
        self.requests = [None; 2];
    }
}

/// What a hand could take: the nearest grabbable object hit by short rays out of the palm (from the
/// back of the hand, through the palm, out to the pickup's reach at the VR world scale
/// `world_scale`), or the object the palm presses into, or a vehicle's door ([`doors::reach`]). An
/// object heavier than [`PickupConfig::max_mass_kg`] is not one, nor the body `standing_on` the
/// player stands on, which a hand would lift from under the player. `cast` returns the first hit
/// between two points.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn reach(
    palm: &PalmBox,
    press: Option<SurfaceHit>,
    standing_on: Option<u32>,
    tuning: &ReachTuning,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<Reach> {
    let ReachTuning {
        world_scale,
        pickup: config,
        doors: door_config,
        characters: character_config,
        ..
    } = *tuning;
    let origin = palm.point(Vec3::new(0.0, 1.0, 0.0));
    let length = (config.reach_m * world_scale).max(0.0) + 2.0 * palm.half_extents.y;
    let hits = REACH_DIRECTIONS
        .iter()
        .filter_map(|&local| {
            let direction = (palm.orientation * local).normalize_or_zero();
            cast(origin, origin + direction * length)
        })
        .chain(press);
    let reaches: Vec<Reach> = hits
        .filter_map(|hit| {
            if let Surface::Character { object, body } = hit.surface {
                // SAFETY: forwarded from the caller; the hit was found this tick.
                return unsafe {
                    characters::reach(
                        object,
                        body,
                        &hit,
                        palm.centre,
                        world_scale,
                        character_config,
                    )
                };
            }
            let Surface::Solid {
                object: Some(object),
                body,
            } = hit.surface
            else {
                return None;
            };
            // A vehicle's door, open or at a closed door's handle area, before anything else: the
            // player may stand on the vehicle whose door they open.
            // SAFETY: forwarded from the caller; the hit was found this tick.
            if let Some(door) = unsafe {
                doors::reach(
                    object,
                    body,
                    &hit,
                    palm.centre,
                    door_config.reach_m * world_scale,
                    door_config,
                )
            } {
                return Some(door);
            }
            if standing_on == Some(body) {
                return None;
            }

            // SAFETY: forwarded from the caller; the hit was found this tick.
            let classified = unsafe { bodies::classify(object, body) };
            // A body joined to the world or another body is taken whatever its weight, since its
            // constraint carries it.
            let kind = match classified {
                Some(TargetKind::Weapon) => TargetKind::Weapon,
                // SAFETY: forwarded from the caller.
                other => match unsafe { articulated_kind(object, body, tuning) } {
                    Some(articulated) => articulated,
                    None => other?,
                },
            };
            if let TargetKind::Prop { mass_kg } = kind
                && !grasp::can_hold(mass_kg, config)
            {
                return None;
            }
            Some(Reach {
                object,
                body,
                point: hit.position,
                distance: hit.position.distance(palm.centre),
                kind,
            })
        })
        .collect();
    grasp::nearest(reaches)
}

/// What the object at `object`, reached on its dynamic body `body`, is when the body is joined to
/// the world or another body by an enabled constraint: an articulated object, light enough for
/// its constraint to carry. `None` for a vehicle, which its doors' reach handles, and for a body
/// that is not joined.
///
/// # Safety
///
/// Main thread, outside the physics step; `object` and `body` must come from a cast this tick.
unsafe fn articulated_kind(
    object: NonZeroUsize,
    body: u32,
    tuning: &ReachTuning,
) -> Option<TargetKind> {
    if !tuning.pickup.articulated || unsafe { doors::engine::vehicle_of(object) }.is_some() {
        return None;
    }
    let mass_kg = unsafe { bodies::read(body, object) }.ok()?.mass_kg?;
    if !grasp::can_hold_articulated(mass_kg, tuning.pickup) {
        return None;
    }
    unsafe { articulated::joined(body, tuning.hand_bodies) }
        .map(|_| TargetKind::Articulated { mass_kg })
}

/// What a hand's reach takes, and how far it reaches.
#[derive(Copy, Clone)]
pub(super) struct ReachTuning<'a> {
    /// The VR world scale, which scales the reaches.
    pub world_scale: f32,
    pub pickup: &'a PickupConfig,
    pub doors: &'a DoorsConfig,
    pub characters: &'a CharactersConfig,
    /// The hand bodies, whose grip joints do not make what they hold articulated.
    pub hand_bodies: [Option<u32>; 2],
}

/// What a pickup tick works from.
pub(super) struct TickInputs {
    /// The acting hands' palms; a hand without one lets go.
    pub palms: [Option<PalmMotion>; 2],
    /// The player's body, which a carried object is kept out of.
    pub space: Option<PlayerKeepOut>,
    pub joints: JointInputs,
    pub dt: f32,
    /// How far a hand gripping a latched door pulls it before the latch gives, world metres.
    pub unlatch_pull_m: f32,
    /// The VR world scale.
    pub world_scale: f32,
    pub characters: CharactersConfig,
    /// What each hand reaches this tick, from which a hand that yanked a character takes hold of
    /// its ragdoll.
    pub reaches: [Option<Reach>; 2],
}

/// The pickups' simulation tick: take hold and let go as the input tick asked, keep the grips on
/// latched doors, and drive what the hands carry.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn tick(
    state: &mut PickupState,
    inputs: TickInputs,
    config: &PickupConfig,
) -> TickOutcome {
    let TickInputs {
        palms,
        space,
        joints,
        dt,
        unlatch_pull_m,
        world_scale,
        characters: character_config,
        reaches,
    } = inputs;
    let mut outcome = TickOutcome::default();
    for hand in Hand::BOTH {
        let i = hand.index();
        if joints.lost[i] && state.held[i].is_some_and(|h| h.joined) {
            state.held[i] = None;
            outcome.released[i] = true;
            outcome.lost += 1;
            outcome.error =
                Some("physical hands: pickup: the hand's joint to the object ended".to_owned());
        }
    }
    let now = Instant::now();
    let input_fresh = state
        .input_at
        .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER);
    if !input_fresh {
        // The bindings stopped reading the grips: no release will come.
        state.drop_all();
    }

    for hand in Hand::BOTH {
        let i = hand.index();
        match state.requests[i].take() {
            Some(Request::Release) => {
                if state.latches[i].take().is_some() {
                    outcome.released[i] = true;
                }
                if let Some(seize) = state.seizes[i].take() {
                    outcome.released[i] = true;
                    if let Some(palm) = palms[i] {
                        shove_on_release(hand, &seize, &palm, &character_config);
                    }
                }
                if state.regrips[i].take().is_some() {
                    outcome.released[i] = true;
                }
                if let Some(held) = state.held[i].take() {
                    let shared = state.held[hand.other().index()].is_some_and(|o| o.same(&held));
                    // A joined object leaves with the velocity the hand gave it.
                    if !shared && !held.joined {
                        // SAFETY: forwarded from the caller.
                        match unsafe { throw(&held, palms[i], config) } {
                            Ok(speed) => outcome.throw = Some(speed),
                            Err(e) => outcome.error = Some(e.to_string()),
                        }
                    }
                    outcome.released[i] = true;
                }
            }
            Some(Request::Grab(reach)) => 'grab: {
                let Some(palm) = palms[i] else {
                    break 'grab;
                };
                if let TargetKind::Door(door) = reach.kind {
                    outcome.door = Some(door.slot);
                    if door.latched {
                        // SAFETY: forwarded from the caller; the door's vehicle was reached by
                        // this tick's casts.
                        match unsafe { Latch::take(door, reach.point) } {
                            Some(latch) => {
                                tracing::info!(
                                    hand = hand.label(),
                                    slot = doors::slot_label(door.slot),
                                    part = door.part_index,
                                    "physical hands: doors: a hand gripped a latched door"
                                );
                                state.latches[i] = Some(latch);
                                outcome.taken[i] = true;
                                outcome.door_grips += 1;
                            }
                            None => {
                                outcome.error = Some(
                                    "physical hands: doors: the door's grip could not be placed"
                                        .to_owned(),
                                );
                            }
                        }
                        break 'grab;
                    }
                }
                if let TargetKind::Character(target) = reach.kind
                    && !target.ragdoll
                {
                    // SAFETY: forwarded from the caller; the character was reached by this
                    // tick's casts.
                    match unsafe { Seize::take(target, reach.point) } {
                        Some(seize) => {
                            tracing::info!(
                                hand = hand.label(),
                                seated = target.seated.is_some(),
                                "physical hands: characters: a hand took hold of a character"
                            );
                            state.seizes[i] = Some(seize);
                            outcome.taken[i] = true;
                            outcome.character_grips += 1;
                        }
                        None => {
                            outcome.error = Some(
                                "physical hands: characters: the grip could not be placed"
                                    .to_owned(),
                            );
                        }
                    }
                    break 'grab;
                }
                // SAFETY: forwarded from the caller.
                match unsafe { bodies::read(reach.body, reach.object) } {
                    Ok(body) => match body
                        .mass_kg
                        .filter(|&m| grasp::can_take(&reach.kind, m, config))
                    {
                        Some(mass_kg) => {
                            state.held[i] = Some(Held {
                                object: reach.object,
                                body: reach.body,
                                grip: Grip::of(palm.pose, body.pose, reach.point),
                                mass_kg,
                                joined: joints.join[i],
                                kind: HeldKind::of(&reach.kind),
                            });
                            outcome.taken[i] = true;
                            if matches!(reach.kind, TargetKind::Articulated { .. }) {
                                tracing::info!(
                                    hand = hand.label(),
                                    body = reach.body,
                                    mass_kg,
                                    joined = joints.join[i],
                                    "physical hands: a hand took hold of an articulated object"
                                );
                                outcome.articulated_grips += 1;
                            }
                        }
                        None => {
                            outcome.error = Some(
                                "physical hands: pickup: the body is fixed or too heavy".to_owned(),
                            );
                        }
                    },
                    Err(e) => outcome.error = Some(e.to_string()),
                }
            }
            None => {}
        }
        if state.held[i].is_some() && palms[i].is_none() {
            state.held[i] = None;
            outcome.lost += 1;
        }
        if state.latches[i].is_some() && palms[i].is_none() {
            state.latches[i] = None;
            outcome.lost += 1;
        }
        if state.seizes[i].is_some() && palms[i].is_none() {
            state.seizes[i] = None;
            outcome.lost += 1;
        }
        if state.regrips[i].is_some() && palms[i].is_none() {
            state.regrips[i] = None;
            outcome.lost += 1;
        }
        // SAFETY: forwarded from the caller.
        unsafe {
            keep_regrip(
                state,
                hand,
                palms[i],
                reaches[i],
                &joints,
                config,
                &mut outcome,
            )
        };
        // SAFETY: forwarded from the caller.
        unsafe {
            keep_seize(
                state,
                hand,
                palms[i],
                world_scale,
                &character_config,
                &mut outcome,
            )
        };
        // SAFETY: forwarded from the caller.
        unsafe { keep_latch(state, hand, palms[i], &joints, unlatch_pull_m, &mut outcome) };
    }

    // Drive each carried body once, from every hand that carries it; a joined hold is only
    // reported.
    let mut driven = [false; 2];
    for hand in Hand::BOTH {
        let i = hand.index();
        let Some(held) = state.held[i].filter(|_| !driven[i]) else {
            continue;
        };
        let holders = [0, 1].map(|k| state.held[k].filter(|h| h.same(&held)));
        for (k, holder) in holders.iter().enumerate() {
            driven[k] |= holder.is_some();
        }
        // SAFETY: forwarded from the caller.
        let body = match unsafe { bodies::read(held.body, held.object) } {
            Ok(body) => body,
            Err(e) => {
                lose(state, &held, &mut outcome, e.to_string());
                continue;
            }
        };
        let holding = holders.iter().flatten().count();
        let mut holds = [None; 2];
        for k in 0..2 {
            let (Some(holder), Some(palm)) = (holders[k], palms[k]) else {
                continue;
            };
            let anchor = holder.grip.anchor(body.pose);
            if holder.joined {
                let grip_target = holder.grip.target(palm.pose);
                // The hand's body carries the object through the joint, so the keep-out moves the
                // hand's goal: the object's centre stays out of the player's body as a carried
                // one does.
                let keep_out = space.as_ref().map_or(Vec3::ZERO, |space| {
                    space.offset(holder.grip.centre_at(grip_target))
                });
                let target = grip_target.point + keep_out;
                outcome.joints[k] = Some(HoldTarget {
                    object: held.object,
                    body: held.body,
                    keep_out,
                    shared: holding > 1,
                });
                let (load, twist) = joints.loads[k];
                outcome.reports[k] = Some(HeldReport {
                    mass_kg: held.mass_kg,
                    liftable: grasp::can_lift(held.mass_kg, holding, config),
                    shared: holding > 1,
                    centre: body.pose.centre,
                    anchor,
                    target,
                    lag_m: anchor.distance(target),
                    load,
                    twist,
                    joined: true,
                });
                continue;
            }
            let mut target = holder.grip.target(palm.pose);
            if let Some(space) = &space {
                target.point += space.offset(holder.grip.centre_at(target));
            }
            if grasp::torn_away(anchor, target.point, config.break_distance_m) {
                state.held[k] = None;
                outcome.released[k] = true;
                outcome.lost += 1;
                outcome.error = Some("physical hands: pickup: the object was torn away".to_owned());
                continue;
            }
            holds[k] = Some(HandHold {
                anchor,
                target,
                velocity: palm.point_velocity(target.point),
                angular_velocity: palm.angular_velocity,
            });
        }
        let hands = holds.iter().flatten().count();
        if hands == 0 {
            continue;
        }
        let solution = hold::solve(&body.rigid(), holds, dt, &HoldTuning::of(config));
        // SAFETY: forwarded from the caller.
        if let Err(e) = unsafe { bodies::drive(held.body, held.object, solution.velocities) } {
            lose(state, &held, &mut outcome, e.to_string());
            continue;
        }
        for (k, hold) in holds.iter().enumerate() {
            let Some(hold) = hold else {
                continue;
            };
            outcome.reports[k] = Some(HeldReport {
                mass_kg: held.mass_kg,
                liftable: grasp::can_lift(held.mass_kg, hands, config),
                shared: hands > 1,
                centre: body.pose.centre,
                anchor: hold.anchor,
                target: hold.target.point,
                lag_m: hold.anchor.distance(hold.target.point),
                load: solution.loads[k],
                twist: solution.torque_loads[k],
                joined: false,
            });
        }
    }
    outcome
}

/// What a pickup tick did, for the readout, the haptics, and the overlay.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct TickOutcome {
    /// Each hand's hold after the tick.
    pub reports: [Option<HeldReport>; 2],
    /// The hands that took hold this tick.
    pub taken: [bool; 2],
    /// The hands that let go this tick.
    pub released: [bool; 2],
    /// The speed of an object thrown this tick.
    pub throw: Option<f32>,
    /// Holds lost this tick.
    pub lost: u64,
    pub error: Option<String>,
    /// What each hand holds through a joint, for the hand bodies to join.
    pub joints: [Option<HoldTarget>; 2],
    /// Where each hand gripping a latched door is held, world space.
    pub pins: [Option<Vec3>; 2],
    /// The latches the hands undid this tick.
    pub unlatched: [bool; 2],
    /// Grips taken on a latched door this tick.
    pub door_grips: u64,
    /// The slot of a door a hand took this tick.
    pub door: Option<DoorSlot>,
    /// Grips taken on characters this tick.
    pub character_grips: u64,
    /// Seated characters the hands pulled out this tick.
    pub hijacks: u64,
    /// Articulated objects the hands took hold of this tick.
    pub articulated_grips: u64,
    /// Characters on foot the hands yanked this tick.
    pub yanks: u64,
    /// Fallen characters' ragdolls the hands took hold of this tick.
    pub regrips: u64,
}

/// The directions of the reach rays in the wrist frame: out of the palm (-Y), along the fingers
/// (-Z), between the two, and out of either edge of the hand.
pub(super) const REACH_DIRECTIONS: [Vec3; 5] = [
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(0.0, -1.0, -1.0),
    Vec3::new(1.0, -0.5, 0.0),
    Vec3::new(-1.0, -0.5, 0.0),
];

/// One hand's hold.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Held {
    object: NonZeroUsize,
    body: u32,
    grip: Grip,
    mass_kg: f32,
    /// Whether the hand holds the object through a joint rather than carrying it.
    joined: bool,
    kind: HeldKind,
}

impl Held {
    fn same(&self, other: &Held) -> bool {
        self.object == other.object && self.body == other.body
    }
}

/// What a held body is to the hand, for its finger pose.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum HeldKind {
    /// A loose object.
    Prop,
    /// An object hinged to the world or another body.
    Articulated,
    /// A vehicle's door, held once open.
    Door,
    /// A body of a character's ragdoll.
    Ragdoll,
}

impl HeldKind {
    fn of(kind: &TargetKind) -> Self {
        match kind {
            TargetKind::Articulated { .. } => HeldKind::Articulated,
            TargetKind::Door(_) => HeldKind::Door,
            TargetKind::Character(_) => HeldKind::Ragdoll,
            TargetKind::Prop { .. } | TargetKind::Weapon => HeldKind::Prop,
        }
    }

    const fn source(self) -> GripSource {
        match self {
            HeldKind::Prop => GripSource::Pickup,
            HeldKind::Articulated => GripSource::Articulated,
            HeldKind::Door => GripSource::Door,
            HeldKind::Ragdoll => GripSource::Character,
        }
    }
}

/// What the input tick asked of a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Request {
    Grab(Reach),
    Release,
}

/// Let go of `held` from the palm `palm`, and return the throw's speed: the grab point leaves with
/// the palm's velocity at it, scaled and capped, and the body keeps its spin. An object one hand
/// cannot lift, or one let go by an untracked hand, keeps the velocity it has.
///
/// # Safety
///
/// Main thread, outside the physics step.
unsafe fn throw(
    held: &Held,
    palm: Option<PalmMotion>,
    config: &PickupConfig,
) -> Result<f32, bodies::BodyError> {
    let body = unsafe { bodies::read(held.body, held.object) }?;
    let Some(palm) = palm.filter(|_| grasp::can_lift(held.mass_kg, 1, config)) else {
        return Ok(0.0);
    };
    let anchor = held.grip.anchor(body.pose);
    let point = grasp::throw_velocity(palm.point_velocity(anchor), config);
    let spin = body.velocities.angular;
    let linear = (point - spin.cross(anchor - body.pose.centre))
        .clamp_length_max(config.max_throw_speed_mps);
    unsafe {
        bodies::drive(
            held.body,
            held.object,
            Velocities {
                linear,
                angular: spin,
            },
        )
    }?;
    Ok(point.length())
}

/// Let go of `held` in every hand that holds it, as a lost hold.
fn lose(state: &mut PickupState, held: &Held, outcome: &mut TickOutcome, why: String) {
    for hand in Hand::BOTH {
        let i = hand.index();
        if state.held[i].is_some_and(|h| h.same(held)) {
            state.held[i] = None;
            outcome.released[i] = true;
        }
    }
    outcome.lost += 1;
    outcome.error = Some(why);
}
