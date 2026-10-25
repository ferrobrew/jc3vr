//! Renderer-free checks of the production teleport and spawn routes.

use super::*;
use crate::{
    config,
    director::{spawn, teleport},
    ui::director::test_game,
};

#[test]
fn actual_spawn_status_is_read_only_without_a_renderer() {
    assert_eq!(route(FeatureId::DirectorSpawn), FeatureRoute::Director);
    let before = configuration_snapshot();
    let save_status = config::status();
    let bookmarks = serde_json::to_value(teleport::bookmarks()).unwrap();
    let editors = test_game::editor_state();
    for (position, in_flight, pending, last_submission, expected) in [
        (
            None,
            false,
            false,
            None,
            [
                "Player position unavailable",
                "Teleport: idle",
                "Spawn request: idle",
                "Last spawn request: none processed",
            ],
        ),
        (
            Some([12.5, -6.0, 42.0]),
            true,
            true,
            None,
            [
                "Player at (12.5, -6.0, 42.0)",
                "Teleport: queued or in flight",
                "Spawn request: queued",
                "Last spawn request: none processed",
            ],
        ),
        (
            Some([0.0, 3.0, 7.0]),
            false,
            false,
            Some(spawn::Submission::Submitted),
            [
                "Player at (0.0, 3.0, 7.0)",
                "Teleport: idle",
                "Spawn request: idle",
                "Last spawn request: submitted; object completion is not tracked",
            ],
        ),
        (
            None,
            true,
            false,
            Some(spawn::Submission::NoTags),
            [
                "Player position unavailable",
                "Teleport: queued or in flight",
                "Spawn request: idle",
                "Last spawn request: not submitted (no tags)",
            ],
        ),
        (
            Some([2.0, 4.0, 8.0]),
            false,
            false,
            Some(spawn::Submission::TagsContainNul),
            [
                "Player at (2.0, 4.0, 8.0)",
                "Teleport: idle",
                "Spawn request: idle",
                "Last spawn request: not submitted (tags contain a NUL)",
            ],
        ),
        (
            None,
            false,
            false,
            Some(spawn::Submission::NoSpawnSystem),
            [
                "Player position unavailable",
                "Teleport: idle",
                "Spawn request: idle",
                "Last spawn request: not submitted (no spawn system)",
            ],
        ),
        (
            Some([2.0, 4.0, 8.0]),
            false,
            true,
            Some(spawn::Submission::Submitted),
            [
                "Player at (2.0, 4.0, 8.0)",
                "Teleport: idle",
                "Spawn request: queued",
                "Last spawn request: submitted; object completion is not tracked",
            ],
        ),
    ] {
        let native = test_game::TeleportState {
            position,
            in_flight,
        };
        let _native = test_game::install_teleport(native);
        let submission = spawn::Status {
            pending,
            last_submission,
        };
        let _submission = spawn::test_status::install(submission);
        let mut selection = NavigationState {
            feature: FeatureId::DirectorSpawn,
            section: FeatureSection::Status,
        };
        let original_selection = selection;
        for width in [420.0, 1100.0] {
            let context = egui::Context::default();
            let labels = spawn_frame(&context, &mut selection, width, Vec::new());
            assert_status_labels(&labels, expected, bookmarks.as_array().unwrap().len());
            for (_, rect) in labels {
                for pressed in [true, false] {
                    let labels = spawn_frame(
                        &context,
                        &mut selection,
                        width,
                        pointer_event(rect.center(), pressed),
                    );
                    assert_status_labels(&labels, expected, bookmarks.as_array().unwrap().len());
                    assert_eq!(selection, original_selection);
                    assert_eq!(configuration_snapshot(), before);
                    assert_eq!(config::status(), save_status);
                    assert_eq!(
                        serde_json::to_value(teleport::bookmarks()).unwrap(),
                        bookmarks
                    );
                    assert_eq!(test_game::editor_state(), editors);
                    assert_eq!(test_game::teleport_live_state(), Some(native));
                    assert_eq!(spawn::status(), submission);
                    assert_eq!(spawn::test_status::snapshot(), Some(submission));
                }
            }
        }
    }
}

#[test]
fn actual_spawn_controls_retain_forms_bookmarks_presets_and_picker_without_commands() {
    let _native = test_game::install_teleport(test_game::TeleportState {
        position: Some([10.0, 20.0, 30.0]),
        in_flight: false,
    });
    let submission = spawn::Status {
        pending: false,
        last_submission: Some(spawn::Submission::Submitted),
    };
    let _submission = spawn::test_status::install(submission);
    let before = configuration_snapshot();
    let save_status = config::status();
    let bookmarks = teleport::bookmarks();
    let saved_bookmarks = serde_json::to_value(&bookmarks).unwrap();
    let editors = test_game::editor_state();
    let mut selection = NavigationState {
        feature: FeatureId::DirectorSpawn,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = spawn_frame(&context, &mut selection, width, Vec::new());
        for expected in [
            "Player at (10.0, 20.0, 30.0)",
            "Mode:",
            "Auto",
            "Instant",
            "Streamed",
            "Name:",
            "Save bookmark here",
            "Coordinates:",
            "Go",
            "Tags:",
            "Spawn",
            "Distance (m)",
            "Face the player",
            "Definitions:",
            "all pools",
            "Spawns are asynchronous and skip the out-of-view checks; the object appears once it streams in.",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "missing DirectorSpawn control or instruction: {expected}"
            );
        }
        for preset in spawn::PRESETS {
            assert!(labels.iter().any(|(text, _)| text == preset.label));
        }
        if bookmarks.is_empty() {
            assert!(labels.iter().any(|(text, _)| text == "No bookmarks yet."));
        }
        for (index, bookmark) in bookmarks.iter().enumerate() {
            let expected = format!("{}. {}", index + 1, bookmark.name);
            assert!(labels.iter().any(|(text, _)| text == &expected));
        }
        for prefix in [
            "Teleport:",
            "Saved bookmarks:",
            "Spawn request:",
            "Last spawn request:",
        ] {
            assert!(labels.iter().all(|(text, _)| !text.starts_with(prefix)));
        }
        assert_eq!(selection, original_selection);
        assert_eq!(configuration_snapshot(), before);
        assert_eq!(config::status(), save_status);
        assert_eq!(
            serde_json::to_value(teleport::bookmarks()).unwrap(),
            saved_bookmarks
        );
        assert_eq!(test_game::editor_state(), editors);
        assert_eq!(spawn::status(), submission);
    }
}

#[test]
fn teleport_fixture_intercepts_only_while_installed_and_restores_nested_state() {
    assert_eq!(test_game::teleport_live_state(), None);
    let native = test_game::TeleportState {
        position: Some([1.0, 2.0, 3.0]),
        in_flight: true,
    };
    {
        let _native = test_game::install_teleport(native);
        assert_eq!(test_game::teleport_live_state(), Some(native));
        {
            let nested = test_game::TeleportState {
                position: None,
                in_flight: false,
            };
            let _nested = test_game::install_teleport(nested);
            assert_eq!(test_game::teleport_live_state(), Some(nested));
        }
        assert_eq!(test_game::teleport_live_state(), Some(native));
    }
    assert_eq!(test_game::teleport_live_state(), None);
}

fn configuration_snapshot() -> (serde_json::Value, f32, bool, bool) {
    let cfg = config::get();
    (
        serde_json::to_value(&cfg).unwrap(),
        cfg.director.time_scale,
        cfg.director.paused,
        cfg.director.cinematic,
    )
}

fn spawn_frame(
    context: &egui::Context,
    selection: &mut NavigationState,
    width: f32,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 2400.0),
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

fn assert_status_labels(
    labels: &[(String, egui::Rect)],
    expected: [&str; 4],
    bookmark_count: usize,
) {
    let count = format!("Saved bookmarks: {bookmark_count}");
    let expected = [expected[0], expected[1], &count, expected[2], expected[3]];
    assert_eq!(
        labels.len(),
        expected.len(),
        "Status must contain only readouts"
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
