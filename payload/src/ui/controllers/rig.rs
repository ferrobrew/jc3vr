//! The body rig: the grounded rig's step phase and demand, the bend, the lean's reach, the yaw,
//! and the standing height; who owns the root, the step drive, the push-back at walls, the cockpit
//! shift, and the ledger's attribution; and the body fit's switches, tuning, and the recenter
//! button, the IK body's among them.

use std::time::Instant;

use egui::Ui;

use crate::{
    controllers::{
        body::{
            firm_ragdoll,
            ik_body::IkBodyConfig,
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
                "Legs {:+.1}° · chest {:+.1}° · head {:+.1}° · turning {}",
                o.body_yaw.to_degrees(),
                o.chest_yaw.to_degrees(),
                o.yaw_target.to_degrees(),
                o.turn_cause.map_or("no", |cause| cause.label()),
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
    ui.checkbox(
        &mut body.procedural.firm_ragdoll,
        "Firm ragdoll on the line and the canopy",
    )
    .on_hover_text(
        "Hang the hips and legs from the physics, the upper body held to the pose, while the \
         grapple lock holds the line and under the canopy.",
    );
    let firm = firm_ragdoll::status();
    ui.label(match firm.preset {
        Some(preset) => format!(
            "Firm ragdoll: {} on preset {preset}, weight {:.2}",
            if firm.active { "in" } else { "going out" },
            firm.weight
        ),
        None => "Firm ragdoll: out".to_owned(),
    });
    ui.label(format!(
        "Muscle target (dry run): {} composed, {} refused · legs from the {} · upper body moved \
         {:.3} m · thighs turned {:.0}° and {:.0}°",
        firm.targets,
        firm.refused,
        if firm.ik_legs { "IK body" } else { "clip" },
        firm.report.upper_shift_m,
        firm.report.thigh_turn_deg[0],
        firm.report.thigh_turn_deg[1],
    ));
    let r = &mut body.rig;
    ui.checkbox(&mut r.steps.enabled, "Step after the head")
        .on_hover_text("Walk Rico under the player's neck once the head is out past a lean.");
    ui.checkbox(&mut r.follow.enabled, "Body follows the head's yaw")
        .on_hover_text("Turn the legs under the view for a large or sustained turn of the head.");
    ui.checkbox(
        &mut r.follow.turn_animation,
        "Turn in place with the game's animation",
    )
    .on_hover_text("Let the game's turn-in-place acts play while Rico stands.");
    ui.checkbox(
        &mut r.follow.square_hips,
        "Square the clip's hips to the legs",
    )
    .on_hover_text("Steer the root so the animation's hips, not its root, face the legs' heading.");
    ui.checkbox(&mut r.pushback.enabled, "Stop at walls")
        .on_hover_text(
            "Hold the view at a wall the body cannot pass; off, only the fade answers it.",
        );
    ik_body_settings(ui, &mut body.ik);
    ui.collapsing("Stance", |ui| {
        let s = &mut body.procedural.stance;
        ui.checkbox(&mut s.enabled, "Square the feet standing idle")
            .on_hover_text("Move the game's foot targets to a square stance under the hips.");
        ui.add(egui::Slider::new(&mut s.width_m, 0.1..=0.6).text("Feet apart (m)"));
        ui.add(egui::Slider::new(&mut s.toe_out_deg, -10.0..=40.0).text("Toes out (°)"));
        ui.add(egui::Slider::new(&mut s.blend_s, 0.0..=1.5).text("Blend (s)"))
            .on_hover_text("How quickly the stance comes in standing still and goes out moving.");
    });
    ui.collapsing("Lean and steps", |ui| {
        let p = &mut r.pelvis;
        ui.add(egui::Slider::new(&mut p.lean_radius_m, 0.0..=0.5).text("Lean radius (m)"))
            .on_hover_text(
                "How far the neck may be out over the pelvis before a step is considered.",
            );
        ui.add(egui::Slider::new(&mut p.back_radius_m, 0.0..=0.5).text("Lean radius back (m)"))
            .on_hover_text(
                "The lean radius straight behind the pelvis: stepping back steps sooner.",
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
        ui.add(egui::Slider::new(&mut s.drift_min_m, 0.0..=0.2).text("Drift past (m)"))
            .on_hover_text("A side or back offset held this far out, standing still, drifts out.");
        ui.add(egui::Slider::new(&mut s.drift_after_s, 0.0..=5.0).text("Drift after (s)"));
        ui.add(
            egui::Slider::new(&mut s.drift_steady_mps, 0.0..=0.3).text("Head steady below (m/s)"),
        );
        ui.add(egui::Slider::new(&mut s.drift_speed_mps, 0.0..=0.5).text("Drift speed (m/s)"));
        ui.add(egui::Slider::new(&mut r.steps.walk_stick, 0.0..=1.0).text("Walk stick"))
            .on_hover_text("How far a step pushes the move stick for the game's walk.");
    });
    ui.collapsing("Yaw", |ui| {
        let y = &mut r.yaw;
        ui.add(egui::Slider::new(&mut y.chest_clamp_deg, 0.0..=90.0).text("Chest clamp (°)"))
            .on_hover_text("The furthest the chest turns from the legs.");
        ui.add(egui::Slider::new(&mut y.chest_follow_s, 0.0..=1.0).text("Chest follow (s)"));
        ui.add(egui::Slider::new(&mut y.turn_deg, 0.0..=120.0).text("Legs turn past (°)"));
        ui.add(egui::Slider::new(&mut y.sustain_deg, 0.0..=120.0).text("Sustained past (°)"))
            .on_hover_text("The head held this far for the sustain time turns the legs.");
        ui.add(egui::Slider::new(&mut y.sustain_s, 0.0..=5.0).text("Sustain time (s)"));
        ui.add(
            egui::Slider::new(&mut y.moving_deadzone_deg, 0.0..=90.0).text("Legs turn moving (°)"),
        );
        ui.add(egui::Slider::new(&mut y.moving_sustain_s, 0.0..=2.0).text("Moving sustain (s)"))
            .on_hover_text(
                "While moving, the head held past the moving angle this long turns the legs.",
            );
        ui.add(egui::Slider::new(&mut y.settle_deg, 0.0..=30.0).text("Settle (°)"));
        ui.add(egui::Slider::new(&mut y.slew_deg_s, 10.0..=720.0).text("Turn rate (°/s)"));
        ui.add(egui::Slider::new(&mut y.steady_deg, 0.0..=20.0).text("Head steady within (°)"));
        ui.add(egui::Slider::new(&mut y.steady_s, 0.0..=10.0).text("Steady for (s)"))
            .on_hover_text("A head held steady this long settles the legs under it slowly.");
        ui.add(egui::Slider::new(&mut y.steady_settle_deg, 0.0..=40.0).text("Steady settle past (°)"));
        ui.add(egui::Slider::new(&mut y.steady_slew_deg_s, 1.0..=200.0).text("Steady settle rate (°/s)"));
        ui.add(egui::Slider::new(&mut y.look_down_deg, 0.0..=90.0).text("Looking down past (°)"))
            .on_hover_text(
                "Looking further down than this starts only the large turn, and fades the chest's follow.",
            );
        ui.add(egui::Slider::new(&mut y.look_down_full_deg, 0.0..=90.0).text("Chest holds past (°)"));
        ui.add(egui::Slider::new(&mut y.hand_bias, 0.0..=1.0).text("Hands' pull on the chest"));
        ui.add(egui::Slider::new(&mut y.hand_min_m, 0.0..=0.6).text("Hands count past (m)"));
    });
}

/// The IK body's switches and the tuning that matters in the headset.
fn ik_body_settings(ui: &mut Ui, ik: &mut IkBodyConfig) {
    ui.checkbox(&mut ik.enabled, "IK body standing on foot")
        .on_hover_text(
            "Pose Rico's pelvis and legs from the learned estimate while he stands: the feet \
             planted by IK, stepping for themselves, and the square stance only without an \
             estimate.",
        );
    ui.checkbox(&mut ik.airborne, "IK body in the air")
        .on_hover_text(
            "Hang the pelvis under the head in a jump, the legs relaxed toward an air pose.",
        );
    ui.checkbox(
        &mut ik.ragdoll_target,
        "IK pose as the firm ragdoll's target",
    )
    .on_hover_text(
        "Compose the firm ragdoll's muscle target from the drawn upper body and the IK body's \
             legs rather than the raw clip. Until the ragdoll's pre-physics step is bound, the \
             target is composed as a dry run for the readout.",
    );
    ui.checkbox(&mut ik.climb_feet, "IK feet on the face while climbing")
        .on_hover_text(
            "While the hands climb, brace each foot on the face in front of it, or let it hang \
             relaxed where nothing is in reach.",
        );
    ui.collapsing("IK body", |ui| {
        ui.add(egui::Slider::new(&mut ik.walk_weight, 0.0..=1.0).text("Walk share"))
            .on_hover_text("How much of the IK pose turns the clip's pelvis on the walk.");
        ui.add(egui::Slider::new(&mut ik.blend_s, 0.0..=1.0).text("Blend (s)"))
            .on_hover_text("How long a hand-over between the IK pose and a clip takes.");
        ui.add(egui::Slider::new(&mut ik.resume_s, 0.0..=2.0).text("Resume after a walk (s)"));
        let c = &mut ik.contact;
        ui.add(egui::Slider::new(&mut c.step_s, 0.1..=0.8).text("Step duration (s)"));
        ui.add(egui::Slider::new(&mut c.arc_m, 0.0..=0.2).text("Step lift (m)"));
        ui.add(egui::Slider::new(&mut c.reach_share, 0.8..=1.2).text("Out of reach past (× leg)"))
            .on_hover_text("A foot steps once its hip is this far from it, in leg lengths.");
        ui.add(
            egui::Slider::new(&mut c.reach_drop_m, 0.0..=0.3).text("Reach counts a drop of (m)"),
        )
        .on_hover_text(
            "How far the pelvis may come down to keep a planted foot before the foot steps.",
        );
        ui.add(egui::Slider::new(&mut c.twist_deg, 10.0..=90.0).text("Twist past (°)"));
        ui.add(egui::Slider::new(&mut c.twist_hold_s, 0.0..=1.5).text("Twist held for (s)"));
        ui.add(egui::Slider::new(&mut c.com_margin_m, 0.0..=0.3).text("Balance margin (m)"))
            .on_hover_text("How far outside the feet the body's weight goes before a foot steps.");
        ui.add(egui::Slider::new(&mut c.root_tolerance_m, 0.0..=1.0).text("Root carried past (m)"));
        ui.add(egui::Slider::new(&mut c.tidy_m, 0.0..=0.5).text("Tidy past (m)"))
            .on_hover_text(
                "A foot this far off its stance spot steps back to it after the tidy time.",
            );
        ui.add(egui::Slider::new(&mut c.tidy_s, 0.0..=5.0).text("Tidy after (s)"));
        ui.add(egui::Slider::new(&mut c.max_step_m, 0.2..=1.2).text("Longest step (m)"))
            .on_hover_text("A longer step hands the legs to the game's walk.");
        ui.add(egui::Slider::new(&mut c.walk_speed_mps, 0.1..=3.0).text("Walk past (m/s)"))
            .on_hover_text("Both feet due at once with the pelvis this fast is a walk.");
        ui.add(egui::Slider::new(&mut ik.max_pelvis_drop_m, 0.0..=0.5).text("Pelvis drop (m)"))
            .on_hover_text("The furthest the pelvis comes down to keep the planted feet in reach.");
        ui.add(egui::Slider::new(&mut ik.hips_reach, 0.0..=1.0).text("Hips reach"));
        ui.add(egui::Slider::new(&mut ik.hips_rotation_reach, 0.0..=1.0).text("Hips turn reach"));
        ui.add(egui::Slider::new(&mut ik.hips_resist, 0.0..=1.0).text("Hips resist"))
            .on_hover_text("How firmly HumanIK holds the hips against the feet's and hands' pull.");
        ui.add(egui::Slider::new(&mut ik.foot_reach, 0.0..=1.0).text("Feet reach"));
        ui.add(egui::Slider::new(&mut ik.foot_pull, 0.0..=1.0).text("Feet pull"))
            .on_hover_text("How much reaching a foot drags the body after it.");
        ui.add(egui::Slider::new(&mut ik.knee_hint_reach, 0.0..=1.0).text("Knee hints"));
        ui.add(egui::Slider::new(&mut ik.air_relax, 0.0..=1.0).text("Air pose share"));
        ui.add(egui::Slider::new(&mut ik.fk.smooth_s, 0.0..=0.5).text("Pelvis smoothing (s)"));
        let b = &mut ik.brace;
        ui.add(egui::Slider::new(&mut b.reach_share, 0.6..=1.1).text("Brace lets go past (× leg)"))
            .on_hover_text(
                "A braced foot lets go once its hip is this far from it, in leg lengths.",
            );
        ui.add(
            egui::Slider::new(&mut b.acquire_share, 0.4..=1.1).text("Brace taken within (× leg)"),
        );
        ui.add(egui::Slider::new(&mut b.knee_m, 0.0..=0.4).text("Brace knee toward the face (m)"));
        ui.add(egui::Slider::new(&mut b.move_s, 0.0..=1.0).text("Brace move (s)"))
            .on_hover_text(
                "How long a foot takes to move to a new brace, or to fade in or out of one.",
            );
    });
}
