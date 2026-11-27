//! The status rows for the systems outside the controllers: the settings file, the
//! rendering, the controller input's set-up, and the HUD.

use std::sync::OnceLock;

use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_hud::hud::{atlas, config::HUD_CONFIG, world_markers, world_markers::DistanceSource};
use jc3vr_input::frame::Hand;
use jc3vr_view::{
    vr,
    vr::{
        DirectArm, DirectStatus, PublishedStatus, ResolutionStatus, VR_CONFIG, input as vr_input,
        input::{ActionsOrigin, InputStatus, layout, restart, self_check},
    },
};
use openxr as xr;
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const SESSION: Row = Row::new("VR session", session);
pub(crate) const DIRECT_BLIT: Row = Row::new("Direct eye blit", direct_blit);
pub(crate) const RENDER_SIZE: Row = Row::new("Render size", render_size);
pub(crate) const INPUT_ACTIONS: Row = Row::new("Controller actions", input_actions);
pub(crate) const LAYOUT_FINGERPRINT: Row = Row::new("Layout fingerprint", layout_fingerprint);
pub(crate) const SELF_CHECK: Row = Row::new("Self-check", self_check);
pub(crate) const ATLAS: Row = Row::new("Atlas parts", atlas);
pub(crate) const WORLD_MARKERS: Row = Row::new("World markers", world_markers);

fn session() -> Line {
    if !VR_CONFIG.query(|c| c.enabled) {
        return Line::off("");
    }
    session_line(vr::published_status(), vr::render_params(0).is_some())
}

fn direct_blit() -> Line {
    direct_blit_line(vr::direct_status())
}

fn render_size() -> Line {
    let native = VR_CONFIG.query(|c| c.native_resolution);
    render_size_line(native, vr::resolution_status())
}

fn input_actions() -> Line {
    let configured = CONTROLLERS_CONFIG.query(|c| c.dominant_hand);
    input_line(&vr_input::status(), configured)
}

fn layout_fingerprint() -> Line {
    let status = vr_input::status();
    let Some(hand) = status.suggested_for else {
        return Line::waiting("Not attached");
    };
    let fingerprint = format_smolstr!("{:016X}", fingerprint(hand));
    let short = &fingerprint[..8];
    let configured = CONTROLLERS_CONFIG.query(|c| c.dominant_hand);
    match restart::reason(&status, configured) {
        Some(reason) if reason.blocks_input() => {
            Line::failed(short).with_note(format!("{fingerprint}, restart the game"))
        }
        Some(_) => Line::waiting(short).with_note(format!("{fingerprint}, restart for the hand")),
        None => Line::ok(short).with_note(fingerprint.to_string()),
    }
}

fn self_check() -> Line {
    let status = self_check::status();
    match (status.report.is_some(), status.problems) {
        (false, _) if status.skipped.is_some() => Line::off("Checked before"),
        (false, _) => Line::waiting("Not run"),
        (true, 0) => Line::ok(format_smolstr!("Passed, {} checked", status.findings.len())),
        (true, problems) => Line::failed(format_smolstr!(
            "{} of {} one-sided",
            problems,
            status.findings.len()
        )),
    }
}

fn atlas() -> Line {
    if !HUD_CONFIG.query(|c| c.atlas.enabled) {
        return Line::off("");
    }
    let status = atlas::status();
    if status.move_failures > 0 || status.restore_failures > 0 {
        return Line::failed(format_smolstr!(
            "{} fails, see log",
            (status.move_failures + status.restore_failures)
        ))
        .with_note(format!(
            "{} moves and {} restores failed",
            status.move_failures, status.restore_failures
        ));
    }
    let Some(frame) = status.frame else {
        return Line::waiting("No frame");
    };
    let parts = [
        (frame.ammo.is_some(), "ammo"),
        (frame.heat.is_some(), "heat"),
        (frame.cross.is_some(), "cross"),
        (frame.tether.is_some(), "tether"),
    ];
    let found: Vec<SmolStr> = parts
        .iter()
        .filter(|(found, _)| *found)
        .map(|(_, name)| format_smolstr!("{}", name))
        .collect();
    if found.is_empty() {
        Line::waiting("None found")
    } else {
        Line::ok(SmolStr::from(found.join(" ")))
    }
}

fn world_markers() -> Line {
    let Some(status) = world_markers::status() else {
        return Line::waiting("Not drawing");
    };
    let count = |source| {
        status
            .markers
            .iter()
            .filter(|marker| marker.source == source)
            .count()
    };
    let depth = format!(
        "Distance from {} depth, {} ray, {} point",
        count(DistanceSource::Depth),
        count(DistanceSource::Ray),
        count(DistanceSource::Point)
    );
    let line = if status.skipped > 0 {
        Line::ok(format_smolstr!(
            "{} placed, {} left",
            status.markers.len(),
            status.skipped
        ))
    } else {
        Line::ok(format_smolstr!("{} placed", status.markers.len()))
    };
    line.with_note(depth)
}

fn session_line(status: PublishedStatus, rendering: bool) -> Line {
    let state = status.session_state.map(session_state_label);
    if rendering {
        return Line::ok(state.unwrap_or_else(|| SmolStr::new_static("Rendering")));
    }
    if status.runtime_up {
        Line::waiting(state.unwrap_or_else(|| SmolStr::new_static("No state")))
    } else if status.bring_up_failed {
        Line::failed("Retrying, see log")
    } else {
        Line::waiting("Starting")
    }
}

fn session_state_label(state: xr::SessionState) -> SmolStr {
    format_smolstr!(
        "{}",
        match state {
            xr::SessionState::IDLE => "Idle",
            xr::SessionState::READY => "Ready",
            xr::SessionState::SYNCHRONIZED => "Synchronized",
            xr::SessionState::VISIBLE => "Visible",
            xr::SessionState::FOCUSED => "Focused",
            xr::SessionState::STOPPING => "Stopping",
            xr::SessionState::LOSS_PENDING => "Loss pending",
            xr::SessionState::EXITING => "Exiting",
            _ => "Unknown state",
        }
    )
}

fn direct_blit_line(status: DirectStatus) -> Line {
    match status.arm {
        None => Line::waiting("No frame yet"),
        Some(DirectArm::Armed) => match (status.direct_eyes, status.back_buffer_readable) {
            (2, _) => Line::ok("Both eyes"),
            (_, Some(false)) => Line::off("Buffer unreadable"),
            (eyes, _) => Line::waiting(format_smolstr!("{} of 2 eyes", eyes)),
        },
        Some(DirectArm::GaveUp) => Line::failed("See log"),
        Some(DirectArm::AcquireFailed) => Line::waiting("Acquire failed"),
        Some(DirectArm::NotArmed) => Line::off("Single-pass or mono"),
    }
}

fn render_size_line(native: bool, status: ResolutionStatus) -> Line {
    let size = |(w, h): (u32, u32)| format_smolstr!("{}x{}", w, h);
    match (status.target, status.engine) {
        (_, None) => Line::waiting("No engine"),
        (Some(target), Some(engine)) if target == engine => Line::ok(size(engine)),
        (Some(target), Some(_)) => Line::waiting(format_smolstr!("To {}", size(target))),
        (None, Some(engine)) if native => Line::waiting(size(engine)),
        (None, Some(engine)) => Line::off(size(engine)),
    }
}

fn input_line(status: &InputStatus, configured: Hand) -> Line {
    match restart::reason(status, configured) {
        Some(reason) if reason.blocks_input() => return Line::failed("Restart the game"),
        Some(_) => return Line::ok("Restart for hand"),
        None => {}
    }
    if status.error.is_some() {
        return Line::failed("See log");
    }
    match status.origin {
        Some(ActionsOrigin::Created) if status.attached => Line::ok("Created"),
        Some(ActionsOrigin::RewrappedAndAttached | ActionsOrigin::Rewrapped) if status.attached => {
            Line::ok("Reused")
        }
        _ => Line::waiting("Not attached"),
    }
}

/// The layout fingerprint for a player whose dominant hand is `hand`, hashed once per hand: the
/// layout is fixed for the payload's lifetime.
fn fingerprint(hand: Hand) -> u64 {
    static FINGERPRINTS: [OnceLock<u64>; 2] = [OnceLock::new(), OnceLock::new()];
    *FINGERPRINTS[hand.index()].get_or_init(|| layout::fingerprint(hand))
}

#[cfg(test)]
mod tests {
    use jc3vr_view::vr::input::restart::RestartReason;

    use super::*;
    use crate::status::Health;

    #[test]
    fn the_session_reads_rendering_first() {
        let up = PublishedStatus {
            runtime_up: true,
            session_state: Some(xr::SessionState::FOCUSED),
            bring_up_failed: false,
        };
        assert_eq!(session_line(up, true).text(), "Ok, Focused");
        assert_eq!(session_line(up, false).text(), "Waiting, Focused");
        let reused = PublishedStatus {
            session_state: None,
            ..up
        };
        assert_eq!(session_line(reused, true).text(), "Ok, Rendering");
        let down = PublishedStatus {
            runtime_up: false,
            session_state: None,
            bring_up_failed: true,
        };
        assert_eq!(session_line(down, false).health, Health::Failed);
    }

    #[test]
    fn the_direct_blit_names_its_fallback() {
        let armed = |direct_eyes, back_buffer_readable| DirectStatus {
            arm: Some(DirectArm::Armed),
            direct_eyes,
            back_buffer_readable,
        };
        assert_eq!(
            direct_blit_line(armed(2, Some(true))).text(),
            "Ok, Both eyes"
        );
        assert_eq!(
            direct_blit_line(armed(0, Some(false))).text(),
            "Off, Buffer unreadable"
        );
        assert_eq!(
            direct_blit_line(armed(1, Some(true))).health,
            Health::Waiting
        );
        let gave_up = DirectStatus {
            arm: Some(DirectArm::GaveUp),
            ..armed(0, None)
        };
        assert_eq!(direct_blit_line(gave_up).health, Health::Failed);
    }

    #[test]
    fn the_render_size_waits_for_a_resize() {
        let status = |target, engine| ResolutionStatus { target, engine };
        assert_eq!(
            render_size_line(true, status(Some((2016, 2224)), Some((2016, 2224)))).text(),
            "Ok, 2016x2224"
        );
        assert_eq!(
            render_size_line(true, status(Some((2016, 2224)), Some((1920, 1080)))).text(),
            "Waiting, To 2016x2224"
        );
        assert_eq!(
            render_size_line(false, status(None, Some((1920, 1080)))).text(),
            "Off, 1920x1080"
        );
    }

    #[test]
    fn input_needs_a_restart_or_an_attachment() {
        let attached = InputStatus {
            attached: true,
            origin: Some(ActionsOrigin::Created),
            suggested_for: Some(Hand::Right),
            ..InputStatus::default()
        };
        assert_eq!(input_line(&attached, Hand::Right).text(), "Ok, Created");
        assert_eq!(
            input_line(&attached, Hand::Left).text(),
            "Ok, Restart for hand"
        );
        let stale = InputStatus {
            restart: Some(RestartReason::LayoutChanged),
            ..InputStatus::default()
        };
        assert_eq!(input_line(&stale, Hand::Right).health, Health::Failed);
        assert_eq!(
            input_line(&InputStatus::default(), Hand::Right).health,
            Health::Waiting
        );
    }

    #[test]
    fn the_fingerprint_depends_on_the_hand() {
        assert_ne!(fingerprint(Hand::Left), fingerprint(Hand::Right));
        assert_eq!(fingerprint(Hand::Left), layout::fingerprint(Hand::Left));
    }
}
