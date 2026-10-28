//! Pointer interaction with the actual paired sliders and chord controls.

use super::*;

fn frame(
    context: &egui::Context,
    cfg: &mut config::Config,
    events: Vec<egui::Event>,
    chord: bool,
) -> Vec<egui::Rect> {
    let output = context.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            if chord {
                menu_panel(ui, cfg);
            } else {
                thresholds(ui, cfg);
            }
        },
    );
    let mut rows = Vec::new();
    for shape in output.shapes {
        if let egui::Shape::Text(text) = shape.shape
            && text
                .galley
                .text()
                .contains(if chord { "chord window" } else { "Trigger" })
        {
            rows.push(text.visual_bounding_rect());
        }
    }
    rows
}

fn click(context: &egui::Context, cfg: &mut config::Config, pos: egui::Pos2, chord: bool) {
    for pressed in [true, false] {
        frame(
            context,
            cfg,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            chord,
        );
    }
}

#[test]
fn actual_widgets_keep_pair_order_and_chord_windows_positive() {
    let context = egui::Context::default();
    let mut cfg = config::Config::new();
    cfg.controllers.input.trigger_press = PressThresholds {
        press: 0.9,
        release: 0.8,
    };
    let rows = frame(&context, &mut cfg, vec![], false);
    assert_eq!(rows.len(), 2);
    click(
        &context,
        &mut cfg,
        egui::pos2(20.0, rows[0].center().y),
        false,
    );
    assert!(cfg.controllers.input.trigger_press.press < 0.8);
    assert!(
        cfg.controllers.input.trigger_press.release <= cfg.controllers.input.trigger_press.press
    );
    click(
        &context,
        &mut cfg,
        egui::pos2(100.0, rows[1].center().y),
        false,
    );
    assert!(cfg.controllers.input.trigger_press.release > 0.5);
    assert!(
        cfg.controllers.input.trigger_press.release <= cfg.controllers.input.trigger_press.press
    );
    let rows = frame(&context, &mut cfg, vec![], true);
    assert_eq!(rows.len(), 2);
    for row in rows {
        click(&context, &mut cfg, egui::pos2(10.0, row.center().y), true);
    }
    assert!(cfg.controllers.bindings.stick_chord_window_s > 0.0);
    assert!(cfg.controllers.bindings.panel_chord_window_s > 0.0);
    config::validate::controllers(&cfg.controllers).unwrap();
}
