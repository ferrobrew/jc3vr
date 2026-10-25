//! Renderer-free checks of the production HUD status route and the controls availability guard.

use std::time::{Duration, Instant};

use glam::Vec2;

use super::*;
use crate::{
    config,
    hud::{
        HUD_STATE, StageSafeArea, StageSafeAreaReadout, StageSafeAreaSample,
        anchored::status as anchored_status, atlas::status as atlas_status,
        stage_safe_area_readout, test_stage_safe_area,
        world_markers::status as world_markers_status,
    },
    pause_menu::state as pause_menu_state,
};

#[test]
fn actual_hud_status_reads_published_snapshots_without_a_renderer_or_actions() {
    let _fixture = test_stage_safe_area::install(StageSafeAreaReadout::Unavailable);
    assert_eq!(route(FeatureId::Hud), FeatureRoute::Hud);
    assert!(HUD_STATE.lock().depth_status().is_none());
    assert!(world_markers_status().is_none());
    assert!(atlas_status().frame.is_none());
    let before = hud_state();
    let save_status = config::status();
    let expected = unavailable_status_labels();
    let mut selection = NavigationState {
        feature: FeatureId::Hud,
        section: FeatureSection::Status,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = hud_frame(&context, &mut selection, width, Vec::new());
        assert_status_labels(&labels, &expected);
        for (_, rect) in labels {
            for pressed in [true, false] {
                let labels = hud_frame(
                    &context,
                    &mut selection,
                    width,
                    pointer_event(rect.center(), pressed),
                );
                assert_status_labels(&labels, &expected);
                assert_eq!(selection, original_selection);
                assert_eq!(hud_state(), before);
                assert_eq!(config::status(), save_status);
            }
        }
        assert_eq!(selection, original_selection);
        assert_eq!(hud_state(), before);
        assert_eq!(config::status(), save_status);
    }
}

#[test]
fn actual_hud_status_renders_owned_safe_area_samples_without_actions() {
    let layout = StageSafeArea {
        bounds: [32.0, -12.0, 1248.0, 702.0],
        percentage: 0.05,
        stage: Vec2::new(1280.0, 720.0),
        aspect: 1.0,
    };
    let recent = Instant::now() + Duration::from_secs(3600);
    let stale = Instant::now() - Duration::from_secs(5);
    let values = [
        "Safe area (32, -12)-(1248, 702), 5% inset, stage 1280x720",
        "Visible frame (0, -280) 1280x1280 at aspect 1.00: band 280 stage px above and below \
         the stage; the safe area reaches 12 px into the top band",
    ];
    for (readout, rows) in [
        (
            StageSafeAreaReadout::Busy,
            vec!["Unavailable: the stage safe-area publication is busy; retry on the next frame."],
        ),
        (
            StageSafeAreaReadout::Captured(StageSafeAreaSample::new(layout, recent)),
            vec![
                "Capture: recent main UI movie sample.",
                values[0],
                values[1],
            ],
        ),
        (
            StageSafeAreaReadout::Captured(StageSafeAreaSample::new(layout, stale)),
            vec![
                "Capture: stale; the main UI movie has not captured recently.",
                values[0],
                values[1],
            ],
        ),
        (
            StageSafeAreaReadout::Captured(StageSafeAreaSample::new(
                StageSafeArea {
                    stage: Vec2::ZERO,
                    ..layout
                },
                recent,
            )),
            vec![
                "Capture: recent main UI movie sample.",
                "Unavailable: the captured stage safe-area layout is invalid.",
            ],
        ),
    ] {
        let _fixture = test_stage_safe_area::install(readout);
        let before = hud_state();
        let save_status = config::status();
        let mut expected = unavailable_status_labels();
        let start = expected
            .iter()
            .position(|text| text == "Stage safe area")
            .unwrap()
            + 1;
        expected.splice(start..start + 1, rows.into_iter().map(str::to_owned));
        let mut selection = NavigationState {
            feature: FeatureId::Hud,
            section: FeatureSection::Status,
        };
        let original_selection = selection;
        for width in [420.0, 1100.0] {
            let context = egui::Context::default();
            let labels = hud_frame(&context, &mut selection, width, Vec::new());
            assert_status_labels(&labels, &expected);
            for (_, rect) in labels {
                for pressed in [true, false] {
                    let labels = hud_frame(
                        &context,
                        &mut selection,
                        width,
                        pointer_event(rect.center(), pressed),
                    );
                    assert_status_labels(&labels, &expected);
                    assert_eq!(selection, original_selection);
                    assert_eq!(hud_state(), before);
                    assert_eq!(config::status(), save_status);
                    assert_eq!(stage_safe_area_readout(), readout);
                }
            }
        }
    }
}

#[test]
fn actual_hud_controls_keep_the_renderer_availability_guard() {
    let before = hud_state();
    let save_status = config::status();
    let mut selection = NavigationState {
        feature: FeatureId::Hud,
        section: FeatureSection::Controls,
    };
    let original_selection = selection;
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let labels = hud_frame(&context, &mut selection, width, Vec::new());
        assert_eq!(labels.len(), 1);
        assert_eq!(
            labels[0].0,
            "Unavailable: the DX11 renderer is not attached to this view."
        );
        for pressed in [true, false] {
            let labels = hud_frame(
                &context,
                &mut selection,
                width,
                pointer_event(labels[0].1.center(), pressed),
            );
            assert_eq!(labels.len(), 1);
            assert_eq!(
                labels[0].0,
                "Unavailable: the DX11 renderer is not attached to this view."
            );
            assert_eq!(selection, original_selection);
            assert_eq!(hud_state(), before);
            assert_eq!(config::status(), save_status);
        }
    }
}

#[derive(Debug, PartialEq)]
struct HudState {
    saved_config: serde_json::Value,
    transient_director: (f32, bool, bool),
    depth: Option<String>,
    world_markers_unavailable: bool,
    atlas: String,
    anchored: String,
    pause_menu: String,
}

fn hud_state() -> HudState {
    let cfg = config::get();
    HudState {
        saved_config: serde_json::to_value(&cfg).unwrap(),
        transient_director: (
            cfg.director.time_scale,
            cfg.director.paused,
            cfg.director.cinematic,
        ),
        depth: HUD_STATE.lock().depth_status().map(|status| {
            format!(
                "{:?} {:?} {}",
                status.stats, status.smoothed, status.near_engaged
            )
        }),
        world_markers_unavailable: world_markers_status().is_none(),
        atlas: format!("{:?}", atlas_status()),
        anchored: format!("{:?}", anchored_status()),
        pause_menu: format!("{:?}", pause_menu_state()),
    }
}

fn unavailable_status_labels() -> Vec<String> {
    let atlas = atlas_status();
    let anchored = anchored_status();
    assert_eq!(anchored, Default::default());
    vec![
        "Dynamic distance".to_owned(),
        "Live: not sampled yet".to_owned(),
        "World markers".to_owned(),
        "Not drawing (off, outside gameplay, or the panel is off).".to_owned(),
        "Stage safe area".to_owned(),
        "Unavailable: no main UI movie capture has published the stage safe area.".to_owned(),
        "HUD atlas".to_owned(),
        format!(
            "Moves {}, restores {}, move failures {}, restore failures {}, drift {}",
            atlas.moves, atlas.restores, atlas.move_failures, atlas.restore_failures, atlas.drift,
        ),
        "Not drawing (off, outside gameplay, or the stage layout is not applied).".to_owned(),
        "Weapon readout on no hand; health not sampled; heat level 0; wrist 0% (glance false, \
         attention false, heat not drawn)"
            .to_owned(),
        "Selection cross not drawn at a hand".to_owned(),
        "Tether readout not drawn".to_owned(),
        "Pause menu".to_owned(),
        format!("State: {:?}", pause_menu_state()),
    ]
}

fn assert_status_labels(labels: &[(String, egui::Rect)], expected: &[String]) {
    assert_eq!(
        labels.len(),
        expected.len(),
        "HUD Status contains only readouts"
    );
    for ((text, _), expected) in labels.iter().zip(expected) {
        assert_eq!(text, expected);
    }
    for forbidden in [
        "Redirect HUD into our texture",
        "EXPERIMENT:",
        "PARKED:",
        "VR pages in the pause menu",
        "Auto-configure split from display tree",
        "Dump display tree to log",
        "Clip path",
        "Hide",
        "Show",
        "HUD texture",
        "Split layer textures",
        "Preview size (px)",
        "Unavailable: the DX11 renderer",
    ] {
        assert!(
            labels.iter().all(|(text, _)| !text.starts_with(forbidden)),
            "unexpected HUD Status control or renderer dependency: {forbidden}"
        );
    }
}

fn hud_frame(
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
