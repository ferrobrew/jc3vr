//! Runtime controls and a separate read-only OpenXR status snapshot.

use jc3vr_render::{
    headpose, vr,
    vr::{EyeMask, MaskStatus, MirrorView, VR_CONFIG},
};

pub fn egui_debug_vr(ui: &mut egui::Ui) {
    // Live-editable, mutating the shared config directly (the frame loop reads it each frame), the
    // same pattern the other tabs use.
    let mut cfg = VR_CONFIG.edit();
    ui.checkbox(&mut cfg.enabled, "Enabled (bring up the OpenXR session)")
        .on_hover_text("Off leaves the mod in flatscreen stereo and tears any live runtime down.");
    ui.checkbox(&mut cfg.native_resolution, "Native per-eye resolution")
        .on_hover_text(
            "Drive the engine to render each eye at the HMD-recommended resolution; disabled \
             automatically on a resize fault.",
        );
    ui.checkbox(&mut cfg.mirror, "Desktop mirror")
        .on_hover_text("Show the headset view in the game window while a session runs.");
    ui.add_enabled_ui(cfg.mirror, |ui| {
        ui.indent("mirror", |ui| {
            ui.horizontal(|ui| {
                ui.label("View");
                ui.selectable_value(&mut cfg.mirror_view, MirrorView::Left, "Left");
                ui.selectable_value(&mut cfg.mirror_view, MirrorView::Right, "Right");
                ui.selectable_value(&mut cfg.mirror_view, MirrorView::StereoPair, "Stereo pair")
                    .on_hover_text(
                        "Both eyes side by side, uncropped, in cross-eyed order (the right eye on \
                     the left). F10 toggles between this and the last single eye.",
                    );
            });
            ui.checkbox(&mut cfg.mirror_frame_stamp, "Frame stamp while capturing")
                .on_hover_text(
                    "While a capture take runs, draw the mirror's present counter as black and \
                     white blocks in the window's bottom-left corner; the take's frames.jsonl \
                     logs the same counter per present.",
                );
        });
    });
    ui.checkbox(&mut cfg.own_back_buffer, "Mod-owned back buffer")
        .on_hover_text(
            "Render into a mod-owned target so the DXGI swapchain stays at the window size instead \
             of following the per-eye render resolution. Makes the desktop mirror's present 1:1 and \
             stops single-pass double-wide forcing a 2x-width swapchain. Takes effect on the next \
             resize, which toggling this issues.",
        );
    ui.checkbox(&mut cfg.direct_eye_blit, "Direct eye blit")
        .on_hover_text(
            "Blit each eye straight from the engine's back buffer into the headset swapchain on \
             the eye's own draw, skipping the full-size copy into the eye capture unless the \
             mirror, a screenshot, the recorder, or the previews need that eye. Off copies every \
             eye and blits at the frame submit.",
        );
    ui.checkbox(
        &mut cfg.overlap_render_update,
        "Overlap the render update with the draw",
    )
    .on_hover_text(
        "Start the next frame's runtime wait, view location, and render update while the previous \
         frame's draw task and frame tail are still running, waiting for the tail only where the \
         next frame begins its OpenXR frame. Off waits for the tail before the render update, the \
         serial ordering.",
    );
    ui.checkbox(
        &mut cfg.large_cache_cores,
        "Run on the cores with the largest cache",
    )
    .on_hover_text(
        "On a processor whose L3 caches differ in size (a two-die part with stacked cache on one), \
         keep the game's threads on the cores of the largest one, where the draw task runs far \
         faster. Left alone on a processor with one size of L3. Off puts the affinity back.",
    );
    ui.label(match jc3vr_core::cpu_placement::applied_mask() {
        Some(mask) => format!("Cores in use: {mask:#x}"),
        None => "Cores in use: as the scheduler places them".to_owned(),
    });
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
    visibility_mask(ui);
}

/// The runtime's hidden-area mesh per eye: its counts and share of the field of view, and an outline
/// of each eye's field-of-view rectangle with the hidden triangles drawn in it.
fn visibility_mask(ui: &mut egui::Ui) {
    ui.collapsing("Hidden-area mask", |ui| {
        match vr::visibility_mask_status() {
            MaskStatus::NotFetched => {
                ui.label("Not fetched yet (no located frame this session).");
            }
            MaskStatus::Unsupported => {
                ui.label("The runtime does not offer XR_KHR_visibility_mask.");
            }
            MaskStatus::Failed(error) => {
                ui.label(format!("The query failed: {error}"));
            }
            MaskStatus::Fetched(masks) => {
                ui.horizontal(|ui| {
                    for (eye, mask) in masks.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.label(format!(
                                "Eye {eye}: {} vertices, {} triangles, {:.1}% hidden",
                                mask.vertices.len(),
                                mask.indices.len() / 3,
                                100.0 * mask.hidden_fraction(),
                            ));
                            draw_mask(ui, mask);
                        });
                    }
                });
            }
        }
    });
}

/// Draw the field-of-view rectangle and the hidden triangles' outlines, mapped from the tangent
/// plane into a box of the rectangle's aspect, up at the top.
fn draw_mask(ui: &mut egui::Ui, mask: &EyeMask) {
    let [left, right, down, up] = mask.fov_tangents;
    let (width, height) = (right - left, up - down);
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let size = egui::vec2(MASK_PREVIEW_WIDTH, MASK_PREVIEW_WIDTH * height / width);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let to_screen = |[x, y]: [f32; 2]| {
        egui::pos2(
            rect.left() + (x - left) / width * rect.width(),
            rect.top() + (up - y) / height * rect.height(),
        )
    };
    let frame = egui::Stroke::new(1.0, egui::Color32::GRAY);
    painter.rect_stroke(rect, 0.0, frame, egui::StrokeKind::Inside);
    let edge = egui::Stroke::new(1.0, egui::Color32::LIGHT_RED);
    for triangle in mask.indices.as_chunks::<3>().0 {
        let points: Option<Vec<egui::Pos2>> = triangle
            .iter()
            .map(|&index| mask.vertices.get(index as usize).copied().map(to_screen))
            .collect();
        if let Some(points) = points {
            painter.add(egui::Shape::closed_line(points, edge));
        }
    }
}

/// The preview box's width in points.
const MASK_PREVIEW_WIDTH: f32 = 160.0;

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
