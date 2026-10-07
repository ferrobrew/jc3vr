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
//! neither grapples nor reaches the game. A pickup grab of a weapon clicks the game's use-item
//! action, so that the game picks the weapon up its own way ([`finish_tick`]). A pickup grab of an
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

use std::{num::NonZeroUsize, time::Instant};

use glam::Vec3;
use jc3gi::input::input_action_map::Action;

use crate::{
    config::Config,
    controllers::{
        bindings::{
            grab::GrabVolume,
            hold::{Desired, MapKind, Target, Want},
        },
        frame::Hand,
        physical_hands::{
            STALE_AFTER, STATE, bodies,
            config::PickupConfig,
            contact::{Surface, SurfaceHit},
            grasp::{self, Grip, PalmMotion, Reach, TargetKind},
            hold::{self, HandHold, HoldTuning, PlayerKeepOut},
            palm::PalmBox,
            physics::HoldTarget,
            pulse,
            rigid::Velocities,
        },
        weapons::{FinishInputs, haptics::PulseSource},
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
    /// Weapons picked up through the game's use-item action.
    pub weapons: u64,
    /// Objects let go of.
    pub releases: u64,
    /// The speed of the last object let go of, metres per second.
    pub last_throw_mps: Option<f32>,
    /// Holds lost: the body was freed, changed hands, or was torn away.
    pub lost: u64,
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
                desired.push(Desired {
                    target: Target {
                        map: MapKind::Player,
                        action: Action::USE_ITEM,
                    },
                    want: Want::Click,
                });
                state.status.pickup.weapons += 1;
                tracing::debug!(target: "controllers", ?hand, "physical hands: weapon pickup");
                pulses.push(hand);
            }
            TargetKind::Prop { .. } => {
                state.pickup.requests[i] = Some(Request::Grab(reach));
            }
        }
    }
    drop(state);
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

/// Keep what the hands hold through a game menu: the grips are not read there, and a release is
/// acted on when gameplay resumes. Called from the input tick in place of [`finish_tick`].
pub(crate) fn pause() {
    STATE.lock().pickup.input_at = Some(Instant::now());
}

/// The pickups' state across ticks.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PickupState {
    held: [Option<Held>; 2],
    requests: [Option<Request>; 2],
    /// When the input tick last acted on the grips.
    input_at: Option<Instant>,
}

impl PickupState {
    pub(super) const fn new() -> Self {
        Self {
            held: [None; 2],
            requests: [None; 2],
            input_at: None,
        }
    }

    pub(super) fn holding(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some()
    }

    /// Whether `hand` carries what it holds, rather than holding it through a joint.
    pub(super) fn carrying(&self, hand: Hand) -> bool {
        self.held[hand.index()].is_some_and(|h| !h.joined)
    }

    /// What each hand holds through a joint.
    pub(super) fn joint_targets(&self) -> [Option<HoldTarget>; 2] {
        self.held.map(|h| {
            h.filter(|h| h.joined).map(|h| HoldTarget {
                object: h.object,
                body: h.body,
            })
        })
    }

    /// The objects the hands hold, as addresses, zero for none.
    pub(super) fn held_objects(&self) -> [usize; 2] {
        self.held.map(|h| h.map_or(0, |h| h.object.get()))
    }

    /// Let go of everything without throwing, and forget the requests.
    pub(super) fn drop_all(&mut self) {
        self.held = [None; 2];
        self.requests = [None; 2];
    }
}

/// What a hand could take: the nearest grabbable object hit by short rays out of the palm (from the
/// back of the hand, through the palm, out to `reach_m`), or the object the palm presses into. An
/// object heavier than [`PickupConfig::max_mass_kg`] is not one. `cast` returns the first hit
/// between two points.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn reach(
    palm: &PalmBox,
    reach_m: f32,
    press: Option<SurfaceHit>,
    config: &PickupConfig,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<Reach> {
    let origin = palm.point(Vec3::new(0.0, 1.0, 0.0));
    let length = reach_m.max(0.0) + 2.0 * palm.half_extents.y;
    let hits = REACH_DIRECTIONS
        .iter()
        .filter_map(|&local| {
            let direction = (palm.orientation * local).normalize_or_zero();
            cast(origin, origin + direction * length)
        })
        .chain(press);
    let reaches: Vec<Reach> = hits
        .filter_map(|hit| {
            let Surface::Solid {
                object: Some(object),
                body,
            } = hit.surface
            else {
                return None;
            };
            // SAFETY: forwarded from the caller; the hit was found this tick.
            let kind = unsafe { bodies::classify(object, body) }?;
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

/// The pickups' simulation tick: take hold and let go as the input tick asked, and drive what the
/// hands carry. `palms` are the acting hands' palms; a hand without one lets go. `space` is the
/// player's body, which a carried object is kept out of.
///
/// # Safety
///
/// Main thread, outside the physics step.
pub(super) unsafe fn tick(
    state: &mut PickupState,
    palms: [Option<PalmMotion>; 2],
    space: Option<PlayerKeepOut>,
    joints: JointInputs,
    dt: f32,
    config: &PickupConfig,
) -> TickOutcome {
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
            Some(Request::Grab(reach)) => {
                let Some(palm) = palms[i] else {
                    continue;
                };
                // SAFETY: forwarded from the caller.
                match unsafe { bodies::read(reach.body, reach.object) } {
                    Ok(body) => match body.mass_kg.filter(|&m| grasp::can_hold(m, config)) {
                        Some(mass_kg) => {
                            state.held[i] = Some(Held {
                                object: reach.object,
                                body: reach.body,
                                grip: Grip::of(palm.pose, body.pose, reach.point),
                                mass_kg,
                                joined: joints.join[i],
                            });
                            outcome.taken[i] = true;
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
                let target = holder.grip.target(palm.pose).point;
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
}

impl Held {
    fn same(&self, other: &Held) -> bool {
        self.object == other.object && self.body == other.body
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
