use std::collections::HashSet;

use super::*;
use crate::ui::{
    catalog::{FEATURES, PageId},
    edit_boundary, feature_dock,
    layout::View,
    navigation::{feature_body, scroll_key},
};

#[test]
fn production_navigation_dispatches_every_catalog_feature_with_independent_scroll_keys() {
    let context = egui::Context::default();
    let mut keys = HashSet::new();
    for width in [420.0, 1100.0] {
        for feature in FEATURES {
            for section in FeatureSection::ALL {
                let mut selection = NavigationState {
                    feature: feature.id,
                    section,
                };
                let expected = route(feature.id);
                let mut dispatched = Vec::new();
                let _ = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        feature_body(ui, &mut selection, |ui, selected| {
                            dispatched.push(route(selected.feature));
                            ui.label(selected.feature.definition().key);
                        });
                    },
                );
                assert!(!dispatched.is_empty());
                assert!(dispatched.iter().all(|actual| *actual == expected));
                if width == 420.0 {
                    assert!(keys.insert(scroll_key(selection)));
                }
            }
        }
    }
    assert_eq!(keys.len(), FEATURES.len() * FeatureSection::ALL.len());
    assert_eq!(route(FeatureId::DirectorSafety), FeatureRoute::Director);
    assert_eq!(route(FeatureId::DirectorSpawn), FeatureRoute::Director);
    assert_eq!(FeatureId::Environment.definition().page, PageId::Capture);
}

#[test]
fn production_read_only_boundary_reads_but_does_not_edit() {
    let context = egui::Context::default();
    let mut value = false;
    let mut rect = egui::Rect::NOTHING;
    let mut reads = 0;
    for pressed in [None, Some(true), Some(false)] {
        let events = pressed.map_or_else(Vec::new, |pressed| {
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        });
        let _ = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                edit_boundary(ui, false, |ui| {
                    reads += 1;
                    let response = ui.checkbox(&mut value, "Existing feature switch");
                    rect = response.rect;
                    assert!(!response.enabled());
                });
            },
        );
    }
    assert!(reads >= 3);
    assert!(!value);
}

struct DockHarness {
    context: egui::Context,
    dock: egui_dock::DockState<View>,
    selection: NavigationState,
    values: [bool; 2],
    rects: [egui::Rect; 2],
    reads: [usize; 2],
    time: f64,
}

#[derive(Debug, PartialEq)]
struct FrameMetadata {
    time: f64,
    viewport: egui::Rect,
    content: egui::Rect,
    scale: f32,
    texture_limit: usize,
    focused: bool,
    deltas: [f32; 3],
    raw_metadata: String,
}

fn frame_metadata(context: &egui::Context) -> FrameMetadata {
    context.input(|input| {
        let mut raw = input.raw.clone();
        raw.events.clear();
        raw.modifiers = Default::default();
        raw.hovered_files.clear();
        raw.dropped_files.clear();
        FrameMetadata {
            time: input.time,
            viewport: input.viewport_rect(),
            content: input.content_rect(),
            scale: input.pixels_per_point,
            texture_limit: input.max_texture_side,
            focused: input.focused,
            deltas: [input.unstable_dt, input.predicted_dt, input.stable_dt],
            raw_metadata: format!("{raw:?}"),
        }
    })
}

fn assert_neutral_interactions(context: &egui::Context) {
    context.input(|input| {
        assert!(input.events.is_empty());
        assert!(input.raw.events.is_empty());
        assert!(input.keys_down.is_empty());
        assert_eq!(input.modifiers, egui::Modifiers::default());
        assert!(!input.pointer.any_down());
        assert!(!input.pointer.any_pressed());
        assert!(!input.pointer.any_released());
        assert!(!input.any_touches());
        assert_eq!(input.smooth_scroll_delta, egui::Vec2::ZERO);
        assert_eq!(input.zoom_delta(), 1.0);
        assert_eq!(input.rotation_delta(), 0.0);
    });
}

impl DockHarness {
    fn new() -> Self {
        let mut dock = egui_dock::DockState::new(vec![View::Workspace]);
        let surface = dock.add_window(vec![View::Feature {
            key: FeatureId::Performance.definition().key.to_owned(),
            section: FeatureSection::Status,
        }]);
        let window = dock.get_window_state_mut(surface).unwrap();
        window.set_position(egui::pos2(950.0, 80.0));
        window.set_size(egui::vec2(400.0, 300.0));
        Self {
            context: egui::Context::default(),
            dock,
            selection: NavigationState::default(),
            values: [false; 2],
            rects: [egui::Rect::NOTHING; 2],
            reads: [0; 2],
            time: 40.0,
        }
    }

    fn frame(&mut self, editing: bool, events: Vec<egui::Event>) {
        self.time += 0.1;
        let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(1400.0, 900.0));
        let mut raw = egui::RawInput {
            screen_rect: Some(viewport),
            time: Some(self.time),
            focused: true,
            predicted_dt: 0.025,
            max_texture_side: Some(8192),
            events,
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.5);
        let output = self.context.run_ui(raw, |ui| {
            let before = frame_metadata(ui.ctx());
            assert_eq!(before.time, self.time);
            assert_eq!(before.viewport, viewport);
            assert_eq!(before.scale, 1.5);
            assert_eq!(before.texture_limit, 8192);
            assert!(before.focused);
            ui.set_max_width(900.0);
            feature_dock(
                ui,
                &mut self.dock,
                &mut self.selection,
                editing,
                |ui, selection| {
                    assert_eq!(frame_metadata(ui.ctx()), before);
                    let index = usize::from(selection.feature == FeatureId::Performance);
                    self.reads[index] += 1;
                    if !editing {
                        let disabled_alpha = ui.visuals().disabled_alpha;
                        assert!(disabled_alpha > 0.0);
                        assert!(
                            ui.painter().opacity() / disabled_alpha > 0.9,
                            "read-only body has unexpected window transparency"
                        );
                        assert_neutral_interactions(ui.ctx());
                    }
                    ui.label("Live snapshot: available");
                    let response = ui.checkbox(&mut self.values[index], "Feature switch");
                    self.rects[index] = response.rect;
                    assert_eq!(response.enabled(), editing);
                },
            );
            assert_eq!(frame_metadata(ui.ctx()), before);
            if !editing {
                assert_neutral_interactions(ui.ctx());
            }
        });
        assert_eq!(output.pixels_per_point, 1.5);
    }

    fn gesture(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.frame(false, vec![egui::Event::PointerMoved(from)]);
        self.frame(
            false,
            vec![egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        self.frame(false, vec![egui::Event::PointerMoved(to)]);
        self.frame(
            false,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
    }
}

#[test]
fn production_dock_read_only_disables_main_and_detached_controls_and_layout_gestures() {
    let mut harness = DockHarness::new();
    for _ in 0..5 {
        harness.frame(true, Vec::new());
    }
    assert!(harness.reads.iter().all(|reads| *reads > 0));
    assert!(harness.rects.iter().all(|rect| rect.is_positive()));
    let held = harness.rects[1].center();
    harness.frame(
        true,
        vec![
            egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(held),
            egui::Event::PointerButton {
                pos: held,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    harness.context.input(|input| {
        assert!(input.key_down(egui::Key::Space));
        assert!(input.pointer.primary_down());
    });
    let saved = serde_json::to_value(&harness.dock).unwrap();
    let selection = harness.selection;
    let reads = harness.reads;
    harness
        .context
        .set_dragged_id(egui::Id::new("held-window-drag"));
    for rect in harness.rects {
        harness.gesture(rect.center(), rect.center());
    }
    harness.gesture(egui::pos2(50.0, 12.0), egui::pos2(500.0, 300.0));
    harness.gesture(egui::pos2(1050.0, 90.0), egui::pos2(1150.0, 220.0));
    harness.gesture(egui::pos2(1348.0, 378.0), egui::pos2(1370.0, 500.0));
    harness.gesture(egui::pos2(1332.0, 90.0), egui::pos2(1332.0, 90.0));
    assert_eq!(harness.values, [false; 2]);
    assert_eq!(harness.selection, selection);
    assert_eq!(serde_json::to_value(&harness.dock).unwrap(), saved);
    assert!(harness.context.dragged_id().is_none());
    assert!(
        harness
            .reads
            .iter()
            .zip(reads)
            .all(|(after, before)| *after > before)
    );
    harness.frame(true, Vec::new());
    assert_neutral_interactions(&harness.context);
    assert_eq!(harness.values, [false; 2]);
    assert!(harness.context.dragged_id().is_none());
}

#[test]
fn actual_preview_route_reports_missing_renderer_without_a_device() {
    let context = egui::Context::default();
    let mut selection = NavigationState {
        feature: FeatureId::Previews,
        section: FeatureSection::Visualizations,
    };
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        feature(ui, &mut selection, None)
    });
    assert_eq!(route(selection.feature), FeatureRoute::Previews);
}
