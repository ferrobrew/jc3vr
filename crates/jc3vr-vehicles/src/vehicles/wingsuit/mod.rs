//! The wingsuit under the controller scheme: its arm steering, the evade and the gestures the arms
//! throw, the head's look-steer removed, and the take-off from the ground.
//!
//! - [`arm_steering`]: the arm-pose steering, and [`arm_tick`] adding it to the move stick, with
//!   its guide on the overlay ([`guide`]);
//! - [`evade`]: the boost wingsuit's barrel-roll evade, thrown by banking the arms;
//! - [`gestures`]: the suit's deploy, air brake, and close, from the arms;
//! - [`look_steer`]: the wingsuit's head look-steer removed;
//! - [`take_off`]: the boost wingsuit's take-off from the ground along the head's heading.

pub mod arm_steering;
pub(super) mod arm_tick;
pub mod evade;
pub mod gestures;
pub(super) mod guide;
pub mod look_steer;
pub mod take_off;
