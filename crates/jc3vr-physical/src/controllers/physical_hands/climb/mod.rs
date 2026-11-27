//! Climbing with the hands: an empty hand whose body touches the static world, or a moving body
//! heavy enough such as a vehicle, grips it with the grip, and while it grips, the player's body
//! moves against the hand's motion, so that pulling a gripping hand down lifts the body; a grip on
//! a moving body is held in the body's own frame and carries the player with it. The design is in
//! `docs/mod/input/hand-climbing.md`, the engine side in
//! `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! - [`tuning`]: the tuning, with the push-up's;
//! - [`solve`]: the motion, pure: where the gripping hands would have the body, the velocity that
//!   takes it there, the fling a release keeps, and the mantle over a ledge;
//! - [`engine`]: the identity and motion of the gripped bodies, the native gate's context, and
//!   whether the player is in the air;
//! - [`native`]: the refusal of the game's automatic stunt-area entry while the hands climb, and
//!   the hand-over of a mantle's landing on a moving body to the stunt positions;
//! - [`lifetime`]: the gripped bodies' lifetime stamps, advanced as the world destroys them;
//! - [`state`]: the grips and the shared state the tick's two halves and the rest of the
//!   controllers read.
//!
//! Each simulation tick runs in this order:
//!
//! 1. The player aim control's post-simulation update ([`observe::observe`], from the palms' tick)
//!    publishes each driven hand's palm in the rig, the climbable surface in its reach, and where
//!    the player's body is, and pulses the hands for the grips the last step took or ended.
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

pub mod engine;
pub(crate) mod lifetime;
pub(crate) mod native;
mod observe;
pub mod solve;
mod state;
mod step;
pub mod tuning;
mod types;

pub use self::{
    observe::{observe, reach},
    state::{
        climbing, finish_tick, gripped, reach_hold, stand_down, static_reaches, status,
        take_landing,
    },
    step::step,
    types::{ClimbStatus, Hold, Observed},
};
