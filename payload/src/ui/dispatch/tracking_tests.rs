//! Renderer-free checks of the production tracking status and controls routes.

use super::*;
use crate::{
    config,
    recorder::tracking::{self, TrackingStatus},
    vr::input::semantic::history,
};

#[test]
fn actual_tracking_status_reads_the_snapshot_without_a_renderer_or_actions() {
    let _serial = history::TEST_LOCK.lock();
    assert_eq!(route(FeatureId::TrackingCapture), FeatureRoute::Tracking);
    let status = tracking::peek_status();
    assert!(!status.recording);
    let before = tracking_state();
    assert!(!before.recording);
    assert!(!before.pending_request);
    let expected = status_labels(&status);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::TrackingCapture,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = tracking_frame(&context, &mut selection, width, Vec::new());
        assert_status_labels(&labels, &expected);
        for (_, rect) in labels {
            for pressed in [true, false] {
                let labels = tracking_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_status_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(tracking_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(tracking_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_tracking_controls_keep_idle_actions_settings_and_instructions_without_activation() {
    let _serial = history::TEST_LOCK.lock();
    let status = tracking::peek_status();
    assert!(!status.recording);
    let before = tracking_state();
    assert!(!before.recording);
    assert!(!before.pending_request);
    let readouts = status_labels(&status);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::TrackingCapture,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = tracking_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Test tools available",
            "Tracking recording",
            "Start recording",
            "Record the tracked hand joints",
            "Use lossless chunked recording",
            "Records the head, both controllers, their inputs, and the semantic actions each VR \
             frame, starting from where the player stands. Ctrl+Numpad/ toggles it.",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "missing Tracking control or instruction: {expected}"
            );
        }
        for readout in &readouts {
            assert!(
                labels.iter().all(|(text, _)| text != readout),
                "unexpected Tracking Controls status readout: {readout}"
            );
        }
        for forbidden in ["Stop recording", "Replay", "Scenarios", "Signals"] {
            assert!(
                labels.iter().all(|(text, _)| text != forbidden),
                "unexpected idle Tracking control: {forbidden}"
            );
        }
        assert!(
            labels
                .iter()
                .all(|(text, _)| !text.starts_with("Unavailable: the DX11 renderer"))
        );
        assert_eq!(selection, original_selection);
        assert_eq!(tracking_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[derive(Debug, PartialEq)]
struct TrackingState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    status: String,
    recording: bool,
    pending_request: bool,
}

fn tracking_state() -> TrackingState {
    let cfg = config::get();
    TrackingState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        status: format!("{:?}", tracking::peek_status()),
        recording: tracking::is_recording(),
        pending_request: tracking::has_pending_request(),
    }
}

fn status_labels(status: &TrackingStatus) -> Vec<String> {
    let mut labels = vec![if status.recording {
        "recording".to_owned()
    } else {
        "stopped".to_owned()
    }];
    if status.recording || status.path.is_some() {
        labels.push(format!(
            "{} frames, {} dropped, {:.1} s, {} KiB",
            status.frames,
            status.dropped,
            status.duration.as_secs_f32(),
            status.bytes / 1024
        ));
    }
    if let Some(path) = &status.path {
        labels.push("File:".to_owned());
        labels.push(path.display().to_string());
    }
    if let Some(error) = &status.error {
        labels.push(error.clone());
    }
    labels
}

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "Tracking Status must contain only the recorder phase, counters, path, and error"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
    }
}

fn tracking_frame(
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
