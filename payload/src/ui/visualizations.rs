//! Shared visualization switches, known draw blockers, and the global drawer.

use egui::Ui;

use crate::{
    config::{self, Config},
    controllers::{config::GunReadout, overlay, scheme},
    ui::catalog::FeatureId,
};

/// A snapshot taken before any configuration edit guard is acquired.
pub(crate) struct Snapshot {
    config: Config,
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
        if !self.config.controllers.enabled {
            return ViewState::Blocked(Blocker::Controllers);
        }
        if !self.config.controllers.overlay.enabled {
            return ViewState::Blocked(Blocker::Overlay);
        }
        if id.grab_child() && !self.config.controllers.physical_hands.grab_view.enabled {
            return ViewState::Blocked(Blocker::GrabView);
        }
        if self.runtime.cinematic {
            return ViewState::Blocked(Blocker::Cinematic);
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
    let config = config::get();
    let runtime = Runtime {
        controllers_scheme: scheme::is_controllers(),
        gameplay: crate::hooks::in_gameplay(),
        cinematic: crate::director::cinematic::active(),
        overlay_error: overlay::status().error,
    };
    Snapshot { config, runtime }
}

/// Render the same inventory globally or as a feature-local subset, without navigation changes.
/// The caller supplies the snapshot before entering any configuration edit boundary.
pub(crate) fn controls(ui: &mut Ui, snapshot: &Snapshot, feature: Option<FeatureId>) {
    let id = ui.id().with(("visualization-filter", feature));
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
        .id_salt(("visualization-switches", feature))
        .show(ui, |ui| {
            let entries = matching(&snapshot.config, &filter, feature);
            if entries.is_empty() {
                ui.label("No matching visualization switches.");
            }
            let mut previous_group = None;
            for entry in entries {
                let group = (entry.id.requested(&snapshot.config), entry.group);
                if previous_group != Some(group) {
                    ui.separator();
                    ui.label(format!("{} / {}", if group.0 { "Requested" } else { "Off" }, entry.group.label()));
                    previous_group = Some(group);
                }
                ui.push_id(entry.id, |ui| {
                    let mut requested = entry.id.requested(&snapshot.config);
                    if ui.checkbox(&mut requested, entry.label).changed() {
                        edits.push(Edit::Switch(entry.id, requested));
                    }
                    ui.small(snapshot.state(entry.id).label());
                    if let ViewState::Blocked(blocker) = snapshot.state(entry.id) {
                        unblock_control(ui, blocker, &mut edits);
                    }
                });
            }
            ui.separator();
            ui.collapsing("Shared overlay settings", |ui| shared_settings(ui, snapshot, &mut edits));
            ui.separator();
            ui.add_enabled(false, egui::Button::new("Hide active views"))
                .on_hover_text("Unavailable: temporary runtime suppression is not implemented. Saved switches are not changed.");
            ui.add_enabled(false, egui::Button::new("Restore views"));
            ui.small("Temporary Hide/Restore is unfinished. No saved switches are used as a substitute.");
            ui.small("Preview, HUD, sight/scope, and performance tools remain in their existing feature editors. Opening this drawer does not activate their capture or scan work.");
        });
    if !edits.is_empty() {
        apply_edits(&mut config::edit(), &edits);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ViewId {
    BodyMirror,
    ControllerGizmos,
    GhostHands,
    GripMarkers,
    GrappleHighlight,
    GrappleBeam,
    GrabView,
    GrabRays,
    GrabTargets,
    GrabWrist,
    GrabRefusals,
    GrabCandidates,
    ColliderView,
}

impl ViewId {
    fn requested(self, config: &Config) -> bool {
        let overlay = &config.controllers.overlay;
        let grab = &config.controllers.physical_hands.grab_view;
        match self {
            Self::BodyMirror => overlay.body_mirror,
            Self::ControllerGizmos => overlay.debug_gizmos,
            Self::GhostHands => overlay.ghost_hands,
            Self::GripMarkers => overlay.markers,
            Self::GrappleHighlight => overlay.grapple_highlight,
            Self::GrappleBeam => overlay.grapple_beam,
            Self::GrabView => grab.enabled,
            Self::GrabRays => grab.rays,
            Self::GrabTargets => grab.targets,
            Self::GrabWrist => grab.wrist,
            Self::GrabRefusals => grab.refusals,
            Self::GrabCandidates => grab.candidates,
            Self::ColliderView => config.controllers.physical_hands.collider_view.enabled,
        }
    }

    fn set(self, config: &mut Config, requested: bool) {
        let overlay = &mut config.controllers.overlay;
        let grab = &mut config.controllers.physical_hands.grab_view;
        match self {
            Self::BodyMirror => overlay.body_mirror = requested,
            Self::ControllerGizmos => overlay.debug_gizmos = requested,
            Self::GhostHands => overlay.ghost_hands = requested,
            Self::GripMarkers => overlay.markers = requested,
            Self::GrappleHighlight => overlay.grapple_highlight = requested,
            Self::GrappleBeam => overlay.grapple_beam = requested,
            Self::GrabView => grab.enabled = requested,
            Self::GrabRays => grab.rays = requested,
            Self::GrabTargets => grab.targets = requested,
            Self::GrabWrist => grab.wrist = requested,
            Self::GrabRefusals => grab.refusals = requested,
            Self::GrabCandidates => grab.candidates = requested,
            Self::ColliderView => {
                config.controllers.physical_hands.collider_view.enabled = requested
            }
        }
    }

    fn grab_child(self) -> bool {
        matches!(
            self,
            Self::GrabRays
                | Self::GrabTargets
                | Self::GrabWrist
                | Self::GrabRefusals
                | Self::GrabCandidates
        )
    }

    fn overlay_producer(self) -> bool {
        matches!(
            self,
            Self::ControllerGizmos | Self::GhostHands | Self::GrappleHighlight | Self::GrappleBeam
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    DebugGeometry,
    GameplayCues,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Self::DebugGeometry => "Debug geometry",
            Self::GameplayCues => "Gameplay cues",
        }
    }
}

struct Entry {
    id: ViewId,
    label: &'static str,
    group: Group,
    feature: FeatureId,
}

const ENTRIES: &[Entry] = &[
    Entry {
        id: ViewId::BodyMirror,
        label: "Body mirror",
        group: Group::DebugGeometry,
        feature: FeatureId::RootWalls,
    },
    Entry {
        id: ViewId::ControllerGizmos,
        label: "Controller gizmos",
        group: Group::DebugGeometry,
        feature: FeatureId::RootWalls,
    },
    Entry {
        id: ViewId::GhostHands,
        label: "Ghost hands",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabView,
        label: "Grab view",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabRays,
        label: "Grab view: reach rays",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabTargets,
        label: "Grab view: target lines",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabWrist,
        label: "Grab view: palm state on wrists",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabRefusals,
        label: "Grab view: refused grabs",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrabCandidates,
        label: "Grab view: nearby candidates",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::ColliderView,
        label: "Collider view",
        group: Group::DebugGeometry,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GripMarkers,
        label: "Grip markers",
        group: Group::GameplayCues,
        feature: FeatureId::PhysicalHands,
    },
    Entry {
        id: ViewId::GrappleHighlight,
        label: "Grapple target highlight",
        group: Group::GameplayCues,
        feature: FeatureId::Grapple,
    },
    Entry {
        id: ViewId::GrappleBeam,
        label: "Grapple beam",
        group: Group::GameplayCues,
        feature: FeatureId::Grapple,
    },
];

#[derive(Default)]
struct Runtime {
    controllers_scheme: bool,
    gameplay: bool,
    cinematic: bool,
    overlay_error: Option<String>,
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
    Controllers,
    Overlay,
    GrabView,
    Cinematic,
    Scheme,
    Gameplay,
}

impl Blocker {
    fn label(self) -> &'static str {
        match self {
            Self::Controllers => "Requested; blocked: controllers are disabled.",
            Self::Overlay => "Requested; blocked: the world-space overlay is disabled.",
            Self::GrabView => "Requested; blocked: the grab view is disabled.",
            Self::Cinematic => "Requested; blocked: cinematic capture suppresses the overlay.",
            Self::Scheme => "Requested; blocked: the controller scheme is inactive.",
            Self::Gameplay => "Requested; blocked: gameplay is inactive.",
        }
    }
}

fn matching<'a>(config: &Config, filter: &str, feature: Option<FeatureId>) -> Vec<&'a Entry> {
    let filter = filter.trim().to_lowercase();
    let mut entries: Vec<_> = ENTRIES
        .iter()
        .filter(|entry| feature.is_none_or(|feature| entry.feature == feature))
        .filter(|entry| {
            filter.is_empty()
                || entry.label.to_lowercase().contains(&filter)
                || entry.group.label().to_lowercase().contains(&filter)
                || entry
                    .feature
                    .definition()
                    .label
                    .to_lowercase()
                    .contains(&filter)
        })
        .collect();
    entries.sort_by_key(|entry| (!entry.id.requested(config), entry.group));
    entries
}

#[derive(Clone, Copy)]
enum Edit {
    Switch(ViewId, bool),
    Controllers(bool),
    Overlay(bool),
    GunReadout(GunReadout),
    MirrorDistance(f32),
    DepthTolerance(f32),
    RelativeDepthTolerance(f32),
    MinimumLineWidth(f32),
    XrayAlpha(f32),
}

fn unblock_control(ui: &mut Ui, blocker: Blocker, edits: &mut Vec<Edit>) {
    let action = match blocker {
        Blocker::Controllers => Some(("Enable controllers", Edit::Controllers(true))),
        Blocker::Overlay => Some(("Enable the world-space overlay", Edit::Overlay(true))),
        Blocker::GrabView => Some(("Enable the grab view", Edit::Switch(ViewId::GrabView, true))),
        _ => None,
    };
    if let Some((label, edit)) = action
        && ui.small_button(label).clicked()
    {
        edits.push(edit);
    }
}

fn shared_settings(ui: &mut Ui, snapshot: &Snapshot, edits: &mut Vec<Edit>) {
    let mut overlay = snapshot.config.controllers.overlay;
    if ui
        .checkbox(&mut overlay.enabled, "World-space overlay enabled")
        .changed()
    {
        edits.push(Edit::Overlay(overlay.enabled));
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Gun readout");
        let before = overlay.gun_readout;
        ui.selectable_value(&mut overlay.gun_readout, GunReadout::None, "None");
        ui.selectable_value(&mut overlay.gun_readout, GunReadout::Dot, "Dot");
        ui.selectable_value(&mut overlay.gun_readout, GunReadout::Laser, "Laser");
        if before != overlay.gun_readout {
            edits.push(Edit::GunReadout(overlay.gun_readout));
        }
    });
    ui.small("The gun readout selector is not included in the Boolean-switch counts.");
    if ui
        .add(
            egui::Slider::new(&mut overlay.body_mirror_distance_m, 0.5..=4.0)
                .text("Body mirror distance (m)"),
        )
        .changed()
    {
        edits.push(Edit::MirrorDistance(overlay.body_mirror_distance_m));
    }
    if ui
        .add(
            egui::Slider::new(&mut overlay.depth_tolerance_m, 0.0..=0.5)
                .text("Depth tolerance (m)"),
        )
        .changed()
    {
        edits.push(Edit::DepthTolerance(overlay.depth_tolerance_m));
    }
    if ui
        .add(
            egui::Slider::new(&mut overlay.depth_tolerance_rel, 0.0..=0.05)
                .text("Depth tolerance (fraction of distance)"),
        )
        .changed()
    {
        edits.push(Edit::RelativeDepthTolerance(overlay.depth_tolerance_rel));
    }
    if ui
        .add(egui::Slider::new(&mut overlay.min_line_px, 0.5..=4.0).text("Minimum line width (px)"))
        .changed()
    {
        edits.push(Edit::MinimumLineWidth(overlay.min_line_px));
    }
    if ui
        .add(egui::Slider::new(&mut overlay.xray_alpha, 0.0..=1.0).text("Ghost x-ray alpha"))
        .changed()
    {
        edits.push(Edit::XrayAlpha(overlay.xray_alpha));
    }
    if let Some(error) = &snapshot.runtime.overlay_error {
        ui.label(format!("Last overlay pipeline error (retained diagnostic; does not indicate current draw status): {error}"));
    }
}

fn apply_edits(config: &mut Config, edits: &[Edit]) {
    for edit in edits {
        match *edit {
            Edit::Switch(id, requested) => id.set(config, requested),
            Edit::Controllers(enabled) => config.controllers.enabled = enabled,
            Edit::Overlay(enabled) => config.controllers.overlay.enabled = enabled,
            Edit::GunReadout(readout) => config.controllers.overlay.gun_readout = readout,
            Edit::MirrorDistance(distance) => {
                config.controllers.overlay.body_mirror_distance_m = distance
            }
            Edit::DepthTolerance(tolerance) => {
                config.controllers.overlay.depth_tolerance_m = tolerance
            }
            Edit::RelativeDepthTolerance(tolerance) => {
                config.controllers.overlay.depth_tolerance_rel = tolerance
            }
            Edit::MinimumLineWidth(width) => config.controllers.overlay.min_line_px = width,
            Edit::XrayAlpha(alpha) => config.controllers.overlay.xray_alpha = alpha,
        }
    }
}

#[cfg(test)]
#[path = "visualizations/tests.rs"]
mod tests;
