//! The body rig: who owns the root, the gap to the ground point under the upright eyes and what the
//! chase does about it, the lean it leaves alone, the push-back at walls, the cockpit shift, and the
//! ledger's attribution, and the body fit's switches: the procedural body, the body facing the
//! torso, the push-back, and the lean.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::{
        body::{
            procedural::config::ProceduralMode,
            rig::{collision::probe::STALE_AFTER, root::RootStatus},
        },
        config::BodyRemapConfig,
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
        "Lean ({:+.3}, {:+.3}) m · allowed {:.3} m forward · pivot drop {:.3} m below the upright height ({:+.3} m) · torso yaw {:+.1}°",
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
    let p = &f.pushback;
    ui.label(format!(
        "Push-back: threshold {} · obstructed {} · pushed {:.3} m · slid {:.3} m this episode · {} episodes · fade {:.2}",
        p.threshold
            .map_or_else(|| "none".to_owned(), |t| format!("{t:.2} m")),
        yes_no(p.obstructed),
        p.push.length(),
        f.debt_m,
        f.pushbacks,
        p.fade,
    ));
    ui.label(format!(
        "Ledger: {} exact · {} projected · re-based at teleports {} · at seat exits {}",
        s.ledger.exact, s.ledger.projected, f.teleport_rebases, f.exit_rebases
    ));
}

pub(super) fn rig_settings(ui: &mut Ui, body: &mut BodyRemapConfig) {
    let mut procedural = body.procedural.mode == ProceduralMode::Full;
    if ui
        .checkbox(&mut procedural, "Procedural body")
        .on_hover_text(
            "Build the body under the player's head and hand it over to the animation by state.",
        )
        .changed()
    {
        body.procedural.mode = if procedural {
            ProceduralMode::Full
        } else {
            ProceduralMode::Off
        };
    }
    let r = &mut body.rig;
    ui.checkbox(&mut r.torso.follow, "Body faces the torso")
        .on_hover_text("Turn the body under the view to face the player's measured torso.");
    ui.checkbox(&mut r.pushback.enabled, "Stop at walls")
        .on_hover_text(
            "Hold the view at a wall the body cannot pass; off, only the fade answers it.",
        );
    ui.checkbox(&mut r.lean.enabled, "Lean without stepping")
        .on_hover_text("Leave a lean over the feet unchased, by how far the head has dropped.");
}
