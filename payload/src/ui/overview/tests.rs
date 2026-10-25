use std::time::Duration;

use super::*;
use crate::{frame_timing::Totals, status::Line, ui::capture::EditCapture};

#[test]
fn all_status_retains_every_typed_row_and_stably_ranks_problems_and_captures() {
    let mut snapshot = fixture();
    snapshot.video.recording = true;
    snapshot.video.connected = false;
    snapshot.tracking.recording = true;
    snapshot.sections = vec![(
        "features",
        "Features",
        vec![
            ("Idle first", line(Health::Ok, "Healthy idle")),
            ("Failed first", line(Health::Failed, "First failure")),
            ("Intentional off", line(Health::Off, "Unused in this setup")),
            (
                "Waiting first",
                line(Health::Waiting, "Waiting for a player"),
            ),
            ("Failed second", line(Health::Failed, "Second failure")),
            ("Idle second", line(Health::Ok, "Healthy idle")),
            (
                "Waiting second",
                line(Health::Waiting, "Waiting for a sample"),
            ),
        ],
    )];
    let rows = snapshot.rows();
    let all = project(&rows, StatusFilter::All);
    assert_eq!(all.len(), rows.len());
    assert_eq!(all[0].label, "Failed first");
    assert_eq!(all[1].label, "Failed second");
    assert_eq!(all[2].id, RowId::Video);
    assert_eq!(all[3].id, RowId::Tracking);
    let feature_labels: Vec<_> = all
        .iter()
        .filter(|row| matches!(row.id, RowId::Feature { .. }))
        .map(|row| row.label)
        .collect();
    assert_eq!(
        feature_labels,
        [
            "Failed first",
            "Failed second",
            "Waiting first",
            "Waiting second",
            "Idle first",
            "Idle second",
            "Intentional off",
        ],
    );
    let failed = all.iter().find(|row| row.label == "Failed first").unwrap();
    assert_eq!(failed.detail, "First failure");
    assert_eq!(failed.note.as_deref(), Some("Published display note"));
}

#[test]
fn needs_attention_keeps_active_waiting_and_unavailable_but_not_idle_or_off() {
    let mut snapshot = fixture();
    snapshot.controllers = None;
    snapshot.ownership = None;
    snapshot.timing = None;
    snapshot.video.recording = true;
    snapshot.sections = vec![(
        "features",
        "Features",
        vec![
            ("Healthy idle", line(Health::Ok, "Ready")),
            ("Disabled", line(Health::Off, "Intentional")),
            ("Waiting", line(Health::Waiting, "No player")),
            ("Broken", line(Health::Failed, "Error")),
        ],
    )];
    let rows = snapshot.rows();
    let attention = project(&rows, StatusFilter::NeedsAttention);
    assert_eq!(attention[0].label, "Broken");
    assert_eq!(attention[1].id, RowId::Video);
    assert!(attention.iter().any(|row| row.label == "Waiting"));
    assert!(attention.iter().any(|row| row.id == RowId::Controllers));
    assert!(attention.iter().any(|row| row.id == RowId::Ownership));
    assert!(attention.iter().any(|row| row.id == RowId::Timing));
    assert!(!attention.iter().any(|row| row.label == "Healthy idle"));
    assert!(!attention.iter().any(|row| row.label == "Disabled"));
    assert!(!attention.iter().any(|row| row.id == RowId::Tracking));
}

#[test]
fn attachment_focus_and_missing_ownership_are_independent() {
    let mut snapshot = fixture();
    snapshot.controllers = None;
    snapshot.ownership = None;
    let rows = snapshot.rows();
    assert_eq!(
        find(&rows, RowId::Input).state,
        RowState::Health(Health::Ok)
    );
    let controllers = find(&rows, RowId::Controllers);
    assert_eq!(controllers.state, RowState::Unavailable);
    assert!(controllers.detail.contains("No fresh controller frame"));
    assert!(!controllers.detail.contains("unfocused"));
    assert_eq!(find(&rows, RowId::Ownership).state, RowState::Unavailable);

    snapshot.controllers = Some(ControllerSummary {
        focused: false,
        any_active: true,
    });
    let rows = snapshot.rows();
    assert_eq!(
        find(&rows, RowId::Input).state,
        RowState::Health(Health::Ok)
    );
    let controllers = find(&rows, RowId::Controllers);
    assert_eq!(controllers.state, RowState::Health(Health::Waiting));
    assert!(controllers.detail.contains("Session focus: unfocused"));
    assert!(controllers.detail.contains("any controller active: yes"));
}

#[test]
fn ownership_preserves_requested_effective_capture_and_recovery_readouts() {
    let mut snapshot = fixture();
    let ownership = snapshot.ownership.as_mut().unwrap();
    ownership.mode = PanelMode::ReadOnly;
    ownership.editing_requested = false;
    ownership.gameplay_requested = true;
    ownership.gameplay_effective = false;
    ownership.edit_capture.grip_drag = true;
    ownership.awaiting_neutral = true;
    ownership.device_restore_pending = true;
    ownership.blocker = Some(OwnershipBlocker::AwaitingNeutral);
    let rows = snapshot.rows();
    let ownership = find(&rows, RowId::Ownership);
    assert_eq!(ownership.state, RowState::Health(Health::Waiting));
    assert!(
        ownership
            .detail
            .contains("Read-only; editing requested: no")
    );
    assert!(
        ownership
            .detail
            .contains("gameplay requested/effective: yes/no")
    );
    assert!(ownership.detail.contains("no/yes/no/no/no"));
    assert!(ownership.detail.contains("awaiting neutral: yes"));
    assert!(ownership.detail.contains("restore pending: yes"));
    assert!(ownership.detail.contains("blocker: awaiting neutral input"));
}

#[test]
fn recorder_activity_is_independent_of_connection_counters_and_last_error() {
    let mut snapshot = fixture();
    snapshot.video.recording = true;
    snapshot.video.connected = false;
    snapshot.video.size = Some((1920, 1080));
    snapshot.video.frames_sent = 11;
    snapshot.video.frames_duplicated = 3;
    snapshot.video.frames_dropped = 2;
    let rows = snapshot.rows();
    let video = find(&rows, RowId::Video);
    assert_eq!(video.state, RowState::Health(Health::Waiting));
    assert!(video.active_capture);
    assert!(
        video
            .detail
            .contains("Recording; waiting for the host listener")
    );
    assert!(video.detail.contains("connected: no; 1920x1080"));
    assert!(video.detail.contains("sent: 11; duplicated: 3; dropped: 2"));

    snapshot.video.connected = true;
    let rows = snapshot.rows();
    assert_eq!(
        find(&rows, RowId::Video).state,
        RowState::Health(Health::Ok)
    );
    snapshot.video.recording = false;
    snapshot.video.error = Some("Writer disconnected".to_owned());
    let rows = snapshot.rows();
    let video = find(&rows, RowId::Video);
    assert_eq!(video.state, RowState::Health(Health::Failed));
    assert!(!video.active_capture);
    assert_eq!(
        video.note.as_deref(),
        Some("Last writer error: Writer disconnected")
    );
    assert!(
        project(&rows, StatusFilter::NeedsAttention)
            .iter()
            .any(|row| row.id == RowId::Video)
    );
}

#[test]
fn tracking_errors_dropped_frames_and_activity_remain_separate() {
    let mut snapshot = fixture();
    snapshot.tracking.recording = true;
    snapshot.tracking.frames = 22;
    snapshot.tracking.dropped = 4;
    snapshot.tracking.bytes = 512;
    snapshot.tracking.duration = Duration::from_millis(1500);
    snapshot.tracking.error = Some("Buffered write error".to_owned());
    let rows = snapshot.rows();
    let tracking = find(&rows, RowId::Tracking);
    assert_eq!(tracking.state, RowState::Health(Health::Failed));
    assert!(tracking.active_capture);
    assert!(
        tracking
            .detail
            .contains("frames: 22; dropped: 4; stored bytes: 512; duration: 1.5 s")
    );
    assert_eq!(
        tracking.note.as_deref(),
        Some("Last capture error: Buffered write error")
    );
    snapshot.tracking.error = None;
    let rows = snapshot.rows();
    assert_eq!(
        find(&rows, RowId::Tracking).state,
        RowState::Health(Health::Ok)
    );
}

#[test]
fn timing_collecting_and_overlay_last_report_do_not_claim_current_visualization_activity() {
    let mut snapshot = fixture();
    snapshot.timing = None;
    snapshot.overlay.drawn = 17;
    snapshot.overlay.frames = 12;
    let rows = snapshot.rows();
    let timing = find(&rows, RowId::Timing);
    assert_eq!(timing.state, RowState::Health(Health::Waiting));
    assert!(timing.detail.contains("Collecting"));
    let overlay = find(&rows, RowId::Overlay);
    assert!(!overlay.active_capture);
    assert!(
        overlay
            .detail
            .contains("Last reported primitives per eye: 17")
    );
    assert!(overlay.detail.contains("No freshness signal"));
    snapshot.overlay.error = Some("Pipeline unavailable".to_owned());
    let rows = snapshot.rows();
    assert_eq!(
        find(&rows, RowId::Overlay).state,
        RowState::Health(Health::Failed)
    );
}

#[test]
fn every_overview_destination_renders_without_a_renderer_or_live_producers() {
    let snapshot = fixture();
    for width in [420.0, 1100.0] {
        for feature in [
            FeatureId::Session,
            FeatureId::AllStatus,
            FeatureId::ActiveTools,
        ] {
            let context = egui::Context::default();
            let mut selection = NavigationState {
                feature,
                section: FeatureSection::Status,
            };
            let expected = selection;
            let mut rendered = false;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    render_snapshot(ui, feature, &mut selection, &snapshot);
                    rendered = true;
                },
            );
            assert!(rendered);
            assert!(!output.shapes.is_empty());
            assert_eq!(selection, expected);
        }
    }
}

#[test]
fn real_destination_widget_changes_only_navigation_on_click() {
    let context = egui::Context::default();
    let mut selection = NavigationState::default();
    let target = NavigationState {
        feature: FeatureId::Video,
        section: FeatureSection::Controls,
    };
    let mut rect = egui::Rect::NOTHING;
    let mut clicked = false;
    for pressed in [None, Some(true), Some(false)] {
        let events = pressed.map_or_else(Vec::new, |pressed| {
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        });
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                let response = destination(ui, "Open video recording", target, &mut selection);
                rect = response.rect;
                clicked |= response.clicked();
            },
        );
    }
    assert!(clicked);
    assert_eq!(selection, target);
}

#[cfg(feature = "profiler")]
#[test]
fn idle_profiler_result_does_not_establish_capture_activity() {
    let mut snapshot = fixture();
    snapshot.profiler.last_result = Some(Ok(PathBuf::from("profile/completed.json")));
    let rows = snapshot.rows();
    let tools = active_tool_rows(&rows);
    let profiler = tools.iter().find(|row| row.id == RowId::Profiler).unwrap();
    assert_eq!(profiler.state, RowState::Health(Health::Off));
    assert!(!profiler.active_capture);
    assert!(
        profiler
            .detail
            .contains("Not recording; Capture progress unavailable")
    );
    assert_eq!(
        profiler.note.as_deref(),
        Some("Last trace written: profile/completed.json"),
    );
    assert!(
        !project(&rows, StatusFilter::NeedsAttention)
            .iter()
            .any(|row| row.id == RowId::Profiler)
    );
    assert!(tools.iter().any(|row| row.id == RowId::Timing));
}

#[cfg(feature = "profiler")]
#[test]
fn recording_profiler_is_active_in_tools_and_attention_with_independent_progress_and_result() {
    let mut snapshot = fixture();
    snapshot.profiler.recording = true;
    snapshot.profiler.progress = Some((1.5, 5.0));
    snapshot.profiler.last_result = Some(Ok(PathBuf::from("profile/previous.json")));
    let rows = snapshot.rows();
    let tools = active_tool_rows(&rows);
    assert_eq!(tools[0].id, RowId::Profiler);
    let profiler = tools[0];
    assert_eq!(profiler.state, RowState::Health(Health::Ok));
    assert!(profiler.active_capture);
    assert!(
        profiler
            .detail
            .contains("Recording; Reported capture progress: 1.5 / 5.0 s")
    );
    assert_eq!(
        profiler.note.as_deref(),
        Some("Last trace written: profile/previous.json"),
    );
    assert_eq!(
        project(&rows, StatusFilter::NeedsAttention)[0].id,
        RowId::Profiler
    );
    assert!(!find(&rows, RowId::Timing).active_capture);

    snapshot.profiler.progress = None;
    let rows = snapshot.rows();
    assert!(find(&rows, RowId::Profiler).active_capture);
    assert!(
        find(&rows, RowId::Profiler)
            .detail
            .contains("Capture progress unavailable")
    );
}

#[cfg(feature = "profiler")]
#[test]
fn failed_inactive_profiler_remains_attention_without_claiming_recording() {
    let mut snapshot = fixture();
    snapshot.profiler.progress = Some((5.0, 5.0));
    snapshot.profiler.last_result = Some(Err("Trace write error".to_owned()));
    let rows = snapshot.rows();
    let tools = active_tool_rows(&rows);
    assert_eq!(tools[0].id, RowId::Profiler);
    let profiler = tools[0];
    assert_eq!(profiler.state, RowState::Health(Health::Failed));
    assert!(!profiler.active_capture);
    assert!(
        profiler
            .detail
            .contains("Not recording; Reported capture progress: 5.0 / 5.0 s")
    );
    assert_eq!(
        profiler.note.as_deref(),
        Some("Last trace error: Trace write error")
    );
    assert_eq!(
        project(&rows, StatusFilter::NeedsAttention)[0].id,
        RowId::Profiler
    );

    snapshot.profiler.recording = true;
    let rows = snapshot.rows();
    let profiler = find(&rows, RowId::Profiler);
    assert_eq!(profiler.state, RowState::Health(Health::Failed));
    assert!(profiler.active_capture);
}

fn fixture() -> Snapshot {
    Snapshot {
        input: InputStatus {
            attached: true,
            ..Default::default()
        },
        controllers: Some(ControllerSummary {
            focused: true,
            any_active: true,
        }),
        ownership: Some(OwnershipSnapshot {
            mode: PanelMode::Editing,
            visible: true,
            editing_requested: true,
            gameplay_requested: false,
            edit_capture: EditCapture::default(),
            gameplay_effective: false,
            blocker: Some(OwnershipBlocker::Editing),
            awaiting_neutral: false,
            focused: true,
            tracking_ready: true,
            prior_game_device_enabled: Some(true),
            observed_game_device_enabled: false,
            intended_game_device_enabled: false,
            device_restore_pending: false,
        }),
        video: recorder::Status {
            recording: false,
            size: None,
            frames_sent: 0,
            frames_duplicated: 0,
            frames_dropped: 0,
            connected: false,
            error: None,
            folder: None,
        },
        tracking: TrackingStatus::default(),
        #[cfg(feature = "profiler")]
        profiler: ProfilerCapture::default(),
        timing: Some(frame_timing::Report {
            window_s: 1.0,
            totals: Totals::default(),
        }),
        overlay: overlay::OverlayStatus {
            frames: 1,
            ..Default::default()
        },
        sections: Vec::new(),
    }
}

fn line(health: Health, detail: &str) -> Line {
    Line {
        health,
        detail: "Legacy detail".into(),
        note: Some("Legacy note".to_owned()),
        display_detail: detail.into(),
        display_note: Some("Published display note".into()),
    }
}

fn find(rows: &[Row], id: RowId) -> &Row {
    rows.iter().find(|row| row.id == id).unwrap()
}
