//! The Tools page's tracking capture and replay features: the tracking recorder
//! (`crate::recorder::tracking`) and the replay (`crate::testing::replay`).

use std::path::PathBuf;

use egui::Color32;
use parking_lot::Mutex;

use crate::{
    config,
    recorder::tracking,
    testing::{
        replay::{self, Pacing, Phase, ReplayOptions, ReplayOutcome, StartMode},
        request,
    },
};

/// Render the tracking recorder's controls.
pub(crate) fn tracking_feature(ui: &mut egui::Ui) {
    {
        let mut cfg = config::edit();
        ui.checkbox(&mut cfg.testing.enabled, "Test tools available")
            .on_hover_text(
                "Whether the recorder and the replay can be started from here, \
                 the director's keys, and the request file. Nothing runs until it is started.",
            );
    }
    egui::CollapsingHeader::new("Tracking recording")
        .default_open(true)
        .show(ui, recording_ui);
}

pub(crate) fn tracking_status(ui: &mut egui::Ui) {
    let status = tracking::peek_status();
    ui.label(if status.recording {
        "recording"
    } else {
        "stopped"
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
}

pub(crate) fn replay_feature(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Replay")
        .default_open(true)
        .show(ui, replay_ui);
}

pub(crate) fn replay_status(ui: &mut egui::Ui) {
    let status = replay::status();
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
}

fn recording_ui(ui: &mut egui::Ui) {
    let status = tracking::status();
    ui.label(if status.recording {
        "Tracking recording: running with the session recording"
    } else {
        "Tracking recording: start a session recording with F1 to capture data"
    });
    let mut cfg = config::edit();
    ui.checkbox(
        &mut cfg.testing.record_hand_joints,
        "Record the tracked hand joints",
    )
    .on_hover_text("They triple a frame's size, and only the finger curls read them.");
    drop(cfg);
    ui.label(
        "A session recording (F1) records the head, both controllers, their inputs, the semantic \
         actions, and the mod's controller, grip, and gun release events each VR frame, starting \
         from where the player stands.",
    );
    ui.separator();
    lookback_ui(ui);
}

fn lookback_ui(ui: &mut egui::Ui) {
    let status = tracking::lookback_status();
    ui.horizontal(|ui| {
        let label = format!("Save the last {} s", tracking::lookback::WINDOW.as_secs());
        if ui
            .add_enabled(status.frames > 0, egui::Button::new(label))
            .on_hover_text(
                "Write the rolling window of recent frames and events to a new recording in this \
                 session's tracking folder.",
            )
            .clicked()
        {
            tracking::lookback::request_save();
        }
        ui.label(if status.running {
            format!(
                "{} frames, {:.1} s, {} KiB held",
                status.frames,
                status.seconds,
                status.bytes / 1024
            )
        } else {
            "kept only while motion controllers are enabled".to_owned()
        });
    });
    let dropped = tracking::events::dropped();
    if dropped > 0 {
        ui.label(format!("{dropped} events dropped waiting for a frame"));
    }
    if let Some(path) = &status.path {
        path_line(ui, "Last lookback", path);
    }
    if let Some(error) = &status.error {
        ui.colored_label(Color32::LIGHT_RED, error);
    }
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
    });
    ui.label(
        "Plays the recording's head and controllers in place of the headset's, with or without a \
         session. Ctrl+Numpad* replays the last recording or stops.",
    );
    ui.label(format!(
        "A script can drop {} into the configuration directory to start and stop the tools.",
        request::FILE_NAME
    ));
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
