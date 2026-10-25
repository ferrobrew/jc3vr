//! Renderer-free checks of the production keyboard status and controls routes.

use std::path::PathBuf;

use super::*;
use crate::{
    config,
    director::{
        keymap::{self, Binding, Keymap},
        markers,
    },
    recorder::{self, tracking},
    testing::{replay, scenario},
    vr::input::semantic::history,
};

#[test]
fn actual_keyboard_status_reads_effective_bindings_without_a_renderer_or_actions() {
    let _serial = history::TEST_LOCK.lock();
    assert_eq!(route(FeatureId::Keyboard), FeatureRoute::Director);
    let before = keyboard_state();
    assert!(!before.recording);
    assert!(!before.tracking_recording);
    assert!(!before.tracking_pending_request);
    assert!(!before.replay_active);
    assert!(!before.replay_driving);
    assert!(!before.scenario_running);
    let bindings = expected_bindings();
    assert_eq!(before.effective_bindings, bindings);
    assert!(!bindings.is_empty());
    let expected = binding_labels(&bindings);
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Keyboard,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = keyboard_frame(&context, &mut selection, width, Vec::new());
        assert_binding_labels(&labels, &expected);
        for (_, rect) in labels {
            for pressed in [true, false] {
                let labels = keyboard_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_binding_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(keyboard_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(keyboard_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_keyboard_controls_keep_the_binding_commands_and_reset_without_activation() {
    let _serial = history::TEST_LOCK.lock();
    let before = keyboard_state();
    assert!(!before.recording);
    assert!(!before.tracking_recording);
    assert!(!before.tracking_pending_request);
    assert!(!before.replay_active);
    assert!(!before.replay_driving);
    assert!(!before.scenario_running);
    let mut expected = binding_labels(&expected_bindings());
    expected.push(
        "Edit director.keymap in the settings file to rebind (virtual-key codes).".to_owned(),
    );
    expected.push("Reset to defaults".to_owned());
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Keyboard,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = keyboard_frame(&context, &mut selection, width, Vec::new());
        assert_eq!(
            labels.iter().map(|(text, _)| text).collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>(),
            "Keyboard Controls retains each chord, action button label, rebinding instruction, and reset button label"
        );
        assert_eq!(selection, original_selection);
        assert_eq!(keyboard_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[derive(Debug, PartialEq)]
struct KeyboardState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    saved_bindings: Vec<Binding>,
    effective_bindings: Vec<Binding>,
    markers: serde_json::Value,
    recorder_status: String,
    recording: bool,
    tracking_status: String,
    tracking_recording: bool,
    tracking_pending_request: bool,
    replay_status: String,
    replay_active: bool,
    replay_driving: bool,
    scenario_status: String,
    scenario_running: bool,
    scenario_path: Option<PathBuf>,
}

fn keyboard_state() -> KeyboardState {
    let cfg = config::get();
    let map = keymap::current();
    KeyboardState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        saved_bindings: map.bindings.to_vec(),
        effective_bindings: map.effective().copied().collect(),
        markers: serde_json::to_value(markers::recent()).unwrap(),
        recorder_status: format!("{:?}", recorder::status()),
        recording: recorder::is_recording(),
        tracking_status: format!("{:?}", tracking::peek_status()),
        tracking_recording: tracking::is_recording(),
        tracking_pending_request: tracking::has_pending_request(),
        replay_status: format!("{:?}", replay::status()),
        replay_active: replay::is_active(),
        replay_driving: replay::is_driving(),
        scenario_status: format!("{:?}", scenario::status()),
        scenario_running: scenario::is_running(),
        scenario_path: scenario::last_path(),
    }
}

fn expected_bindings() -> Vec<Binding> {
    let cfg = config::get();
    let saved = cfg.director.keymap.bindings.as_ref();
    let defaults = Keymap::new();
    let mut bindings = saved.to_vec();
    for default in defaults.bindings.iter() {
        if saved
            .iter()
            .all(|binding| binding.action != default.action && binding.chord != default.chord)
        {
            bindings.push(*default);
        }
    }
    bindings
}

fn binding_labels(bindings: &[Binding]) -> Vec<String> {
    bindings
        .iter()
        .flat_map(|binding| [binding.chord.describe(), binding.action.label().to_owned()])
        .collect()
}

fn assert_binding_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.iter().map(|(text, _)| text).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>(),
        "Keyboard Status contains only the effective chord and action labels"
    );
}

fn keyboard_frame(
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
