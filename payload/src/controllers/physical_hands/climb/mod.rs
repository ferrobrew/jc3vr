//! Climbing with the hands: an empty hand whose body touches the static world, or a moving body
//! heavy enough such as a vehicle, grips it with the grip, and while it grips, the player's body
//! moves against the hand's motion, so that pulling a gripping hand down lifts the body; a grip on
//! a moving body is held in the body's own frame and carries the player with it. The design is in `docs/mod/input/hand-climbing.md`, the
//! engine side in `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! - [`config`]: the settings;
//! - [`solve`]: the motion, pure: where the gripping hands would have the body, the velocity that
//!   takes it there, the fling a release keeps, and the mantle over a ledge;
//! - [`engine`]: the player's character proxy: where it is, its support, and its velocity;
//! - [`state`]: the grips and the shared state the tick's two halves and the rest of the
//!   controllers read.
//!
//! Each simulation tick runs in this order:
//!
//! 1. The player aim control's post-simulation update ([`observe::observe`], from the palms' tick)
//!    publishes each driven hand's palm in the rig, the static surface in its reach, and where the
//!    player's body is, and pulses the hands for the grips the last step took or ended.
//! 2. The input tick ([`finish_tick`]) asks for a grip where a hand's grip press found the climb's
//!    surface ([`reach_hold`], offered to the grip as a fallback target), and for a release where
//!    the grip lets go.
//! 3. The hand bodies' pre-simulation step ([`step::step`]) takes and ends the grips, pins each
//!    gripping hand's body where it took hold, and moves the player: the velocity that carries the
//!    body to where the gripping hands would have it, and a support on the gripped surface, so
//!    that the game keeps the player on foot instead of starting a fall. A release keeps the
//!    climbing speed, and a mantle carries the body over a ledge and lets go once it stands there.
//!
//! The grips are taken and the player moved only in the pre-simulation step, on the thread that
//! steps the world, outside the step. The other halves share a leaf lock of their own.

pub mod config;
pub mod engine;
pub(crate) mod lifetime;
pub(crate) mod native;
mod observe;
pub mod solve;
mod state;
mod step;
mod types;

pub use config::ClimbConfig;

pub(crate) use self::{
    observe::{observe, reach},
    state::{enabled, finish_tick, gripped, reach_hold, stand_down, static_reaches, take_landing},
    step::step,
    types::Observed,
};
pub use self::{
    state::{climbing, status},
    types::{ClimbStatus, Hold},
};
