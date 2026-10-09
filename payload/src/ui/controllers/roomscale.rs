//! Posture and roomscale: the posture in effect, the play area, the head height, the chase, the
//! teleport re-bases, the comfort fade, the crouch, and the seat phase, and their settings.

use egui::Ui;

use crate::{
    controllers::{
        config::{Posture, RoomscaleConfig, SeatTransition},
        roomscale::{
            self, FrameStatus, RoomscaleStatus, posture::EffectivePosture, probe::ProbeResult,
        },
    },
    hooks::camera::height,
    ui::controllers::{stance, vec3, yes_no},
};

pub(super) fn roomscale_readout(ui: &mut Ui, status: &RoomscaleStatus) {
    if !status.active {
        ui.label("Inactive: no VR frame is rendering.");
    }
    camera_height_readout(ui);
    ui.separator();
    let f = &status.frame;
    ui.label(format!(
        "Posture: {} (setting {:?})",
        f.posture.label(),
        f.setting
    ));
    posture_inputs(ui, f);
    ui.label(match (f.stage.available, f.stage.bounds) {
        (false, _) => "Stage: no STAGE space".to_owned(),
        (true, None) => "Stage: no play area bounds".to_owned(),
        (true, Some(b)) => format!("Stage: play area {:.2} × {:.2} m", b.x, b.y),
    });
    ui.label(match f.stage.head_height {
        Some(h) => format!("Head above the floor: {h:.2} m"),
        None => "Head above the floor: unknown".to_owned(),
    });
    if f.stage.floor_jumps > 0 {
        ui.label(format!(
            "Floor jumps taken out: {} · shift {:+.2} m",
            f.stage.floor_jumps, f.stage.floor_shift_m
        ));
    }
    ui.label(match f.head_height {
        Some(h) => format!(
            "Head height remap: eyes {:+.2} m from the character's (from {})",
            h.cockpit_y,
            h.source.label()
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

/// What the automatic posture decided from, and the seated eye reference.
fn posture_inputs(ui: &mut Ui, f: &FrameStatus) {
    if let Some(c) = f.classifier {
        let t = c.thresholds;
        ui.label(format!(
            "Auto: {:?} ({}) · seated eyes {:.2} m{} · standing eyes {:.2} m · sits below {:.2} m · floor below {:.2} m{} · stands above {:.2} m · dwell {:.1} s",
            c.detected,
            c.reason.label(),
            t.seated_eye_m,
            if t.seated_estimated { " (estimated)" } else { "" },
            t.standing_eye_m,
            t.sit_below_m,
            t.floor_below_m,
            if c.floor_level { " (at floor level)" } else { "" },
            t.stand_above_m,
            c.dwell_s,
        ));
    }
    let r = &f.seated_reference;
    ui.label(match r.reference {
        Some(reference) => format!(
            "Seated eye reference: {:.2} m ({}) · median {} over {} samples",
            reference.height_m,
            reference.source.label(),
            r.median_m
                .map_or_else(|| "not yet".to_owned(), |m| format!("{m:.2} m")),
            r.samples,
        ),
        None => "Seated eye reference: none yet".to_owned(),
    });
}

pub(super) fn roomscale_settings(ui: &mut Ui, r: &mut RoomscaleConfig) {
    ui.horizontal(|ui| {
        ui.label("Posture");
        ui.selectable_value(&mut r.posture, Posture::Auto, "Auto")
            .on_hover_text(
                "Seated or standing by the head's height above the floor; standing becomes \
             roomscale with a large enough play area.",
            );
        ui.selectable_value(&mut r.posture, Posture::Seated, "Seated");
        ui.selectable_value(&mut r.posture, Posture::Standing, "Standing");
        ui.selectable_value(&mut r.posture, Posture::Roomscale, "Roomscale");
    });
    ui.add(
        egui::Slider::new(&mut r.roomscale_min_extent_m, 0.5..=3.0)
            .text("Auto roomscale play area (m)"),
    );
    ui.add(
        egui::Slider::new(&mut r.auto_posture.stand_after_s, 0.0..=5.0)
            .text("Auto: standing after (s)"),
    );
    ui.add(
        egui::Slider::new(&mut r.auto_posture.sit_after_s, 1.0..=120.0)
            .text("Auto: seated after (s)"),
    );
    ui.checkbox(
        &mut r.auto_posture.chair_height_only,
        "Auto: seated only at a chair's height",
    )
    .on_hover_text(
        "Count the wait only while a standing head is steady at a chair's height: a head at floor \
         level, or one that bobs, stays standing.",
    );
    let s = &mut r.seated_reference;
    ui.checkbox(&mut s.adaptive, "Adaptive seated eye reference")
        .on_hover_text("Move the seated eye reference slowly toward the median head height.");
    ui.add_enabled_ui(s.adaptive, |ui| {
        ui.indent("seated_reference", |ui| {
            ui.add(egui::Slider::new(&mut s.window_s, 10.0..=600.0).text("Window (s)"));
            ui.add(egui::Slider::new(&mut s.rate_mps, 0.001..=0.05).text("Rate (m/s)"));
            ui.add(egui::Slider::new(&mut s.deadband_m, 0.0..=0.1).text("Deadband (m)"));
        });
    });

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

    ui.separator();
    stance::stance_settings(ui, &mut r.stance);
}

/// The camera's height against Rico's animated eyes, both above his root, and what sets the
/// difference ([`crate::hooks::camera::height`]).
fn camera_height_readout(ui: &mut Ui) {
    let Some(r) = height::readout() else {
        ui.label("Camera height: the headpose has not placed the camera yet");
        return;
    };
    let frame = &r.roomscale.frame;
    ui.label(format!(
        "Camera height above Rico's root: {:.2} m · Rico's animated eyes {:.2} m · difference {:+.2} m",
        r.camera_eyes_m,
        r.rico_eyes_m,
        r.camera_eyes_m - r.rico_eyes_m
    ));
    ui.label(format!(
        "Vertical offset {:+.2} m ({}, world scale {:.2}) · head above the floor {} · player eyes {:.2} m · character eyes assumed {:.2} m",
        r.vertical_offset_m,
        if r.remap.is_some() { "remapped" } else { "not remapped" },
        frame.world_scale,
        r.floor_head_height_m
            .map_or_else(|| "unknown".to_owned(), |h| format!("{h:.2} m")),
        r.player_eye_height_m,
        r.character_eye_estimate_m,
    ));
    if r.driving_view_m != glam::Vec3::ZERO {
        ui.label(format!(
            "Driving view correction ({:+.2}, {:+.2}, {:+.2}) m, body frame",
            r.driving_view_m.x, r.driving_view_m.y, r.driving_view_m.z,
        ));
    }
    ui.label(format!(
        "{} · {} · crouched {} · seated crouch {} · visual crouch {} · capsule {}{}",
        r.place(),
        frame.posture.label(),
        yes_no(frame.crouched),
        yes_no(frame.seated_crouch),
        yes_no(r.roomscale.visual_crouch),
        yes_no(r.roomscale.sim.capsule_owned),
        if r.roomscale.sim.capsule_blocked {
            " (no headroom)"
        } else {
            ""
        },
    ));
}
