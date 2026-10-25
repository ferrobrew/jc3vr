//! Read-only Overview projections over published subsystem snapshots.

#[cfg(test)]
#[path = "overview/tests.rs"]
mod tests;

#[cfg(feature = "profiler")]
use std::path::PathBuf;

use egui::{Color32, Response, RichText, Ui};

#[cfg(feature = "profiler")]
use crate::profiler::capture;
use crate::{
    controllers::{frame, overlay},
    egui_impl, frame_timing, recorder,
    recorder::tracking::TrackingStatus,
    status::{self, Health},
    ui::{
        capture::{OwnershipBlocker, OwnershipSnapshot, PanelMode},
        catalog::{FeatureId, FeatureSection},
        navigation::NavigationState,
    },
    vr::input::{self, InputStatus},
};

pub(crate) fn render(ui: &mut Ui, feature: FeatureId, selection: &mut NavigationState) {
    let snapshot = Snapshot::live();
    render_snapshot(ui, feature, selection, &snapshot);
}

#[derive(Debug)]
struct Snapshot {
    input: InputStatus,
    controllers: Option<ControllerSummary>,
    ownership: Option<OwnershipSnapshot>,
    video: recorder::Status,
    tracking: TrackingStatus,
    #[cfg(feature = "profiler")]
    profiler: ProfilerCapture,
    timing: Option<frame_timing::Report>,
    overlay: overlay::OverlayStatus,
    sections: Vec<StatusSection>,
}

impl Snapshot {
    fn live() -> Self {
        // Status producers may read configuration, so no configuration guard spans these reads.
        Self {
            input: input::status(),
            controllers: frame::latest().map(|frame| ControllerSummary {
                focused: frame.focused,
                any_active: frame.any_active(),
            }),
            ownership: egui_impl::debug_ui_capture(),
            video: recorder::status(),
            tracking: recorder::tracking::status(),
            #[cfg(feature = "profiler")]
            profiler: ProfilerCapture {
                recording: capture::is_recording(),
                progress: capture::progress(),
                last_result: capture::last_result(),
            },
            timing: frame_timing::latest(),
            overlay: overlay::status(),
            sections: status::read_all().collect(),
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![
            input_row(&self.input),
            controller_row(self.controllers),
            ownership_row(self.ownership),
            video_row(&self.video),
            tracking_row(&self.tracking),
        ];
        #[cfg(feature = "profiler")]
        rows.push(profiler_row(&self.profiler));
        rows.extend([timing_row(self.timing.as_ref()), overlay_row(&self.overlay)]);
        for (section, title, lines) in &self.sections {
            rows.extend(lines.iter().map(|(label, line)| Row {
                id: RowId::Feature { section, label },
                group: title,
                label,
                state: RowState::Health(line.health),
                active_capture: false,
                detail: line.display_detail.to_string(),
                note: line.display_note.as_ref().map(ToString::to_string),
            }));
        }
        rows
    }
}

type StatusSection = (
    &'static str,
    &'static str,
    Vec<(&'static str, status::Line)>,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ControllerSummary {
    focused: bool,
    any_active: bool,
}

#[cfg(feature = "profiler")]
#[derive(Debug, Default)]
struct ProfilerCapture {
    recording: bool,
    progress: Option<(f32, f32)>,
    last_result: Option<Result<PathBuf, String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowId {
    Input,
    Controllers,
    Ownership,
    Video,
    Tracking,
    #[cfg(feature = "profiler")]
    Profiler,
    Timing,
    Overlay,
    Feature {
        section: &'static str,
        label: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowState {
    Health(Health),
    Unavailable,
}

#[derive(Debug)]
struct Row {
    id: RowId,
    group: &'static str,
    label: &'static str,
    state: RowState,
    active_capture: bool,
    detail: String,
    note: Option<String>,
}

impl Row {
    fn priority(&self) -> u8 {
        match self.state {
            RowState::Health(Health::Failed) => 0,
            _ if self.active_capture => 1,
            RowState::Health(Health::Waiting) | RowState::Unavailable => 2,
            RowState::Health(Health::Ok) => 3,
            RowState::Health(Health::Off) => 4,
        }
    }

    fn needs_attention(&self) -> bool {
        self.active_capture
            || matches!(
                self.state,
                RowState::Health(Health::Failed | Health::Waiting) | RowState::Unavailable
            )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StatusFilter {
    #[default]
    All,
    NeedsAttention,
}

fn project(rows: &[Row], filter: StatusFilter) -> Vec<&Row> {
    let mut visible: Vec<_> = rows
        .iter()
        .filter(|row| filter == StatusFilter::All || row.needs_attention())
        .collect();
    visible.sort_by_key(|row| row.priority());
    visible
}

fn input_row(input: &InputStatus) -> Row {
    Row {
        id: RowId::Input,
        group: "Session",
        label: "OpenXR input",
        state: RowState::Health(if input.error.is_some() || input.restart.is_some() {
            Health::Failed
        } else if input.attached {
            Health::Ok
        } else {
            Health::Waiting
        }),
        active_capture: false,
        detail: if input.attached {
            "Actions attached to a session; attachment does not establish focus.".to_owned()
        } else {
            "No attached input session.".to_owned()
        },
        note: input.error.clone().or_else(|| {
            input
                .restart
                .as_ref()
                .map(|reason| format!("Restart required: {reason:?}"))
        }),
    }
}

fn controller_row(controllers: Option<ControllerSummary>) -> Row {
    let (state, detail) = match controllers {
        Some(frame) => (
            RowState::Health(if frame.focused {
                Health::Ok
            } else {
                Health::Waiting
            }),
            format!(
                "Session focus: {}; any controller active: {}.",
                if frame.focused {
                    "focused"
                } else {
                    "unfocused"
                },
                yes_no(frame.any_active),
            ),
        ),
        None => (
            RowState::Unavailable,
            "No fresh controller frame; session focus and controller activity are unavailable."
                .to_owned(),
        ),
    };
    Row {
        id: RowId::Controllers,
        group: "Session",
        label: "Session focus and controllers",
        state,
        active_capture: false,
        detail,
        note: None,
    }
}

fn ownership_row(ownership: Option<OwnershipSnapshot>) -> Row {
    let (state, detail) = match ownership {
        Some(snapshot) => {
            let capture = snapshot.edit_capture;
            let state = if snapshot.gameplay_requested && !snapshot.gameplay_effective {
                Health::Waiting
            } else if snapshot.mode == PanelMode::Hidden {
                Health::Off
            } else {
                Health::Ok
            };
            (
                RowState::Health(state),
                format!(
                    "{}; editing requested: {}; gameplay requested/effective: {}/{}; edit capture (pointer/grip/keyboard/text/scroll): {}/{}/{}/{}/{}; focused: {}; tracking ready: {}; awaiting neutral: {}; restore pending: {}; blocker: {}.",
                    match snapshot.mode {
                        PanelMode::Hidden => "Hidden",
                        PanelMode::Editing => "Editing",
                        PanelMode::ReadOnly => "Read-only",
                    },
                    yes_no(snapshot.editing_requested),
                    yes_no(snapshot.gameplay_requested),
                    yes_no(snapshot.gameplay_effective),
                    yes_no(capture.pointer),
                    yes_no(capture.grip_drag),
                    yes_no(capture.keyboard),
                    yes_no(capture.text),
                    yes_no(capture.scroll),
                    yes_no(snapshot.focused),
                    yes_no(snapshot.tracking_ready),
                    yes_no(snapshot.awaiting_neutral),
                    yes_no(snapshot.device_restore_pending),
                    blocker_label(snapshot.blocker),
                ),
            )
        }
        None => (
            RowState::Unavailable,
            "No published panel ownership snapshot.".to_owned(),
        ),
    };
    Row {
        id: RowId::Ownership,
        group: "Session",
        label: "Debug panel ownership",
        state,
        active_capture: false,
        detail,
        note: None,
    }
}

fn video_row(video: &recorder::Status) -> Row {
    let state = capture_health(video.recording, video.error.is_some(), video.connected);
    let activity = if video.recording && !video.connected {
        "Recording; waiting for the host listener"
    } else if video.recording {
        "Recording"
    } else {
        "Not recording"
    };
    let size = video.size.map_or_else(
        || "stream size unavailable".to_owned(),
        |(width, height)| format!("{width}x{height}"),
    );
    Row {
        id: RowId::Video,
        group: "Capture and tools",
        label: "Video recording",
        state: RowState::Health(state),
        active_capture: video.recording,
        detail: format!(
            "{activity}; connected: {}; {size}; sent: {}; duplicated: {}; dropped: {}.",
            yes_no(video.connected),
            video.frames_sent,
            video.frames_duplicated,
            video.frames_dropped,
        ),
        note: video
            .error
            .as_ref()
            .map(|error| format!("Last writer error: {error}")),
    }
}

fn tracking_row(tracking: &TrackingStatus) -> Row {
    Row {
        id: RowId::Tracking,
        group: "Capture and tools",
        label: "Tracking capture",
        state: RowState::Health(capture_health(
            tracking.recording,
            tracking.error.is_some(),
            true,
        )),
        active_capture: tracking.recording,
        detail: format!(
            "{}; frames: {}; dropped: {}; stored bytes: {}; duration: {:.1} s.",
            if tracking.recording {
                "Recording"
            } else {
                "Not recording"
            },
            tracking.frames,
            tracking.dropped,
            tracking.bytes,
            tracking.duration.as_secs_f64(),
        ),
        note: tracking
            .error
            .as_ref()
            .map(|error| format!("Last capture error: {error}")),
    }
}

fn capture_health(recording: bool, has_error: bool, ready: bool) -> Health {
    if has_error {
        Health::Failed
    } else if recording && !ready {
        Health::Waiting
    } else if recording {
        Health::Ok
    } else {
        Health::Off
    }
}

#[cfg(feature = "profiler")]
fn profiler_row(profiler: &ProfilerCapture) -> Row {
    let has_error = matches!(profiler.last_result.as_ref(), Some(Err(_)));
    let activity = if profiler.recording {
        "Recording"
    } else {
        "Not recording"
    };
    let progress = profiler.progress.map_or_else(
        || "Capture progress unavailable".to_owned(),
        |(elapsed, total)| format!("Reported capture progress: {elapsed:.1} / {total:.1} s"),
    );
    let note = profiler.last_result.as_ref().map(|result| match result {
        Ok(path) => format!("Last trace written: {}", path.display()),
        Err(error) => format!("Last trace error: {error}"),
    });
    Row {
        id: RowId::Profiler,
        group: "Capture and tools",
        label: "Profiler trace capture",
        state: RowState::Health(capture_health(profiler.recording, has_error, true)),
        active_capture: profiler.recording,
        detail: format!("{activity}; {progress}."),
        note,
    }
}

fn timing_row(timing: Option<&frame_timing::Report>) -> Row {
    Row {
        id: RowId::Timing,
        group: "Capture and tools",
        label: "Performance monitor",
        state: RowState::Health(if timing.is_some() {
            Health::Ok
        } else {
            Health::Waiting
        }),
        active_capture: false,
        detail: timing.map_or_else(
            || "Collecting; no complete timing window is available.".to_owned(),
            |report| {
                format!(
                    "Last complete window: {:.2} s; game frames: {}.",
                    report.window_s, report.totals.frames,
                )
            },
        ),
        note: None,
    }
}

fn overlay_row(overlay: &overlay::OverlayStatus) -> Row {
    Row {
        id: RowId::Overlay,
        group: "Capture and tools",
        label: "World overlay visualization",
        state: RowState::Health(if overlay.error.is_some() {
            Health::Failed
        } else if overlay.frames == 0 {
            Health::Waiting
        } else {
            Health::Ok
        }),
        active_capture: false,
        detail: format!(
            "Last reported primitives per eye: {}; frames with drawing since load: {}. No freshness signal or current active-visualization count is available.",
            overlay.drawn, overlay.frames,
        ),
        note: overlay
            .error
            .as_ref()
            .map(|error| format!("Last overlay error: {error}")),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn blocker_label(blocker: Option<OwnershipBlocker>) -> &'static str {
    match blocker {
        None => "none",
        Some(OwnershipBlocker::Editing) => "editing",
        Some(OwnershipBlocker::Unfocused) => "unfocused",
        Some(OwnershipBlocker::TrackingRecovery) => "tracking recovery",
        Some(OwnershipBlocker::AwaitingNeutral) => "awaiting neutral input",
        Some(OwnershipBlocker::DeviceCapturePending) => "device capture pending",
        Some(OwnershipBlocker::GameDeviceDisabled) => "game device disabled",
    }
}

fn render_snapshot(
    ui: &mut Ui,
    feature: FeatureId,
    selection: &mut NavigationState,
    snapshot: &Snapshot,
) {
    let rows = snapshot.rows();
    match feature {
        FeatureId::Session => {
            ui.heading("Session and input summary");
            render_rows(
                ui,
                rows.iter().filter(|row| {
                    matches!(row.id, RowId::Input | RowId::Controllers | RowId::Ownership)
                }),
            );
            ui.separator();
            ui.heading("Needs attention and active captures");
            render_rows(ui, project(&rows, StatusFilter::NeedsAttention));
        }
        FeatureId::AllStatus => {
            let key = ui.id().with("overview-status-filter");
            let mut filter =
                ui.data_mut(|data| data.get_temp::<StatusFilter>(key).unwrap_or_default());
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut filter, StatusFilter::All, "All status");
                ui.selectable_value(&mut filter, StatusFilter::NeedsAttention, "Needs attention");
            });
            ui.data_mut(|data| data.insert_temp(key, filter));
            ui.label("Failures and active captures appear first. Waiting and unavailable states remain distinct; Off is intentional or unused in this setup.");
            render_rows(ui, project(&rows, filter));
        }
        FeatureId::ActiveTools => {
            ui.heading("Performance, captures, and visualization readouts");
            render_rows(ui, active_tool_rows(&rows));
            ui.label("Other visualization switches remain in Visualizations. A configured switch does not establish that a view is currently drawing; preview renderer availability is reported by its owning view.");
        }
        _ => return,
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (label, feature, section) in [
            (
                "All feature status",
                FeatureId::AllStatus,
                FeatureSection::Status,
            ),
            (
                "Open performance",
                FeatureId::Performance,
                FeatureSection::Status,
            ),
            (
                "Open tracking capture",
                FeatureId::TrackingCapture,
                FeatureSection::Controls,
            ),
            (
                "Open video recording",
                FeatureId::Video,
                FeatureSection::Controls,
            ),
        ] {
            destination(ui, label, NavigationState { feature, section }, selection);
        }
    });
}

fn active_tool_rows(rows: &[Row]) -> Vec<&Row> {
    project(rows, StatusFilter::All)
        .into_iter()
        .filter(|row| match row.id {
            RowId::Video | RowId::Tracking | RowId::Timing | RowId::Overlay => true,
            #[cfg(feature = "profiler")]
            RowId::Profiler => true,
            _ => false,
        })
        .collect()
}

fn render_rows<'a>(ui: &mut Ui, rows: impl IntoIterator<Item = &'a Row>) {
    let mut shown = false;
    for row in rows {
        shown = true;
        ui.group(|ui| {
            ui.label(format!("{} / {}", row.group, row.label));
            let (word, color) = match row.state {
                RowState::Health(Health::Failed) => ("Failed", ui.visuals().error_fg_color),
                RowState::Health(Health::Waiting) => ("Waiting", ui.visuals().warn_fg_color),
                RowState::Health(Health::Ok) => ("Ok", Color32::from_rgb(90, 190, 90)),
                RowState::Health(Health::Off) => ("Off", ui.visuals().weak_text_color()),
                RowState::Unavailable => ("Unavailable", ui.visuals().warn_fg_color),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(word).color(color));
                if row.active_capture {
                    ui.label("Active capture");
                }
            });
            ui.label(&row.detail);
            if let Some(note) = &row.note {
                ui.label(note);
            }
        });
    }
    if !shown {
        ui.label(
            "No failures, active captures, waiting states, or unavailable snapshots are reported.",
        );
    }
}

fn destination(
    ui: &mut Ui,
    label: &str,
    target: NavigationState,
    selection: &mut NavigationState,
) -> Response {
    let response = ui.button(label);
    if response.clicked() {
        *selection = target;
    }
    response
}
