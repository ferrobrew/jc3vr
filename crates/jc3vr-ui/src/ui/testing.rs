//! The Tools page's tracking capture feature: the tracking recorder (`jc3vr_view::recorder::tracking`).

use egui::Color32;
use jc3vr_tooling::testing::{TESTING_CONFIG, request};
use jc3vr_view::recorder::tracking;

/// Render the tracking recorder's controls.
pub(crate) fn tracking_feature(ui: &mut egui::Ui) {
    {
        let mut cfg = TESTING_CONFIG.edit();
        ui.checkbox(&mut cfg.enabled, "Test tools available")
            .on_hover_text(
                "Whether the recorder can be started from here, the director's keys, and the \
                 request file. Nothing runs until it is started.",
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

fn recording_ui(ui: &mut egui::Ui) {
    let status = tracking::status();
    ui.label(if status.recording {
        "Tracking recording: running with the session recording"
    } else {
        "Tracking recording: start a session recording with F1 to capture data"
    });
    let mut cfg = TESTING_CONFIG.edit();
    ui.checkbox(
        &mut cfg.record_hand_joints,
        "Record the tracked hand joints",
    )
    .on_hover_text("They triple a frame's size, and only the finger curls read them.");
    drop(cfg);
    ui.label(
        "A session recording (F1) records the head, both controllers, their inputs, the semantic \
         actions, and the mod's controller, grip, and gun release events each VR frame, starting \
         from where the player stands.",
    );
    ui.label(format!(
        "A script can drop {} into the configuration directory to start and stop a recording.",
        request::FILE_NAME
    ));
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

/// A labelled path, selectable so it can be copied.
fn path_line(ui: &mut egui::Ui, label: &str, path: &std::path::Path) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        let text = path.display().to_string();
        let mut shown = text.as_str();
        ui.add(egui::TextEdit::singleline(&mut shown).desired_width(ui.available_width()));
    });
}
