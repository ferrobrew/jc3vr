use jc3gi::heat_manager::HeatManager;

use super::*;
use crate::{config, ui::director::test_game};

#[test]
fn actual_director_safety_status_is_read_only_without_a_renderer() {
    let before = configuration_snapshot();
    let save_status = config::status();
    for (actor, heat, expected) in [
        (
            None,
            None,
            ["Live: no player character", "Live: no heat manager"],
        ),
        (
            Some((true, false)),
            Some((5, true, HeatManager::LEVEL_CAP_NONE)),
            [
                "Live: god mode on, unlimited ammo off",
                "Live: level 5, lock on, cap none",
            ],
        ),
        (
            Some((false, true)),
            Some((2, false, 3)),
            [
                "Live: god mode off, unlimited ammo on",
                "Live: level 2, lock off, cap 3",
            ],
        ),
        (
            None,
            Some((0, false, HeatManager::LEVEL_CAP_NONE)),
            [
                "Live: no player character",
                "Live: level 0, lock off, cap none",
            ],
        ),
        (
            Some((true, true)),
            None,
            [
                "Live: god mode on, unlimited ammo on",
                "Live: no heat manager",
            ],
        ),
    ] {
        let _fixture = test_game::install(actor, heat);
        let actions = test_game::snapshot();
        let mut selection = NavigationState {
            feature: FeatureId::DirectorSafety,
            section: FeatureSection::Status,
        };
        let original_selection = selection;
        for width in [420.0, 1100.0] {
            let context = egui::Context::default();
            let labels = director_frame(&context, &mut selection, width, Vec::new());
            assert_director_status_labels(&labels, expected);
            for (_, rect) in labels {
                for pressed in [true, false] {
                    let labels = director_frame(
                        &context,
                        &mut selection,
                        width,
                        pointer_event(rect.center(), pressed),
                    );
                    assert_director_status_labels(&labels, expected);
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
fn actual_director_safety_controls_retain_settings_and_heat_commands_without_a_renderer() {
    let before = configuration_snapshot();
    for width in [420.0, 1100.0] {
        let _fixture = test_game::install(
            Some((true, false)),
            Some((2, true, HeatManager::LEVEL_CAP_NONE)),
        );
        let original_actions = test_game::snapshot();
        let context = egui::Context::default();
        let mut selection = NavigationState {
            feature: FeatureId::DirectorSafety,
            section: FeatureSection::Controls,
        };
        let original_selection = selection;
        let controls = director_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Cinematic mode (hide the HUD, the panel, and the overlay)",
            "Time scale",
            "1x",
            "1/2",
            "1/4",
            "1/10",
            "Slow-motion hotkey rate",
            "Pause the game clock",
            "God mode (re-asserted every frame)",
            "Unlimited ammo (re-asserted every frame)",
            "Lock heat (no rise, no decay)",
            "Cap heat at",
            "Set level:",
        ] {
            assert!(
                controls.iter().any(|(text, _)| text == expected),
                "missing DirectorSafety control: {expected}"
            );
        }
        assert!(!controls.iter().any(|(text, _)| text.starts_with("Live:")));
        assert_eq!(test_game::snapshot(), original_actions);

        let command_start = controls
            .iter()
            .position(|(text, _)| text == "Set level:")
            .unwrap()
            + 1;
        let commands = &controls[command_start..];
        let mut expected_actions = original_actions.unwrap();
        for level in 0..=HeatManager::MAX_LEVEL {
            let label = level.to_string();
            let (_, rect) = commands
                .iter()
                .find(|(text, _)| text == &label)
                .unwrap_or_else(|| panic!("missing DirectorSafety heat command: {level}"));
            for pressed in [true, false] {
                let controls = director_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert!(!controls.iter().any(|(text, _)| text.starts_with("Live:")));
            }
            expected_actions.heat_commands.push(level);
            assert_eq!(test_game::snapshot().as_ref(), Some(&expected_actions));
            assert_eq!(selection, original_selection);
            assert_eq!(configuration_snapshot(), before);
        }
    }
}

#[test]
fn director_fixture_intercepts_only_while_installed_and_restores_nested_state() {
    assert_eq!(test_game::snapshot(), None);
    assert_eq!(test_game::actor_live_state(), None);
    assert_eq!(test_game::heat_live_state(), None);
    assert!(!test_game::record_heat_command(1));
    {
        let _fixture = test_game::install(Some((true, false)), Some((2, false, 3)));
        assert_eq!(test_game::actor_live_state(), Some(Some((true, false))));
        assert_eq!(test_game::heat_live_state(), Some(Some((2, false, 3))));
        assert!(test_game::record_heat_command(2));
        let before_nested = test_game::snapshot();
        {
            let _nested = test_game::install(None, None);
            assert_eq!(test_game::actor_live_state(), Some(None));
            assert_eq!(test_game::heat_live_state(), Some(None));
            assert!(test_game::record_heat_command(0));
        }
        assert_eq!(test_game::snapshot(), before_nested);
    }
    assert_eq!(test_game::snapshot(), None);
    assert_eq!(test_game::actor_live_state(), None);
    assert_eq!(test_game::heat_live_state(), None);
    assert!(!test_game::record_heat_command(1));
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

fn director_frame(
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

fn assert_director_status_labels(labels: &[(String, egui::Rect)], expected: [&str; 2]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "DirectorSafety Status must contain only actor and heat-manager readouts"
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
