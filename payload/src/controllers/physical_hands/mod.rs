//! Physical hands: each empty hand's palm collides with the world. The design is in
//! `docs/mod/input/physical-hands.md`, the engine side in `docs/engine/gameplay/melee-and-damage.md`.
//!
//! - [`palm`]: the palm box at the tracked wrist, and the points its contacts are probed from;
//! - [`contact`]: whether each probe point has passed into a surface, and how deep;
//! - [`push`]: the body's push away from what the palms press into;
//! - [`shove`]: a palm's soft contact with a loose body, and how much the body gives;
//! - [`pickup`]: taking hold of loose objects and picking up weapons with the grip, with the pure
//!   grip in [`grasp`] and the solver that carries what the hands hold in [`hold`];
//! - [`rigid`]: a rigid body's response to impulses;
//! - [`touch`]: what the contacts do to the world: knocks, shoves, and pressure pulses;
//! - [`physics`]: the hand bodies, which replace the palms' probes for the hands they drive;
//! - [`characters`]: taking hold of characters, and pulling a seated one out through an open door;
//! - [`articulated`]: whether a body hangs from the world or another body by a constraint;
//! - [`climb`]: climbing by gripping the static world with a hand its body drives;
//! - [`vault`]: pushing the body up with the hands pressed down on a surface;
//! - [`stunt`]: stunt positions on vehicles, entered and let go of with the grip;
//! - [`doors`]: opening vehicle doors by hand, a latched one gripped at its handle area;
//! - [`grab_view`]: the grab view, a diagnostic of what the grip and the reach saw;
//! - [`world`]: the Havok world, lent out for one call at a time;
//! - [`config`]: the settings.
//!
//! A hand its body drives ([`physics::drives`]) meets the world through its body. Otherwise, the
//! palms are probed rather than simulated: no body is added to the physics world. Each simulation
//! tick, once the player aim control's post-simulation update has run ([`tick::after_aim`]), the
//! palms' tick runs on the game thread at the start of the frame's render update
//! ([`tick::main_thread_tick`]), and each probe point of each acting palm is cast
//! from where it last was in the open to where it is now, through the player aim's ray filter,
//! which never hits the player. Characters are passed through. What the casts find drives these:
//!
//! 1. A press into a solid surface pushes the body away from it ([`push`]), as movement added to
//!    the on-foot locomotion task's own ([`push_body`], from the roomscale simulation half), so
//!    that the view moves with the body as it does for stick movement.
//! 2. The rendered hand is held at the surface ([`apply`], from [`crate::controllers::hands`]).
//! 3. A press into a dynamic object shoves it every tick, within the hand's strength ([`shove`]);
//!    the share it does not give pushes the body away, as a wall does. A fast hit also knocks it.
//! 4. An object within reach of a palm can be taken with the grip ([`pickup`]), held at the point
//!    the hand touched ([`grasp`]) and carried within the hands' strength ([`hold`]); the rendered
//!    hand stays on the grab point, and what the hand cannot move pushes the body.
//!
//! A palm acts only on foot, on the ground or falling ([`tick::mode_acts`]), for a tracked hand the
//! controller drives that holds nothing: not a gun, not the foregrip, not a grenade or a charge
//! being thrown.
//!
//! The knocks, the shoves, and the holds change engine state that is the main thread's, so they
//! are made only when the aim update runs on the thread the input tick runs on
//! ([`state::input_tick`] notes it); otherwise the palms stand down and the readout says why.
//!
//! The module is split by concern: [`status`] holds the readout types, [`state`] the palms' state
//! across ticks and its stand-down, and [`tick`] the simulation tick that drives it all.

pub mod articulated;
mod bodies;
pub mod characters;
pub mod climb;
pub mod collider_view;
pub mod config;
pub mod contact;
pub mod doors;
mod draw;
mod engine;
pub mod grab_view;
pub mod grasp;
pub mod hold;
pub(crate) mod human_hull;
pub mod palm;
pub mod physics;
pub mod pickup;
pub mod push;
pub(crate) mod remote;
pub mod rigid;
pub mod shove;
pub mod sounds;
mod state;
pub mod stunt;
mod targets;
mod tick;
mod touch;
pub mod vault;
mod world;

mod status;

use std::num::NonZeroUsize;

pub use bodies::BodyError;
pub(crate) use bodies::BodyState;
pub use config::PhysicalHandsConfig;
use glam::{Quat, Vec3};
use jc3gi::{character::character::Character, types::math::Vector3};
pub(crate) use state::input_tick;
use state::{STALE_AFTER, State};
pub use status::{HandStatus, Idle, PhysicalHandsStatus, ShoveReport, status};
pub(crate) use status::{PalmReadout, palm_readout, press_summary};
#[cfg(test)]
pub(crate) use targets::test_support::DoorProvider;
pub(crate) use targets::{DoorEligibility, targets};
pub(crate) use tick::{after_aim, free_hands, main_thread_tick, mode_acts};

use crate::{
    config::Config,
    controllers::{hands::CockpitHand, physical_hands::state::STATE},
};

/// The loose object each hand carries, by [`Hand::index`](crate::controllers::frame::Hand::index),
/// for the loadout table: a prop, not a grip on something hinged, a door, or a character, which are
/// the world's.
pub(crate) fn held_props() -> [Option<NonZeroUsize>; 2] {
    STATE.lock().pickup.props()
}

/// When the hands are composed for: the simulation tick, or a frame rendered between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DrawTime {
    /// The simulation tick: the hand bodies' offsets as the last step left them.
    Tick,
    /// A rendered frame: the hand bodies' offsets blended between the last two steps, so the
    /// drawn hand does not step at the physics rate.
    Render,
}

/// Hold each pressed hand at the surface it presses into, in the cockpit frame, and draw each hand
/// body's hand at its body, for `at`. Called by [`crate::controllers::hands::active_hands`] and
/// [`crate::controllers::hands::render_active_hands`].
pub fn apply(cockpit: &mut [Option<CockpitHand>; 2], at: DrawTime) {
    let (enabled, palm_offset) = Config::lock_query(|c| {
        let p = &c.controllers.physical_hands;
        (p.enabled && p.clamp_hand, p.palm_offset_m)
    });
    match at {
        DrawTime::Tick => physics::apply(cockpit, palm_offset),
        DrawTime::Render => physics::apply_at_render(cockpit, palm_offset),
    }
    if !enabled {
        return;
    }
    let offsets = {
        let state = STATE.lock();
        if !state.fresh(std::time::Instant::now()) {
            return;
        }
        state.clamp
    };
    for (hand, offset) in cockpit.iter_mut().zip(offsets) {
        if let Some(hand) = hand.as_mut()
            && offset != Vec3::ZERO
        {
            hand.wrist.position += offset;
            hand.grip.position += offset;
            hand.aim.position += offset;
        }
    }
}

/// Add the palms' push to the on-foot locomotion task's velocity. Called by the roomscale
/// simulation half before its chase reads the task's velocity, so that the push counts as the
/// task's own movement.
///
/// # Safety
///
/// `character` must be the live local player character, after the locomotion task wrote its
/// velocity.
/// Give the dynamic body `body` the velocities `linear` and `angular`, world space, after checking
/// it still belongs to `owner`: a dropped weapon leaving the hand with the hand's motion.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn set_body_velocities(
    body: u32,
    owner: NonZeroUsize,
    linear: Vec3,
    angular: Vec3,
) -> Result<(), BodyError> {
    unsafe { bodies::drive(body, owner, rigid::Velocities { linear, angular }) }
}

/// The dynamic body `body`'s centre of mass and orientation, game space, after checking it still
/// belongs to `owner`: a live grenade or a dropped magazine near the hands, which a body still
/// owned by it proves alive.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn body_pose(body: u32, owner: NonZeroUsize) -> Result<(Vec3, Quat), BodyError> {
    let state = unsafe { bodies::read(body, owner) }?;
    Ok((state.pose.centre, state.pose.orientation))
}

/// The dynamic body `body`'s state, game space, after checking it still belongs to `owner`: a
/// weapon the grapple's snatch pulls in.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn body_state(body: u32, owner: NonZeroUsize) -> Result<BodyState, BodyError> {
    unsafe { bodies::read(body, owner) }
}

pub(crate) unsafe fn push_body(character: &mut Character) {
    let enabled = Config::lock_query(|c| {
        c.controllers.physical_hands.enabled && c.controllers.physical_hands.push
    });
    let palms = if enabled {
        let state = STATE.lock();
        if state.fresh(std::time::Instant::now()) {
            state.status.push
        } else {
            glam::Vec2::ZERO
        }
    } else {
        glam::Vec2::ZERO
    };
    let push = palms + physics::push();
    if push == glam::Vec2::ZERO || !push.is_finite() {
        return;
    }
    let mut wanted = Vector3 { data: [0.0; 3] };
    unsafe { character.GetWantedWorldVelocity(&mut wanted) };
    let [x, y, z] = wanted.data;
    let pushed = Vector3 {
        data: [x + push.x, y, z + push.y],
    };
    unsafe { character.SetWantedWorldVelocity(&pushed) };
}
