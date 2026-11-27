//! Physical hands: each empty hand's palm collides with the world. The design is in
//! `docs/mod/input/physical-hands.md`, the engine side in `docs/engine/gameplay/melee-and-damage.md`.
//!
//! - [`palm`]: the palm box at the tracked wrist, and the points its contacts are probed from;
//! - [`contact`]: whether each probe point has passed into a surface, and how deep;
//! - [`push`]: the body's push away from what the palms press into;
//! - [`shove`]: a palm's soft contact with a loose body, and how much the body gives;
//! - [`pickup`]: taking hold of loose objects and picking up weapons with the grip, with the pure
//!   grip in [`grasp`] and the solver that carries what the hands hold in [`hold`];
//! - [`rigid`](jc3vr_body::controllers::physics::rigid): a rigid body's response to impulses (in [`jc3vr_body::controllers::physics`]);
//! - [`touch`]: what the contacts do to the world: knocks, shoves, and pressure pulses;
//! - [`physics`]: the hand bodies, which replace the palms' probes for the hands they drive;
//! - [`characters`]: taking hold of characters, and pulling a seated one out through an open door
//!   or a broken window;
//! - [`articulated`]: whether a body hangs from the world or another body by a constraint;
//! - [`climb`]: climbing by gripping the static world with a hand its body drives;
//! - [`vault`]: pushing the body up with the hands pressed down on a surface;
//! - [`stunt`]: stunt positions on vehicles, entered and let go of with the grip;
//! - [`doors`]: opening vehicle doors by hand, a latched one gripped at its handle area;
//! - [`grab_view`]: the grab view, a diagnostic of what the grip and the reach saw;
//! - [`world`]: the Havok world, lent out for one call at a time (in
//!   [`jc3vr_body::controllers::physics`]);
//! - [`config`]: the settings, and [`tuning`] the tuning read from the shipped `hands.toml`.
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
//!    the on-foot locomotion task's own ([`palm_push`], composed by the body rig's simulation
//!    half), so that the view moves with the body as it does for stick movement.
//! 2. The rendered hand is held at the surface ([`apply`], from [`jc3vr_body::controllers::hands`]).
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
//! are made only from the palms' tick on the game thread, outside its frame scopes
//! ([`tick::main_thread_tick`]); a tick anywhere else stands down and the readout says why.
//!
//! The module is split by concern: [`status`] holds the readout types, [`state`] the palms' state
//! across ticks and its stand-down, and [`tick`] the simulation tick that drives it all.

pub mod articulated;
mod bodies;
mod carry_collision;
pub mod characters;
pub mod climb;
pub mod collider_view;
pub mod config;
pub mod contact;
pub mod doors;
mod engine;
pub mod grab_view;
pub mod grasp;
pub mod grip_sources;
pub mod hold;
pub mod human_hull;
pub mod impacts;
pub mod palm;
pub mod physics;
pub mod pickup;
pub mod push;
pub(crate) mod query;
pub mod register_body;
pub mod remote;
mod seated;
pub mod shove;
pub mod sounds;
mod state;
pub mod stunt;
mod targets;
mod tick;
mod touch;
pub mod tuning;
pub mod vault;
pub mod views;

mod status;

use std::num::NonZeroUsize;

pub use config::PhysicalHandsConfig;
use glam::Vec3;
use jc3vr_body::controllers::physics::{line_body, world};
use jc3vr_core::published::Stamp;
use state::{State, stale_after};
pub use status::{HandStatus, Idle, PhysicalHandsStatus, ShoveReport, status};
pub(crate) use status::{PalmReadout, palm_readout, press_summary};
#[cfg(any(test, feature = "test-support"))]
pub use targets::test_support::DoorProvider;
pub use targets::{door_offer_current, targets};
pub use tick::{after_aim, free_hands, main_thread_tick, mode_acts};

pub mod lifetime;

use jc3vr_body::controllers::{
    grab::PickupKind,
    hands::{CockpitHand, pose::palm_centre_m},
};
use jc3vr_input::frame::Hand;

use crate::controllers::physical_hands::state::STATE;

/// The loose object each hand carries, by [`Hand::index`](jc3vr_input::frame::Hand::index),
/// for the loadout table: a prop, not a grip on something hinged, a door, or a character, which are
/// the world's.
#[inline]
pub fn held_props() -> [Option<NonZeroUsize>; 2] {
    STATE.lock().pickup.props()
}

/// What a hand holds through its grip: the object, its body, the kind of hold, and the hold's last
/// report, for the tracking recorder.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeldBody {
    pub object: NonZeroUsize,
    pub body: u32,
    pub kind: PickupKind,
    pub report: Option<pickup::HeldReport>,
}

/// What each hand holds through its grip, by [`Hand::index`].
pub fn held_bodies() -> [Option<HeldBody>; 2] {
    let state = STATE.lock();
    let objects = state.pickup.held_objects();
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let (_, body) = state.pickup.grip(hand)?;
        Some(HeldBody {
            object: NonZeroUsize::new(objects[i])?,
            body: body?,
            kind: state.pickup.held_kind(hand)?,
            report: state.status.hands[i].held,
        })
    })
}

use jc3vr_body::controllers::hands::DrawTime;

/// Hold each pressed hand at the surface it presses into, in the cockpit frame, and draw each hand
/// body's hand at its body, for `at`. Called by [`jc3vr_body::controllers::hands::active_hands`] and
/// its `active_hands_from_frame`.
pub fn apply(cockpit: &mut [Option<CockpitHand>; 2], at: DrawTime) {
    match at {
        DrawTime::Tick => physics::apply(cockpit, palm_centre_m()),
        DrawTime::Render => physics::apply_at_render(cockpit, palm_centre_m()),
    }
    let offsets = {
        let state = STATE.lock();
        if !state.fresh(Stamp::now()) {
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

/// The palms' push this tick, world `(x, z)`: the wanted velocity's palm-push contribution
/// (`jc3vr_body::controllers::physics::wanted::Contributor::PalmPush`), which the body adds to the
/// on-foot locomotion task's velocity before its step drive reads it, so that the push counts as
/// the task's own movement. `None` for no push.
pub(crate) fn palm_push() -> Option<glam::Vec2> {
    let palms = {
        let state = STATE.lock();
        if state.fresh(Stamp::now()) {
            state.status.push
        } else {
            glam::Vec2::ZERO
        }
    };
    admitted_push(stunt::game_holds(), palms, physics::push())
}

fn admitted_push(
    yield_movement: bool,
    palms: glam::Vec2,
    physics: glam::Vec2,
) -> Option<glam::Vec2> {
    let push = palms + physics;
    (!yield_movement && push.is_finite() && push != glam::Vec2::ZERO).then_some(push)
}

#[cfg(test)]
mod movement_tests {
    use super::*;

    #[test]
    fn cached_palm_push_is_not_admitted_while_native_ownership_is_true_or_unknown() {
        let cached = glam::Vec2::new(2.0, 3.0);
        assert_eq!(admitted_push(true, cached, glam::Vec2::ZERO), None);
        assert_eq!(admitted_push(true, cached, cached), None);
        assert_eq!(admitted_push(false, cached, glam::Vec2::ZERO), Some(cached));
    }
}
