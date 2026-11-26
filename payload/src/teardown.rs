//! The worker threads the eject stops before it removes the hooks, in their order
//! (`jc3vr_core::lifecycle::stop_workers`).
//!
//! Two positions are load-bearing, and the test below pins them:
//!
//! - The frame tail goes first. It presents the desktop mirror, on whose present the video
//!   recorder's observer captures its frames, and it can still be submitting the last frame, so
//!   stopping it first leaves no producer feeding the writers that stop after it.
//! - The test tools stop before the settings. The visual stage and the benchmark put back the
//!   configuration they overrode, and the settings' stop writes a change still waiting to settle,
//!   so the file keeps the player's values rather than a test's.
//!
//! The other workers share nothing with each other and keep the order they were written in.

use jc3vr_body::controllers::body::estimator;
use jc3vr_core::lifecycle::{self, StopWorker, WorkerRegistrationError};
use jc3vr_hud::hud;
use jc3vr_render::image_writer;
use jc3vr_tooling::{recorder, testing};
use jc3vr_ui::ui;
use jc3vr_view::{recorder::tracking, vr};
use jc3vr_weapons::controllers::weapons;

use crate::{config, tuning};

/// Register every worker's stop with the lifecycle, in [`WORKERS`] order.
pub(crate) fn register() -> Result<(), WorkerRegistrationError> {
    WORKERS
        .iter()
        .try_for_each(|&(name, stop)| lifecycle::register_worker(name, stop))
}

const WORKERS: &[(&str, StopWorker)] = &[
    ("the frame tail", vr::tail::shutdown),
    #[cfg(feature = "profiler")]
    (
        "the profiler capture writer",
        jc3vr_core::profiler::capture::shutdown,
    ),
    ("the image writer", image_writer::shutdown),
    ("the video recorder", recorder::shutdown),
    ("the tracking recorder", tracking::shutdown),
    ("the test tools", testing::shutdown),
    ("the UI layout writer", ui::layout::shutdown),
    ("the tuning watcher", tuning::shutdown),
    ("the settings writers", config::shutdown),
    (
        "the inventory sidecar writer",
        weapons::inventory::sidecar::shutdown,
    ),
    ("the game icon loader", hud::game_icons::shutdown),
    ("the body estimator loader", estimator::shutdown),
];

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn position(name: &str) -> usize {
        WORKERS
            .iter()
            .position(|&(worker, _)| worker == name)
            .unwrap_or_else(|| panic!("{name} is not a worker"))
    }

    #[test]
    fn the_frame_tail_stops_first() {
        assert_eq!(position("the frame tail"), 0);
    }

    #[test]
    fn the_test_tools_restore_the_configuration_before_the_settings_are_written() {
        assert!(position("the test tools") < position("the settings writers"));
    }

    #[test]
    fn every_worker_is_named_once() {
        for (index, &(name, _)) in WORKERS.iter().enumerate() {
            assert_eq!(position(name), index, "{name} is listed twice");
        }
    }
}
