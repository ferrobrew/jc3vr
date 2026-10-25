use super::*;
use crate::ui::capture::{CapturePolicy, InputObservation, PanelRequest};

struct Harness {
    context: egui::Context,
    state: NavigationState,
    ownership: OwnershipSnapshot,
    time: f64,
    wheel_up: usize,
    wheel_down: usize,
}

impl Harness {
    fn new() -> Self {
        let mut policy = CapturePolicy::default();
        let input = InputObservation {
            focused: true,
            tracking_ready: true,
            controls_neutral: true,
            game_device_enabled: true,
        };
        policy.update(Some(PanelRequest::Open), input);
        let ownership = policy
            .update(
                None,
                InputObservation {
                    game_device_enabled: false,
                    ..input
                },
            )
            .snapshot;
        Self {
            context: egui::Context::default(),
            state: NavigationState::default(),
            ownership,
            time: 0.0,
            wheel_up: 0,
            wheel_down: 0,
        }
    }

    fn frame(&mut self, width: f32, events: Vec<egui::Event>) -> NavigationFrame {
        self.time += 1.0 / 60.0;
        let features = [FeatureSnapshot {
            feature: self.state.feature,
            summary: "Synthetic snapshot: ready",
            visualization: ViewAvailability::Unavailable("GPU preview unavailable in pure tests."),
        }];
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            time: Some(self.time),
            focused: true,
            events,
            ..Default::default()
        };
        self.ownership.filter_egui_input(&mut input);
        let mut frame = NavigationFrame::default();
        let mut commands = Vec::new();
        let mut interactions = Vec::new();
        let _output = self.context.run_ui(input, |ui| {
            frame = navigation(
                ui,
                &mut self.state,
                NavigationSnapshot {
                    ownership: self.ownership,
                    features: &features,
                },
            );
            commands.append(&mut frame.commands);
            interactions.append(&mut frame.interactions);
        });
        frame.commands = commands;
        frame.interactions = interactions;
        frame
    }

    fn settled_target(&mut self, width: f32, target_id: NavigationTarget) -> (egui::Id, Rect) {
        let mut previous = None;
        let mut observed = Vec::new();
        for _ in 0..256 {
            let frame = self.frame(width, Vec::new());
            let id = frame
                .target_ids
                .iter()
                .find(|(target, _)| *target == target_id)
                .expect("rendered target ID")
                .1;
            let response = self
                .context
                .read_response(id)
                .expect("rendered target response");
            let rects = (id, response.rect, response.interact_rect);
            observed.push(rects);
            if previous == Some(rects) {
                let is_feature = matches!(target_id, NavigationTarget::Feature(_));
                if response.interact_rect.is_positive()
                    && (!is_feature || response.interact_rect.contains_rect(response.rect))
                {
                    return (id, response.interact_rect);
                }
                if is_feature {
                    self.scroll_feature_into_view(width, &frame, &response);
                }
            }
            previous = Some(rects);
        }
        panic!(
            "{target_id:?} has no stable visible hit rectangle after wheel access: {observed:?}"
        );
    }

    fn scroll_feature_into_view(
        &mut self,
        width: f32,
        frame: &NavigationFrame,
        target: &egui::Response,
    ) {
        let visible = frame
            .target_ids
            .iter()
            .filter_map(|(target_id, id)| {
                if matches!(target_id, NavigationTarget::Feature(_)) {
                    self.context.read_response(*id)
                } else {
                    None
                }
            })
            .find(|response| {
                response.interact_rect.is_positive()
                    && response.interact_rect.contains_rect(response.rect)
            })
            .expect("feature popup has a fully visible row to receive the wheel");
        let position = visible.interact_rect.center();
        let delta = if target.rect.min.y < target.interact_rect.min.y {
            self.wheel_up += 1;
            6.0
        } else {
            self.wheel_down += 1;
            -6.0
        };
        // Small point events avoid residual wheel smoothing during the later click.
        let before = self.state;
        self.frame(width, vec![egui::Event::PointerMoved(position)]);
        self.frame(
            width,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert_eq!(self.state, before, "scrolling must not select a feature");
    }

    fn click(&mut self, width: f32, target_id: NavigationTarget) -> NavigationFrame {
        assert!(
            self.ownership.edit_capture.any(),
            "enabled click requires edit ownership"
        );
        // egui hit-tests the previous pass; panels and popup areas first measure their contents.
        let (id, hit_rect) = self.settled_target(width, target_id);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0));
        let position = hit_rect.center();
        assert!(
            viewport.contains_rect(hit_rect),
            "{target_id:?} hit region leaves the viewport: {hit_rect:?}"
        );
        let approach = self.frame(width, vec![egui::Event::PointerMoved(position)]);
        let hovered = self
            .context
            .read_response(id)
            .expect("hovered target response");
        assert!(
            hovered.enabled()
                && approach
                    .interactions
                    .iter()
                    .any(|interaction| interaction.target == target_id && interaction.hovered),
            "{target_id:?} is not hittable at {position:?}: {hovered:?}"
        );
        assert_eq!(
            hovered.interact_rect, hit_rect,
            "{target_id:?} moved during pointer approach"
        );
        if matches!(target_id, NavigationTarget::Feature(_)) {
            assert!(
                viewport.contains_rect(hovered.rect),
                "{target_id:?} popup row leaves the viewport: {:?}",
                hovered.rect
            );
            assert!(
                hovered.interact_rect.contains_rect(hovered.rect),
                "{target_id:?} popup row remains clipped: {hovered:?}"
            );
        }
        let press = self.frame(
            width,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }],
        );
        let pressed = self
            .context
            .read_response(id)
            .expect("pressed target response");
        assert!(
            press
                .interactions
                .iter()
                .any(|interaction| interaction.target == target_id && interaction.pressed),
            "{target_id:?} did not receive the press: {pressed:?}"
        );
        let released = self.frame(
            width,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        let response = self
            .context
            .read_response(id)
            .expect("released target response");
        assert!(
            released
                .interactions
                .iter()
                .any(|interaction| interaction.target == target_id && interaction.clicked),
            "{target_id:?} did not receive the click: {response:?}"
        );
        released
    }

    fn rejected_pointer_click(&mut self, width: f32, rect: Rect) -> NavigationFrame {
        let position = rect.center();
        self.frame(width, vec![egui::Event::PointerMoved(position)]);
        self.frame(
            width,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }],
        );
        self.frame(
            width,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        )
    }
}

fn target(frame: &NavigationFrame, target: NavigationTarget) -> Rect {
    frame
        .targets
        .iter()
        .find(|(id, _)| *id == target)
        .expect("navigation target")
        .1
}

#[test]
fn navigation_feature_parity() {
    let mut harness = Harness::new();
    for page in PageId::ALL {
        harness.click(1100.0, NavigationTarget::Page(page));
        assert_eq!(harness.state.feature.definition().page, page);
        for feature in FEATURES.iter().filter(|feature| feature.page == page) {
            harness.click(1100.0, NavigationTarget::FeatureSelector);
            harness.click(1100.0, NavigationTarget::Feature(feature.id));
            assert_eq!(harness.state.feature, feature.id);
            let frame = harness.frame(1100.0, Vec::new());
            assert_eq!(
                frame.shown_summary.as_deref(),
                Some("Synthetic snapshot: ready")
            );
        }
        for feature in FEATURES.iter().rev().filter(|feature| feature.page == page) {
            harness.click(1100.0, NavigationTarget::FeatureSelector);
            harness.click(1100.0, NavigationTarget::Feature(feature.id));
            assert_eq!(harness.state.feature, feature.id);
        }
    }
    assert!(
        harness.wheel_down > 0,
        "feature walk must reach rows below the menu viewport"
    );
    assert!(
        harness.wheel_up > 0,
        "reverse feature walk must reach rows above the menu viewport"
    );
}

#[test]
fn narrow_layout_keeps_pages_features_sections_and_recovery_visible() {
    let mut harness = Harness::new();
    for page in PageId::ALL {
        let frame = harness.frame(420.0, Vec::new());
        assert!(frame.narrow);
        for id in [
            NavigationTarget::AllowGameplay,
            NavigationTarget::Visualizations,
            NavigationTarget::Recall,
            NavigationTarget::Hide,
            NavigationTarget::PageSelector,
            NavigationTarget::FeatureSelector,
        ] {
            let rect = target(&frame, id);
            assert!(rect.min.x >= 0.0 && rect.max.x <= 420.0, "{id:?}: {rect:?}");
            assert!(rect.min.y >= 0.0 && rect.max.y < 800.0, "{id:?}: {rect:?}");
        }
        harness.click(420.0, NavigationTarget::PageSelector);
        harness.click(420.0, NavigationTarget::Page(page));
        assert_eq!(harness.state.feature.definition().page, page);
        for feature in FEATURES.iter().filter(|feature| feature.page == page) {
            harness.click(420.0, NavigationTarget::FeatureSelector);
            harness.click(420.0, NavigationTarget::Feature(feature.id));
            assert_eq!(
                harness.state.feature, feature.id,
                "{} at narrow width",
                feature.key
            );
            let frame = harness.frame(420.0, Vec::new());
            assert_eq!(
                frame.shown_summary.as_deref(),
                Some("Synthetic snapshot: ready")
            );
        }
        for feature in FEATURES.iter().rev().filter(|feature| feature.page == page) {
            harness.click(420.0, NavigationTarget::FeatureSelector);
            harness.click(420.0, NavigationTarget::Feature(feature.id));
            assert_eq!(
                harness.state.feature, feature.id,
                "{} at narrow width",
                feature.key
            );
        }
        for section in FeatureSection::ALL {
            harness.click(420.0, NavigationTarget::Section(section));
            assert_eq!(harness.state.section, section);
        }
    }
    assert!(
        harness.wheel_down > 0,
        "narrow feature walk must reach rows below the menu viewport"
    );
    assert!(
        harness.wheel_up > 0,
        "narrow reverse walk must reach rows above the menu viewport"
    );
}

#[test]
fn old_layout_selection_recovers_without_orphaning_a_feature() {
    for legacy in [
        "Performance",
        "Render",
        "Previews",
        "Diagnostics",
        "Status",
        "Camera",
        "Game",
        "Hud",
        "Environment",
        "Vr",
        "Director",
        "Controllers",
        "Testing",
    ] {
        let recovered = NavigationState::recover(Some(legacy));
        assert!(
            FEATURES
                .iter()
                .any(|feature| feature.id == recovered.feature)
        );
    }
    assert_eq!(
        NavigationState::recover(Some("removed-feature")),
        NavigationState::default()
    );
    assert_eq!(NavigationState::recover(None), NavigationState::default());
    for feature in FEATURES {
        assert_eq!(
            NavigationState::recover(Some(feature.key)).feature,
            feature.id
        );
    }
}

#[test]
fn read_only_navigation_ignores_pointer_and_still_reads_snapshots() {
    let mut harness = Harness::new();
    harness.state.section = FeatureSection::Status;
    harness.ownership.mode = PanelMode::ReadOnly;
    harness.ownership.edit_capture = Default::default();
    let before = harness.state;
    let frame = harness.frame(1100.0, Vec::new());
    let clicked = harness.rejected_pointer_click(
        1100.0,
        target(&frame, NavigationTarget::Page(PageId::Weapons)),
    );
    assert_eq!(harness.state, before);
    assert!(clicked.commands.is_empty());
    assert_eq!(
        clicked.shown_summary.as_deref(),
        Some("Synthetic snapshot: ready")
    );
}

#[test]
fn header_returns_commands_without_mutating_the_input_snapshot() {
    let mut harness = Harness::new();
    let clicked = harness.click(1100.0, NavigationTarget::AllowGameplay);
    assert_eq!(clicked.commands, vec![NavigationCommand::AllowGameplay]);
    assert_eq!(harness.ownership.mode, PanelMode::Editing);
}

#[test]
fn unavailable_and_available_visualizations_need_no_renderer() {
    let mut harness = Harness::new();
    harness.state.section = FeatureSection::Visualizations;
    harness.frame(1100.0, Vec::new());
    let features = [FeatureSnapshot {
        feature: harness.state.feature,
        summary: "Synthetic snapshot: view adapter present",
        visualization: ViewAvailability::Available,
    }];
    let _output = harness.context.run_ui(egui::RawInput::default(), |ui| {
        navigation(
            ui,
            &mut harness.state,
            NavigationSnapshot {
                ownership: harness.ownership,
                features: &features,
            },
        );
    });
}
