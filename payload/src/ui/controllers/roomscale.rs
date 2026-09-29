//! Posture and roomscale: the posture in effect, the play area, the head height, the chase, the
//! teleport re-bases, the comfort fade, the crouch, and the seat phase, and their settings.

use egui::Ui;

use crate::{
    controllers::{
        config::{Posture, RoomscaleConfig, SeatTransition},
        roomscale::{
            self, RoomscaleStatus,
            posture::{EffectivePosture, HeadHeightSource},
            probe::ProbeResult,
        },
    },
    ui::controllers::{vec3, yes_no},
};

pub(super) fn roomscale_readout(ui: &mut Ui, status: &RoomscaleStatus) {
    if !status.active {
        ui.label("Inactive: no VR frame is rendering.");
    }
    let f = &status.frame;
    ui.label(format!("Posture: {}", f.posture.label()));
    ui.label(match (f.stage.available, f.stage.bounds) {
        (false, _) => "Stage: no STAGE space".to_owned(),
        (true, None) => "Stage: no play area bounds".to_owned(),
        (true, Some(b)) => format!("Stage: play area {:.2} × {:.2} m", b.x, b.y),
    });
    ui.label(match f.stage.head_height {
        Some(h) => format!("Head above the floor: {h:.2} m"),
        None => "Head above the floor: unknown".to_owned(),
    });
    ui.label(match f.head_height {
        Some(h) => format!(
            "Head height remap: eyes {:+.2} m from the character's ({})",
            h.cockpit_y,
            match h.source {
                HeadHeightSource::Floor => "from the floor",
                HeadHeightSource::Baseline => "from the recenter baseline",
            }
        ),
        None => "Head height remap: off".to_owned(),
    });
    ui.label(format!(
        "Eye drop {:.2} m · crouched {} · seated crouch {} · visual crouch {} · capsule {}{}",
        f.eye_drop_m,
        yes_no(f.crouched),
        yes_no(f.seated_crouch),
        yes_no(status.visual_crouch),
        yes_no(status.sim.capsule_owned),
        if status.sim.capsule_blocked {
            " (no headroom)"
        } else {
            ""
        },
    ));

    ui.separator();
    ui.label(format!(
        "Chase: offset ({:+.2}, {:+.2}) m · engaged {} · velocity ({:+.2}, {:+.2}) m/s · task ({:+.2}, {:+.2}) m/s · stick {:.2}",
        f.chase_offset.x,
        f.chase_offset.y,
        yes_no(status.sim.chase_engaged),
        status.sim.chase_velocity.x,
        status.sim.chase_velocity.y,
        status.sim.task_velocity.x,
        status.sim.task_velocity.y,
        status.sim.stick,
    ));
    ui.label(format!("Cockpit shift: {}", vec3(f.cockpit_shift)));
    ui.label(format!(
        "Teleports: {} ({} by distance) · re-based {}",
        status.teleports, status.warps, f.teleport_rebases
    ));

    ui.separator();
    ui.label(match f.probe {
        None => "Head probe: not running".to_owned(),
        Some(ProbeResult::Clear) => "Head probe: clear".to_owned(),
        Some(ProbeResult::Obstructed { depth }) => {
            format!("Head probe: {depth:.2} m into geometry")
        }
    });
    ui.label(format!(
        "Fade: level {:.2} · head {:.2} · seat {:.2}",
        status.fade.level, status.fade.head, status.fade.seat
    ));
    ui.label(format!(
        "Seat: {}{}",
        f.seat.label(),
        f.last_seat_event
            .map(|e| format!(" (last edge: {e:?})"))
            .unwrap_or_default()
    ));
    if f.posture == EffectivePosture::Seated && ui.button("Toggle seated crouch").clicked() {
        roomscale::toggle_seated_crouch();
    }
}

pub(super) fn roomscale_settings(ui: &mut Ui, r: &mut RoomscaleConfig) {
    ui.horizontal(|ui| {
        ui.label("Posture");
        ui.selectable_value(&mut r.posture, Posture::Auto, "Auto")
            .on_hover_text("Standing or roomscale, by the play area's size.");
        ui.selectable_value(&mut r.posture, Posture::Seated, "Seated");
        ui.selectable_value(&mut r.posture, Posture::Standing, "Standing");
        ui.selectable_value(&mut r.posture, Posture::Roomscale, "Roomscale");
    });
    ui.add(
        egui::Slider::new(&mut r.roomscale_min_extent_m, 0.5..=3.0)
            .text("Auto roomscale play area (m)"),
    );

    ui.checkbox(&mut r.chase.enabled, "Roomscale chase")
        .on_hover_text("In the roomscale posture, walk the body toward the physical head.");
    ui.add_enabled_ui(r.chase.enabled, |ui| {
        ui.indent("chase", |ui| {
            ui.add(egui::Slider::new(&mut r.chase.deadzone_m, 0.0..=0.5).text("Deadzone (m)"));
            ui.add(
                egui::Slider::new(&mut r.chase.max_speed_mps, 0.5..=8.0).text("Max speed (m/s)"),
            );
            ui.add(
                egui::Slider::new(&mut r.chase.time_constant_s, 0.03..=1.0)
                    .text("Time constant (s)"),
            );
            ui.add(
                egui::Slider::new(&mut r.chase.warp_min_m, 1.0..=50.0)
                    .text("Unannounced teleport distance (m)"),
            );
        });
    });

    let c = &mut r.crouch;
    ui.checkbox(&mut c.visual, "Visual crouch")
        .on_hover_text("Drop the hips and plant the feet when the head drops.");
    ui.add_enabled_ui(c.visual, |ui| {
        ui.indent("crouch", |ui| {
            ui.checkbox(&mut c.capsule, "Crouch capsule")
                .on_hover_text("Shorten the collision capsule while crouched.");
            ui.add(egui::Slider::new(&mut c.hips_share, 0.0..=1.0).text("Hips share of the drop"));
            ui.add(egui::Slider::new(&mut c.enter_drop_m, 0.1..=1.2).text("Crouched below (m)"));
            ui.add(egui::Slider::new(&mut c.exit_drop_m, 0.05..=1.2).text("Standing above (m)"));
        });
    });
    ui.add(egui::Slider::new(&mut c.seated_drop_m, 0.1..=1.0).text("Seated crouch drop (m)"));

    let h = &mut r.head_fade;
    ui.checkbox(&mut h.enabled, "Head-through-geometry fade");
    ui.add_enabled_ui(h.enabled, |ui| {
        ui.indent("head_fade", |ui| {
            ui.add(egui::Slider::new(&mut h.start_depth_m, 0.0..=0.5).text("Fade starts (m)"));
            ui.add(egui::Slider::new(&mut h.full_depth_m, 0.05..=1.0).text("Fully dark (m)"));
        });
    });
    ui.add(egui::Slider::new(&mut r.fade.out_s, 0.0..=1.0).text("Fade out (s)"));
    ui.add(egui::Slider::new(&mut r.fade.in_s, 0.0..=2.0).text("Fade in (s)"));

    ui.horizontal(|ui| {
        ui.label("Seat transition");
        ui.selectable_value(&mut r.seat_transition, SeatTransition::Instant, "Instant")
            .on_hover_text("Re-base at the seat lock; the entry plays behind a fade.");
        ui.selectable_value(&mut r.seat_transition, SeatTransition::Fade, "Fade")
            .on_hover_text("Fade through the entry and exit, clearing once seated or out.");
        ui.selectable_value(&mut r.seat_transition, SeatTransition::Ride, "Ride")
            .on_hover_text("Ride the animation with nothing hidden.");
    });
    ui.add_enabled(
        r.seat_transition == SeatTransition::Fade,
        egui::Slider::new(&mut r.seat_fade_hold_s, 0.0..=2.0).text("Seat fade hold (s)"),
    );
}
