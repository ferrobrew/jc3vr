//! The parachute under the controller scheme: its handles and the ripcord that opens it.
//!
//! - [`handles`]: the handles, taken with the grip and pulled to steer and brake, and
//!   [`handle_tick`] adding them to the move stick;
//! - [`rig`]: where the game's parachute rig puts the handles, and the unheld handle props;
//! - [`ripcord`]: the ripcord at the hip, taken with the grip and pulled to open the parachute
//!   from a fall or the wingsuit, and [`ripcord_tick`] running it on the input tick.

pub(super) mod handle_tick;
pub mod handles;
pub mod rig;
pub mod ripcord;
pub(super) mod ripcord_tick;
