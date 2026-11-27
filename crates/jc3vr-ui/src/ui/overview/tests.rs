use super::*;

#[test]
fn ranking_keeps_every_row_and_ranks_failures_then_waiting_rows_stably() {
    let snapshot = fixture();
    let rows = snapshot.rows();
    let labels: Vec<_> = ranked(&rows).iter().map(|row| row.label).collect();
    assert_eq!(
        labels,
        [
            "Failed first",
            "Failed second",
            "Waiting first",
            "Waiting second",
            "Idle first",
            "Idle second",
            "Intentional off",
        ],
    );
    let attention: Vec<_> = rows
        .iter()
        .filter(|row| row.needs_attention())
        .map(|row| row.label)
        .collect();
    assert_eq!(
        attention,
        [
            "Failed first",
            "Waiting first",
            "Failed second",
            "Waiting second"
        ]
    );
}

#[test]
fn status_renders_without_a_renderer_or_live_producers() {
    let snapshot = fixture();
    for width in [420.0, 1100.0] {
        let context = egui::Context::default();
        let mut selection = NavigationState {
            feature: FeatureId::Status,
        };
        let expected = selection;
        let mut rendered = false;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1200.0),
                )),
                ..Default::default()
            },
            |ui| {
                render_snapshot(ui, &mut selection, &snapshot);
                rendered = true;
            },
        );
        assert!(rendered);
        assert!(!output.shapes.is_empty());
        assert_eq!(selection, expected);
    }
}

#[test]
fn real_destination_widget_changes_only_navigation_on_click() {
    let context = egui::Context::default();
    let mut selection = NavigationState::default();
    let target = NavigationState {
        feature: FeatureId::Video,
    };
    let mut rect = egui::Rect::NOTHING;
    let mut clicked = false;
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
                let response = destination(ui, "Open video recording", target, &mut selection);
                rect = response.rect;
                clicked |= response.clicked();
            },
        );
    }
    assert!(clicked);
    assert_eq!(selection, target);
}

fn fixture() -> Snapshot {
    let line = |health, detail: &str| Line {
        health,
        detail: detail.into(),
        note: None,
    };
    Snapshot {
        sections: vec![(
            "Features",
            vec![
                ("Idle first", line(Health::Ok, "Healthy idle")),
                ("Failed first", line(Health::Failed, "First failure")),
                ("Intentional off", line(Health::Off, "Unused in this setup")),
                (
                    "Waiting first",
                    line(Health::Waiting, "Waiting for a player"),
                ),
                ("Failed second", line(Health::Failed, "Second failure")),
                ("Idle second", line(Health::Ok, "Healthy idle")),
                (
                    "Waiting second",
                    line(Health::Waiting, "Waiting for a sample"),
                ),
            ],
        )],
        session: None,
    }
}
