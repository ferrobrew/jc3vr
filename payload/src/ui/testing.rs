//! The Testing tab: the tracking recorder (`crate::recorder::tracking`), the replay
//! (`crate::testing::replay`), and the scenario runner (`crate::testing::scenario`).

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use egui::Color32;
use parking_lot::Mutex;

use crate::{
    config,
    recorder::tracking,
    testing::{
        replay::{self, Pacing, Phase, ReplayOptions, ReplayOutcome, StartMode},
        request, scenario,
        signals::Signal,
    },
};

/// Render the Testing tab body.
pub(crate) fn tracking_feature(ui: &mut egui::Ui) {
    {
        let mut cfg = config::edit();
        ui.checkbox(&mut cfg.testing.enabled, "Test tools available")
            .on_hover_text(
                "Whether the recorder, the replay, and the scenarios can be started from here, \
                 the director's keys, and the request file. Nothing runs until it is started.",
            );
    }
    egui::CollapsingHeader::new("Tracking recording")
        .default_open(true)
        .show(ui, recording_ui);
}

pub(crate) fn replay_feature(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Replay")
        .default_open(true)
        .show(ui, replay_ui);
    egui::CollapsingHeader::new("Scenarios")
        .default_open(true)
        .show(ui, scenarios_ui);
    egui::CollapsingHeader::new("Signals")
        .default_open(false)
        .show(ui, signals_ui);
}

fn recording_ui(ui: &mut egui::Ui) {
    let status = tracking::status();
    ui.horizontal(|ui| {
        if status.recording {
            if ui.button("Stop recording").clicked() {
                tracking::request_stop();
            }
        } else if ui.button("Start recording").clicked() {
            tracking::request_start();
        }
        ui.label(if status.recording {
            "recording"
        } else {
            "stopped"
        });
    });
    if status.recording || status.path.is_some() {
        ui.label(format!(
            "{} frames, {} dropped, {:.1} s, {} KiB",
            status.frames,
            status.dropped,
            status.duration.as_secs_f32(),
            status.bytes / 1024
        ));
    }
    if let Some(path) = &status.path {
        path_line(ui, "File", path);
    }
    if let Some(error) = &status.error {
        ui.colored_label(Color32::LIGHT_RED, error);
    }
    let mut cfg = config::edit();
    ui.checkbox(
        &mut cfg.testing.record_hand_joints,
        "Record the tracked hand joints",
    )
    .on_hover_text("They triple a frame's size, and only the finger curls read them.");
    ui.checkbox(
        &mut cfg.testing.record_compression,
        "Use experimental lossless recording compression",
    )
    .on_hover_text(
        "Applies to the next recording. Uses version 3 chunks without dropping samples or joints. Real-capture performance acceptance is pending; version 2 remains the default.",
    );
    ui.label(
        "Records the head, both controllers, their inputs, and the semantic actions each VR \
         frame, starting from where the player stands. Ctrl+Numpad/ toggles it.",
    );
}

/// The replay form's fields.
struct ReplayForm {
    path: String,
    options: ReplayOptions,
}

static REPLAY_FORM: Mutex<ReplayForm> = Mutex::new(ReplayForm {
    path: String::new(),
    options: ReplayOptions {
        start: StartMode::Marker,
        pacing: Pacing::RealTime,
    },
});

fn replay_ui(ui: &mut egui::Ui) {
    let status = replay::status();
    let mut form = REPLAY_FORM.lock();
    ui.horizontal(|ui| {
        ui.label("Recording:");
        ui.add(
            egui::TextEdit::singleline(&mut form.path).desired_width(ui.available_width() - 90.0),
        );
        if ui
            .button("Last")
            .on_hover_text("This session's last tracking recording.")
            .clicked()
            && let Some(path) = tracking::status().path
        {
            form.path = path.display().to_string();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Start:");
        for mode in StartMode::ALL {
            ui.selectable_value(&mut form.options.start, mode, mode.label());
        }
    });
    ui.horizontal(|ui| {
        ui.label("Pacing:");
        for pacing in Pacing::ALL {
            ui.selectable_value(&mut form.options.pacing, pacing, pacing.label());
        }
    });
    ui.horizontal(|ui| {
        if status.phase == Phase::Idle {
            let path = form.path.trim();
            if ui
                .add_enabled(!path.is_empty(), egui::Button::new("Replay"))
                .clicked()
            {
                replay::start(PathBuf::from(path), form.options);
            }
        } else if ui.button("Stop").clicked() {
            replay::stop();
        }
        ui.label(match status.phase {
            Phase::Idle => "idle".to_owned(),
            Phase::Loading => "loading".to_owned(),
            Phase::Teleporting => "teleporting to the start".to_owned(),
            Phase::Playing => format!(
                "playing frame {} of {}, {:.1} s",
                status.frame,
                status.frames,
                status.elapsed.as_secs_f32()
            ),
        });
    });
    for warning in &status.warnings {
        ui.colored_label(Color32::YELLOW, warning);
    }
    match &status.last {
        Some(ReplayOutcome::Completed { frames, duration }) => {
            ui.label(format!(
                "Last replay: {frames} frames in {:.1} s",
                duration.as_secs_f32()
            ));
        }
        Some(ReplayOutcome::Stopped) => {
            ui.label("Last replay: stopped");
        }
        Some(ReplayOutcome::Failed(error)) => {
            ui.colored_label(Color32::LIGHT_RED, error.to_string());
        }
        None => {}
    }
    ui.label(
        "Plays the recording's head and controllers in place of the headset's, with or without a \
         session. Ctrl+Numpad* replays the last recording or stops.",
    );
}

/// The scenario files, listed at most every [`LIST_REFRESH`] rather than every frame.
static LISTING: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);

/// How often the scenario folder is listed again.
const LIST_REFRESH: Duration = Duration::from_secs(2);

fn scenarios_ui(ui: &mut egui::Ui) {
    let folder = scenario::folder();
    match &folder {
        Some(folder) => path_line(ui, "Folder", folder),
        None => {
            ui.label("No scenario folder: set testing.scenario_dir.");
        }
    }
    {
        let mut cfg = config::edit();
        ui.horizontal(|ui| {
            ui.label("Folder setting:");
            ui.text_edit_singleline(&mut cfg.testing.scenario_dir)
                .on_hover_text("Empty for the testing folder in the configuration directory.");
        });
    }
    let files = {
        let mut listing = LISTING.lock();
        if listing
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= LIST_REFRESH)
        {
            *listing = Some((Instant::now(), scenario::list()));
        }
        listing
            .as_ref()
            .map(|(_, files)| files.clone())
            .unwrap_or_default()
    };
    let status = scenario::status();
    let running = status.running.is_some();
    if files.is_empty() {
        ui.label("No scenario files in the folder.");
    }
    egui::Grid::new("testing_scenarios").show(ui, |ui| {
        for file in &files {
            let name = file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            ui.label(name);
            if ui.add_enabled(!running, egui::Button::new("Run")).clicked() {
                scenario::start(file.clone());
            }
            ui.end_row();
        }
    });
    if let Some((name, results)) = &status.running {
        ui.horizontal(|ui| {
            ui.label(format!("Running {name}"));
            if ui.button("Stop").clicked() {
                scenario::stop();
            }
        });
        for result in results {
            verdict_line(
                ui,
                &result.name,
                result.verdict.passed,
                &result.verdict.detail,
                true,
            );
        }
    }
    if let Some(error) = &status.error {
        ui.colored_label(Color32::LIGHT_RED, error);
    }
    if let Some(report) = &status.last {
        ui.separator();
        let total = report.results.len();
        let passed = report.passed();
        let text = match &report.aborted {
            Some(reason) => format!("Last: {}: did not finish: {reason}", report.name),
            None => format!(
                "Last: {}: {passed} of {total} passed over {} frames",
                report.name, report.frames
            ),
        };
        let all = report.aborted.is_none() && passed == total;
        ui.colored_label(
            if all {
                Color32::LIGHT_GREEN
            } else {
                Color32::LIGHT_RED
            },
            text,
        );
        for result in &report.results {
            verdict_line(
                ui,
                &result.name,
                result.verdict.passed,
                &result.verdict.detail,
                false,
            );
        }
        if let Some(summary) = &report.summary {
            path_line(ui, "Summary", summary);
        }
    }
    ui.label(format!(
        "Ctrl+Numpad- runs the last scenario again or stops it. A script can drop {} into the \
         configuration directory to start and stop the tools.",
        request::FILE_NAME
    ));
}

fn signals_ui(ui: &mut egui::Ui) {
    egui::Grid::new("testing_signals")
        .striped(true)
        .show(ui, |ui| {
            for signal in Signal::ALL {
                ui.label(signal.name());
                ui.label(signal.read().to_string());
                ui.label(egui::RichText::new(signal.description()).weak());
                ui.end_row();
            }
        });
}

fn verdict_line(ui: &mut egui::Ui, name: &str, passed: bool, detail: &str, live: bool) {
    let (word, color) = match (passed, live) {
        (true, _) => ("PASS", Color32::LIGHT_GREEN),
        (false, true) => ("....", Color32::GRAY),
        (false, false) => ("FAIL", Color32::LIGHT_RED),
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, word);
        ui.label(name);
        ui.label(egui::RichText::new(detail).weak());
    });
}

/// A labelled path, selectable so it can be copied.
fn path_line(ui: &mut egui::Ui, label: &str, path: &std::path::Path) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        let text = path.display().to_string();
        let mut shown = text.as_str();
        ui.add(egui::TextEdit::singleline(&mut shown).desired_width(ui.available_width()));
    });
}
