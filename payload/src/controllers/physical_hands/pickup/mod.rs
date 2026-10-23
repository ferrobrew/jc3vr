//! Picking things up with an empty hand. The design is in `docs/mod/input/physical-hands.md`, the
//! engine side in `docs/engine/gameplay/item-pickup.md`.
//!
//! Each simulation tick, from the palms' tick ([`super::main_thread_tick`]), every empty hand that holds
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
//!
//! The module is split by concern: [`state`] holds the pickups' state across ticks, [`reach`] what
//! a hand could take, and [`tick`] the simulation tick that drives it; [`grips`] holds the grips a
//! hand keeps on what it cannot simply carry.

mod grips;
mod reach;
mod state;
mod tick;

use std::time::Instant;

use glam::Vec3;
use jc3gi::{
    equipment::weapon::WeaponBase, input::input_action_map::Action, vehicle::vehicle::DoorSlot,
};
pub(super) use reach::{REACH_DIRECTIONS, ReachTuning, reach};
use state::{Held, HeldKind};
pub(super) use state::{PickupState, Request};
pub(super) use tick::{JointInputs, TickInputs, TickOutcome, tick};

use crate::{
    config::Config,
    controllers::{
        bindings::hold::{Desired, MapKind, Target, Want},
        frame::Hand,
        grab::GrabVolume,
        hands::ik,
        haptics::{self, Event},
        physical_hands::{
            STATE, bodies, grab_view,
            grasp::{Reach, TargetKind},
        },
        weapons::{
            self, FinishInputs,
            hold::{
                Cause,
                pickup::{Refusal, TakeOptions},
            },
            regions::WorldTouch,
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

/// Act on the input tick's pickup grabs and grip releases: a grab of a weapon clicks the game's
/// use-item action into `desired`, a grab of an object asks the next simulation tick to take hold
/// of it, and a released grip asks it to let go. Called from the input tick after the tables.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
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
        if !matches!(inputs.grabs[i], Some(GrabVolume::Pickup(_))) {
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
    let (holding, regions) =
        Config::lock_query(|c| (c.controllers.weapons.hold, c.controllers.weapons.regions));
    let targets = ik::tick_records().current;
    for (hand, reach) in weapons {
        let refusal = if holding.direct_pickup {
            let options = TakeOptions {
                drop_replaced: holding.drop_replaced,
                slide_s: if holding.take_slide {
                    holding.snap_s
                } else {
                    0.0
                },
                cause: Cause::Pickup,
                // A long gun is taken where the hand closed on it.
                touch: targets[hand.index()]
                    .filter(|_| regions.enabled)
                    .map(|target| WorldTouch {
                        point: reach.point,
                        wrist: target.wrist.orientation,
                        grip_radius: regions.grip_radius_m,
                        reach: regions.reach_m,
                    }),
            };
            // SAFETY: the input tick runs on the main thread outside the world update.
            let refusal = unsafe { take_weapon(hand, reach, options) }.err();
            weapons::hold::record_pickup(refusal.map(|r| r.to_string()));
            if let Some(refusal) = refusal {
                tracing::debug!(target: "controllers", ?hand, %refusal, "physical hands: direct weapon pickup refused");
                grab_view::refused(hand, Some(reach.point), &refusal.to_string());
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
        haptics::play(hand, Event::Take);
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
