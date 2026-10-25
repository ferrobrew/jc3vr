use jc3gi::environment::WeatherController;

use super::*;
use crate::{config, ui::environment::test_game};

#[test]
fn actual_environment_status_is_read_only_without_a_renderer() {
    let before = configuration_snapshot();
    let save_status = config::status();
    for (time, weather, expected) in [
        (
            None,
            None,
            vec![
                "WorldTime singleton unavailable.",
                "Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.",
            ],
        ),
        (
            Some((17.25, 2.5)),
            Some((3.25, 0.75, 0.5)),
            vec![
                "Live time of day: hour 17.25",
                "Live day-cycle speed: 2.50",
                "Live weather severity: 3.25",
                "Live rain intensity: 0.75",
                "Live snow ratio: 0.50",
            ],
        ),
        (
            Some((0.0, 0.0)),
            None,
            vec![
                "Live time of day: hour 0.00",
                "Live day-cycle speed: 0.00",
                "Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.",
            ],
        ),
        (
            None,
            Some((0.125, 0.0, 1.0)),
            vec![
                "WorldTime singleton unavailable.",
                "Live weather severity: 0.12",
                "Live rain intensity: 0.00",
                "Live snow ratio: 1.00",
            ],
        ),
    ] {
        let _fixture = test_game::install(time, weather);
        let actions = test_game::snapshot();
        let mut selection = NavigationState {
            feature: FeatureId::Environment,
            section: FeatureSection::Status,
        };
        let original_selection = selection;
        for width in [420.0, 1100.0] {
            let context = egui::Context::default();
            let labels = environment_frame(&context, &mut selection, width, Vec::new());
            assert_status_labels(&labels, &expected);
            for (_, rect) in labels {
                for pressed in [true, false] {
                    let labels = environment_frame(
                        &context,
                        &mut selection,
                        width,
                        pointer_event(rect.center(), pressed),
                    );
                    assert_status_labels(&labels, &expected);
                    assert_eq!(selection, original_selection);
                    assert_eq!(configuration_snapshot(), before);
                    assert_eq!(config::status(), save_status);
                    assert_eq!(test_game::snapshot(), actions);
                }
            }
            assert_eq!(selection, original_selection);
            assert_eq!(configuration_snapshot(), before);
            assert_eq!(config::status(), save_status);
            assert_eq!(test_game::snapshot(), actions);
        }
    }
}

#[test]
fn actual_environment_controls_retain_all_weather_event_commands_without_a_renderer() {
    let before = configuration_snapshot();
    let save_status = config::status();
    for width in [420.0, 1100.0] {
        let _fixture = test_game::install(None, None);
        let mut expected_actions = test_game::snapshot().unwrap();
        let context = egui::Context::default();
        let mut selection = NavigationState {
            feature: FeatureId::Environment,
            section: FeatureSection::Controls,
        };
        let original_selection = selection;
        let mut controls = environment_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Time of day",
            "Weather",
            "WorldTime singleton unavailable.",
            "Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.",
            "Sunny",
            "Rain",
            "Snow",
            "Restore",
            "Instant",
        ] {
            assert!(
                controls.iter().any(|(text, _)| text == expected),
                "missing Environment control or unavailable readout: {expected}"
            );
        }
        assert!(!controls.iter().any(|(text, _)| text.starts_with("Live ")));
        assert_eq!(test_game::snapshot().as_ref(), Some(&expected_actions));
        for (label, event) in [
            ("Sunny", WeatherController::EVENT_SUNNY),
            ("Rain", WeatherController::EVENT_RAIN),
            ("Snow", WeatherController::EVENT_SNOW),
            ("Restore", WeatherController::EVENT_RESTORE),
            ("Instant", WeatherController::EVENT_INSTANT),
        ] {
            let (_, rect) = controls
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("missing Environment weather command: {label}"));
            let position = rect.center();
            for pressed in [true, false] {
                controls = environment_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(position, pressed),
                );
                assert!(!controls.iter().any(|(text, _)| text.starts_with("Live ")));
                assert_eq!(selection, original_selection);
                assert_eq!(configuration_snapshot(), before);
                assert_eq!(config::status(), save_status);
            }
            expected_actions
                .events
                .push(event.to_string_lossy().into_owned());
            assert_eq!(test_game::snapshot().as_ref(), Some(&expected_actions));
        }
    }
}

#[test]
fn environment_fixture_intercepts_only_while_installed_and_restores_nested_state() {
    assert_eq!(test_game::snapshot(), None);
    assert!(!test_game::installed());
    assert!(!test_game::record_event(WeatherController::EVENT_SUNNY));
    {
        let _fixture = test_game::install(Some((8.5, 1.0)), Some((0.5, 0.25, 0.0)));
        assert!(test_game::installed());
        assert!(test_game::record_event(WeatherController::EVENT_RAIN));
        let before_nested = test_game::snapshot();
        {
            let _nested = test_game::install(None, None);
            assert!(test_game::installed());
            assert_eq!(test_game::snapshot().unwrap().time, None);
            assert_eq!(test_game::snapshot().unwrap().weather, None);
            assert!(test_game::record_event(WeatherController::EVENT_INSTANT));
        }
        assert_eq!(test_game::snapshot(), before_nested);
    }
    assert_eq!(test_game::snapshot(), None);
    assert!(!test_game::installed());
    assert!(!test_game::record_event(WeatherController::EVENT_SNOW));
}

#[derive(Debug, PartialEq)]
struct ConfigurationSnapshot {
    saved: serde_json::Value,
    time_scale: f32,
    paused: bool,
    cinematic: bool,
}

fn configuration_snapshot() -> ConfigurationSnapshot {
    let cfg = config::get();
    ConfigurationSnapshot {
        saved: serde_json::to_value(&cfg).unwrap(),
        time_scale: cfg.director.time_scale,
        paused: cfg.director.paused,
        cinematic: cfg.director.cinematic,
    }
}

fn environment_frame(
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

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[&str]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "Environment Status must contain only the clock and weather observations"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
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
