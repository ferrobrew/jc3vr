//! Hand bodies: each empty hand is a body in the game's physics world, driven toward its controller
//! within the hand's strength. The design is in `docs/mod/input/physical-hands.md`, the engine side
//! in `docs/engine/physics/havok-bodies.md`.
//!
//! - [`body`]: one hand body in the physics world, made and destroyed as the engine's own free
//!   bodies are;
//! - [`callback`]: the physics system's pre-simulation callback that drives the bodies;
//! - [`lifecycle`]: the bodies' own state across steps, and the transitions that make, add, drive,
//!   park, join, and destroy them;
//! - [`contact`]: the contacts each body starts, as the physics system's collision listener
//!   reports them, and [`blows`], which pulses the hands by them and deals the blows they strike
//!   characters with, as worked out in [`hit`];
//! - [`drive`]: the drive, pure: the velocities that carry a body to its hand within the hand's
//!   strength, and the push a held-back hand gives the player;
//! - [`render`]: the offset that draws a hand where its body is, pure;
//! - [`joint`]: the fixed constraint that joins a hand body to an object its grip holds, with
//!   `joint_grabs`, or to a held gun's body;
//! - [`guns`]: a held gun as a body of its own, joined to the hand bodies that hold it;
//! - [`state`]: the shared state the tick's two halves and the rest of the controllers read, and
//!   the small readers over it;
//! - [`config`]: the settings.
//!
//! Each simulation tick runs in this order:
//!
//! 1. The player aim control's post-simulation update ([`observe::observe`]) publishes each
//!    acting hand's palm pose and velocity, world space, and takes the last step's results: where
//!    each body was left against where it was sent, which draws the hand at its body
//!    ([`observe::apply`]), and the share of its strength each hand used, which pulses it.
//! 2. Before the next step, the pre-simulation callback ([`step::step`]) makes, adds, or parks each
//!    hand's body, reads where the last step left it, and drives it toward where its palm will be
//!    at the end of this step, predicted from the published pose and velocity; the push a held-back
//!    hand gives the player is worked out here. The climb
//!    ([`crate::controllers::physical_hands::climb`]) first takes and ends its grips and moves the
//!    player, and a gripping hand's body is held where it took hold instead.
//! 3. The step integrates the velocities and resolves the bodies' contacts: a hand stops at a wall,
//!    shoves a crate, and leans on a car.
//!
//! The bodies change the physics world, so they are touched only from the callback, which the
//! physics system calls on the game thread outside the step, and from the shutdown cleanup on the
//! game thread. The other halves read and write a leaf lock of their own, never the VR runtime's.
//!
//! A hand acts on foot on the ground, for a tracked hand the controllers drive that holds nothing
//! and is not carrying a loose object with the grip; a hand that holds one through a joint acts,
//! and its body is joined to the object's. A hand that stops acting for a moment keeps
//! its body in place; one that stays out (a drawn gun, a vehicle, a swim) has its body taken out of
//! the simulation, and every body is destroyed when the game leaves gameplay, the hand bodies are
//! switched off, the player's character changes, or the payload unloads.

mod blows;
mod body;
mod callback;
pub mod config;
mod contact;
pub mod drive;
pub mod guns;
pub mod hit;
mod joint;
mod lifecycle;
mod observe;
mod render;
mod state;
mod step;
mod types;

pub(crate) use blows::queue_shove;
pub use blows::{Shove, ShoveForce};
pub use config::{HandBodiesConfig, HandHitsConfig};
pub(crate) use joint::affine as havok_affine;

pub(crate) use self::{
    observe::{apply, apply_at_render, frame_tick, observe},
    state::{
        body_pose, drives, enabled, pressing, publish_holds, publish_pins, push, take_joint_losses,
    },
    step::{step, step_seconds},
};
pub use self::{
    state::{callback_status, status},
    types::{BodyPhase, CallbackStatus, HandBodiesStatus, HandBodyStatus, HoldTarget},
};

#[cfg(test)]
mod tests {
    use crate::controllers::physical_hands::{
        config::PhysicalHandsConfig,
        physics::{drive::DriveTuning, state::holding_tuning},
    };

    #[test]
    fn a_joined_hand_holds_with_a_holding_hands_strength() {
        let config = PhysicalHandsConfig::new();
        let b = &config.bodies;
        let tuning = DriveTuning {
            mass_kg: b.mass_kg,
            max_force_n: b.force_n(),
            max_torque_nm: b.torque_nm(),
            linear_gain: b.linear_gain,
            angular_gain: b.angular_gain,
            max_speed_mps: b.max_speed_mps,
            max_angular_speed_rps: b.max_angular_speed_rps,
        };
        let one = holding_tuning(&tuning, &config, false);
        assert_eq!(one.max_force_n, config.pickup.hand_force_n);
        assert_eq!(one.max_torque_nm, config.pickup.hand_torque_nm);
        let two = holding_tuning(&tuning, &config, true);
        assert_eq!(two.max_force_n, config.pickup.hand_force_n);
        assert!(
            (two.max_torque_nm
                - config.pickup.hand_torque_nm * config.pickup.two_hand_angular_share)
                .abs()
                < 1e-5
        );
        // Never stronger than the hand itself.
        let weak = DriveTuning {
            max_force_n: 10.0,
            ..tuning
        };
        assert_eq!(holding_tuning(&weak, &config, false).max_force_n, 10.0);
    }
}
