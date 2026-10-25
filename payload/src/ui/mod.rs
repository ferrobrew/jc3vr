//! egui debug UI: the dockable tab tree and per-tab bodies, plus the startup build banner.
//!
//! The tabs live in an `egui_dock` tree so any tab can be dragged out into its own floating window
//! (e.g. Performance parked beside the game view while tuning Render features). The layout is
//! persisted to `ui_layout.json` in the configuration directory ([`crate::config::paths`]), apart
//! from the settings, so a customized arrangement survives relaunches.

use std::{sync::OnceLock, time::Instant};

use serde::{Deserialize, Serialize};

use crate::{
    config::{self, SaveStatus},
    egui_impl::{debug_ui_capture, request_debug_ui},
    hud::egui_panel,
    ui::capture::{OwnershipBlocker, PanelMode, PanelRequest},
};

pub mod camera;
pub(crate) mod capture;
#[path = "inventory/catalog.rs"]
pub(crate) mod catalog;
pub mod controllers;
pub mod diagnostics;
pub mod director;
mod dispatch;
pub mod environment;
pub mod game;
pub mod hud;
#[cfg(test)]
pub(crate) mod inventory;
mod layout;
pub(crate) mod navigation;
mod overview;
pub mod performance;
pub mod previews;
pub mod render;
pub mod status;
pub mod testing;
mod util;
pub(crate) mod visualizations;
pub mod vr;

/// The dockable tabs, in default display order. The serialized names are part of the persisted
/// layout file, so renaming a variant orphans its saved position (it falls back to being appended).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Performance,
    Render,
    Previews,
    Diagnostics,
    Status,
    Camera,
    Game,
    Hud,
    Environment,
    Vr,
    Director,
    Controllers,
    Testing,
}

#[cfg(test)]
impl Tab {
    /// Every legacy tab, used to test saved-layout migration.
    const ALL: [Tab; 13] = [
        Tab::Performance,
        Tab::Render,
        Tab::Previews,
        Tab::Diagnostics,
        Tab::Status,
        Tab::Camera,
        Tab::Game,
        Tab::Hud,
        Tab::Environment,
        Tab::Vr,
        Tab::Director,
        Tab::Controllers,
        Tab::Testing,
    ];
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

struct FeatureBodies<'a, F> {
    render: F,
    selection: &'a mut navigation::NavigationState,
    editing: bool,
}

impl<F: FnMut(&mut egui::Ui, &mut navigation::NavigationState)> egui_dock::TabViewer
    for FeatureBodies<'_, F>
{
    type Tab = layout::View;

    fn title(&mut self, tab: &mut layout::View) -> egui::WidgetText {
        match tab {
            layout::View::Workspace => "Features".into(),
            layout::View::Feature { key, section } => format!(
                "{}: {}",
                navigation::NavigationState::recover(Some(key))
                    .feature
                    .definition()
                    .label,
                section.label()
            )
            .into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut layout::View) {
        edit_boundary(ui, self.editing, |ui| match tab {
            layout::View::Workspace => {
                navigation::feature_body(ui, self.selection, &mut self.render);
            }
            layout::View::Feature { key, section } => {
                let mut selection = navigation::NavigationState {
                    feature: navigation::NavigationState::recover(Some(key)).feature,
                    section: *section,
                };
                ui.push_id(("detached-feature", key.as_str(), *section), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(navigation::scroll_key(selection))
                        .show(ui, |ui| (self.render)(ui, &mut selection));
                });
            }
        });
    }

    fn closeable(&mut self, tab: &mut layout::View) -> bool {
        self.editing && !matches!(tab, layout::View::Workspace)
    }
}

fn feature_dock(
    ui: &mut egui::Ui,
    dock: &mut egui_dock::DockState<layout::View>,
    selection: &mut navigation::NavigationState,
    editing: bool,
    render: impl FnMut(&mut egui::Ui, &mut navigation::NavigationState),
) {
    // Dock windows create fresh UIs, so both their input and their bodies need an explicit gate.
    if !editing {
        neutralize_dock_input(ui.ctx());
        ui.ctx().stop_dragging();
        ui.ctx().memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
    }
    let mut frozen = dock.clone();
    let shown = if editing { &mut *dock } else { &mut frozen };
    egui_dock::DockArea::new(shown)
        .id(egui::Id::new("jc3vr_features_v2"))
        .style(egui_dock::Style::from_egui(ui.style().as_ref()))
        .draggable_tabs(editing)
        .allowed_splits(if editing {
            egui_dock::AllowedSplits::All
        } else {
            egui_dock::AllowedSplits::None
        })
        .tab_context_menus(editing)
        .secondary_button_context_menu(editing)
        .show_close_buttons(editing)
        .show_leaf_collapse_buttons(editing)
        .show_leaf_close_all_buttons(false)
        .show_inside(
            ui,
            &mut FeatureBodies {
                render,
                selection,
                editing,
            },
        );
}

fn neutralize_dock_input(context: &egui::Context) {
    let options = context.options(|options| options.input_options);
    context.input_mut(|input| {
        let mut raw = input.raw.clone();
        raw.events.clear();
        raw.modifiers = egui::Modifiers::default();
        raw.hovered_files.clear();
        raw.dropped_files.clear();
        let mut frame = raw.clone();
        frame.screen_rect = Some(input.viewport_rect());
        frame.safe_area_insets = Some(input.safe_area_insets());
        frame.time = Some(input.time);
        frame.max_texture_side = Some(input.max_texture_side);
        // Rebuild private touch, wheel, and gesture state without replacing frame metadata.
        let mut neutral =
            egui::InputState::default().begin_pass(frame, false, input.pixels_per_point, options);
        neutral.raw = raw;
        neutral.time = input.time;
        neutral.unstable_dt = input.unstable_dt;
        neutral.predicted_dt = input.predicted_dt;
        neutral.stable_dt = input.stable_dt;
        neutral.focused = input.focused;
        *input = neutral;
    });
}

/// The settings file's path, selectable so it can be copied, and whether it is up to date.
fn settings_line(ui: &mut egui::Ui) {
    let state = match config::status() {
        SaveStatus::UpToDate => "saved",
        SaveStatus::Pending => "saving",
        SaveStatus::Failed => "save failed, see the log",
        SaveStatus::Unavailable => "not saved: no configuration directory",
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Settings ({state}):"))
                .weak()
                .small(),
        );
        if let Some(path) = config::paths::settings_file() {
            let text = path.display().to_string();
            let mut shown = text.as_str();
            ui.add(
                egui::TextEdit::singleline(&mut shown)
                    .font(egui::TextStyle::Small)
                    .desired_width(ui.available_width()),
            );
        }
    });
}

/// The injection confirmation: a large banner announcing this payload's build stamp for the first
/// seconds after startup, on whatever surface the egui overlay renders to (the flat overlay, and
/// the VR floating panel). Its *absence* after running the inject script is the tell that the
/// injection failed and the game is still running a stale resident payload — the failure mode this
/// exists to make obvious from inside the headset.
pub fn startup_banner(ctx: &egui::Context) {
    static FIRST_FRAME: OnceLock<Instant> = OnceLock::new();
    let age = FIRST_FRAME
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs_f32();
    if age > BANNER_SECONDS {
        return;
    }
    egui::Area::new(egui::Id::new("jc3vr_build_banner"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(0, 96, 0, 230))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "jc3vr injected — build {}",
                            crate::BUILD_STAMP
                        ))
                        .size(26.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                    );
                });
        });
}

/// How long (seconds) the startup banner stays up.
const BANNER_SECONDS: f32 = 15.0;

/// The restart notice ([`crate::vr::input::restart`]) on whatever surface the egui overlay renders
/// to, for as long as it holds. The headset has its own panel for it
/// ([`crate::hud::restart_notice`]); this is the desktop window's copy.
pub fn restart_banner(ctx: &egui::Context) {
    let Some(reason) = crate::vr::input::restart::current() else {
        return;
    };
    egui::Area::new(egui::Id::new("jc3vr_restart_banner"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(120, 20, 15, 235))
                .show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.label(
                        egui::RichText::new(reason.headline())
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.label(egui::RichText::new(reason.detail()).color(egui::Color32::WHITE));
                });
        });
}

pub fn egui_debug_window(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    capture_header(ui);
    let editing = debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.any());
    edit_boundary(ui, editing, |ui| egui_debug_window_body(ui, renderer));
}

fn edit_boundary(ui: &mut egui::Ui, editing: bool, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_enabled_ui(editing, body);
}

fn capture_header(ui: &mut egui::Ui) {
    let Some(snapshot) = debug_ui_capture() else {
        ui.label("Editing requested; waiting for the input sample.");
        return;
    };
    ui.horizontal_wrapped(|ui| {
        let mut allow_gameplay = snapshot.mode == PanelMode::ReadOnly;
        if ui
            .add_enabled(
                snapshot.edit_capture.any(),
                egui::Checkbox::new(&mut allow_gameplay, "Allow gameplay input"),
            )
            .changed()
            && allow_gameplay
        {
            request_debug_ui(PanelRequest::AllowGameplay);
        }
        ui.label(match snapshot.mode {
            PanelMode::Hidden => "Mode: Hidden",
            PanelMode::Editing => "Mode: Editing",
            PanelMode::ReadOnly => "Mode: Read-only overlay",
        });
        if ui
            .add_enabled(snapshot.edit_capture.any(), egui::Button::new("Hide"))
            .clicked()
        {
            egui_panel::hide();
        }
    });
    let status = match snapshot.blocker {
        None => "Gameplay input: effective.",
        Some(OwnershipBlocker::Editing) => {
            "Editing input: effective. The game is not automatically paused."
        }
        Some(OwnershipBlocker::Unfocused) => "Waiting for game focus.",
        Some(OwnershipBlocker::TrackingRecovery) => "Waiting for tracking/input recovery.",
        Some(OwnershipBlocker::AwaitingNeutral) => {
            "Waiting for all held inputs to return to neutral."
        }
        Some(OwnershipBlocker::DeviceCapturePending) => {
            "Waiting for the game input device to acknowledge editing capture."
        }
        Some(OwnershipBlocker::GameDeviceDisabled) if snapshot.device_restore_pending => {
            "Waiting for the game input device to acknowledge restoration."
        }
        Some(OwnershipBlocker::GameDeviceDisabled) => {
            "Gameplay input is blocked: the game input device is disabled."
        }
    };
    ui.label(format!(
        "{} requested. {status}",
        if snapshot.gameplay_requested {
            "Gameplay"
        } else {
            "Editing"
        },
    ));
    if snapshot.mode == PanelMode::ReadOnly {
        ui.label("Controls are locked. Hide then reopen with F8 or the panel shortcut to edit. F4 controls the desktop overlay.");
    }
    ui.separator();
}

fn egui_debug_window_body(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Build {}", crate::BUILD_STAMP))
                .weak()
                .small(),
        );
        if egui_panel::is_active()
            && ui
                .small_button("Recall panel")
                .on_hover_text("Bring the floating panel back in front of your head.")
                .clicked()
        {
            egui_panel::recall();
        }
    });
    settings_line(ui);

    let visualizations = visualizations::snapshot();
    let mut slot = layout::STATE.lock();
    let state = slot.get_or_insert_with(layout::Layout::load);
    let editing = debug_ui_capture().is_some_and(|snapshot| snapshot.edit_capture.any());
    ui.horizontal_wrapped(|ui| {
        ui.toggle_value(
            &mut state.drawer,
            format!("Visualizations: {}", visualizations.summary().label()),
        );
        if ui.button("Detach this view").clicked() { state.detach(state.selection); }
        ui.menu_button("Layout", |ui| {
            if ui.button("Restore default layout").clicked() { state.reset(); }
            if ui.button("Recover detached views").clicked() { state.recover(); }
            if ui.add_enabled(state.has_legacy(), egui::Button::new("Migrate old layout")).clicked() { state.migrate_legacy(); }
            ui.label("Drag a view tab back to the main dock to re-dock it. Closed views remain reachable from the feature navigation.");
        });
    });
    if !state.notice.is_empty() {
        ui.label(&state.notice);
    }
    if state.drawer {
        egui::Panel::right("debug-visualization-drawer-v2")
            .resizable(editing)
            .default_size(320.0)
            .show(ui, |ui| {
                edit_boundary(ui, editing, |ui| {
                    ui.heading("Visualizations");
                    visualizations::controls(ui, &visualizations, None);
                });
            });
    }
    let layout::Layout {
        dock, selection, ..
    } = state;
    feature_dock(ui, dock, selection, editing, |ui, selection| {
        dispatch::feature(ui, selection, Some(&mut *renderer));
    });
    if editing {
        state.save();
    }
}
