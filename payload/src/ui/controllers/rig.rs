//! The body rig: who owns the root, the gap to the ground point under the upright eyes and what the
//! chase does about it, the lean it leaves alone, the cockpit shift, and the ledger's attribution,
//! and their settings.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::{
        body::rig::{config::RigConfig, root::RootStatus},
        roomscale::probe::STALE_AFTER,
    },
    ui::controllers::{vec3, yes_no},
};

pub(super) fn rig_readout(ui: &mut Ui, status: &RootStatus) {
    let f = &status.frame;
    let s = &status.sim;
    let now = Instant::now();
    if !f.fresh(now) {
        ui.label("Inactive: no VR frame is rendering.");
    }
    ui.label(format!(
        "Owner: {} · on-foot tick {}",
        s.owner.map_or("none yet", |o| o.label()),
        yes_no(s.at.is_some_and(|at| now.saturating_duration_since(at) < STALE_AFTER)),
    ));
    let m = &f.measurement;
    ui.label(format!(
        "Gap {:.3} m · outside the lean {:.3} m · to the target ({:+.3}, {:+.3}) m cockpit",
        f.gap_m(),
        f.residual_m(),
        m.target.x,
        m.target.y,
    ));
    ui.label(format!(
        "Effective gap ({:+.3}, {:+.3}) m world · chase ({:+.2}, {:+.2}) m/s · task ({:+.2}, {:+.2}) m/s · head ({:+.2}, {:+.2}) m/s",
        s.effective_gap.x,
        s.effective_gap.y,
        s.chase_velocity.x,
        s.chase_velocity.y,
        s.task_velocity.x,
        s.task_velocity.y,
        m.frame.head_velocity.x,
        m.frame.head_velocity.y,
    ));
    ui.label(format!(
        "Lean ({:+.3}, {:+.3}) m · allowed {:.3} m forward · pivot drop {:.3} m below the standing height ({:+.3} m) · torso yaw {:+.1}°",
        m.lean.vector.x,
        m.lean.vector.y,
        m.lean.allowance_m,
        m.lean.drop_m,
        f.standing_m,
        f.torso_yaw.to_degrees(),
    ));
    ui.label(format!(
        "Feet ({:+.2}, {:+.2}) m cockpit · cockpit shift {}",
        m.feet.x,
        m.feet.y,
        vec3(f.shift)
    ));
    ui.label(format!(
        "Ledger: {} exact · {} projected · re-based at teleports {} · at seat exits {}",
        s.ledger.exact, s.ledger.projected, f.teleport_rebases, f.exit_rebases
    ));
}

pub(super) fn rig_settings(ui: &mut Ui, r: &mut RigConfig) {
    let c = &mut r.chase;
    ui.checkbox(&mut c.enabled, "Body follows the head")
        .on_hover_text("In the standing posture, walk the body toward the physical head.");
    ui.add_enabled_ui(c.enabled, |ui| {
        ui.indent("rig_chase", |ui| {
            ui.add(egui::Slider::new(&mut c.gain, 0.05..=1.0).text("Gain per tick"));
            ui.add(
                egui::Slider::new(&mut c.feed_forward, 0.0..=1.5)
                    .text("Head velocity feed-forward"),
            );
            ui.add(egui::Slider::new(&mut c.max_speed_mps, 0.5..=12.0).text("Max speed (m/s)"));
            ui.add(
                egui::Slider::new(&mut c.warp_min_m, 1.0..=50.0)
                    .text("Unannounced teleport distance (m)"),
            );
        });
    });
    let l = &mut r.lean;
    ui.checkbox(&mut l.enabled, "Lean without stepping")
        .on_hover_text("Leave a lean over the feet unchased, by how far the head has dropped.");
    ui.add_enabled_ui(l.enabled, |ui| {
        ui.indent("rig_lean", |ui| {
            ui.add(egui::Slider::new(&mut l.gain, 0.0..=10.0).text("Lean per metre of drop"));
            ui.add(egui::Slider::new(&mut l.deadband_m, 0.0..=0.2).text("Drop deadband (m)"));
            ui.add(egui::Slider::new(&mut l.max_m, 0.0..=1.0).text("Max lean (m)"));
            ui.add(egui::Slider::new(&mut l.side_share, 0.0..=1.0).text("Sideways share"));
            ui.add(egui::Slider::new(&mut l.back_share, 0.0..=1.0).text("Backward share"));
        });
    });
}
