//! HUD controls, renderer-backed texture previews, and renderer-free published status readouts.

use std::time::Instant;

use crate::{
    config,
    hud::{
        HUD_STATE, StageSafeAreaReadout, aim, stage_safe_area_readout,
        world_markers::layout::StageFrame,
    },
};

/// Preview width (px) for the redirected HUD texture, independent of the Previews and performance feature's preview size.
const HUD_PREVIEW_WIDTH: f32 = 512.0;

pub fn egui_debug_hud(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    // The CONFIG lock is scoped to this block and dropped before HUD_STATE is locked for the
    // preview.
    let redirect = {
        let mut cfg = config::edit();
        let hud = &mut cfg.hud;
        ui.checkbox(
            &mut hud.redirect,
            "Redirect HUD into our texture (drops it from the scene composite)",
        );
        ui.add_enabled_ui(hud.redirect, |ui| {
            ui.checkbox(&mut hud.quad, "Draw the HUD as a floating quad per eye");
            ui.checkbox(
                &mut hud.depth_shift.enabled,
                "Dynamic distance: bring the panel nearer while riding a vehicle",
            );
            ui.checkbox(
                &mut hud.world_markers.enabled,
                "World markers: draw on-screen markers at their world points instead of on the panel",
            );
            ui.add_enabled_ui(hud.atlas.enabled, |ui| {
                let atlas = &mut hud.atlas;
                ui.checkbox(&mut atlas.ammo.enabled, "Ammunition counter above each gun hand");
                ui.checkbox(
                    &mut atlas.cross.enabled,
                    "Weapon selection cross at the hand steering the weapon wheel",
                );
                ui.checkbox(
                    &mut atlas.tether.enabled,
                    "Tether readout above the off-hand's forearm",
                );
                ui.checkbox(
                    &mut atlas.wrist.enabled,
                    "Health and heat on the inside of the off-hand's wrist",
                );
            })
            .response
            .on_disabled_hover_text("Turn on HUD on the hands in the pause menu's VR display page.");
        });
        ui.separator();
        ui.checkbox(
            &mut hud.egui_panel.controller_pointer,
            "Point at the VR debug panel with the controllers",
        )
        .on_hover_text(
            "The trigger clicks, B right-clicks, and the stick scrolls. The grip grabs the panel, \
             and the stick then pushes and pulls it.",
        );
        hud.redirect
    };

    if redirect {
        // The preview matches the current mode's effective aspect (width / height), so it tracks
        // the live texture shape.
        let aspect = crate::hud::current_aspect();
        let mut hud = HUD_STATE.lock();
        ui.collapsing("HUD texture", |ui| match hud.preview_id(renderer) {
            Some(id) => {
                let size = egui::vec2(
                    HUD_PREVIEW_WIDTH,
                    HUD_PREVIEW_WIDTH / aspect.max(f32::EPSILON),
                );
                ui.add(egui::Image::new(egui::ImageSource::Texture(
                    egui::load::SizedTexture { id, size },
                )));
            }
            None => {
                ui.label("(redirect not yet applied)");
            }
        });
    }
}

pub(crate) fn status(ui: &mut egui::Ui) {
    let depth_status = HUD_STATE.lock().depth_status();
    ui.heading("Dynamic distance");
    match depth_status {
        Some(status) => {
            let state = if status.near_engaged { "near" } else { "base" };
            let distance = status
                .smoothed
                .map_or("-".to_string(), |d| format!("{d:.2} m"));
            ui.label(format!(
                "Live: {state} at {distance} ({})",
                if status.near_engaged {
                    "in a vehicle"
                } else {
                    "not in a vehicle"
                },
            ));
        }
        None => {
            ui.label("Live: not sampled yet");
        }
    }
    ui.separator();
    ui.heading("Aim depth");
    ui.label(format!(
        "Live: {:.2} m (rests at {:.1} m while nothing is aimed at)",
        aim::smoothed(),
        aim::REST_DISTANCE,
    ));
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
                ("Reticles and hit marker", frame.aim),
                ("Grenade readout", frame.grenades),
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
        "Ammunition counters {}; health {}; heat level {}; wrist {:.0}% (glance {}, attention \
         {}, heat {}, grenades {})",
        match anchored.counts {
            [true, true] => "on both hands",
            [true, false] => "on the left hand",
            [false, true] => "on the right hand",
            [false, false] => "on no hand",
        },
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
        if anchored.grenades_drawn {
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
