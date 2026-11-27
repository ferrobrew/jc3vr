//! The airframe a grapple reel arriving at a helicopter's door side was attached to, as the
//! grapple's lock publishes it for the vehicles.
//!
//! The lock's arrival detour writes it in the world update as the reel reaches the door side
//! (`grapple::physical::lock`), with the moment it arrived; a reader on a later input tick judges
//! its freshness from that moment. The latch holds until the next arrival or until a reader clears
//! it.

use jc3vr_core::published::{Published, Stage, Stamp};

/// The body of the last door-side arrival's airframe and the moment it arrived, if one was noted
/// and not cleared since.
pub fn noted() -> Option<(u32, Stamp)> {
    NOTED.latest().map(|noted| (noted.value, noted.at))
}

/// Publish the body of the airframe a reel arriving at a helicopter's door side was attached to,
/// stamped now.
pub fn publish(body: u32) {
    NOTED.publish(body);
}

/// Forget the noted airframe.
pub fn clear() {
    NOTED.clear();
}

static NOTED: Published<u32> = Published::new(Stage::EngineCall);
