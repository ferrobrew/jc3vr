//! Renderer-free checks of the production Replay status and controls routes.

use super::*;
use crate::{
    config,
    testing::replay::{self, Phase, ReplayOutcome, ReplayStatus},
};

#[test]
fn actual_replay_status_reads_the_snapshot_without_a_renderer_or_actions() {
    assert_eq!(route(FeatureId::Replay), FeatureRoute::Replay);
    let status = replay::status();
    assert_eq!(status.phase, Phase::Idle);
    let before = replay_state();
    assert!(!before.active);
    assert!(!before.driving);
    let expected = status_labels(&status);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Replay,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = replay_frame(&context, &mut selection, width, Vec::new());
        assert_status_labels(&labels, &expected);
        for (_, rect) in labels {
            for pressed in [true, false] {
                let labels = replay_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_status_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(replay_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(replay_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_replay_controls_keep_the_idle_form_scenarios_and_signals_without_activation() {
    let status = replay::status();
    assert_eq!(status.phase, Phase::Idle);
    let before = replay_state();
    assert!(!before.active);
    assert!(!before.driving);
    let readouts = status_labels(&status);
    let mut selection = NavigationState {
        feature: FeatureId::Replay,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = replay_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Recording:",
            "Last",
            "Start:",
            "at the start marker",
            "at the bookmark",
            "in place",
            "Pacing:",
            "real time",
            "one per frame",
            "Plays the recording's head and controllers in place of the headset's, with or without a \
             session. Ctrl+Numpad* replays the last recording or stops.",
            "Scenarios",
            "Folder setting:",
            "Signals",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "missing Replay control or instruction: {expected}"
            );
        }
        assert_eq!(
            labels.iter().filter(|(text, _)| text == "Replay").count(),
            2,
            "idle Replay Controls retains its header and Replay button"
        );
        for readout in &readouts {
            assert!(
                labels.iter().all(|(text, _)| text != readout),
                "unexpected Replay Controls status readout: {readout}"
            );
        }
        for forbidden in [
            "Stop",
            "Start recording",
            "Stop recording",
            "Test tools available",
        ] {
            assert!(
                labels.iter().all(|(text, _)| text != forbidden),
                "unexpected idle Replay control: {forbidden}"
            );
        }
        assert!(
            labels
                .iter()
                .all(|(text, _)| !text.starts_with("Unavailable: the DX11 renderer"))
        );
        assert_eq!(selection, original_selection);
        assert_eq!(replay_state(), before);
    }
}

#[derive(Debug, PartialEq)]
struct ReplayState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    status: String,
    active: bool,
    driving: bool,
}

fn replay_state() -> ReplayState {
    let cfg = config::get();
    ReplayState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        status: format!("{:?}", replay::status()),
        active: replay::is_active(),
        driving: replay::is_driving(),
    }
}

fn status_labels(status: &ReplayStatus) -> Vec<String> {
    let mut labels = vec![match status.phase {
        Phase::Idle => "idle".to_owned(),
        Phase::Loading => "loading".to_owned(),
        Phase::Teleporting => "teleporting to the start".to_owned(),
        Phase::Playing => format!(
            "playing frame {} of {}, {:.1} s",
            status.frame,
            status.frames,
            status.elapsed.as_secs_f32()
        ),
    }];
    labels.extend(status.warnings.iter().cloned());
    match &status.last {
        Some(ReplayOutcome::Completed { frames, duration }) => labels.push(format!(
            "Last replay: {frames} frames in {:.1} s",
            duration.as_secs_f32()
        )),
        Some(ReplayOutcome::Stopped) => labels.push("Last replay: stopped".to_owned()),
        Some(ReplayOutcome::Failed(error)) => labels.push(error.to_string()),
        None => {}
    }
    labels
}

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "Replay Status must contain only phase, warnings, and the last outcome"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
    }
}

fn replay_frame(
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
