//! Renderer-free navigation for the approved debug-page hierarchy.
//!
//! The caller owns the Context and supplies read snapshots. This bounded shell exposes widget
//! rectangles and commands for tests; it neither dispatches legacy page bodies nor edits settings.

use egui::{ComboBox, Panel, Rect, Ui};

use crate::ui::{
    capture::{OwnershipBlocker, OwnershipSnapshot, PanelMode},
    inventory::{FEATURES, FeatureId, FeatureSection, PageId},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationCommand {
    AllowGameplay,
    HidePanel,
    RecallPanel,
    OpenVisualizations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewAvailability<'a> {
    Available,
    Unavailable(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FeatureSnapshot<'a> {
    pub feature: FeatureId,
    pub summary: &'a str,
    pub visualization: ViewAvailability<'a>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NavigationSnapshot<'a> {
    pub ownership: OwnershipSnapshot,
    pub features: &'a [FeatureSnapshot<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NavigationTarget {
    Page(PageId),
    Feature(FeatureId),
    Section(FeatureSection),
    PageSelector,
    FeatureSelector,
    AllowGameplay,
    Visualizations,
    Recall,
    Hide,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NavigationInteraction {
    pub target: NavigationTarget,
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
}

#[derive(Debug, Default)]
pub(crate) struct NavigationFrame {
    pub targets: Vec<(NavigationTarget, Rect)>,
    pub target_ids: Vec<(NavigationTarget, egui::Id)>,
    pub interactions: Vec<NavigationInteraction>,
    pub commands: Vec<NavigationCommand>,
    pub shown_summary: Option<String>,
    pub narrow: bool,
}

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
    pub section: FeatureSection,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            feature: FeatureId::Session,
            section: FeatureSection::Controls,
        }
    }
}

impl NavigationState {
    /// This recovers selection only. The existing dock JSON remains an adapter responsibility.
    pub(crate) fn recover(saved_feature_or_legacy_tab: Option<&str>) -> Self {
        let feature = saved_feature_or_legacy_tab
            .and_then(|key| {
                FeatureId::from_key(key).or(match key {
                    "Performance" => Some(FeatureId::Performance),
                    "Render" => Some(FeatureId::Stereo),
                    "Previews" => Some(FeatureId::Previews),
                    "Diagnostics" => Some(FeatureId::RenderDiagnostics),
                    "Status" => Some(FeatureId::AllStatus),
                    "Camera" => Some(FeatureId::HeadView),
                    "Game" => Some(FeatureId::Locomotion),
                    "Hud" => Some(FeatureId::Hud),
                    "Environment" => Some(FeatureId::Environment),
                    "Vr" => Some(FeatureId::Runtime),
                    "Director" => Some(FeatureId::DirectorSafety),
                    "Controllers" => Some(FeatureId::Devices),
                    "Testing" => Some(FeatureId::TrackingCapture),
                    _ => None,
                })
            })
            .unwrap_or(FeatureId::Session);
        Self {
            feature,
            section: FeatureSection::Controls,
        }
    }

    pub(crate) fn select_page(&mut self, page: PageId) {
        if self.feature.definition().page != page {
            self.feature = FEATURES
                .iter()
                .find(|feature| feature.page == page)
                .expect("every page has a feature")
                .id;
            self.section = FeatureSection::Controls;
        }
    }
}

/// Run inside `Context::run_ui`; no window, GPU renderer, or game singleton is needed.
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
        ui.horizontal_wrapped(|ui| {
            for section in FeatureSection::ALL {
                let response = ui.selectable_value(&mut state.section, section, section.label());
                frame.record_target(NavigationTarget::Section(section), &response);
            }
        });
    });
    let feature = snapshot
        .features
        .iter()
        .find(|feature| feature.feature == state.feature);
    frame.shown_summary = feature.map(|feature| feature.summary.to_owned());
    ui.heading(state.feature.definition().label);
    match state.section {
        FeatureSection::Controls => {
            ui.label("Existing control editors are not connected to this navigation seam.");
            ui.collapsing("Advanced tuning", |ui| {
                ui.label("No editor is connected.");
            });
            ui.collapsing("Diagnostics", |ui| {
                ui.label("No diagnostic provider is connected.");
            });
        }
        FeatureSection::Status => {
            ui.label(feature.map_or(
                "Live status unavailable: no snapshot provider.",
                |feature| feature.summary,
            ));
        }
        FeatureSection::Visualizations => {
            ui.label(match feature.map(|feature| feature.visualization) {
                Some(ViewAvailability::Available) => {
                    "View available through the visualization adapter."
                }
                Some(ViewAvailability::Unavailable(reason)) => reason,
                None => "Visualization unavailable: no snapshot provider.",
            });
        }
    }
    frame
}

#[cfg(test)]
#[path = "navigation/tests.rs"]
mod tests;
