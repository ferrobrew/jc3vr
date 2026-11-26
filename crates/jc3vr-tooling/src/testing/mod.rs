//! The test tools the capture harness (`tools/rendering`) drives: the visual configurations
//! ([`visual`]), the staging of a scene for them ([`stage`]), and the benchmark ([`benchmark`]),
//! started through the request file a script drops into the configuration directory ([`request`]),
//! which also starts and stops a tracking recording ([`jc3vr_view::recorder::tracking`]).
//! `testing.enabled` switches the request file and the tracking recorder off.
//!
//! [`update`] drives the tools from the top of the game's update on the game thread.

mod config;
pub mod request;
pub(crate) mod stage;
mod text;

pub use config::{TESTING_CONFIG, TestingConfig};
use jc3vr_render::testing::{benchmark, visual};

/// Drive the test tools for this frame: the capture harness, the request file, the benchmark, and
/// the visual stage. Game thread, once per frame from the top of the game's update, after the
/// director's pass (which services the stage's teleport).
pub fn update() {
    jc3vr_render::screenshot::harness::tick();
    visual::tick();
    request::poll();
    benchmark::tick();
    stage::update();
}

/// Stop the tools and wait for their threads, for eject. Returns whether none is left running.
#[must_use = "a thread that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    benchmark::shutdown();
    visual::shutdown();
    jc3vr_render::screenshot::harness::snapshot::shutdown()
}
