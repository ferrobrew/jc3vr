//! Runtime controls and a separate read-only OpenXR status snapshot.

use crate::{config, headpose, vr};

pub fn egui_debug_vr(ui: &mut egui::Ui) {
    // Live-editable, mutating the shared config directly (the frame loop reads it each frame), the
    // same pattern the other tabs use.
    let mut cfg = config::edit();
    ui.checkbox(&mut cfg.vr.enabled, "Enabled (bring up the OpenXR session)")
        .on_hover_text("Off leaves the mod in flatscreen stereo and tears any live runtime down.");
    ui.checkbox(&mut cfg.vr.native_resolution, "Native per-eye resolution")
        .on_hover_text(
            "Drive the engine to render each eye at the HMD-recommended resolution; disabled \
             automatically on a resize fault.",
        );
    ui.checkbox(&mut cfg.vr.mirror, "Desktop mirror")
        .on_hover_text("Show one eye in the game window while a session runs.");
    ui.add_enabled_ui(cfg.vr.mirror, |ui| {
        ui.indent("mirror", |ui| {
            ui.horizontal(|ui| {
                ui.label("Eye");
                ui.selectable_value(&mut cfg.vr.mirror_eye, 0, "Left");
                ui.selectable_value(&mut cfg.vr.mirror_eye, 1, "Right");
            });
        });
    });
    ui.checkbox(&mut cfg.vr.own_back_buffer, "Mod-owned back buffer")
        .on_hover_text(
            "Render into a mod-owned target so the DXGI swapchain stays at the window size instead \
             of following the per-eye render resolution. Makes the desktop mirror's present 1:1 and \
             stops single-pass double-wide forcing a 2x-width swapchain. Takes effect on the next \
             resize, which toggling this issues.",
        );
    ui.checkbox(&mut cfg.vr.direct_eye_blit, "Direct eye blit")
        .on_hover_text(
            "Blit each eye straight from the engine's back buffer into the headset swapchain on \
             the eye's own draw, skipping the full-size copy into the eye capture unless the \
             mirror, a screenshot, the recorder, or the previews need that eye. Off copies every \
             eye and blits at the frame submit.",
        );
}

pub(crate) fn status(ui: &mut egui::Ui) {
    let status = vr::status();

    ui.label(format!("Session: {}", session_label(&status)));
    ui.label(match &status.runtime_name {
        Some(name) => format!("Runtime: {name}"),
        None => "Runtime: (none)".to_string(),
    });
    ui.label(match status.eye_resolution {
        Some((w, h)) => format!("Per-eye resolution: {w} × {h}"),
        None => "Per-eye resolution: (no session)".to_string(),
    });
    ui.label(format!("Headpose source: {:?}", headpose::source()));
}

/// A human-readable label for the runtime's current session state.
fn session_label(status: &vr::VrStatus) -> &'static str {
    if status.busy {
        // The runtime lock was held by the frame tail; the rest of the snapshot is not meaningful.
        return "(busy -- the frame tail holds the runtime lock)";
    }
    if !status.enabled {
        "disabled (flatscreen stereo)"
    } else if status.running {
        "running"
    } else if status.instance_up {
        "instance up, session idle"
    } else {
        "no runtime (retrying)"
    }
}
