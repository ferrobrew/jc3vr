//! Renderer-free checks of the production HUD status route and the controls availability guard.

use super::*;
use crate::{
    config,
    hud::{
        HUD_STATE, anchored::status as anchored_status, atlas::status as atlas_status,
        world_markers::status as world_markers_status,
    },
    pause_menu::state as pause_menu_state,
};

#[test]
fn actual_hud_status_reads_published_snapshots_without_a_renderer_or_actions() {
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
        "Safe area (",
        "Visible frame (",
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
