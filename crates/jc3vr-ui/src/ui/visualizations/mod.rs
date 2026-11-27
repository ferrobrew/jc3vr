//! The visualization switches and the global drawer. Each switch's line reads what the overlay
//! collected from the switch's source at its last snapshot, the producers' own published output,
//! rather than restating the gates that decide whether a producer draws.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::Ui;
use jc3vr_gameplay::controllers::config::{CONTROLLERS_CONFIG, ControllersConfig};
use jc3vr_render::controllers::overlay;
use jc3vr_view::controllers::overlay::{primitive::Source, store, store::SourceMask};

use crate::ui::catalog::FeatureId;

/// A snapshot taken before any configuration edit guard is acquired.
pub(crate) struct Snapshot {
    config: ControllersConfig,
    /// Each source's primitives collected after suppression and the cap, at the overlay's last
    /// snapshot.
    collected: [usize; Source::COUNT],
}

impl Snapshot {
    /// How many switches are on.
    pub(crate) fn requested(&self) -> usize {
        ENTRIES
            .iter()
            .filter(|entry| entry.id.requested(&self.config))
            .count()
    }

    fn collected(&self, id: ViewId) -> usize {
        self.collected[id.source().index()]
    }
}

pub(crate) fn snapshot() -> Snapshot {
    let config = CONTROLLERS_CONFIG.lock().clone();
    Snapshot {
        config,
        collected: overlay::pass::status().visible_counts,
    }
}

/// Render the drawer's switches and the temporary hide, without navigation changes. The caller
/// supplies the snapshot before entering any configuration edit boundary.
pub(crate) fn controls(ui: &mut Ui, snapshot: &Snapshot) {
    let id = ui.id().with("visualization-filter");
    let mut filter = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filter names or groups"));
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, filter.clone()));
    let mut edits = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("visualization-switches")
        .show(ui, |ui| {
            let entries = matching(&snapshot.config, &filter);
            if entries.is_empty() {
                ui.label("No matching visualization switches.");
            }
            for entry in entries {
                ui.push_id(entry.id, |ui| {
                    let mut requested = entry.id.requested(&snapshot.config);
                    if ui.checkbox(&mut requested, entry.label).changed() {
                        edits.push((entry.id, requested));
                    }
                    if requested {
                        ui.small(format!(
                            "{} primitives at the last overlay snapshot",
                            snapshot.collected(entry.id)
                        ));
                    }
                });
            }
            ui.separator();
            temporary_controls(ui);
        });
    if !edits.is_empty() {
        apply_edits(&mut CONTROLLERS_CONFIG.edit(), &edits);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ViewId {
    BodyMirror,
    BodyRig,
    BodyEstimate,
    GrabView,
    ColliderView,
    DockedGuns,
}

impl ViewId {
    fn source(self) -> Source {
        match self {
            Self::BodyMirror => Source::BodyMirror,
            Self::BodyRig | Self::BodyEstimate => Source::BodyRig,
            Self::GrabView => Source::GrabView,
            Self::ColliderView => Source::ColliderView,
            Self::DockedGuns => Source::DockedGuns,
        }
    }

    fn requested(self, config: &ControllersConfig) -> bool {
        let overlay = &config.overlay;
        match self {
            Self::BodyMirror => overlay.body_mirror,
            Self::BodyRig => overlay.body_rig,
            Self::BodyEstimate => overlay.body_estimate,
            Self::GrabView => config.physical_hands.grab_view.enabled,
            Self::ColliderView => config.physical_hands.collider_view.enabled,
            Self::DockedGuns => overlay.docked_guns,
        }
    }

    fn set(self, config: &mut ControllersConfig, requested: bool) {
        let overlay = &mut config.overlay;
        match self {
            Self::BodyMirror => overlay.body_mirror = requested,
            Self::BodyRig => overlay.body_rig = requested,
            Self::BodyEstimate => overlay.body_estimate = requested,
            Self::GrabView => config.physical_hands.grab_view.enabled = requested,
            Self::ColliderView => config.physical_hands.collider_view.enabled = requested,
            Self::DockedGuns => overlay.docked_guns = requested,
        }
    }
}

struct Entry {
    id: ViewId,
    label: &'static str,
    feature: FeatureId,
}

const ENTRIES: &[Entry] = &[
    Entry {
        id: ViewId::BodyMirror,
        label: "Body mirror",
        feature: FeatureId::BodyFit,
    },
    Entry {
        id: ViewId::BodyRig,
        label: "Body rig",
        feature: FeatureId::BodyFit,
    },
    Entry {
        id: ViewId::BodyEstimate,
        label: "Body estimate",
        feature: FeatureId::BodyFit,
    },
    Entry {
        id: ViewId::GrabView,
        label: "Grab view",
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::ColliderView,
        label: "Collider view",
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::DockedGuns,
        label: "Docked guns",
        feature: FeatureId::Inventory,
    },
];

/// Whether the drawer hides every registered view's output, until Restore or payload teardown.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// The output mask that hides every registered view, and the body mirror's pouches with it.
fn hide_mask() -> SourceMask {
    let mut mask = SourceMask::NONE;
    for entry in ENTRIES {
        mask.insert(entry.id.source());
    }
    mask.insert(Source::BodyMirrorPouches);
    mask
}

fn temporary_controls(ui: &mut Ui) {
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if ui
        .add_enabled(!hidden, egui::Button::new("Hide registered views"))
        .on_hover_text(
            "Suppress only these views' rendered output, leaving their switches and producers \
             alone. It lasts until Restore or payload teardown.",
        )
        .clicked()
    {
        HIDDEN.store(true, Ordering::Relaxed);
        store::set_source_mask(hide_mask());
    }
    if ui
        .add_enabled(hidden, egui::Button::new("Restore registered views"))
        .clicked()
    {
        HIDDEN.store(false, Ordering::Relaxed);
        store::set_source_mask(SourceMask::NONE);
    }
}

fn matching<'a>(config: &ControllersConfig, filter: &str) -> Vec<&'a Entry> {
    let filter = filter.trim().to_lowercase();
    let mut entries: Vec<_> = ENTRIES
        .iter()
        .filter(|entry| {
            filter.is_empty()
                || entry.label.to_lowercase().contains(&filter)
                || entry
                    .feature
                    .definition()
                    .label
                    .to_lowercase()
                    .contains(&filter)
        })
        .collect();
    entries.sort_by_key(|entry| !entry.id.requested(config));
    entries
}

fn apply_edits(config: &mut ControllersConfig, edits: &[(ViewId, bool)]) {
    for &(id, requested) in edits {
        id.set(config, requested);
    }
}

#[cfg(test)]
mod tests;
