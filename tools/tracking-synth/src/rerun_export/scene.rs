//! The frames: the head and the controllers in the cockpit frame, the cockpit's placement in the
//! world, the grounded rig in the room frame, and the frames' scalars.
//!
//! The tracked poses are children of `world/cockpit`, whose transform is the registration's
//! placement, so a 3D view rooted at `world` shows them in the world and one rooted at
//! `world/cockpit` shows them as the runtime gave them. The rig is under `world/cockpit/room`, the
//! cockpit lowered to its floor.

use body_rig::{StepPhase, angle};
use glam::{Quat, Vec3};
use rerun::{
    Arrows3D, Clear, Color, LineStrips3D, Points3D, Quaternion, RecordingStreamResult, Rgba32,
    Rotation3D, Scalars, StateChange, Transform3D, TransformAxes3D,
};
use tracking_format::{
    Frame,
    event::ControllerState,
    model::{Hand, HandFrame, Pose, TrackedPose},
};

use crate::{
    rerun_export::{Context, registration::Correspondence},
    rig_eval::EvalRow,
};

/// What a frame and the controller state read after it offer the registration.
pub(crate) fn correspondence(
    frame: &Frame,
    state: Option<&ControllerState>,
    scale: f32,
    eye_height: f32,
    segment: u32,
) -> Correspondence {
    let grips = match &frame.controllers {
        Some(controllers) => controllers
            .hands
            .each_ref()
            .map(|hand| (hand.active && usable(&hand.grip)).then_some(hand.grip.pose)),
        None => [None, None],
    };
    Correspondence {
        root: state
            .and_then(|s| s.root.as_ref())
            .map(|root| root.position),
        shift: state.map_or(Vec3::ZERO, |s| s.roomscale.cockpit_shift_m),
        floor: floor(frame, eye_height).unwrap_or(-eye_height),
        grips,
        wrists: state.map_or([None, None], |s| {
            s.late.hands.each_ref().map(|h| h.latest_wrist)
        }),
        scale,
        segment,
    }
}

/// The floor's height in the cockpit: under the eyes by the STAGE space's head height when the
/// runtime located it, and by the calibrated eye height otherwise.
pub(crate) fn floor(frame: &Frame, eye_height: f32) -> Option<f32> {
    let head = head_pose(frame)?;
    Some(match frame.stage.head_height {
        Some(height) if height.is_finite() => head.position.y - height,
        _ => head.position.y - eye_height,
    })
}

/// The head: the eyes' midpoint and their orientations' midpoint, while tracked.
pub(crate) fn head_pose(frame: &Frame) -> Option<Pose> {
    let head = frame.head.as_ref()?;
    if !frame.session.head_tracked {
        return None;
    }
    let [left, right] = head.eyes.map(|eye| eye.pose);
    let pose = Pose {
        position: (left.position + right.position) * 0.5,
        orientation: left.orientation.slerp(right.orientation, 0.5).normalize(),
    };
    (pose.position.is_finite() && pose.orientation.is_finite()).then_some(pose)
}

/// The entity name of a hand.
pub(crate) fn hand_name(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    }
}

/// How long a velocity arrow's travel lasts, seconds: an arrow is the velocity times this.
pub(crate) const ARROW_S: f32 = 0.25;

/// Log the stretch's frames, with the rig's rows among them.
pub(crate) fn log_frames(context: &Context<'_>, rows: &[EvalRow]) -> RecordingStreamResult<()> {
    let rec = context.rec;
    rec.log_static("world/cockpit/head", &TransformAxes3D::new(0.15))?;
    let mut rows = rows.iter().peekable();
    let mut rig_shown = false;
    let mut last_phase: Option<StepPhase> = None;
    let mut last_time: Option<u64> = None;
    for (index, frame) in context.frames.iter().enumerate() {
        context.set_time(frame.time_ns);
        if let Some(last) = last_time {
            let ms = frame.time_ns.saturating_sub(last) as f64 / 1.0e6;
            rec.log("plots/frame/frame_time_ms", &Scalars::single(ms))?;
        }
        last_time = Some(frame.time_ns);
        let state = context.states.per_frame[index].map(|i| context.states.all[i].1);

        if let Some(placement) = context.fit.placements[index] {
            rec.log(
                "world/cockpit",
                &Transform3D::from_translation_rotation_scale(
                    context.world(placement.translation),
                    rotation(Quat::from_rotation_y(placement.yaw)),
                    placement.scale,
                ),
            )?;
        }
        if let Some(error) = context.fit.wrist_errors[index] {
            rec.log("plots/registration/wrist_error_m", &Scalars::single(error))?;
        }
        match head_pose(frame) {
            Some(head) => {
                rec.log("world/cockpit/head", &transform(&head))?;
                rec.log(
                    "world/cockpit/head/gaze",
                    &Arrows3D::from_vectors([[0.0, 0.0, -0.4]]).with_colors([GAZE]),
                )?;
            }
            None => rec.log("world/cockpit/head", &Clear::recursive())?,
        }
        if let Some(height) = frame.stage.head_height {
            rec.log("plots/height/head_over_floor", &Scalars::single(height))?;
        }
        let floor = floor(frame, context.eye_height);
        if let Some(floor) = floor {
            rec.log(
                "world/cockpit/room",
                &Transform3D::from_translation([0.0, floor, 0.0]),
            )?;
        }
        if let Some(state) = state {
            let shift = state.roomscale.cockpit_shift_m;
            rec.log(
                "world/cockpit/room/root_anchor",
                &Points3D::new([[shift.x, 0.0, shift.z]])
                    .with_radii([0.03])
                    .with_colors([RICO]),
            )?;
        }
        if let Some(controllers) = &frame.controllers {
            for hand in Hand::BOTH {
                log_hand(context, hand, &controllers.hands[hand.index()], state)?;
            }
        }

        while rows
            .peek()
            .is_some_and(|row| ns(row.time_s) < frame.time_ns)
        {
            rows.next();
        }
        match rows.peek() {
            Some(row) if ns(row.time_s) == frame.time_ns => {
                log_rig(context, row)?;
                if last_phase != Some(row.output.phase) {
                    rec.log(
                        "states/rig_step",
                        &StateChange::single(phase_name(row.output.phase)),
                    )?;
                    last_phase = Some(row.output.phase);
                }
                rig_shown = true;
                rows.next();
            }
            _ if rig_shown => {
                rec.log("world/cockpit/room/rig", &Clear::recursive())?;
                rec.log("states/rig_step", &StateChange::single("not grounded"))?;
                last_phase = None;
                rig_shown = false;
            }
            _ => {}
        }
    }
    Ok(())
}

/// The default length of a ray that hit nothing, metres.
const RAY_M: f32 = 2.0;

const GAZE: Color = Color(Rgba32::from_rgb(230, 230, 230));
const RICO: Color = Color(Rgba32::from_rgb(255, 140, 0));
const HIT_OBJECT: Color = Color(Rgba32::from_rgb(80, 220, 80));
const HIT: Color = Color(Rgba32::from_rgb(230, 210, 60));
const MISS: Color = Color(Rgba32::from_rgb(140, 140, 140));
const RIG: Color = Color(Rgba32::from_rgb(90, 160, 255));
const LEAN_INSIDE: Color = Color(Rgba32::from_rgb(90, 200, 120));
const LEAN_DEMAND: Color = Color(Rgba32::from_rgb(255, 120, 60));

fn phase_name(phase: StepPhase) -> &'static str {
    phase.label()
}

fn ns(seconds: f64) -> u64 {
    (seconds * 1.0e9).round().max(0.0) as u64
}

fn usable(pose: &TrackedPose) -> bool {
    pose.position_valid && pose.orientation_valid
}

fn rotation(orientation: Quat) -> Rotation3D {
    Rotation3D::from(Quaternion::from_xyzw(orientation.to_array()))
}

fn transform(pose: &Pose) -> Transform3D {
    Transform3D::from_translation_rotation(pose.position.to_array(), rotation(pose.orientation))
}

fn log_hand(
    context: &Context<'_>,
    hand: Hand,
    frame: &HandFrame,
    state: Option<&ControllerState>,
) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let name = hand_name(hand);
    let base = format!("world/cockpit/{name}");
    if !frame.active || !usable(&frame.grip) {
        return rec.log(base, &Clear::recursive());
    }
    rec.log(format!("{base}/grip"), &transform(&frame.grip.pose))?;
    rec.log(format!("{base}/grip"), &TransformAxes3D::new(0.08))?;
    if let Some(velocity) = frame.grip.linear_velocity {
        rec.log(
            format!("plots/hands/speed/{name}"),
            &Scalars::single(velocity.length()),
        )?;
    }
    rec.log(
        format!("plots/hands/trigger/{name}"),
        &Scalars::single(frame.inputs.trigger),
    )?;
    rec.log(
        format!("plots/hands/squeeze/{name}"),
        &Scalars::single(frame.inputs.squeeze),
    )?;
    if usable(&frame.aim) {
        rec.log(format!("{base}/aim"), &transform(&frame.aim.pose))?;
        rec.log(format!("{base}/aim"), &TransformAxes3D::new(0.05))?;
        let aim = state.and_then(|s| s.aim.hands[hand.index()].as_ref());
        // Only the aim pose's own ray is drawn from the aim pose; a gun sight's ray starts at the
        // sight, which the recording places only by its target.
        let (length, color) = match aim {
            Some(aim) if aim.source == "aim_pose" => match aim.hit_distance_m {
                Some(distance) if aim.hit_object => (distance, HIT_OBJECT),
                Some(distance) => (distance, HIT),
                None => (RAY_M, MISS),
            },
            _ => (RAY_M, MISS),
        };
        rec.log(
            format!("{base}/aim/ray"),
            &Arrows3D::from_vectors([[0.0, 0.0, -length]])
                .with_colors([color])
                .with_radii([0.004]),
        )?;
    }
    if let Some(joints) = &frame.joints {
        let points: Vec<[f32; 3]> = joints
            .iter()
            .filter(|joint| joint.position_valid)
            .map(|joint| joint.pose.position.to_array())
            .collect();
        rec.log(
            format!("{base}/joints"),
            &Points3D::new(points).with_radii([0.006]),
        )?;
    }
    Ok(())
}

/// The rig's pivots, its spine, its lean ring, its step demand, and its heading, in the room frame,
/// and its scalars.
fn log_rig(context: &Context<'_>, row: &EvalRow) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let solve = &row.output.solve;
    let pelvis = Vec3::new(row.pelvis.x, 0.0, row.pelvis.y);
    let neck = solve.neck;
    let waist = solve.waist;
    rec.log(
        "world/cockpit/room/rig/pivots",
        &Points3D::new([neck.to_array(), waist.to_array(), pelvis.to_array()])
            .with_radii([0.03, 0.03, 0.04])
            .with_colors([RIG, RIG, RICO])
            .with_labels(["neck", "waist", "pelvis"]),
    )?;
    rec.log(
        "world/cockpit/room/rig/spine",
        &LineStrips3D::new([[pelvis.to_array(), waist.to_array(), neck.to_array()]])
            .with_colors([RIG])
            .with_radii([0.01]),
    )?;
    let centre = Vec3::new(pelvis.x, neck.y, pelvis.z);
    let ring: Vec<[f32; 3]> = (0..=RING_SEGMENTS)
        .map(|i| {
            let a = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            (centre + Vec3::new(a.cos(), 0.0, a.sin()) * solve.lean_reach_m).to_array()
        })
        .collect();
    let demanding = solve.demand_m() > 0.0;
    rec.log(
        "world/cockpit/room/rig/lean",
        &LineStrips3D::new([ring]).with_colors([if demanding { LEAN_DEMAND } else { LEAN_INSIDE }]),
    )?;
    if demanding {
        let demand = Vec3::new(solve.step_demand.x, 0.0, solve.step_demand.y);
        rec.log(
            "world/cockpit/room/rig/demand",
            &Arrows3D::from_vectors([demand.to_array()])
                .with_origins([(Vec3::new(neck.x, neck.y, neck.z) - demand).to_array()])
                .with_colors([LEAN_DEMAND]),
        )?;
    } else {
        rec.log("world/cockpit/room/rig/demand", &Clear::flat())?;
    }
    let forward = angle::forward(row.output.body_yaw);
    rec.log(
        "world/cockpit/room/rig/heading",
        &Arrows3D::from_vectors([[forward.x * 0.4, 0.0, forward.y * 0.4]])
            .with_origins([pelvis.to_array()])
            .with_colors([RIG]),
    )?;
    let o = &row.output;
    let scalars: [(&str, f64); 9] = [
        ("bend_deg", f64::from(solve.bend.angle().to_degrees())),
        ("demand_m", f64::from(solve.demand_m())),
        (
            "yaw_error_deg",
            f64::from(angle::wrap(row.head_yaw - o.body_yaw).to_degrees()),
        ),
        ("offset_m", f64::from(solve.offset.length())),
        ("lean_reach_m", f64::from(solve.lean_reach_m)),
        ("crouch_m", f64::from(solve.crouch_m)),
        ("neck_height_m", f64::from(neck.y)),
        ("standing_neck_m", f64::from(o.standing_neck_m)),
        ("pelvis_speed_mps", f64::from(row.speed_mps)),
    ];
    for (name, value) in scalars {
        rec.log(format!("plots/rig/{name}"), &Scalars::single(value))?;
    }
    Ok(())
}

/// The lean ring's segments.
const RING_SEGMENTS: usize = 48;
