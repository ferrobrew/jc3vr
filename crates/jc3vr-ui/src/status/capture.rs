//! The status rows for the session's focus, the debug panel's ownership of the input, and the
//! capture tools: the video and tracking recorders, the profiler, and the world overlay.

#[cfg(feature = "profiler")]
use jc3vr_core::profiler::capture as profiler;
use jc3vr_input::{
    capture::{OwnershipBlocker, OwnershipSnapshot, PanelMode, debug_ui_capture},
    frame,
};
use jc3vr_render::controllers::overlay;
use jc3vr_tooling::recorder;
use jc3vr_view::{recorder as tracking, recorder::TrackingStatus};
use smol_str::format_smolstr;

use crate::{
    status::{Line, Row},
    ui::director::host_line,
};

pub(crate) const CONTROLLERS: Row = Row::new("Session focus", controllers);
pub(crate) const PANEL: Row = Row::new("Debug panel", panel);
pub(crate) const TOOLS: &[Row] = &[
    Row::new("Video recording", video),
    Row::new("Tracking capture", tracking_capture),
    #[cfg(feature = "profiler")]
    Row::new("Profiler trace", profiler_trace),
    Row::new("World overlay", world_overlay),
];

fn controllers() -> Line {
    match frame::latest() {
        Some(frame) if frame.focused => Line::ok(format_smolstr!(
            "Focused, controllers {}",
            active_word(frame.any_active())
        )),
        Some(frame) => Line::waiting(format_smolstr!(
            "Unfocused, controllers {}",
            active_word(frame.any_active())
        )),
        None => Line::waiting("No controller frame"),
    }
}

fn panel() -> Line {
    panel_line(debug_ui_capture())
}

fn video() -> Line {
    let video = recorder::status();
    let capturing = matches!(video.host, Some(recorder::HostState::Capturing(_)));
    let line = capture_line(video.recording, capturing, video.error.as_deref());
    Line {
        detail: format_smolstr!("{}, {} sidecar lines", host_line(&video), video.lines),
        ..line
    }
}

fn tracking_capture() -> Line {
    tracking_line(&tracking::status())
}

#[cfg(feature = "profiler")]
fn profiler_trace() -> Line {
    let last = profiler::last_result();
    let error = match &last {
        Some(Err(error)) => Some(error.as_str()),
        _ => None,
    };
    let line = capture_line(profiler::is_recording(), true, error);
    match (profiler::progress(), last) {
        (Some((elapsed, total)), _) => Line {
            detail: format_smolstr!("{elapsed:.1} of {total:.1} s"),
            ..line
        },
        (None, Some(Ok(path))) => line.with_note(format!("Last trace: {}", path.display())),
        (None, _) => line,
    }
}

fn world_overlay() -> Line {
    overlay_line(&overlay::pass::status())
}

fn panel_line(ownership: Option<OwnershipSnapshot>) -> Line {
    let Some(snapshot) = ownership else {
        return Line::waiting("No snapshot");
    };
    let mode = match snapshot.mode {
        PanelMode::Hidden => "Hidden",
        PanelMode::Editing => "Editing",
        PanelMode::ReadOnly => "Read-only",
    };
    let line = if snapshot.gameplay_requested && !snapshot.gameplay_effective {
        Line::waiting(format_smolstr!("{mode}, gameplay input held back"))
    } else if snapshot.mode == PanelMode::Hidden {
        Line::off(mode)
    } else {
        Line::ok(mode)
    };
    match blocker_label(snapshot.blocker) {
        Some(blocker) => line.with_note(format!("Blocked: {blocker}")),
        None => line,
    }
}

fn tracking_line(tracking: &TrackingStatus) -> Line {
    let line = capture_line(tracking.recording, true, tracking.error.as_deref());
    if !tracking.recording {
        return line;
    }
    Line {
        detail: format_smolstr!(
            "{} frames, {} dropped, {:.1} s",
            tracking.frames,
            tracking.dropped,
            tracking.duration.as_secs_f64()
        ),
        ..line
    }
}

fn overlay_line(overlay: &overlay::pass::OverlayStatus) -> Line {
    if let Some(error) = &overlay.error {
        return Line::failed("See log").with_note(error.as_str());
    }
    let detail = format_smolstr!("{} drawn per eye, {} frames", overlay.drawn, overlay.frames);
    if overlay.frames == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}

/// A recorder's state: failed with its last error, on while it records (waiting until it is
/// ready), and off otherwise.
fn capture_line(recording: bool, ready: bool, error: Option<&str>) -> Line {
    let line = if error.is_some() {
        Line::failed("See log")
    } else if recording && !ready {
        Line::waiting("Starting")
    } else if recording {
        Line::ok("Recording")
    } else {
        Line::off("")
    };
    match error {
        Some(error) => line.with_note(error),
        None => line,
    }
}

fn active_word(active: bool) -> &'static str {
    if active { "active" } else { "idle" }
}

fn blocker_label(blocker: Option<OwnershipBlocker>) -> Option<&'static str> {
    Some(match blocker? {
        OwnershipBlocker::Editing => "editing",
        OwnershipBlocker::TrackingRecovery => "tracking recovery",
        OwnershipBlocker::AwaitingNeutral => "awaiting neutral input",
        OwnershipBlocker::DeviceCapturePending => "device capture pending",
        OwnershipBlocker::GameDeviceDisabled => "game device disabled",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Health;

    #[test]
    fn a_recorder_fails_on_an_error_and_is_off_when_idle() {
        assert_eq!(capture_line(false, true, None).health, Health::Off);
        assert_eq!(capture_line(true, false, None).health, Health::Waiting);
        assert_eq!(capture_line(true, true, None).text(), "Ok, Recording");
        let failed = capture_line(false, true, Some("disk full"));
        assert_eq!(failed.health, Health::Failed);
        assert_eq!(failed.note.as_deref(), Some("disk full"));
    }

    #[test]
    fn the_overlay_waits_for_its_first_frame() {
        let status = overlay::pass::OverlayStatus::default();
        assert_eq!(overlay_line(&status).health, Health::Waiting);
        let drawn = overlay::pass::OverlayStatus {
            frames: 3,
            drawn: 12,
            ..status
        };
        assert_eq!(
            overlay_line(&drawn).text(),
            "Ok, 12 drawn per eye, 3 frames"
        );
    }
}
