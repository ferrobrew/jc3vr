//! Renderer-free checks of the production video status and controls routes.

use super::*;
use crate::{config, director::markers, recorder};

#[test]
fn actual_video_status_reads_the_recorder_snapshot_without_a_renderer_or_actions() {
    assert_eq!(route(FeatureId::Video), FeatureRoute::Director);
    let before = video_state();
    assert!(!before.recorder.recording);
    assert!(!before.recording_flag);
    let expected = status_labels(&before.recorder);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Video,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = video_frame(&context, &mut selection, width, Vec::new());
        assert_status_labels(&labels, &expected);
        for (_, rect) in labels {
            for pressed in [true, false] {
                let labels = video_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_status_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(video_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(video_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_video_controls_keep_the_idle_action_settings_instructions_and_markers() {
    let before = video_state();
    assert!(!before.recorder.recording);
    assert!(!before.recording_flag);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Video,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = video_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Start recording",
            "Eye:",
            "left",
            "right",
            "Scale",
            "Frame rate (header)",
            "Host:",
            "Port:",
            "Run scripts/capture_host.sh on the host to listen; the stream is yuv4mpegpipe over TCP \
             and the per-frame pose sidecar goes to the session folder.",
            "Label:",
            "Mark",
            "Appended to markers.jsonl in the session folder.",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "missing Video control or instruction: {expected}"
            );
        }
        for forbidden in [
            "Stop recording",
            "stopped",
            "recording",
            "recording, waiting for the host listener",
        ] {
            assert!(
                labels.iter().all(|(text, _)| text != forbidden),
                "unexpected Video Controls label while idle: {forbidden}"
            );
        }
        for prefix in [
            "Stream ",
            "Sidecar in the session folder under ",
            "Last error: ",
            "Unavailable: the DX11 renderer",
        ] {
            assert!(
                labels.iter().all(|(text, _)| !text.starts_with(prefix)),
                "unexpected Video Controls readout or renderer dependency: {prefix}"
            );
        }
        assert_eq!(selection, original_selection);
        assert_eq!(video_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[derive(Debug, PartialEq)]
struct VideoState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    recorder: RecorderSnapshot,
    recording_flag: bool,
    markers: serde_json::Value,
}

#[derive(Debug, PartialEq)]
struct RecorderSnapshot {
    recording: bool,
    size: Option<(u32, u32)>,
    frames_sent: u32,
    frames_duplicated: u32,
    frames_dropped: u32,
    connected: bool,
    error: Option<String>,
    folder: Option<String>,
}

fn video_state() -> VideoState {
    let cfg = config::get();
    let status = recorder::status();
    VideoState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        recorder: RecorderSnapshot {
            recording: status.recording,
            size: status.size,
            frames_sent: status.frames_sent,
            frames_duplicated: status.frames_duplicated,
            frames_dropped: status.frames_dropped,
            connected: status.connected,
            error: status.error,
            folder: status.folder,
        },
        recording_flag: recorder::is_recording(),
        markers: serde_json::to_value(markers::recent()).unwrap(),
    }
}

fn status_labels(status: &RecorderSnapshot) -> Vec<String> {
    let state = if status.recording {
        if status.connected {
            "recording"
        } else {
            "recording, waiting for the host listener"
        }
    } else {
        "stopped"
    };
    let mut labels = vec![state.to_owned()];
    if let Some((w, h)) = status.size {
        labels.push(format!(
            "Stream {w}x{h}: {} frames sent, {} repeated, {} dropped",
            status.frames_sent, status.frames_duplicated, status.frames_dropped
        ));
    }
    if let Some(folder) = &status.folder {
        labels.push(format!("Sidecar in the session folder under {folder}"));
    }
    if let Some(error) = &status.error {
        labels.push(format!("Last error: {error}"));
    }
    labels
}

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "Video Status must contain only the recorder snapshot"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
    }
}

fn video_frame(
    context: &egui::Context,
    selection: &mut NavigationState,
    width: f32,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| feature(ui, selection, None),
    );
    let mut labels = Vec::new();
    for clipped in &output.shapes {
        collect_text(&clipped.shape, &mut labels);
    }
    labels
}

fn collect_text(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => labels.push((
            text.galley.text().to_owned(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, labels);
            }
        }
        _ => {}
    }
}

fn pointer_event(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
