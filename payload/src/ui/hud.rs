//! HUD controls, renderer-backed texture previews, and renderer-free published status readouts.

use std::time::Instant;

use parking_lot::Mutex;

use crate::{
    config,
    hud::{
        HUD_STATE, StageSafeAreaReadout,
        config::{CropExtents, WristVisibility},
        scaleform::request_set_clip_visible,
        stage_safe_area_readout,
        world_markers::layout::StageFrame,
    },
};

/// Preview size (px) for the redirected HUD texture, independent of the Render tab's preview size.
static HUD_PREVIEW_WIDTH: Mutex<f32> = Mutex::new(512.0);

/// The clip path the Scaleform visibility controls operate on. Editable so paths from a tree dump
/// can be tried live without a rebuild.
static SCALEFORM_CLIP_PATH: Mutex<String> = Mutex::new(String::new());

pub fn egui_debug_hud(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    let live_distance = HUD_STATE.lock().depth_status().map(|status| {
        let distance = status
            .smoothed
            .map_or("-".to_string(), |d| format!("{d:.2} m"));
        format!("Last applied distance: {distance}. The policy readout is in Live status.")
    });

    // Redirect toggle and the quad placement/follow parameters. The CONFIG lock is scoped to this
    // block and dropped before HUD_STATE is locked for the preview.
    let redirect = {
        let mut cfg = config::edit();
        ui.checkbox(
            &mut cfg.hud.redirect,
            "Redirect HUD into our texture (drops it from the scene composite)",
        );
        ui.add_enabled_ui(cfg.hud.redirect, |ui| {
            ui.add(egui::Slider::new(&mut cfg.hud.hud_aspect, 0.5..=2.5).text("HUD aspect (w/h)"));
            ui.add(
                egui::Slider::new(&mut cfg.hud.movie_aspect, 0.5..=2.5).text("Movie aspect (w/h)"),
            );
            let mut pinned = cfg.hud.render_resolution.is_some();
            ui.horizontal(|ui| {
                ui.checkbox(&mut pinned, "Pin resolution")
                    .on_hover_text(
                        "Set the HUD texture's longer axis in pixels outright, instead of scaling it \
                         from the per-eye render resolution. The HUD is a fixed-apparent-size panel, \
                         so its texel budget need not follow the scene's.",
                    );
                if pinned {
                    let mut pixels = cfg.hud.render_resolution.unwrap_or(3072);
                    ui.add(egui::Slider::new(&mut pixels, 512..=8192).text("px"));
                    cfg.hud.render_resolution = Some(pixels);
                } else {
                    cfg.hud.render_resolution = None;
                }
            });
            ui.add_enabled_ui(!pinned, |ui| {
                ui.add(
                    egui::Slider::new(&mut cfg.hud.render_scale, 0.1..=2.0)
                        .text("Render scale (x)"),
                )
                .on_hover_text(
                    "Multiplies the geometric mean of the per-eye render resolution to give the HUD \
                     texture's longer axis.",
                );
            });
            ui.checkbox(&mut cfg.hud.quad, "Draw the HUD as a floating quad per eye");
            ui.checkbox(
                &mut cfg.hud.suppress_overlays,
                "Suppress full-screen overlays (damage flash, drowning)",
            );
            ui.checkbox(
                &mut cfg.hud.world_lock_menus,
                "World-lock panel in menus (pause/map stay put instead of head-following)",
            );
            ui.horizontal(|ui| {
                ui.label("Reticle align:");
                ui.radio_value(
                    &mut cfg.hud.reticle_align,
                    crate::hud::ReticleAlign::PanelSubtense,
                    "Panel subtense",
                )
                .on_hover_text(
                    "Default: land the reticle where the head->aim ray crosses the panel",
                );
                ui.radio_value(
                    &mut cfg.hud.reticle_align,
                    crate::hud::ReticleAlign::GameProjection,
                    "Game projection",
                )
                .on_hover_text("A/B: follow the game's own screen-space aim mapping");
            });
            ui.checkbox(
                &mut cfg.hud.cursor.enabled,
                "Virtual mouse cursor on the panel (remaps mouse-to-UI coordinates)",
            );
            if cfg.hud.cursor.enabled {
                ui.indent("hud_cursor", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.cursor.size, 0.004..=0.05)
                            .text("Cursor size (fraction of distance)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.cursor.lift, 0.0..=0.2)
                            .text("Cursor lift off the panel (m)"),
                    );
                });
            }
            ui.checkbox(
                &mut cfg.hud.marker_warp,
                "Warp the panel to per-element world depths (markers + center bubble)",
            );
            if cfg.hud.marker_warp {
                ui.indent("hud_warp", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_radius, 0.01..=0.3)
                            .text("Marker warp radius (uv)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_max_depth, 10.0..=1000.0)
                            .logarithmic(true)
                            .text("Marker depth clamp (m)"),
                    );
                    ui.checkbox(
                        &mut cfg.hud.center_depth_from_aim,
                        "Drive the center bubble's depth from the aim point",
                    );
                    if cfg.hud.center_depth_from_aim {
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.center_bubble_radius, 0.01..=0.4)
                                .text("Center bubble radius (uv)"),
                        );
                    }
                });
            }
            ui.checkbox(
                &mut cfg.hud.split,
                "PARKED: split the HUD into depth layers (breaks the UI on pause)",
            );
            if cfg.hud.split {
                ui.indent("hud_split", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_distance, 0.3..=50.0)
                            .logarithmic(true)
                            .text("Marker layer distance (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.center_distance, 0.3..=10.0)
                            .text("Center layer distance (m)"),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Clip path prefix");
                        let mut prefix = cfg.hud.split_path_prefix.as_str().to_string();
                        if ui.text_edit_singleline(&mut prefix).changed()
                            && let Err(e) = cfg.hud.split_path_prefix.set(&prefix)
                        {
                            tracing::warn!("{e}");
                        }
                    });
                });
            }
            ui.checkbox(
                &mut cfg.hud.depth_shift.enabled,
                "Dynamic distance from the scene depth distribution",
            );
            if cfg.hud.depth_shift.enabled {
                ui.indent("hud_depth_shift", |ui| {
                    ui.checkbox(
                        &mut cfg.hud.depth_shift.use_depth_histogram,
                        "EXPERIMENTAL: drive from the depth histogram instead of the vehicle \
                         state",
                    );
                    if cfg.hud.depth_shift.use_depth_histogram {
                        ui.checkbox(
                            &mut cfg.hud.depth_shift.mask_by_hud,
                            "Weight samples by the HUD's alpha on the panel",
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.min_depth, 0.0..=2.0)
                                .text("Ignore depths below (m)"),
                        );
                    }
                    if cfg.hud.depth_shift.use_depth_histogram && cfg.hud.depth_shift.continuous {
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.percentile, 0.01..=0.5)
                                .text("Percentile"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.margin, 0.0..=2.0)
                                .text("Margin inside (m)"),
                        );
                    } else if cfg.hud.depth_shift.use_depth_histogram {
                        ui.checkbox(
                            &mut cfg.hud.depth_shift.continuous,
                            "EXPERIMENTAL: follow the depth percentile continuously",
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.near_threshold, 0.3..=10.0)
                                .text("Near-field threshold (m)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.near_occupancy, 0.01..=0.9)
                                .text("Near occupancy to engage"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.hysteresis, 0.0..=0.3)
                                .text("Release hysteresis"),
                        );
                    }
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.depth_shift.near_distance, 0.3..=3.0)
                            .text("Near distance (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.depth_shift.halflife, 0.05..=2.0)
                            .text("Easing halflife (s)"),
                    );
                });
            }
            if cfg.hud.quad {
                ui.indent("hud_sliders", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.distance, 0.3..=10.0)
                            .text("Base distance (m)"),
                    )
                    .on_hover_text(
                        live_distance
                            .as_deref()
                            .unwrap_or("Live distance has not been sampled yet."),
                    );
                    ui.add(egui::Slider::new(&mut cfg.hud.panel_scale, 0.2..=3.0).text("Size (x)"));
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.rotation_halflife, 0.01..=2.0)
                            .text("Rotation halflife (s)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.position_halflife, 0.01..=1.0)
                            .text("Position halflife (s)"),
                    );
                });
            }
        });

        // The interactive egui floating panel (issue #24): independent of the redirect, only takes
        // effect while an OpenXR session is running. Toggle with F8 in-headset.
        ui.separator();
        ui.checkbox(
            &mut cfg.hud.egui_panel.enabled,
            "Interactive egui panel floating in VR (F8; only while a session runs)",
        );
        if cfg.hud.egui_panel.enabled {
            ui.indent("egui_panel", |ui| {
                ui.add(
                    egui::Slider::new(&mut cfg.hud.egui_panel.distance, 0.3..=5.0)
                        .text("Panel summon distance (m)"),
                )
                .on_hover_text(
                    "How far in front of the head the panel appears when it is first shown or \
                     recalled.",
                );
                ui.add(
                    egui::Slider::new(&mut cfg.hud.egui_panel.scale, 0.2..=3.0)
                        .text("Panel size (x)"),
                )
                .on_hover_text("The panel's apparent size at the summon distance.");
                ui.checkbox(
                    &mut cfg.hud.egui_panel.show_on_mirror,
                    "Show panel on desktop mirror (off = headset only)",
                );
                ui.checkbox(
                    &mut cfg.hud.egui_panel.controller_pointer,
                    "Point at the panel with the controllers",
                )
                .on_hover_text(
                    "The trigger clicks, B right-clicks, and the stick scrolls. The grip grabs the \
                     panel, and the stick then pushes and pulls it.",
                );
                ui.add(
                    egui::Slider::new(&mut cfg.hud.egui_panel.click_slop, 0.0..=0.15)
                        .text("Controller click slop (panel heights)"),
                )
                .on_hover_text(
                    "How far a held trigger press may wander and still click. Past it, the press \
                     drags from where the ray points.",
                );
                ui.add(
                    egui::Slider::new(&mut cfg.hud.egui_panel.click_duration_s, 0.3..=5.0)
                        .text("Panel click duration (s)"),
                )
                .on_hover_text("How long a press may be held on the panel and still click.");
            });
        }

        // The flat egui overlay on the desktop window (direct back-buffer or deferred mirror-overlay
        // path). Toggle with F4; the VR floating panel above is unaffected.
        ui.separator();
        ui.checkbox(
            &mut cfg.hud.flat_overlay_visible,
            "Flat debug overlay on the desktop window (F4)",
        );

        cfg.hud.redirect
    };

    if redirect {
        // Preview matches the current mode's effective aspect, so it tracks the live texture shape.
        let aspect = crate::hud::current_aspect();
        let preview_width = {
            let mut w = HUD_PREVIEW_WIDTH.lock();
            ui.add(egui::Slider::new(&mut *w, 48.0..=4096.0).text("Preview size (px)"));
            *w
        };
        let mut hud = HUD_STATE.lock();
        ui.collapsing("HUD texture", |ui| match hud.preview_id(renderer) {
            Some(id) => {
                // The preview matches the HUD aspect (width / height).
                let size = egui::vec2(preview_width, preview_width / aspect.max(f32::EPSILON));
                ui.add(egui::Image::new(egui::ImageSource::Texture(
                    egui::load::SizedTexture { id, size },
                )));
            }
            None => {
                ui.label("(redirect not yet applied)");
            }
        });
        let split_enabled = crate::config::Config::lock_query(|c| c.hud.split);
        if split_enabled {
            ui.collapsing("Split layer textures", |ui| {
                let ids = hud.layer_preview_ids(renderer);
                let size = egui::vec2(preview_width, preview_width / aspect.max(f32::EPSILON));
                for (id, label) in ids.iter().zip(["Markers", "Center"]) {
                    ui.label(label);
                    match id {
                        Some(id) => {
                            ui.add(egui::Image::new(egui::ImageSource::Texture(
                                egui::load::SizedTexture { id: *id, size },
                            )));
                        }
                        None => {
                            ui.label("(layer target not created)");
                        }
                    }
                }
            });
        }
    }

    world_markers_ui(ui);
    atlas_ui(ui);
    scaleform_debug_ui(ui);
    pause_menu_ui(ui);
}

pub(crate) fn status(ui: &mut egui::Ui) {
    let depth_status = HUD_STATE.lock().depth_status();
    let use_depth_histogram =
        config::Config::lock_query(|cfg| cfg.hud.depth_shift.use_depth_histogram);
    ui.heading("Dynamic distance");
    match depth_status {
        Some(status) => {
            let state = if status.near_engaged { "near" } else { "base" };
            let distance = status
                .smoothed
                .map_or("-".to_string(), |d| format!("{d:.2} m"));
            match status.stats {
                // The histogram statistics only exist while that policy runs.
                Some(stats) if use_depth_histogram => {
                    ui.label(format!(
                        "Live: {state} at {distance} (near {:.0}%, p{:.2} m)",
                        stats.near_occupancy * 100.0,
                        stats.percentile_depth,
                    ));
                }
                _ if use_depth_histogram => {
                    ui.label(format!(
                        "Live: {state} at {distance} (no depth samples yet)"
                    ));
                }
                _ => {
                    ui.label(format!(
                        "Live: {state} at {distance} ({})",
                        if status.near_engaged {
                            "in a vehicle"
                        } else {
                            "not in a vehicle"
                        },
                    ));
                }
            }
        }
        None => {
            ui.label("Live: not sampled yet");
        }
    }
    ui.separator();
    ui.heading("World markers");
    world_markers_status_ui(ui);
    ui.separator();
    ui.heading("Stage safe area");
    stage_safe_area_status_ui(ui);
    ui.separator();
    ui.heading("HUD atlas");
    atlas_status_ui(ui);
    ui.separator();
    ui.heading("Pause menu");
    ui.label(format!("State: {:?}", crate::pause_menu::state()));
}

/// The world-marker prototype's settings and its experimental safe-area control.
fn world_markers_ui(ui: &mut egui::Ui) {
    ui.collapsing("World markers (prototype)", |ui| {
        let mut cfg = config::edit();
        let m = &mut cfg.hud.world_markers;
        ui.checkbox(
            &mut m.enabled,
            "Draw on-screen markers at their world points instead of on the panel",
        );
        ui.checkbox(
            &mut m.classify_with_headset_fov,
            "Classify on-screen markers with the headset's field of view",
        );
        ui.add(
            egui::Slider::new(&mut m.fov_inset_deg, 0.0..=15.0).text("Field-of-view inset (deg)"),
        );
        ui.add(
            egui::Slider::new(&mut m.fallback_half_fov_deg, 20.0..=80.0)
                .text("Fallback half field of view, no headset (deg)"),
        );
        ui.checkbox(&mut m.hide_from_panel, "Cut the markers out of the panel");
        ui.checkbox(
            &mut m.surface_placement,
            "Place markers on the nearest visible surface along the line of sight",
        );
        if m.surface_placement {
            ui.indent("world_markers_surface", |ui| {
                ui.checkbox(
                    &mut m.ray_fallback,
                    "Raycast where no depth sample is available",
                );
                ui.add(
                    egui::Slider::new(&mut m.surface_pull_m, 0.0..=2.0)
                        .text("Pull toward the eye (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut m.surface_min_m, 0.0..=5.0)
                        .text("Ignore surfaces nearer than (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut m.depth_halflife_s, 0.0..=1.0)
                        .text("Distance smoothing halflife (s)"),
                );
            });
        }
        ui.add(
            egui::Slider::new(&mut m.max_distance_m, 10.0..=2000.0)
                .logarithmic(true)
                .text("Farthest placement (m)"),
        );
        ui.label("Crop around the clip origin (stage px)");
        ui.indent("world_markers_crop", |ui| {
            ui.add(egui::Slider::new(&mut m.crop.left, 0.0..=400.0).text("Left"));
            ui.add(egui::Slider::new(&mut m.crop.right, 0.0..=400.0).text("Right"));
            ui.add(egui::Slider::new(&mut m.crop.up, 0.0..=200.0).text("Up"));
            ui.add(egui::Slider::new(&mut m.crop.down, 0.0..=200.0).text("Down"));
            ui.add(
                egui::Slider::new(&mut m.label_extra_left, 0.0..=800.0)
                    .text("Extra left for the labelled marker"),
            );
        });
        ui.checkbox(
            &mut m.probe_stage_safe_area,
            "EXPERIMENT: lay the gameplay HUD out inside the stage (atlas probe)",
        );
    });
}

fn stage_safe_area_status_ui(ui: &mut egui::Ui) {
    let sample = match stage_safe_area_readout() {
        StageSafeAreaReadout::Unavailable => {
            ui.label("Unavailable: no main UI movie capture has published the stage safe area.");
            return;
        }
        StageSafeAreaReadout::Busy => {
            ui.label(
                "Unavailable: the stage safe-area publication is busy; retry on the next frame.",
            );
            return;
        }
        StageSafeAreaReadout::Captured(sample) => sample,
    };
    let now = Instant::now();
    ui.label(if sample.fresh(now) {
        "Capture: recent main UI movie sample."
    } else {
        "Capture: stale; the main UI movie has not captured recently."
    });
    let Some(layout) = sample.layout else {
        ui.label("Unavailable: the captured stage safe-area layout is invalid.");
        return;
    };
    let [x1, y1, x2, y2] = layout.bounds;
    ui.label(format!(
        "Safe area ({x1:.0}, {y1:.0})-({x2:.0}, {y2:.0}), {:.0}% inset, stage {:.0}x{:.0}",
        layout.percentage * 100.0,
        layout.stage.x,
        layout.stage.y,
    ));
    if let Some(frame) = StageFrame::new(layout.stage, layout.aspect) {
        ui.label(format!(
            "Visible frame ({:.0}, {:.0}) {:.0}x{:.0} at aspect {:.2}: band {:.0} \
             stage px above and below the stage; the safe area reaches {:.0} px into the \
             top band",
            frame.origin.x,
            frame.origin.y,
            frame.size.x,
            frame.size.y,
            layout.aspect,
            frame.vertical_band(),
            (-y1).max(0.0),
        ));
    } else {
        ui.label("Unavailable: the captured stage has no valid visible frame.");
    }
}

fn world_markers_status_ui(ui: &mut egui::Ui) {
    match crate::hud::world_markers::status() {
        None => {
            ui.label("Not drawing (off, outside gameplay, or the panel is off).");
        }
        Some(status) => {
            ui.label(format!(
                "Layout snapshot {:?}, displayed {} ({}); {} markers, {} past the cap; \
                 labelled POI {}",
                status.layout_snapshot,
                status.displayed_snapshot,
                if status.layout_matched {
                    "matched"
                } else {
                    "newest layout"
                },
                status.markers.len(),
                status.skipped,
                status.labelled_id,
            ));
            ui.label(format!(
                "Stage {:.0}x{:.0}, HUD texture {}x{}",
                status.stage.x, status.stage.y, status.texture.0, status.texture.1
            ));
            egui::Grid::new("world_markers_grid")
                .striped(true)
                .show(ui, |ui| {
                    for header in [
                        "id", "world", "origin", "texels", "true", "drawn", "by", "from",
                    ] {
                        ui.label(header);
                    }
                    ui.end_row();
                    for m in &status.markers {
                        ui.label(m.id.to_string());
                        ui.label(format!(
                            "{:.1}, {:.1}, {:.1}",
                            m.world.x, m.world.y, m.world.z
                        ));
                        ui.label(format!("{:.0}, {:.0}", m.origin.x, m.origin.y));
                        ui.label(format!(
                            "{}..{} x {}..{}",
                            m.rect.0, m.rect.2, m.rect.1, m.rect.3
                        ));
                        ui.label(format!("{:.1} m", m.true_distance));
                        ui.label(format!("{:.1} m", m.drawn_distance));
                        ui.label(format!("{:?}", m.source));
                        ui.label(if m.in_cell { "cell" } else { "panel" });
                        ui.end_row();
                    }
                });
        }
    }
}

/// The HUD atlas's settings, including the hand and wrist readouts.
fn atlas_ui(ui: &mut egui::Ui) {
    ui.collapsing(
        "HUD atlas: hand ammunition, wrist status, marker cells",
        |ui| {
            let mut cfg = config::edit();
            let a = &mut cfg.hud.atlas;
            ui.checkbox(
                &mut a.enabled,
                "Move HUD parts into cells above and below the stage, and draw them on their own \
                 quads",
            );
            ui.add(egui::Slider::new(&mut a.gutter_px, 0.0..=24.0).text("Cell gutter (stage px)"));
            ui.checkbox(
                &mut a.markers,
                "Move the world markers into cells (otherwise they are cropped from the panel)",
            );
            ui.separator();
            ui.checkbox(&mut a.ammo.enabled, "Weapon readout above the gun hand");
            ui.indent("atlas_ammo", |ui| {
                extents_ui(ui, &mut a.ammo.extents, "atlas_ammo_extents");
                ui.add(egui::Slider::new(&mut a.ammo.max_scale, 0.1..=1.0).text("Largest scale"));
                ui.add(
                    egui::Slider::new(&mut a.ammo.clearance_m, 0.0..=0.3)
                        .text("Lower edge above the grip (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut a.ammo.pull_m, 0.0..=0.2)
                        .text("Pull toward the head (m)"),
                );
                ui.add(egui::Slider::new(&mut a.ammo.width_m, 0.03..=0.3).text("Width (m)"));
                ui.add(
                    egui::Slider::new(&mut a.ammo.follow_halflife_s, 0.0..=0.3)
                        .text("Follow halflife (s)"),
                );
                ui.add(
                    egui::Slider::new(&mut a.ammo.linger_s, 0.0..=5.0)
                        .text("Linger after the gun leaves the hand (s)"),
                );
            });
            ui.separator();
            ui.checkbox(
                &mut a.cross.enabled,
                "Weapon selection cross at the hand steering the weapon wheel",
            );
            ui.indent("atlas_cross", |ui| {
                ui.add(
                    egui::Slider::new(&mut a.cross.margin_px, 0.0..=60.0)
                        .text("Margin around its extent (clip px)"),
                );
                ui.add(egui::Slider::new(&mut a.cross.max_scale, 0.1..=1.0).text("Largest scale"));
                ui.add(egui::Slider::new(&mut a.cross.width_m, 0.05..=0.5).text("Width (m)"));
                ui.add(
                    egui::Slider::new(&mut a.cross.pull_m, 0.0..=0.3)
                        .text("Pull from the hand toward the head (m)"),
                );
            });
            ui.separator();
            ui.checkbox(
                &mut a.tether.enabled,
                "Tether readout above the off-hand's forearm",
            );
            ui.indent("atlas_tether", |ui| {
                extents_ui(ui, &mut a.tether.extents, "atlas_tether_extents");
                ui.add(egui::Slider::new(&mut a.tether.max_scale, 0.1..=1.0).text("Largest scale"));
                ui.add(
                    egui::Slider::new(&mut a.tether.back_m, 0.0..=0.3)
                        .text("Back along the forearm from the wrist (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut a.tether.clearance_m, 0.0..=0.2)
                        .text("Lower edge above the forearm (m)"),
                );
                ui.add(egui::Slider::new(&mut a.tether.width_m, 0.03..=0.3).text("Width (m)"));
                ui.add(
                    egui::Slider::new(&mut a.tether.follow_halflife_s, 0.0..=0.3)
                        .text("Follow halflife (s)"),
                );
            });
            ui.separator();
            let w = &mut a.wrist;
            ui.checkbox(
                &mut w.enabled,
                "Health and heat on the inside of the off-hand's wrist",
            );
            ui.indent("atlas_wrist", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Shows");
                    for (value, label) in [
                        (
                            WristVisibility::Auto,
                            "on a glance, low health, damage, or heat",
                        ),
                        (WristVisibility::Glance, "on a glance"),
                        (WristVisibility::Always, "always"),
                    ] {
                        ui.radio_value(&mut w.visibility, value, label);
                    }
                });
                ui.checkbox(&mut w.health, "Health bar");
                ui.checkbox(&mut w.heat, "Heat readout");
                extents_ui(ui, &mut w.heat_extents, "atlas_heat_extents");
                ui.add(
                    egui::Slider::new(&mut w.heat_max_scale, 0.1..=1.0).text("Largest heat scale"),
                );
                vec3_ui(
                    ui,
                    &mut w.offset_m,
                    "Offset in the grip, +X out of the palm (m)",
                    0.3,
                );
                ui.add(egui::Slider::new(&mut w.width_m, 0.03..=0.2).text("Width (m)"));
                ui.add(egui::Slider::new(&mut w.bar_aspect, 2.0..=20.0).text("Health bar aspect"));
                ui.add(egui::Slider::new(&mut w.low_health, 0.0..=1.0).text("Low health"));
                ui.add(
                    egui::Slider::new(&mut w.after_damage_s, 0.0..=15.0)
                        .text("Show after damage (s)"),
                );
                ui.add(
                    egui::Slider::new(&mut w.glance.view_cone_deg, 5.0..=60.0)
                        .text("Glance view cone (deg)"),
                );
                ui.add(
                    egui::Slider::new(&mut w.glance.facing_deg, 5.0..=90.0)
                        .text("Glance facing (deg)"),
                );
                ui.add(egui::Slider::new(&mut w.fade_s, 0.0..=1.0).text("Fade (s)"));
            });
        },
    );
}

fn atlas_status_ui(ui: &mut egui::Ui) {
    let status = crate::hud::atlas::status();
    ui.label(format!(
        "Moves {}, restores {}, move failures {}, restore failures {}, drift {}",
        status.moves, status.restores, status.move_failures, status.restore_failures, status.drift,
    ));
    match &status.frame {
        None => {
            ui.label("Not drawing (off, outside gameplay, or the stage layout is not applied).");
        }
        Some(frame) => {
            ui.label(format!(
                "Layout snapshot {}, displayed {} ({}); HUD texture {}x{}",
                frame.snapshot,
                frame.displayed_snapshot,
                if frame.matched {
                    "matched"
                } else {
                    "newest layout"
                },
                frame.texture.0,
                frame.texture.1,
            ));
            let band = frame.layout.bands[0];
            ui.label(format!(
                "Bands {:.0}x{:.0} stage px above and below the stage; {} marker cells, \
                 label cell {}",
                band.size.x,
                band.size.y,
                frame.layout.markers.len(),
                if frame.layout.label.is_some() {
                    "placed"
                } else {
                    "none"
                },
            ));
            for (name, cell) in [
                ("Weapon readout", frame.ammo),
                ("Heat readout", frame.heat),
                ("Selection cross", frame.cross),
                ("Tether readout", frame.tether),
            ] {
                ui.label(match cell {
                    Some(cell) => format!(
                        "{name}: cell ({:.0}, {:.0}) {:.0}x{:.0} at scale {:.2}",
                        cell.rect.min.x,
                        cell.rect.min.y,
                        cell.rect.size.x,
                        cell.rect.size.y,
                        cell.scale,
                    ),
                    None => format!("{name}: not moved"),
                });
            }
        }
    }
    let anchored = crate::hud::anchored::status();
    ui.label(format!(
        "Weapon readout on {}; health {}; heat level {}; wrist {:.0}% (glance {}, attention \
         {}, heat {})",
        anchored
            .ammo_hand
            .map_or("no hand".to_owned(), |hand| format!("{hand:?}")),
        anchored
            .health
            .map_or("not sampled".to_owned(), |(health, trail)| format!(
                "{:.0}% (trail {:.0}%)",
                health * 100.0,
                trail * 100.0
            )),
        anchored.heat_level,
        anchored.wrist_alpha * 100.0,
        anchored.glance,
        anchored.attention,
        if anchored.heat_drawn {
            "drawn"
        } else {
            "not drawn"
        },
    ));
    ui.label(match anchored.cross_alpha {
        Some(alpha) => format!(
            "Selection cross at the steering hand, {:.0}%",
            alpha * 100.0
        ),
        None => "Selection cross not drawn at a hand".to_owned(),
    });
    ui.label(if anchored.tether_drawn {
        "Tether readout above the forearm"
    } else {
        "Tether readout not drawn"
    });
}

/// Sliders for a part's extents around its clip's origin.
fn extents_ui(ui: &mut egui::Ui, extents: &mut CropExtents, id: &str) {
    ui.label("Extents around the clip origin (clip px)");
    ui.indent(id, |ui| {
        ui.add(egui::Slider::new(&mut extents.left, 0.0..=600.0).text("Left"));
        ui.add(egui::Slider::new(&mut extents.right, 0.0..=600.0).text("Right"));
        ui.add(egui::Slider::new(&mut extents.up, 0.0..=400.0).text("Up"));
        ui.add(egui::Slider::new(&mut extents.down, 0.0..=400.0).text("Down"));
    });
}

/// Sliders for a small offset, each axis within `range` metres.
fn vec3_ui(ui: &mut egui::Ui, value: &mut glam::Vec3, label: &str, range: f32) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (axis, name) in [
            (&mut value.x, "X"),
            (&mut value.y, "Y"),
            (&mut value.z, "Z"),
        ] {
            ui.add(egui::Slider::new(axis, -range..=range).text(name));
        }
    });
}

/// The VR menu's toggle.
fn pause_menu_ui(ui: &mut egui::Ui) {
    ui.collapsing("Pause menu", |ui| {
        let mut cfg = config::edit();
        ui.checkbox(&mut cfg.pause_menu.enabled, "VR pages in the pause menu")
            .on_hover_text(
                "Adds a VR entry below Options in the pause menu, opening the game's own settings \
             screen with a tab per VR page. Takes effect the next time the pause menu opens.",
            );
    });
}

/// The Scaleform display-tree debug controls: dump the live clip tree to the log, and toggle a
/// clip's `_visible` by path. Requests are queued here and executed on the game thread.
fn scaleform_debug_ui(ui: &mut egui::Ui) {
    ui.collapsing("Scaleform display tree", |ui| {
        if ui
            .button("Auto-configure split from display tree")
            .on_hover_text(
                "Finds the HUD clip in the live tree, sets the split path prefix, and \
                     collects the anonymous POI pool for the markers layer.",
            )
            .clicked()
        {
            crate::hud::scaleform::request_layout_discovery();
        }
        if ui
            .button("Dump display tree to log")
            .on_hover_text(
                "Walks the live movie's clip tree on the game thread and logs one line per \
                     clip, as dot-joined paths.",
            )
            .clicked()
        {
            crate::hud::scaleform::request_dump_tree();
        }
        ui.horizontal(|ui| {
            let mut path = SCALEFORM_CLIP_PATH.lock();
            if path.is_empty() {
                *path = "MCI_poi_stage".to_string();
            }
            ui.label("Clip path");
            ui.text_edit_singleline(&mut *path);
            if ui.button("Hide").clicked() {
                request_set_clip_visible(path.clone(), false);
            }
            if ui.button("Show").clicked() {
                request_set_clip_visible(path.clone(), true);
            }
        });
    });
}
