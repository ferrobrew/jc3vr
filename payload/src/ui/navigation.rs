//! Renderer-free navigation for the approved debug-page hierarchy.
//!
//! The caller owns the Context and supplies read snapshots. This bounded shell exposes widget
//! rectangles and commands for tests; it neither dispatches legacy page bodies nor edits settings.

#[cfg(test)]
use egui::Rect;
use egui::{ComboBox, Panel, Ui};

#[cfg(test)]
use crate::ui::capture::{OwnershipBlocker, OwnershipSnapshot, PanelMode};
use crate::ui::catalog::{FEATURES, FeatureId, PageId};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationCommand {
    AllowGameplay,
    HidePanel,
    RecallPanel,
    OpenVisualizations,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct FeatureSnapshot<'a> {
    pub feature: FeatureId,
    pub summary: &'a str,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct NavigationSnapshot<'a> {
    pub ownership: OwnershipSnapshot,
    pub features: &'a [FeatureSnapshot<'a>],
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NavigationTarget {
    Page(PageId),
    Feature(FeatureId),
    PageSelector,
    FeatureSelector,
    AllowGameplay,
    Visualizations,
    Recall,
    Hide,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct NavigationInteraction {
    pub target: NavigationTarget,
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct NavigationFrame {
    pub targets: Vec<(NavigationTarget, Rect)>,
    pub target_ids: Vec<(NavigationTarget, egui::Id)>,
    pub interactions: Vec<NavigationInteraction>,
    pub commands: Vec<NavigationCommand>,
    pub shown_summary: Option<String>,
    pub narrow: bool,
}

#[cfg(test)]
impl NavigationFrame {
    fn record_target(&mut self, target: NavigationTarget, response: &egui::Response) {
        self.targets.push((target, response.rect));
        self.target_ids.push((target, response.id));
        self.interactions.push(NavigationInteraction {
            target,
            hovered: response.hovered(),
            pressed: response.is_pointer_button_down_on(),
            clicked: response.clicked(),
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NavigationState {
    pub feature: FeatureId,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            feature: FeatureId::Session,
        }
    }
}

impl NavigationState {
    /// Recover a selection from a saved feature key. A missing or unknown key selects the default
    /// feature.
    pub(crate) fn recover(saved_feature: Option<&str>) -> Self {
        saved_feature
            .and_then(FeatureId::from_key)
            .map_or_else(Self::default, |feature| Self { feature })
    }

    pub(crate) fn select_page(&mut self, page: PageId) {
        if self.feature.definition().page != page {
            self.feature = FEATURES
                .iter()
                .find(|feature| feature.page == page)
                .expect("every page has a feature")
                .id;
        }
    }
}

/// Run inside `Context::run_ui`; no window, GPU renderer, or game singleton is needed.
#[cfg(test)]
pub(crate) fn navigation(
    ui: &mut Ui,
    state: &mut NavigationState,
    snapshot: NavigationSnapshot<'_>,
) -> NavigationFrame {
    let mut frame = NavigationFrame::default();
    if !snapshot.ownership.visible {
        return frame;
    }
    frame.narrow = ui.available_width() < 700.0;
    let interactive = snapshot.ownership.edit_capture.any();
    Panel::top("debug-navigation-header")
        .resizable(false)
        .show(ui, |ui| {
            ui.label(match snapshot.ownership.mode {
                PanelMode::Hidden => "Hidden",
                PanelMode::Editing => "Editing",
                PanelMode::ReadOnly => "Read-only overlay: hide and reopen the panel to edit.",
            });
            ui.label(match snapshot.ownership.blocker {
                None => "Gameplay input active",
                Some(OwnershipBlocker::Editing) => "Gameplay input captured by the debug UI",
                Some(OwnershipBlocker::Unfocused) => "Gameplay requested; waiting for focus",
                Some(OwnershipBlocker::TrackingRecovery) => {
                    "Gameplay requested; waiting for tracking recovery"
                }
                Some(OwnershipBlocker::AwaitingNeutral) => "Input waiting for released controls",
                Some(OwnershipBlocker::DeviceCapturePending) => {
                    "Editing waiting for game-device capture"
                }
                Some(OwnershipBlocker::GameDeviceDisabled) => {
                    "Gameplay requested; game device disabled"
                }
            });
            ui.add_enabled_ui(interactive, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut allow = snapshot.ownership.gameplay_requested;
                    let response = ui.checkbox(&mut allow, "Allow gameplay input");
                    frame.record_target(NavigationTarget::AllowGameplay, &response);
                    if response.changed() && allow {
                        frame.commands.push(NavigationCommand::AllowGameplay);
                    }
                    for (target, label, command) in [
                        (
                            NavigationTarget::Visualizations,
                            "Visualizations",
                            NavigationCommand::OpenVisualizations,
                        ),
                        (
                            NavigationTarget::Recall,
                            "Recall panel",
                            NavigationCommand::RecallPanel,
                        ),
                        (
                            NavigationTarget::Hide,
                            "Hide panel",
                            NavigationCommand::HidePanel,
                        ),
                    ] {
                        let response = ui.button(label);
                        frame.record_target(target, &response);
                        if response.clicked() {
                            frame.commands.push(command);
                        }
                    }
                });
            });
        });
    if !frame.narrow {
        Panel::left("debug-navigation-pages")
            .resizable(false)
            .exact_size(150.0)
            .show(ui, |ui| {
                ui.add_enabled_ui(interactive, |ui| {
                    for page in PageId::ALL {
                        let response = ui.selectable_label(
                            state.feature.definition().page == page,
                            page.label(),
                        );
                        frame.record_target(NavigationTarget::Page(page), &response);
                        if response.clicked() {
                            state.select_page(page);
                        }
                    }
                });
            });
    }
    ui.add_enabled_ui(interactive, |ui| {
        if frame.narrow {
            let response = ComboBox::from_id_salt("debug-page-selector")
                .width(ui.available_width().min(360.0))
                .height(600.0)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(state.feature.definition().page.label())
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    for page in PageId::ALL {
                        let response = ui.selectable_label(
                            state.feature.definition().page == page,
                            page.label(),
                        );
                        frame.record_target(NavigationTarget::Page(page), &response);
                        if response.clicked() {
                            state.select_page(page);
                        }
                    }
                });
            frame.record_target(NavigationTarget::PageSelector, &response.response);
        }
        let page = state.feature.definition().page;
        // Distinct page contents must not inherit another page's measured popup height.
        let response = ComboBox::from_id_salt(("debug-feature-selector", page))
            .width(ui.available_width().min(520.0))
            .height(160.0)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .selected_text(state.feature.definition().label)
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                for feature in FEATURES.iter().filter(|feature| feature.page == page) {
                    let response = ui.selectable_label(state.feature == feature.id, feature.label);
                    frame.record_target(NavigationTarget::Feature(feature.id), &response);
                    if response.clicked() {
                        state.feature = feature.id;
                    }
                }
            });
        frame.record_target(NavigationTarget::FeatureSelector, &response.response);
    });
    let feature = snapshot
        .features
        .iter()
        .find(|feature| feature.feature == state.feature);
    frame.shown_summary = feature.map(|feature| feature.summary.to_owned());
    ui.heading(state.feature.definition().label);
    ui.label(feature.map_or(
        "Live status unavailable: no snapshot provider.",
        |feature| feature.summary,
    ));
    frame
}

/// The production selectors and scroll boundary, shared with renderer-free dispatch tests.
pub(crate) fn feature_body(
    ui: &mut Ui,
    state: &mut NavigationState,
    mut render: impl FnMut(&mut Ui, &mut NavigationState),
) {
    let narrow = ui.available_width() < 700.0;
    if !narrow {
        Panel::left("debug-pages-v2")
            .resizable(false)
            .exact_size(150.0)
            .show(ui, |ui| {
                for page in PageId::ALL {
                    if ui
                        .selectable_label(state.feature.definition().page == page, page.label())
                        .clicked()
                    {
                        state.select_page(page);
                    }
                }
            });
        Panel::left("debug-features-v2")
            .resizable(false)
            .exact_size(200.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("debug-feature-list-v2")
                    .show(ui, |ui| {
                        let page = state.feature.definition().page;
                        for feature in FEATURES.iter().filter(|feature| feature.page == page) {
                            if ui
                                .selectable_label(state.feature == feature.id, feature.label)
                                .clicked()
                            {
                                state.feature = feature.id;
                            }
                        }
                    });
            });
    } else {
        ComboBox::from_id_salt("debug-page-v2")
            .selected_text(state.feature.definition().page.label())
            .width(ui.available_width().min(360.0))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                for page in PageId::ALL {
                    if ui
                        .selectable_label(state.feature.definition().page == page, page.label())
                        .clicked()
                    {
                        state.select_page(page);
                    }
                }
            });
        ComboBox::from_id_salt(("debug-feature-v2", state.feature.definition().page))
            .selected_text(state.feature.definition().label)
            .width(ui.available_width().min(360.0))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                let page = state.feature.definition().page;
                for feature in FEATURES.iter().filter(|feature| feature.page == page) {
                    if ui
                        .selectable_label(state.feature == feature.id, feature.label)
                        .clicked()
                    {
                        state.feature = feature.id;
                    }
                }
            });
    }
    ui.heading(format!(
        "{} > {}",
        state.feature.definition().page.label(),
        state.feature.definition().label
    ));
    egui::ScrollArea::vertical()
        .id_salt(scroll_key(*state))
        .show(ui, |ui| render(ui, state));
}

pub(crate) fn scroll_key(state: NavigationState) -> (&'static str, &'static str) {
    ("debug-feature-content-v2", state.feature.definition().key)
}

#[cfg(test)]
#[path = "navigation/tests.rs"]
mod tests;
