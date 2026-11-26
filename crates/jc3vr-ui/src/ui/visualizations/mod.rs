//! Shared visualization switches, known draw blockers, and the global drawer.

use egui::Ui;
use jc3vr_body::controllers::scheme;
use jc3vr_gameplay::controllers::config::{CONTROLLERS_CONFIG, ControllersConfig};
use jc3vr_render::controllers::overlay;
use jc3vr_view::controllers::overlay::{primitive::Source, store, store::SourceMask};
use parking_lot::Mutex;

use crate::ui::{catalog::FeatureId, controllers};

/// A snapshot taken before any configuration edit guard is acquired.
pub(crate) struct Snapshot {
    config: ControllersConfig,
    runtime: Runtime,
}

/// Counts cover only the switches registered in this inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) requested: usize,
    pub(crate) blocked: usize,
    pub(crate) draw_unverified: usize,
}

impl Summary {
    /// No positive active count is inferred from saved settings or retained primitive counters.
    pub(crate) fn active_count(self) -> Option<usize> {
        (self.draw_unverified == 0).then_some(0)
    }

    pub(crate) fn label(self) -> String {
        match self.active_count() {
            Some(active) => format!("{} requested; {active} active", self.requested),
            None => format!("{} requested; active unknown", self.requested),
        }
    }
}

impl Snapshot {
    pub(crate) fn summary(&self) -> Summary {
        let mut summary = Summary {
            requested: 0,
            blocked: 0,
            draw_unverified: 0,
        };
        for entry in ENTRIES {
            match self.state(entry.id) {
                ViewState::Off => {}
                ViewState::Blocked(_) => {
                    summary.requested += 1;
                    summary.blocked += 1;
                }
                ViewState::DrawUnverified => {
                    summary.requested += 1;
                    summary.draw_unverified += 1;
                }
            }
        }
        summary
    }

    fn state(&self, id: ViewId) -> ViewState {
        if !id.requested(&self.config) {
            return ViewState::Off;
        }
        if self.runtime.temporarily_hidden {
            return ViewState::Blocked(Blocker::TemporaryHide);
        }
        if !self.config.enabled {
            return ViewState::Blocked(Blocker::Controllers);
        }
        if self.runtime.clean_view {
            return ViewState::Blocked(Blocker::CleanView);
        }
        if id.overlay_producer() && !self.runtime.controllers_scheme {
            return ViewState::Blocked(Blocker::Scheme);
        }
        if (id.overlay_producer() || id == ViewId::ColliderView) && !self.runtime.gameplay {
            return ViewState::Blocked(Blocker::Gameplay);
        }
        ViewState::DrawUnverified
    }
}

/// Read existing status only; this does not activate a preview, scan, or draw producer.
pub(crate) fn snapshot() -> Snapshot {
    let config = CONTROLLERS_CONFIG.lock().clone();
    let runtime = Runtime {
        controllers_scheme: scheme::is_controllers(),
        gameplay: jc3vr_core::hooks::in_gameplay(),
        clean_view: jc3vr_core::director::clean_view::hides_everything(),
        temporarily_hidden: TEMPORARY_HIDE.lock().requested_at_hide.is_some(),
        overlay: overlay::pass::status(),
    };
    Snapshot { config, runtime }
}

/// Render the drawer's switches, the overlay's readout, and the views' readouts, without navigation
/// changes. The caller supplies the snapshot before entering any configuration edit boundary.
pub(crate) fn controls(ui: &mut Ui, snapshot: &Snapshot) {
    let id = ui.id().with("visualization-filter");
    let mut filter = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filter names or groups"));
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, filter.clone()));
    let summary = snapshot.summary();
    ui.label(format!(
        "Inventory: {}. {} blocked; {} draw unverified.",
        summary.label(),
        summary.blocked,
        summary.draw_unverified
    ));
    ui.small("Requested switches include view contents. Active drawing needs a current draw sample; retained primitive counts do not prove it.");
    let mut edits = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("visualization-switches")
        .show(ui, |ui| {
            let entries = matching(&snapshot.config, &filter);
            if entries.is_empty() {
                ui.label("No matching visualization switches.");
            }
            let mut previous_requested = None;
            for entry in entries {
                let requested = entry.id.requested(&snapshot.config);
                if previous_requested != Some(requested) {
                    ui.separator();
                    ui.label(if requested { "Requested" } else { "Off" });
                    previous_requested = Some(requested);
                }
                ui.push_id(entry.id, |ui| {
                    switch_control(ui, snapshot, entry, &mut edits);
                    ui.small(snapshot.state(entry.id).label());
                    if let ViewState::Blocked(blocker) = snapshot.state(entry.id) {
                        unblock_control(ui, blocker, &mut edits);
                    }
                });
            }
            ui.separator();
            ui.collapsing("Overlay readout", |ui| overlay_readout(ui, snapshot));
            ui.collapsing("View readouts", controllers::view_readouts);
            ui.separator();
            temporary_controls(ui, snapshot);
            ui.small("Preview, HUD, sight/scope, and performance tools remain in their existing feature editors. Opening this drawer does not activate their capture or scan work.");
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

    fn overlay_producer(self) -> bool {
        matches!(self, Self::BodyRig | Self::BodyEstimate)
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

#[derive(Clone, Copy)]
struct RequestedViews([Option<ViewId>; ENTRIES.len()]);

impl RequestedViews {
    fn capture(config: &ControllersConfig) -> Self {
        Self(std::array::from_fn(|index| {
            let id = ENTRIES[index].id;
            id.requested(config).then_some(id)
        }))
    }

    fn count(self) -> usize {
        self.0.iter().flatten().count()
    }
}

#[derive(Default)]
struct TemporaryHide {
    requested_at_hide: Option<RequestedViews>,
}

impl TemporaryHide {
    fn hide(&mut self, config: &ControllersConfig) -> SourceMask {
        self.requested_at_hide
            .get_or_insert_with(|| RequestedViews::capture(config));
        let mut mask = SourceMask::NONE;
        for entry in ENTRIES {
            mask.insert(entry.id.source());
        }
        mask.insert(Source::BodyMirrorPouches);
        mask
    }

    fn restore(&mut self) -> SourceMask {
        self.requested_at_hide = None;
        SourceMask::NONE
    }
}

static TEMPORARY_HIDE: Mutex<TemporaryHide> = Mutex::new(TemporaryHide {
    requested_at_hide: None,
});

fn temporary_controls(ui: &mut Ui, snapshot: &Snapshot) {
    let requested_at_hide = TEMPORARY_HIDE.lock().requested_at_hide;
    let hidden = requested_at_hide.is_some();
    if ui
        .add_enabled(
            !hidden,
            egui::Button::new("Hide registered views"),
        )
        .on_hover_text("Suppress only this inventory's rendered output. Saved switches, producers, grip, and grapple behavior remain unchanged.")
        .clicked()
    {
        let mask = TEMPORARY_HIDE.lock().hide(&snapshot.config);
        store::set_source_mask(mask);
    }
    if ui
        .add_enabled(hidden, egui::Button::new("Restore registered views"))
        .clicked()
    {
        let mask = TEMPORARY_HIDE.lock().restore();
        store::set_source_mask(mask);
    }
    if let Some(requested) = requested_at_hide {
        ui.small(format!(
            "Temporarily hidden: all registered output. {} switches were requested at Hide; this is not a draw count.",
            requested.count()
        ));
    }
    ui.small("Hide also covers switches enabled while hidden. It lasts until Restore or payload teardown; closing this drawer does not restore output. Restore keeps configuration edits made while hidden.");
    ui.small("The grapple highlight, the sight tuning marker, and other non-inventory output remain unchanged.");
}

#[derive(Default)]
struct Runtime {
    controllers_scheme: bool,
    gameplay: bool,
    clean_view: bool,
    temporarily_hidden: bool,
    overlay: overlay::pass::OverlayStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewState {
    Off,
    Blocked(Blocker),
    DrawUnverified,
}

impl ViewState {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Not requested.",
            Self::Blocked(blocker) => blocker.label(),
            Self::DrawUnverified => {
                "Requested; draw unverified. Source, pose, camera, and depth availability are not sampled here."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blocker {
    TemporaryHide,
    Controllers,
    CleanView,
    Scheme,
    Gameplay,
}

impl Blocker {
    fn label(self) -> &'static str {
        match self {
            Self::TemporaryHide => "Requested; temporarily hidden until Restore.",
            Self::Controllers => "Requested; blocked: controllers are disabled.",
            Self::CleanView => "Requested; blocked: the clean view hides every overlay.",
            Self::Scheme => "Requested; blocked: the controller scheme is inactive.",
            Self::Gameplay => "Requested; blocked: gameplay is inactive.",
        }
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

#[derive(Clone, Copy)]
enum Edit {
    Switch(ViewId, bool),
    Controllers(bool),
}

fn switch_control(ui: &mut Ui, snapshot: &Snapshot, entry: &Entry, edits: &mut Vec<Edit>) {
    let mut requested = entry.id.requested(&snapshot.config);
    if ui.checkbox(&mut requested, entry.label).changed() {
        edits.push(Edit::Switch(entry.id, requested));
    }
}

fn unblock_control(ui: &mut Ui, blocker: Blocker, edits: &mut Vec<Edit>) {
    let action = match blocker {
        Blocker::Controllers => Some(("Enable controllers", Edit::Controllers(true))),
        _ => None,
    };
    if let Some((label, edit)) = action
        && ui.small_button(label).clicked()
    {
        edits.push(edit);
    }
}

fn overlay_readout(ui: &mut Ui, snapshot: &Snapshot) {
    let status = &snapshot.runtime.overlay;
    ui.label(format!(
        "Last overlay snapshot, all sources: {} retained primitives; {} collected after suppression and the cap; {} temporarily suppressed; {} uploaded per eye.",
        status.counts.iter().sum::<usize>(),
        status.visible_counts.iter().sum::<usize>(),
        status.suppressed_counts.iter().sum::<usize>(),
        status.drawn,
    ));
    ui.small("These are last-reported primitive counts, not active-view counts or proof of a current draw.");
    if let Some(error) = &status.error {
        ui.label(format!("Last overlay pipeline error (retained diagnostic; does not indicate current draw status): {error}"));
    }
}

fn apply_edits(config: &mut ControllersConfig, edits: &[Edit]) {
    for edit in edits {
        match *edit {
            Edit::Switch(id, requested) => id.set(config, requested),
            Edit::Controllers(enabled) => config.enabled = enabled,
        }
    }
}

#[cfg(test)]
mod tests;
