//! The Director and Video features: the directing tools for staging footage (`crate::director`),
//! grouped by what a shot needs: time, the actor, heat, where you are, what is in frame, and the
//! keyboard shortcuts; and the video recorder with its take markers.

use egui::Slider;
use jc3gi::heat_manager::HeatManager;
use parking_lot::Mutex;

use crate::{
    config,
    director::{
        clean_view::CleanViewScope,
        heat, keymap, markers, safety, spawn,
        teleport::{self, TeleportMode},
        time,
    },
    recorder::{self, HostState, session_recording},
    ui::catalog::FeatureId,
};

/// Render the Director or Video feature's controls.
pub(crate) fn feature(ui: &mut egui::Ui, id: FeatureId) {
    match id {
        FeatureId::Director => {
            clean_view_ui(ui);
            time_ui(ui);
            safety_ui(ui);
            heat_ui(ui);
            ui.separator();
            teleport_ui(ui);
            ui.separator();
            spawn_ui(ui);
            ui.collapsing("Keyboard shortcuts", keymap_ui);
        }
        FeatureId::Video => {
            session_ui(ui);
            ui.separator();
            recording_ui(ui);
            markers_ui(ui);
        }
        _ => {}
    }
}

/// Render the actor, heat-manager, teleport, and spawn-request facts without settings or
/// commands.
pub(crate) fn director_status(ui: &mut egui::Ui) {
    safety_readout(ui);
    heat_readout(ui);
    let position = teleport::player_position();
    let in_flight = teleport::in_flight();
    let bookmark_count = teleport::bookmarks().len();
    let submission = spawn::status();
    position_readout(ui, position);
    ui.label(if in_flight {
        "Teleport: queued or in flight"
    } else {
        "Teleport: idle"
    });
    ui.label(format!("Saved bookmarks: {bookmark_count}"));
    ui.label(if submission.pending {
        "Spawn request: queued"
    } else {
        "Spawn request: idle"
    });
    ui.label(match submission.last_submission {
        None => "Last spawn request: none processed",
        Some(spawn::Submission::NoTags) => "Last spawn request: not submitted (no tags)",
        Some(spawn::Submission::TagsContainNul) => {
            "Last spawn request: not submitted (tags contain a NUL)"
        }
        Some(spawn::Submission::NoSpawnSystem) => {
            "Last spawn request: not submitted (no spawn system)"
        }
        Some(spawn::Submission::Submitted) => {
            "Last spawn request: submitted; object completion is not tracked"
        }
    });
}

/// Render the recorder snapshot without settings or commands.
pub(crate) fn video_status(ui: &mut egui::Ui) {
    ui.label(session_line(&session_recording::status()));
    let status = recorder::status();
    ui.label(host_line(&status));
    if status.recording {
        ui.label(format!("Sidecar lines written: {}", status.lines));
    }
    if let Some(folder) = &status.folder {
        ui.label(format!("Recording folder in the session folder: {folder}"));
    }
    if let Some(error) = &status.error {
        ui.colored_label(egui::Color32::LIGHT_RED, format!("Last error: {error}"));
    }
}

/// The recorder's one-line state, naming what the capture host records.
pub(crate) fn host_line(status: &recorder::Status) -> String {
    match &status.host {
        None => "Video: stopped".to_owned(),
        Some(HostState::Waiting) => "Video: waiting for the capture host to listen".to_owned(),
        Some(HostState::Starting) => "Video: the capture host is finding the window".to_owned(),
        Some(HostState::Capturing(capture)) => {
            let region = capture.region;
            format!(
                "Video: recording the mirror window, {}x{} at {},{} on {}",
                region.width,
                region.height,
                region.x,
                region.y,
                capture.output.as_deref().unwrap_or("the screen")
            )
        }
        Some(HostState::Finishing) => "Video: the capture host is finalizing the file".to_owned(),
        Some(HostState::Closed) => "Video: not recording (see the error)".to_owned(),
    }
}

fn clean_view_ui(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    let d = &mut cfg.director;
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.clean_view, "Clean view (F4)")
            .on_hover_text(
                "Hides the mod's overlays at the chosen scope without changing their settings. At \
             the desktop scope this hides the debug overlay, so F4 brings it back.",
            );
        ui.selectable_value(&mut d.clean_view_scope, CleanViewScope::Desktop, "Desktop")
            .on_hover_text("Hide the flat debug overlay from the desktop window only.");
        ui.selectable_value(
            &mut d.clean_view_scope,
            CleanViewScope::Everything,
            "Everything",
        )
        .on_hover_text(
            "Also hide the floating HUD, the floating panel, and the in-world overlays. With \
                 the HUD redirect off (flatscreen) the game's own HUD stays; turn the redirect on \
                 in Rendering > HUD to hide it too.",
        );
    });
}

fn time_ui(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    let d = &mut cfg.director;
    ui.add(
        Slider::new(
            &mut d.time_scale,
            time::MIN_TIME_SCALE..=time::MAX_TIME_SCALE,
        )
        .logarithmic(true)
        .text("Time scale"),
    )
    .on_hover_text(
        "The game clock's rate, written into the slow-motion manager's normal speed so the game's \
         own slow-motion effects still blend against it.",
    );
    ui.horizontal(|ui| {
        for (label, scale) in [("1x", 1.0), ("1/2", 0.5), ("1/4", 0.25), ("1/10", 0.1)] {
            if ui.button(label).clicked() {
                d.time_scale = scale;
            }
        }
    });
    ui.add(
        Slider::new(&mut d.slow_motion_scale, time::MIN_TIME_SCALE..=1.0)
            .logarithmic(true)
            .text("Slow-motion hotkey rate"),
    );
    ui.checkbox(&mut d.paused, "Pause the game clock")
        .on_hover_text("The clock's own pause: the world freezes, the render loop keeps running.");
}

fn safety_ui(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    let d = &mut cfg.director;
    ui.checkbox(&mut d.god_mode, "God mode (re-asserted every frame)")
        .on_hover_text(
            "The game's own god mode on the player's character, kept on across respawns.",
        );
    ui.checkbox(
        &mut d.unlimited_ammo,
        "Unlimited ammo (re-asserted every frame)",
    );
}

fn safety_readout(ui: &mut egui::Ui) {
    match safety::live_state() {
        Some((god, ammo)) => {
            ui.label(format!(
                "Live: god mode {}, unlimited ammo {}",
                on_off(god),
                on_off(ammo)
            ));
        }
        None => {
            ui.label("Live: no player character");
        }
    }
}

fn heat_ui(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    let d = &mut cfg.director;
    ui.checkbox(&mut d.heat_lock, "Lock heat (no rise, no decay)")
        .on_hover_text(
            "Sets the heat manager's system lock every frame: the level neither climbs nor clears \
             until unlocked.",
        );
    let mut capped = d.heat_cap.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut capped, "Cap heat at").changed() {
            d.heat_cap = capped.then_some(d.heat_cap.unwrap_or(1));
        }
        if let Some(cap) = d.heat_cap.as_mut() {
            ui.add(Slider::new(cap, 0..=HeatManager::MAX_LEVEL));
        }
    });
    drop(cfg);
    ui.horizontal(|ui| {
        ui.label("Set level:");
        for level in 0..=HeatManager::MAX_LEVEL {
            if ui.button(level.to_string()).clicked() {
                heat::set_level(level);
            }
        }
    });
}

fn heat_readout(ui: &mut egui::Ui) {
    match heat::live_state() {
        Some((level, locked, cap)) => {
            ui.label(format!(
                "Live: level {level}, lock {}, cap {}",
                on_off(locked),
                if cap == HeatManager::LEVEL_CAP_NONE {
                    "none".to_owned()
                } else {
                    cap.to_string()
                }
            ));
        }
        None => {
            ui.label("Live: no heat manager");
        }
    }
}

/// The debug tab's transient text fields: the bookmark name and the coordinate entry.
static TELEPORT_FORM: Mutex<TeleportForm> = Mutex::new(TeleportForm {
    name: String::new(),
    mode: TeleportMode::Auto,
    position: [0.0; 3],
});

struct TeleportForm {
    name: String,
    mode: TeleportMode,
    position: [f32; 3],
}

fn position_readout(ui: &mut egui::Ui, position: Option<[f32; 3]>) {
    match position {
        Some(p) => {
            ui.label(format!("Player at ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]));
        }
        None => {
            ui.label("Player position unavailable");
        }
    }
}

fn teleport_ui(ui: &mut egui::Ui) {
    let mut form = TELEPORT_FORM.lock();
    position_readout(ui, teleport::player_position());
    ui.horizontal(|ui| {
        ui.label("Mode:");
        for (mode, label, hint) in [
            (
                TeleportMode::Auto,
                "Auto",
                "Instant within 400 m, streamed beyond.",
            ),
            (
                TeleportMode::Instant,
                "Instant",
                "Place the character now; the destination must be streamed in.",
            ),
            (
                TeleportMode::Streamed,
                "Streamed",
                "Go through the load state so the destination streams in first.",
            ),
        ] {
            ui.selectable_value(&mut form.mode, mode, label)
                .on_hover_text(hint);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut form.name);
        if ui.button("Save bookmark here").clicked() {
            let name = form.name.clone();
            if teleport::save_current(&name) {
                form.name.clear();
            }
        }
    });
    let mode = form.mode;
    let bookmarks = teleport::bookmarks();
    if bookmarks.is_empty() {
        ui.label("No bookmarks yet.");
    }
    let mut remove = None;
    for (index, bookmark) in bookmarks.iter().enumerate() {
        ui.horizontal(|ui| {
            let p = bookmark.position();
            if ui
                .button(format!("{}. {}", index + 1, bookmark.name))
                .on_hover_text(format!("({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]))
                .clicked()
            {
                teleport::request_bookmark(index, mode);
            }
            if ui.small_button("x").on_hover_text("Remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        teleport::remove(index);
    }
    ui.horizontal(|ui| {
        ui.label("Coordinates:");
        for (axis, value) in ["x", "y", "z"].iter().zip(form.position.iter_mut()) {
            ui.add(
                egui::DragValue::new(value)
                    .prefix(format!("{axis} "))
                    .speed(1.0),
            );
        }
        if ui.button("Go").clicked() {
            teleport::request_position(form.position, mode);
        }
    });
}

/// The spawn picker's filter state.
static SPAWN_PICKER: Mutex<SpawnPicker> = Mutex::new(SpawnPicker {
    pool: None,
    search: String::new(),
});

struct SpawnPicker {
    /// The pool to list, or every pool.
    pool: Option<&'static str>,
    search: String,
}

fn spawn_ui(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    let s = &mut cfg.director.spawn;
    ui.horizontal(|ui| {
        ui.label("Tags:");
        ui.text_edit_singleline(&mut s.tags);
        if ui.button("Spawn").clicked() {
            spawn::request();
        }
    });
    ui.horizontal_wrapped(|ui| {
        for preset in spawn::PRESETS {
            if ui
                .button(preset.label)
                .on_hover_text(format!("tags: {}", preset.tags))
                .clicked()
            {
                s.tags = preset.tags.to_owned();
                spawn::request();
            }
        }
    });
    ui.add(Slider::new(&mut s.distance, 1.0..=100.0).text("Distance (m)"));
    ui.checkbox(&mut s.face_player, "Face the player")
        .on_hover_text("Turn the spawned object back toward the viewer instead of along the view.");
    ui.label(
        "Spawns are asynchronous and skip the out-of-view checks; the object appears once it \
         streams in.",
    );

    let mut picker = SPAWN_PICKER.lock();
    ui.horizontal(|ui| {
        ui.label("Definitions:");
        egui::ComboBox::from_id_salt("director_spawn_pool")
            .selected_text(picker.pool.unwrap_or("all pools"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut picker.pool, None, "all pools");
                for pool in spawn::pools() {
                    ui.selectable_value(&mut picker.pool, Some(pool), pool);
                }
            });
        ui.text_edit_singleline(&mut picker.search)
            .on_hover_text("Filter by name, category, or tag.");
    });
    let needle = picker.search.to_lowercase();
    let pool = picker.pool;
    egui::ScrollArea::vertical()
        .max_height(240.0)
        .show(ui, |ui| {
            for def in spawn::definitions() {
                if pool.is_some_and(|p| p != def.pool) {
                    continue;
                }
                if !needle.is_empty()
                    && !def.name.to_lowercase().contains(&needle)
                    && !def.category.to_lowercase().contains(&needle)
                    && !def.tags.to_lowercase().contains(&needle)
                {
                    continue;
                }
                let label = if def.category.is_empty() {
                    def.name.clone()
                } else {
                    format!("{}  [{}]", def.name, def.category)
                };
                if ui
                    .button(label)
                    .on_hover_text(format!("tags: {}", def.tags))
                    .clicked()
                {
                    s.tags = def.tags.clone();
                    spawn::request();
                }
            }
        });
}

fn session_ui(ui: &mut egui::Ui) {
    let status = session_recording::status();
    ui.horizontal(|ui| {
        let running = status.video || status.tracking;
        let label = if running {
            "Stop session recording (F1)"
        } else {
            "Start session recording (F1)"
        };
        if ui.button(label).clicked() {
            session_recording::toggle();
        }
        if ui
            .button("Mark (F2)")
            .on_hover_text(
                "Drops an unlabelled take marker; with no tracking recording running, also saves \
                 the lookback.",
            )
            .clicked()
        {
            session_recording::mark();
        }
    });
    ui.label(session_line(&status));
    if let Some(error) = &status.tracking_error {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("Tracking recording: {error}"),
        );
    }
    ui.label(
        "Starts the video recorder and a tracking recording together, both into \
         recordings/<stamp>/ in the session folder.",
    );
}

/// The session recording's one-line state.
fn session_line(status: &session_recording::SessionStatus) -> String {
    let running = match (status.video, status.tracking) {
        (true, true) => "video and tracking",
        (true, false) => "video only",
        (false, true) => "tracking only",
        (false, false) => "stopped",
    };
    match &status.stamp {
        Some(stamp) => format!("Session recording: {running} ({stamp})"),
        None => format!("Session recording: {running}"),
    }
}

fn recording_ui(ui: &mut egui::Ui) {
    let status = recorder::status();
    ui.horizontal(|ui| {
        if status.recording {
            if ui.button("Stop recording").clicked() {
                recorder::stop();
            }
        } else if ui.button("Start recording").clicked() {
            recorder::start();
        }
    });
    {
        let mut cfg = config::edit();
        let r = &mut cfg.recorder;
        ui.add_enabled_ui(!status.recording, |ui| {
            ui.horizontal(|ui| {
                ui.label("Capture host:");
                ui.text_edit_singleline(&mut r.host)
                    .on_hover_text("Blank for loopback.");
                ui.label("Port:");
                ui.add(egui::DragValue::new(&mut r.port));
            });
        });
    }
    ui.label(
        "Run scripts/capture_host.sh on the host. It records the mirror window as it is on \
         screen, so the mirror view (F10) decides what is recorded, and the window must stay \
         visible and in place for the take.",
    );
}

/// The debug tab's marker label field.
static MARKER_LABEL: Mutex<String> = Mutex::new(String::new());

fn markers_ui(ui: &mut egui::Ui) {
    let mut label = MARKER_LABEL.lock();
    ui.horizontal(|ui| {
        ui.label("Label:");
        ui.text_edit_singleline(&mut *label);
        if ui.button("Mark").clicked() {
            markers::mark(&label);
        }
    });
    for marker in markers::recent().iter().rev() {
        ui.label(format!(
            "{}  {}  {}",
            marker.index,
            marker.stamp,
            if marker.label.is_empty() {
                "(unlabelled)"
            } else {
                marker.label.as_str()
            }
        ));
    }
    ui.label(
        "Written into the video sidecar while recording; otherwise appended to markers.jsonl in \
         the session folder.",
    );
}

fn keymap_ui(ui: &mut egui::Ui) {
    let map = keymap::current();
    egui::Grid::new("director_keymap").show(ui, |ui| {
        for binding in map.effective() {
            ui.label(binding.chord.describe());
            if ui.button(binding.action.label()).clicked() {
                keymap::run(binding.action);
            }
            ui.end_row();
        }
    });
    ui.label("Edit director.keymap in the settings file to rebind (virtual-key codes).");
    if ui.button("Reset to defaults").clicked() {
        keymap::reset_to_defaults();
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}
