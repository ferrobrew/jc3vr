//! The head and view feature: the camera switch and the frozen-pose diagnostic, the headpose
//! readout, the grapple comfort and turn settings it shares with other features, and the shared
//! matrix-grid widget.

use egui::Slider;
use jc3gi::types::math::Matrix4;

use crate::{
    config, grapple, headpose,
    headpose::config::VrTurnMode,
    hooks,
    vr::{self, FreezeMode, pose_control},
};

pub(crate) fn head_view(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    ui.checkbox(&mut cfg.camera.enabled, "Enabled");
    ui.collapsing("Frozen pose (diagnostic)", |ui| {
        egui_frozen_pose(ui, &mut cfg.vr);
    });
}

/// The camera and headpose readout: the authored camera, the headpose mode and latch, the
/// body-relative rotation, the published position and anchors, and the engine's camera blend.
pub(crate) fn head_view_status(ui: &mut egui::Ui) {
    ui.label(format!(
        "Authored sequence camera: {:?}.",
        hooks::camera::authored::status()
    ));
    ui.label(format!("Mode: {:?}", headpose::sim::mode()));
    ui.label(format!("Latch: {:?}", headpose::sim::latch_state()));

    // The body-relative rotation rather than the sim's raw angles: under the VR source the sim never
    // accumulates them, so the raw angles read as a permanent zero.
    let (yaw, pitch, roll) = headpose::body_relative_rotation().to_euler(glam::EulerRot::YXZ);
    ui.label(format!("Yaw (body-relative): {:+.1}°", yaw.to_degrees()));
    ui.label(format!("Pitch: {:+.1}°", pitch.to_degrees()));
    ui.label(format!("Roll:  {:+.1}°", roll.to_degrees()));

    let pose = headpose::query();
    ui.label(format!(
        "Position: ({:+.2}, {:+.2}, {:+.2})",
        pose.position.x, pose.position.y, pose.position.z
    ));

    ui.label(match headpose::anchor() {
        Some(anchor) => format!(
            "Anchor: ({:+.2}, {:+.2}, {:+.2})",
            anchor.x, anchor.y, anchor.z
        ),
        None => "Anchor: none".to_string(),
    });
    let neck_delta = headpose::neck_delta();
    ui.label(format!(
        "Head → neck: ({:+.2}, {:+.2}, {:+.2})",
        neck_delta.x, neck_delta.y, neck_delta.z
    ));
    let eye_arm = headpose::eye_arm();
    ui.label(format!(
        "Neck → eyes (arm): ({:+.2}, {:+.2}, {:+.2})",
        eye_arm.x, eye_arm.y, eye_arm.z
    ));
    // The engine's sub-frame interpolation fraction (issue #20): stuck at 0 or 1 means the
    // engine's camera lerp is inert and the sim-tick cadence shows as judder.
    ui.label(format!("Camera dtf: {:.3}", hooks::camera::last_dtf()));
}

pub(crate) fn grapple_comfort(ui: &mut egui::Ui) {
    egui_grapple(ui, &mut config::edit().headpose.grapple);
}

pub(crate) fn turn_advanced(ui: &mut egui::Ui) {
    egui_vr_turn(ui, &mut config::edit().headpose.vr_turn);
}

/// The frozen-pose diagnostic ([`crate::vr::pose_control`]): pin the rendered pose, then drive it by
/// hand. Content that only mis-renders *in motion* (a mis-scaled screen-space pass, a render block
/// that slides under the camera) cannot be measured by turning your head -- the same movement is never
/// repeated twice. Frozen, a pose is an exact set of numbers: dial one in, step yaw by exactly one
/// degree, and the two frames differ in exactly that.
///
/// The mode picks *what* is held: the head's contribution to the camera, or the whole camera. For a
/// "slides as the camera moves" measurement it is the full camera you want, since only that holds
/// everything the view is built from.
fn egui_frozen_pose(ui: &mut egui::Ui, vr: &mut vr::VrConfig) {
    ui.horizontal(|ui| {
        ui.label("Freeze:");
        ui.radio_value(&mut vr.freeze_mode, FreezeMode::Off, "Off");
        ui.radio_value(
            &mut vr.freeze_mode,
            FreezeMode::CockpitPose,
            "Head pose (cockpit)",
        )
        .on_hover_text(
            "Capture the current HMD pose (and the body frame and head anchor with it) and reuse it \
             every frame. Isolates HMD pose-noise-driven flicker (present even on a desk) from \
             intrinsic render artifacts. This holds the head's contribution still, not the camera: \
             a camera the game moves still moves.",
        );
        ui.radio_value(
            &mut vr.freeze_mode,
            FreezeMode::FullCamera,
            "Full camera (world)",
        )
        .on_hover_text(
            "Pin the final scene render camera in world space, at the last point before the engine \
             consumes it. Nothing moves the view -- not the head, the body, the animated head bone, \
             or the game's own camera -- and the per-eye offsets are held with it. This is the mode \
             for measuring content that only mis-renders in motion.",
        );
    });
    // While frozen, edit the pose actually driving the render. Unfrozen there is nothing to edit, so
    // the live cockpit pose stands in as a readout -- but only in the modes whose numbers are in that
    // frame. The full-camera mode has no live stand-in (the cockpit pose is a different quantity in a
    // different frame, and showing it here would read as a world position), so it says so instead.
    let frozen = pose_control::current();
    let values = match (frozen, vr.freeze_mode) {
        (Some(values), _) => Some(values),
        (None, FreezeMode::FullCamera) => None,
        (None, _) => headpose::xr::cockpit_pose()
            .map(|p| pose_control::PoseValues::from_pose(p.position, p.orientation)),
    };
    // The two frames are not interchangeable: cockpit metres are head travel from the recenter
    // baseline, world metres are the camera's position in the game world. A number whose frame is
    // ambiguous is worse than no number, so the frame is always stated next to the values.
    ui.label(match (frozen.is_some(), vr.freeze_mode) {
        (true, FreezeMode::FullCamera) => {
            "Frame: world -- the render camera's world-space position and orientation"
        }
        (true, _) => "Frame: cockpit -- the head pose relative to the recenter baseline",
        (false, _) => "Frame: cockpit (live head pose) -- freeze to edit",
    });
    let Some(mut values) = values else {
        ui.label(match vr.freeze_mode {
            FreezeMode::FullCamera => "Pose: no frame has rendered under the freeze yet.",
            _ => "Pose: no VR frame has rendered yet.",
        });
        return;
    };

    let step_m = pose_control::STEP_M;
    let step_deg = pose_control::STEP_DEG;
    let mut changed = false;
    ui.add_enabled_ui(frozen.is_some(), |ui| {
        egui::Grid::new("frozen_pose_grid")
            .num_columns(4)
            .show(ui, |ui| {
                changed |= pose_row(ui, "X", &mut values.position.x, step_m, " m");
                changed |= pose_row(ui, "Y", &mut values.position.y, step_m, " m");
                changed |= pose_row(ui, "Z", &mut values.position.z, step_m, " m");
                changed |= pose_row(ui, "Yaw", &mut values.yaw_deg, step_deg, "°");
                changed |= pose_row(ui, "Pitch", &mut values.pitch_deg, step_deg, "°");
                changed |= pose_row(ui, "Roll", &mut values.roll_deg, step_deg, "°");
            });
        if changed {
            pose_control::set_current(values);
        }

        if ui
            .button("Reset to captured pose")
            .on_hover_text(match pose_control::base() {
                Some(base) => format!(
                    "Return to the pose captured when the freeze engaged: ({:+.3}, {:+.3}, {:+.3}) m, \
                     yaw {:+.2}°, pitch {:+.2}°, roll {:+.2}°.",
                    base.position.x,
                    base.position.y,
                    base.position.z,
                    base.yaw_deg,
                    base.pitch_deg,
                    base.roll_deg,
                ),
                None => "Nothing is frozen.".to_string(),
            })
            .clicked()
        {
            pose_control::reset();
        }
    });
}

/// One editable pose component: direct numeric entry plus exact `-`/`+` nudges of `step`. Returns
/// whether the value changed this frame.
fn pose_row(ui: &mut egui::Ui, label: &str, value: &mut f32, step: f32, suffix: &str) -> bool {
    ui.label(label);
    let mut changed = ui
        .add(
            egui::DragValue::new(value)
                .speed(step * 0.1)
                .max_decimals(4)
                .suffix(suffix),
        )
        .changed();
    if ui
        .button("-")
        .on_hover_text("Step down by exactly one step, snapped to the step grid.")
        .clicked()
    {
        *value = pose_control::nudge(*value, step, -1);
        changed = true;
    }
    if ui
        .button("+")
        .on_hover_text("Step up by exactly one step, snapped to the step grid.")
        .clicked()
    {
        *value = pose_control::nudge(*value, step, 1);
        changed = true;
    }
    ui.end_row();
    changed
}

/// The grapple reel-in body-frame filter (issue #36): which rotation the reel is allowed to
/// compose into the view, and the blend constants around the reel window.
fn egui_grapple(ui: &mut egui::Ui, grapple: &mut grapple::GrappleComfortConfig) {
    use crate::grapple::GrappleComfortMode;

    let mode_label = |mode: GrappleComfortMode| match mode {
        GrappleComfortMode::Off => "Off",
        GrappleComfortMode::HoldView => "Hold view",
        GrappleComfortMode::LevelPitch => "Level pitch",
    };
    egui::ComboBox::from_label("Mode")
        .selected_text(mode_label(grapple.mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut grapple.mode, GrappleComfortMode::Off, "Off");
            ui.selectable_value(&mut grapple.mode, GrappleComfortMode::HoldView, "Hold view")
                .on_hover_text(
                    "Cancel the body rotation the reel adds: the view stays where you were \
                     looking at reel start, and only the HMD (or mouse) moves it.",
                );
            ui.selectable_value(
                &mut grapple.mode,
                GrappleComfortMode::LevelPitch,
                "Level pitch",
            )
            .on_hover_text(
                "Keep the view level while reeling: the body's pitch and roll are dropped, but \
                 its yaw toward the target still composes.",
            );
        });
    ui.add(Slider::new(&mut grapple.engage_s, 0.0..=1.0).text("Engage (s)"));
    ui.add(Slider::new(&mut grapple.release_s, 0.0..=2.0).text("Release (s)"));
    ui.checkbox(&mut grapple.yaw_handoff, "Yaw handoff (body turns to view)")
        .on_hover_text(
            "At reel end, turn the character toward where you are looking instead of sweeping \
             the view toward the character's landing heading. VR, on foot, hold-view only.",
        );
    ui.add(Slider::new(&mut grapple.handoff_timeout_s, 0.0..=3.0).text("Handoff timeout (s)"));
    ui.add(
        Slider::new(&mut grapple.anchor_snap_threshold_mps, 0.0..=120.0)
            .text("Landing snap threshold (m/s)"),
    )
    .on_hover_text(
        "Single-step velocity change of the body-driven head position beyond which it is \
         treated as a landing snap and absorbed; steady motion of any speed passes through. \
         0 disables.",
    );
    ui.add(Slider::new(&mut grapple.anchor_snap_ease_s, 0.0..=1.0).text("Landing snap ease (s)"));
    ui.label(format!("Blend: {:.2}", grapple::blend_factor()));
    let mut log = grapple::telemetry::log_enabled();
    if ui
        .checkbox(&mut log, "Log reel telemetry")
        .on_hover_text(
            "Write per-tick filter state and per-frame pose composition to the log \
             (`grapple_telemetry` target) for offline analysis.",
        )
        .changed()
    {
        grapple::telemetry::set_log_enabled(log);
    }
}

/// The on-foot body-turn knobs used while the HMD owns the head (mouse and right stick turn the body,
/// not the head). Separate from the flatscreen latch, which never runs under an OpenXR session.
fn egui_vr_turn(ui: &mut egui::Ui, turn: &mut headpose::config::VrTurnConfig) {
    ui.horizontal(|ui| {
        ui.label("Mode:");
        ui.radio_value(&mut turn.mode, VrTurnMode::Smooth, "Smooth");
        ui.radio_value(&mut turn.mode, VrTurnMode::Snap, "Snap");
    });
    match turn.mode {
        VrTurnMode::Smooth => {
            ui.add(
                Slider::new(&mut turn.mouse_turn_scale, 0.5..=20.0)
                    .text("Mouse turn scale (°/unit)"),
            )
            .on_hover_text(
                "Body yaw per unit of mouse-look delta. The whole-body turn is still rate-limited \
                 by the Game tab's face-camera turn step.",
            );
            ui.add(Slider::new(&mut turn.smooth_scale, 0.0..=4.0).text("Stick turn scale (×)"))
                .on_hover_text("Right-stick turn rate as a multiple of the mouse sensitivity.");
            ui.add(Slider::new(&mut turn.deadzone, 0.0..=0.5).text("Stick deadzone"));
        }
        VrTurnMode::Snap => {
            ui.add(Slider::new(&mut turn.snap_angle_deg, 5.0..=90.0).text("Snap angle (°)"));
            ui.add(Slider::new(&mut turn.snap_threshold, 0.1..=1.0).text("Snap threshold"));
        }
    }
    ui.add(Slider::new(&mut turn.max_body_lead_deg, 0.0..=180.0).text("Max body lead (°)"))
        .on_hover_text(
            "How far the turn target may lead the body's facing. Caps how long the body keeps \
             turning after a fast flick stops.",
        );
}

pub fn matrix_grid(ui: &mut egui::Ui, id: &str, label: &str, m: &Matrix4, other: Option<&Matrix4>) {
    ui.label(label);
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for r in 0..4 {
            for c in 0..4 {
                let i = r * 4 + c;
                let v = m.data[i];
                let differs = other.is_some_and(|o| (v - o.data[i]).abs() > 1e-5);
                let text = format!("{v:+.3}");
                if differs {
                    ui.colored_label(egui::Color32::YELLOW, text);
                } else {
                    ui.label(text);
                }
            }
            ui.end_row();
        }
    });
}
