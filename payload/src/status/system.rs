//! The status rows for the systems outside the controllers: the settings file, the
//! rendering, the controller input's set-up, and the HUD.

use std::sync::OnceLock;

use openxr as xr;
use smol_str::format_smolstr;

use crate::{
    config::{self, SaveStatus},
    controllers::{
        frame::Hand,
        menu::{read_config, save_status_label},
    },
    frame_timing::{self, FramePercentiles},
    hud::{
        atlas,
        world_markers::{self, DistanceSource},
    },
    status::Line,
    vr::{
        self, DirectArm, DirectStatus, PublishedStatus, ResolutionStatus,
        input::{self as vr_input, ActionsOrigin, InputStatus, layout, restart, self_check},
    },
};

pub(super) fn settings_file() -> Line {
    settings_line(config::status())
}

pub(super) fn session() -> Line {
    if !read_config(|c| c.vr.enabled) {
        return Line::off("");
    }
    session_line(vr::published_status(), vr::render_params(0).is_some())
}

pub(super) fn direct_blit() -> Line {
    if !read_config(|c| c.vr.direct_eye_blit) {
        return Line::off("");
    }
    direct_blit_line(vr::direct_status())
}

pub(super) fn frame_time() -> Line {
    frame_time_line(frame_timing::frame_percentiles())
}

pub(super) fn render_size() -> Line {
    let native = read_config(|c| c.vr.native_resolution);
    render_size_line(native, vr::resolution_status())
}

pub(super) fn input_actions() -> Line {
    let configured = read_config(|c| c.controllers.dominant_hand);
    input_line(&vr_input::status(), configured)
}

pub(super) fn layout_fingerprint() -> Line {
    let status = vr_input::status();
    let Some(hand) = status.suggested_for else {
        return Line::waiting("NOT ATTACHED");
    };
    let fingerprint = format_smolstr!("{:016X}", fingerprint(hand));
    let short = &fingerprint[..8];
    let configured = read_config(|c| c.controllers.dominant_hand);
    match restart::reason(&status, configured) {
        Some(reason) if reason.blocks_input() => {
            Line::failed(short).with_note(format!("{fingerprint}, restart the game"))
        }
        Some(_) => Line::waiting(short).with_note(format!("{fingerprint}, restart for the hand")),
        None => Line::ok(short).with_note(fingerprint.to_string()),
    }
}

pub(super) fn self_check() -> Line {
    let status = self_check::status();
    match (status.report.is_some(), status.problems) {
        (false, _) => Line::waiting("NOT RUN"),
        (true, 0) => Line::ok(format_smolstr!("PASSED, {} CHECKED", status.findings.len())),
        (true, problems) => Line::failed(format_smolstr!(
            "{problems} OF {} ONE-SIDED",
            status.findings.len()
        )),
    }
}

pub(super) fn atlas() -> Line {
    if !read_config(|c| c.hud.atlas.enabled) {
        return Line::off("");
    }
    let status = atlas::status();
    if status.move_failures > 0 || status.restore_failures > 0 {
        return Line::failed(format_smolstr!(
            "{} FAILS, SEE LOG",
            status.move_failures + status.restore_failures
        ))
        .with_note(format!(
            "{} moves and {} restores failed",
            status.move_failures, status.restore_failures
        ));
    }
    let Some(frame) = status.frame else {
        return Line::waiting("NO FRAME");
    };
    let parts = [
        (frame.ammo.is_some(), "AMMO"),
        (frame.heat.is_some(), "HEAT"),
        (frame.cross.is_some(), "CROSS"),
        (frame.tether.is_some(), "TETHER"),
    ];
    let found: Vec<&str> = parts
        .iter()
        .filter(|(found, _)| *found)
        .map(|(_, name)| *name)
        .collect();
    if found.is_empty() {
        Line::waiting("NONE FOUND")
    } else {
        Line::ok(found.join(" "))
    }
}

pub(super) fn world_markers() -> Line {
    if !read_config(|c| c.hud.world_markers.enabled) {
        return Line::off("");
    }
    match world_markers::status() {
        Some(status) if status.skipped > 0 => Line::ok(format_smolstr!(
            "{} PLACED, {} LEFT",
            status.markers.len(),
            status.skipped
        )),
        Some(status) => Line::ok(format_smolstr!("{} PLACED", status.markers.len())),
        None => Line::waiting("NOT DRAWING"),
    }
}

pub(super) fn marker_depth() -> Line {
    let on = read_config(|c| c.hud.world_markers.enabled && c.hud.world_markers.surface_placement);
    if !on {
        return Line::off("");
    }
    let Some(status) = world_markers::status() else {
        return Line::waiting("NOT DRAWING");
    };
    let count = |source| {
        status
            .markers
            .iter()
            .filter(|marker| marker.source == source)
            .count()
    };
    Line::ok(format_smolstr!(
        "{} DEPTH, {} RAY, {} PT",
        count(DistanceSource::Depth),
        count(DistanceSource::Ray),
        count(DistanceSource::Point)
    ))
}

fn settings_line(status: SaveStatus) -> Line {
    let label = save_status_label(status);
    match status {
        SaveStatus::UpToDate | SaveStatus::Pending => Line::ok(label),
        SaveStatus::Failed => Line::failed("SEE LOG"),
        SaveStatus::Unavailable => Line::off(label),
    }
}

fn session_line(status: PublishedStatus, rendering: bool) -> Line {
    let state = status.session_state.map(session_state_label);
    if rendering {
        return Line::ok(state.unwrap_or("RENDERING"));
    }
    if status.runtime_up {
        Line::waiting(state.unwrap_or("NO STATE"))
    } else if status.bring_up_failed {
        Line::failed("RETRYING, SEE LOG")
    } else {
        Line::waiting("STARTING")
    }
}

fn session_state_label(state: xr::SessionState) -> &'static str {
    match state {
        xr::SessionState::IDLE => "IDLE",
        xr::SessionState::READY => "READY",
        xr::SessionState::SYNCHRONIZED => "SYNCHRONIZED",
        xr::SessionState::VISIBLE => "VISIBLE",
        xr::SessionState::FOCUSED => "FOCUSED",
        xr::SessionState::STOPPING => "STOPPING",
        xr::SessionState::LOSS_PENDING => "LOSS PENDING",
        xr::SessionState::EXITING => "EXITING",
        _ => "UNKNOWN STATE",
    }
}

fn direct_blit_line(status: DirectStatus) -> Line {
    match status.arm {
        None => Line::waiting("NO FRAME YET"),
        Some(DirectArm::Armed) => match (status.direct_eyes, status.back_buffer_readable) {
            (2, _) => Line::ok("BOTH EYES"),
            (_, Some(false)) => Line::off("BUFFER UNREADABLE"),
            (eyes, _) => Line::waiting(format_smolstr!("{eyes} OF 2 EYES")),
        },
        Some(DirectArm::ConfigOff) => Line::off(""),
        Some(DirectArm::GaveUp) => Line::failed("SEE LOG"),
        Some(DirectArm::AcquireFailed) => Line::waiting("ACQUIRE FAILED"),
        Some(DirectArm::NotArmed) => Line::off("SINGLE-PASS OR MONO"),
    }
}

fn frame_time_line(percentiles: Option<FramePercentiles>) -> Line {
    match percentiles {
        Some(p) => Line::ok(format_smolstr!(
            "{:.1}/{:.1} MS P50/P95",
            p.p50_ms,
            p.p95_ms
        )),
        None => Line::waiting("MEASURING"),
    }
}

fn render_size_line(native: bool, status: ResolutionStatus) -> Line {
    let size = |(w, h): (u32, u32)| format_smolstr!("{w}X{h}");
    match (status.target, status.engine) {
        (_, None) => Line::waiting("NO ENGINE"),
        (Some(target), Some(engine)) if target == engine => Line::ok(size(engine)),
        (Some(target), Some(_)) => Line::waiting(format_smolstr!("TO {}", size(target))),
        (None, Some(engine)) if native => Line::waiting(size(engine)),
        (None, Some(engine)) => Line::off(size(engine)),
    }
}

fn input_line(status: &InputStatus, configured: Hand) -> Line {
    match restart::reason(status, configured) {
        Some(reason) if reason.blocks_input() => return Line::failed("RESTART THE GAME"),
        Some(_) => return Line::ok("RESTART FOR HAND"),
        None => {}
    }
    if status.error.is_some() {
        return Line::failed("SEE LOG");
    }
    match status.origin {
        Some(ActionsOrigin::Created) if status.attached => Line::ok("CREATED"),
        Some(ActionsOrigin::RewrappedAndAttached | ActionsOrigin::Rewrapped) if status.attached => {
            Line::ok("REUSED")
        }
        _ => Line::waiting("NOT ATTACHED"),
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
    use super::*;
    use crate::{status::Health, vr::input::restart::RestartReason};

    #[test]
    fn the_session_reads_rendering_first() {
        let up = PublishedStatus {
            runtime_up: true,
            session_state: Some(xr::SessionState::FOCUSED),
            bring_up_failed: false,
        };
        assert_eq!(session_line(up, true).text(), "OK, FOCUSED");
        assert_eq!(session_line(up, false).text(), "WAITING, FOCUSED");
        let reused = PublishedStatus {
            session_state: None,
            ..up
        };
        assert_eq!(session_line(reused, true).text(), "OK, RENDERING");
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
            "OK, BOTH EYES"
        );
        assert_eq!(
            direct_blit_line(armed(0, Some(false))).text(),
            "OFF, BUFFER UNREADABLE"
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
    fn frame_times_show_both_percentiles() {
        let p = FramePercentiles {
            p50_ms: 11.1,
            p95_ms: 13.94,
        };
        assert_eq!(frame_time_line(Some(p)).text(), "OK, 11.1/13.9 MS P50/P95");
        assert_eq!(frame_time_line(None).health, Health::Waiting);
    }

    #[test]
    fn the_render_size_waits_for_a_resize() {
        let status = |target, engine| ResolutionStatus { target, engine };
        assert_eq!(
            render_size_line(true, status(Some((2016, 2224)), Some((2016, 2224)))).text(),
            "OK, 2016X2224"
        );
        assert_eq!(
            render_size_line(true, status(Some((2016, 2224)), Some((1920, 1080)))).text(),
            "WAITING, TO 2016X2224"
        );
        assert_eq!(
            render_size_line(false, status(None, Some((1920, 1080)))).text(),
            "OFF, 1920X1080"
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
        assert_eq!(input_line(&attached, Hand::Right).text(), "OK, CREATED");
        assert_eq!(
            input_line(&attached, Hand::Left).text(),
            "OK, RESTART FOR HAND"
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
