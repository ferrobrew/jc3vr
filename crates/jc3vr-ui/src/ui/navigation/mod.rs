//! The debug UI's navigation over the page and feature hierarchy ([`crate::ui::catalog`]): the
//! selection ([`NavigationState`]) and the page and feature selectors with the scroll boundary
//! ([`feature_body`]). Navigation never edits settings.

use egui::{ComboBox, Panel, Ui};

use crate::ui::catalog::{FEATURES, FeatureId, PageId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NavigationState {
    pub feature: FeatureId,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            feature: FeatureId::Status,
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

/// The page and feature selectors and the scroll boundary around a feature's body.
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
mod tests;
