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
//! - [`joint`]: the fixed constraint that joins a hand body to an object its grip holds, or to a
//!   held gun's body;
//! - [`guns`]: a held gun as a body of its own, joined to the hand bodies that hold it;
//! - [`state`]: the shared state the tick's two halves and the rest of the controllers read, and
//!   the small readers over it;
//! - [`config`]: the settings, and their fixed tuning ([`HAND_BODIES`]).
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

pub(super) mod blows;
pub(super) mod body;
mod callback;
pub mod config;
mod contact;
pub(crate) mod drawn_body;
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
pub(crate) use callback::{rail_step_registered, register_rail_step};
pub use config::{HAND_BODIES, HandBodiesConfig, HandHitsTuning};
pub(crate) use jc3vr_body::controllers::physics::havok::{
    affine as havok_affine, vector as havok_vector,
};
pub(crate) use lifecycle::release_body_joints;

pub use self::{
    observe::{apply, apply_at_render, frame_tick, observe, sample_render_offsets},
    state::{
        body_pose, callback_status, drives, enabled, pressing, publish_holds, publish_pins, push,
        status, take_joint_losses,
    },
    step::{step, step_seconds},
    types::{BodyPhase, CallbackStatus, HandBodiesStatus, HandBodyStatus, HoldTarget},
};

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use glam::{Quat, Vec3};
    use jc3vr_input::frame::Pose;

    use crate::controllers::physical_hands::{
        characters::RAGDOLL_CARRY_STRENGTH,
        physics::{HAND_BODIES, HoldTarget, drive::DriveTuning, state::holding_tuning},
        pickup::PICKUP,
    };

    #[test]
    fn a_joined_hand_holds_with_a_holding_hands_strength() {
        let b = &HAND_BODIES;
        let tuning = DriveTuning {
            mass_kg: b.mass_kg,
            max_force_n: b.force_n(1.0),
            max_torque_nm: b.torque_nm(1.0),
            linear_gain: b.linear_gain,
            angular_gain: b.angular_gain,
            max_speed_mps: b.max_speed_mps,
            max_angular_speed_rps: b.max_angular_speed_rps,
        };
        let hold = HoldTarget {
            object: NonZeroUsize::new(100).unwrap(),
            body: 7,
            palm: Pose {
                position: Vec3::ZERO,
                orientation: Quat::IDENTITY,
            },
            mass_kg: 75.0,
            keep_out: Vec3::ZERO,
            shared: false,
            hand_force_n: PICKUP.hand_force_n,
            constrained: false,
        };
        let one = holding_tuning(&tuning, &hold);
        assert_eq!(one.linear_gain, tuning.linear_gain);
        let constrained = HoldTarget {
            constrained: true,
            ..hold
        };
        let soft = holding_tuning(&tuning, &constrained);
        assert!(soft.linear_gain < tuning.linear_gain * 0.5);
        let shared_soft = holding_tuning(
            &tuning,
            &HoldTarget {
                shared: true,
                ..constrained
            },
        );
        assert!(shared_soft.linear_gain < soft.linear_gain);
        assert_eq!(one.mass_kg, tuning.mass_kg + hold.mass_kg);
        assert_eq!(one.max_force_n, PICKUP.hand_force_n);
        assert_eq!(one.max_torque_nm, PICKUP.hand_torque_nm);
        let ragdoll = holding_tuning(
            &tuning,
            &HoldTarget {
                mass_kg: 6.0,
                hand_force_n: PICKUP.hand_force_n * RAGDOLL_CARRY_STRENGTH,
                constrained: false,
                ..hold
            },
        );
        // A ragdoll's joint carries the bone it holds, within the hand's own strength, however
        // hard a carry of the whole body would pull.
        assert_eq!(ragdoll.mass_kg, tuning.mass_kg + 6.0);
        assert_eq!(ragdoll.max_force_n, tuning.max_force_n);
        assert_eq!(ragdoll.max_torque_nm, tuning.max_torque_nm);
        let two = holding_tuning(
            &tuning,
            &HoldTarget {
                shared: true,
                hand_force_n: PICKUP.hand_force_n * 1.5,
                constrained: false,
                ..hold
            },
        );
        assert_eq!(
            two.max_force_n,
            tuning.max_force_n.min(PICKUP.hand_force_n * 1.5)
        );
        assert!(
            (two.max_torque_nm
                - tuning
                    .max_torque_nm
                    .min(PICKUP.hand_torque_nm * 1.5 * PICKUP.two_hand_angular_share,))
            .abs()
                < 1e-5
        );
        let stronger = DriveTuning {
            max_force_n: b.force_n(2.0),
            max_torque_nm: b.torque_nm(2.0),
            ..tuning
        };
        let scaled = holding_tuning(
            &stronger,
            &HoldTarget {
                hand_force_n: PICKUP.hand_force_n * 2.0,
                constrained: false,
                ..hold
            },
        );
        assert_eq!(scaled.max_force_n, PICKUP.hand_force_n * 2.0);
        assert_eq!(scaled.max_torque_nm, PICKUP.hand_torque_nm * 2.0);
        // Never stronger than the hand itself.
        let weak = DriveTuning {
            max_force_n: 10.0,
            ..tuning
        };
        assert_eq!(holding_tuning(&weak, &hold).max_force_n, 10.0);
        let broken = HoldTarget {
            hand_force_n: f32::NAN,
            constrained: false,
            ..hold
        };
        assert_eq!(holding_tuning(&tuning, &broken).max_force_n, 0.0);
        for mass in [f32::NAN, f32::INFINITY, -1.0] {
            assert_eq!(
                holding_tuning(
                    &tuning,
                    &HoldTarget {
                        mass_kg: mass,
                        ..hold
                    }
                )
                .mass_kg,
                tuning.mass_kg
            );
        }
        let half = holding_tuning(
            &tuning,
            &HoldTarget {
                mass_kg: 37.5,
                shared: true,
                ..hold
            },
        );
        assert_eq!(half.mass_kg, tuning.mass_kg + 37.5);
    }
}
