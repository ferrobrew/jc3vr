//! The debug UI's published capture state: the latest ownership snapshot, which any thread reads
//! without the renderer, and the driver the debug UI registers to step the policy and sample the
//! desktop on the input thread.
//!
//! Until a driver is registered (before the debug UI installs), stepping returns `None` and the
//! desktop sample is unavailable, so nothing can be captured.

use std::sync::OnceLock;

use parking_lot::Mutex;

use crate::capture::{
    CaptureUpdate, OwnershipSnapshot,
    desktop::{DesktopInputObservation, DesktopInputUnavailable},
};

/// Input-thread observations before either editing or gameplay receives the sample.
#[derive(Clone, Copy, Debug)]
pub struct DebugUiInput {
    pub focused: bool,
    pub tracking_ready: bool,
    /// Includes all source episodes, panel shortcuts, keys, buttons, composition, and scroll.
    pub controls_neutral: bool,
}

/// The debug UI's half of the capture: it owns the policy instance and the native desktop sampler.
#[derive(Clone, Copy, Debug)]
pub struct CaptureDriver {
    /// Step the policy from an input-thread sample; `None` while the debug UI is not live.
    pub update: fn(DebugUiInput) -> Option<CaptureUpdate>,
    /// Sample the desktop's keys, buttons, and composition on the window's thread.
    pub sample_desktop: fn() -> DesktopInputObservation,
}

/// Install the debug UI's driver. The first registration stands; later ones are ignored, since the
/// driver is a pair of plain functions that do not change across reinstalls.
pub fn register_driver(driver: CaptureDriver) {
    let _ = DRIVER.set(driver);
}

/// The latest policy observation; reading it never takes the renderer lock.
pub fn debug_ui_capture() -> Option<OwnershipSnapshot> {
    *CAPTURE_SNAPSHOT.lock()
}

/// Whether the debug UI owns any editing input (keyboard, text, pointer, or a grip drag).
pub fn input_captured() -> bool {
    debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.any())
}

/// Publish the policy's latest snapshot, or `None` when the debug UI uninstalls.
pub fn publish_capture(snapshot: Option<OwnershipSnapshot>) {
    *CAPTURE_SNAPSHOT.lock() = snapshot;
}

/// Step capture before mode detection, pointer routing, and gameplay output on the input thread.
pub fn update_debug_ui_capture(input: DebugUiInput) -> Option<CaptureUpdate> {
    DRIVER.get().and_then(|driver| (driver.update)(input))
}

/// Sample the desktop once before the capture policy, including while editing disables
/// game-device polling.
pub fn desktop_input_observation() -> DesktopInputObservation {
    DRIVER.get().map_or(
        DesktopInputObservation::Unavailable(DesktopInputUnavailable::WindowNotObserved),
        |driver| (driver.sample_desktop)(),
    )
}

static DRIVER: OnceLock<CaptureDriver> = OnceLock::new();
static CAPTURE_SNAPSHOT: Mutex<Option<OwnershipSnapshot>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CapturePolicy, EditCapture, InputObservation};

    fn snapshot(edit_capture: EditCapture) -> OwnershipSnapshot {
        let observation = InputObservation {
            focused: true,
            tracking_ready: true,
            controls_neutral: true,
            game_device_enabled: true,
        };
        OwnershipSnapshot {
            edit_capture,
            ..CapturePolicy::default().update(None, observation).snapshot
        }
    }

    #[test]
    fn the_input_captured_flag_follows_the_published_edit_capture() {
        publish_capture(None);
        assert!(!input_captured());
        publish_capture(Some(snapshot(EditCapture::default())));
        assert!(!input_captured());
        publish_capture(Some(snapshot(EditCapture {
            keyboard: true,
            ..EditCapture::default()
        })));
        assert!(input_captured());
        publish_capture(None);
        assert!(!input_captured());
    }
}
