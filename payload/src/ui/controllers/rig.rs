//! The body rig: the grounded rig's step phase and demand, the bend, the lean's reach, the yaw,
//! and the standing height; who owns the root, the step drive, the push-back at walls, the cockpit
//! shift, and the ledger's attribution; and the body fit's switches, tuning, and the recenter
//! button.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::{
        body::{
            procedural::config::ProceduralMode,
            rig::{collision::probe::STALE_AFTER, grounded, root::RootStatus},
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
    let grounded = grounded::status();
    match f.rig {
        Some(rig) => {
            let o = rig.output;
            let solve = o.solve;
            ui.label(format!(
                "Step: {} · {} committed · demand {:.3} m · neck out {:.3} m of a {:.3} m lean",
                o.phase.label(),
                o.steps,
                f.residual_m(),
                f.gap_m(),
                solve.lean_reach_m,
            ));
            ui.label(format!(
                "Bend {:.1}° (pitch {:+.1}°, roll {:+.1}°) · crouch {:.3} m · hands reaching {}",
                solve.bend.angle().to_degrees(),
                solve.bend.pitch.to_degrees(),
                solve.bend.roll.to_degrees(),
                solve.crouch_m,
                yes_no(solve.hands_reaching),
            ));
            ui.label(format!(
                "Body yaw {:+.1}° · target {:+.1}° · turning {}",
                o.body_yaw.to_degrees(),
                o.yaw_target.to_degrees(),
                yes_no(o.turning),
            ));
            ui.label(format!(
                "Neck {:.3} m over the floor · standing {:.3} m ({} samples) · spine {:.3} m",
                solve.neck.y, o.standing_neck_m, grounded.height_samples, solve.spine_m,
            ));
        }
        None => {
            ui.label("Not grounded: the rig waits for Rico to be on foot under the player.");
        }
    }
    ui.label(format!(
        "Drive ({:+.2}, {:+.2}) m/s · task ({:+.2}, {:+.2}) m/s · gap left ({:+.3}, {:+.3}) m",
        s.step_velocity.x,
        s.step_velocity.y,
        s.task_velocity.x,
        s.task_velocity.y,
        s.effective_gap.x,
        s.effective_gap.y,
    ));
    ui.label(format!("Cockpit shift {}", vec3(f.shift)));
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
        "Ledger: {} exact · {} projected · re-based at teleports {} · at seat exits {} · snapped at recenters {}",
        s.ledger.exact, s.ledger.projected, f.teleport_rebases, f.exit_rebases, f.snaps,
    ));
}

pub(super) fn rig_settings(ui: &mut Ui, body: &mut BodyRemapConfig) {
    if ui
        .button("Recenter the body")
        .on_hover_text(
            "As the stick-click chord does: re-base the view's yaw and height, put the pelvis \
             under the neck, face the head, and measure the standing height afresh.",
        )
        .clicked()
    {
        crate::vr::recenter();
    }
    let mut procedural = body.procedural.mode == ProceduralMode::Full;
    if ui
        .checkbox(&mut procedural, "Procedural torso")
        .on_hover_text(
            "Pose the spine, the neck, and the clavicles from the player on the animation's hips.",
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
    ui.checkbox(&mut r.steps.enabled, "Step after the head")
        .on_hover_text("Walk Rico under the player's neck once the head is out past a lean.");
    ui.checkbox(&mut r.follow.enabled, "Body follows the head's yaw")
        .on_hover_text("Turn the body under the view once the head turns past the deadzone.");
    ui.checkbox(
        &mut r.follow.turn_animation,
        "Turn in place with the game's animation",
    )
    .on_hover_text("Let the game's turn-in-place acts play while Rico stands.");
    ui.checkbox(&mut r.pushback.enabled, "Stop at walls")
        .on_hover_text(
            "Hold the view at a wall the body cannot pass; off, only the fade answers it.",
        );
    ui.collapsing("Lean and steps", |ui| {
        let p = &mut r.pelvis;
        ui.add(egui::Slider::new(&mut p.lean_radius_m, 0.0..=0.5).text("Lean radius (m)"))
            .on_hover_text(
                "How far the neck may be out over the pelvis before a step is considered.",
            );
        ui.add(egui::Slider::new(&mut p.max_lean_m, 0.1..=1.0).text("Longest lean (m)"));
        ui.add(egui::Slider::new(&mut p.forward_bend_deg, 5.0..=89.0).text("Forward bend (°)"));
        ui.add(egui::Slider::new(&mut p.side_bend_deg, 5.0..=89.0).text("Side bend (°)"));
        ui.add(egui::Slider::new(&mut p.back_bend_deg, 5.0..=89.0).text("Back bend (°)"));
        ui.add(egui::Slider::new(&mut p.hand_reach_m, 0.0..=1.0).text("Hands reaching (m)"))
            .on_hover_text("How far ahead of the neck both hands must reach to widen the lean.");
        ui.add(egui::Slider::new(&mut p.hand_lean_m, 0.0..=0.3).text("Hands' lean (m)"));
        let s = &mut r.step;
        ui.add(egui::Slider::new(&mut s.commit_m, 0.0..=0.3).text("Step threshold (m)"));
        ui.add(egui::Slider::new(&mut s.arrive_m, 0.01..=0.2).text("Arrived within (m)"));
        ui.add(egui::Slider::new(&mut s.response_s, 0.05..=1.5).text("Drive response (s)"));
        ui.add(egui::Slider::new(&mut s.max_speed_mps, 0.2..=4.0).text("Fastest step (m/s)"));
        ui.add(egui::Slider::new(&mut s.settle_speed_mps, 0.0..=0.5).text("Settle speed (m/s)"));
        ui.add(egui::Slider::new(&mut s.settle_s, 0.0..=3.0).text("Longest settle (s)"));
        ui.add(egui::Slider::new(&mut s.give_up_s, 0.5..=10.0).text("Give up after (s)"));
        ui.add(egui::Slider::new(&mut r.steps.walk_stick, 0.0..=1.0).text("Walk stick"))
            .on_hover_text("How far a step pushes the move stick for the game's walk.");
    });
    ui.collapsing("Yaw", |ui| {
        let y = &mut r.yaw;
        ui.add(egui::Slider::new(&mut y.deadzone_deg, 0.0..=90.0).text("Deadzone (°)"));
        ui.add(
            egui::Slider::new(&mut y.moving_deadzone_deg, 0.0..=90.0).text("Deadzone moving (°)"),
        );
        ui.add(egui::Slider::new(&mut y.settle_deg, 0.0..=30.0).text("Settle (°)"));
        ui.add(egui::Slider::new(&mut y.slew_deg_s, 10.0..=720.0).text("Turn rate (°/s)"));
        ui.add(egui::Slider::new(&mut y.hand_bias, 0.0..=1.0).text("Hands' pull"));
        ui.add(egui::Slider::new(&mut y.hand_min_m, 0.0..=0.6).text("Hands count past (m)"));
    });
}
