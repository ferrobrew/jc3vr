//! The offline evaluator of the grounded body rig: the payload's own solver (`body-rig`) run over
//! a recording, with a simulated pelvis that follows the step drive, so the lean radius, the
//! thresholds, and the neck offset can be tuned against what a player really did before a headset
//! session. See `docs/mod/body/rig.md`.
//!
//! The rig works in the room frame, and the recording's poses are in the cockpit frame, which is the
//! room frame up to the floor's height: the eyes' height over the floor is the STAGE space's when
//! the runtime located it, and the cockpit height plus the calibrated eye height otherwise. The
//! stick moves the room and the body together, so it changes nothing here but the yaw's deadzone.
//!
//! The simulated pelvis is the game's character under the drive: its velocity moves toward the
//! drive's at a fixed acceleration, as the stick locomotion's integrator and the proxy do. The rig
//! runs only while the body is grounded (the procedural body's hand-over case is `on foot`, or the
//! on-foot action set in a recording without body states); elsewhere the pelvis is put back under
//! the neck when the rig resumes.
//!
//! A recenter turns the room with the cockpit frame. A manual or seat recenter then snaps the
//! pelvis under the neck, as the rig's recenter does; a follow bank only turns the frame.

use std::io::{self, Write};

use body_rig::{
    GroundedRig, PlayerBody, RigFrame, RigOutput, RigTuning, StepPhase, angle, drive_velocity,
};
use glam::{Quat, Vec2, Vec3};
use tracking_format::{
    Frame, Recording,
    event::EventKind,
    model::{ActionSetKind, RecenterCause, RecenterKind},
};

/// What the evaluation runs with.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EvalOptions {
    pub body: PlayerBody,
    pub tuning: RigTuning,
    /// How fast the simulated character's velocity reaches the drive's, metres per second squared.
    pub accel_mps2: f32,
}

impl EvalOptions {
    /// The defaults for a player `height` tall with the eyes at `eye_height`.
    pub fn new(height: f32, eye_height: f32) -> Self {
        Self {
            body: PlayerBody::from_heights(height, eye_height),
            tuning: RigTuning::new(),
            accel_mps2: DEFAULT_ACCEL_MPS2,
        }
    }
}

/// The simulated character's acceleration toward the drive, metres per second squared: about the
/// stick locomotion's fixed rate.
pub const DEFAULT_ACCEL_MPS2: f32 = 10.0;

/// One evaluated frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EvalRow {
    /// Seconds on the recording's clock.
    pub time_s: f64,
    /// Seconds into the take's video, when the sidecar maps the frame.
    pub video_s: Option<f64>,
    pub output: RigOutput,
    pub head_yaw: f32,
    pub pelvis: Vec2,
    pub speed_mps: f32,
    /// Whether the off hand's stick moves the body.
    pub moving: bool,
}

/// A summary of the evaluation.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct EvalSummary {
    /// Seconds the rig ran.
    pub grounded_s: f64,
    pub steps: u64,
    /// Seconds spent stepping or settling.
    pub driving_s: f64,
    /// Body turns started.
    pub turns: u64,
    /// The mean absolute angle between the head and the body, degrees.
    pub mean_yaw_error_deg: f64,
    /// The largest neck offset over the pelvis, metres.
    pub max_offset_m: f32,
    /// The mean neck offset over the pelvis, metres.
    pub mean_offset_m: f64,
    pub recenters: u64,
}

/// A map from the runtime's predicted display time to seconds into the take's video.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoClock {
    /// `(display_time_ns, unix_ms)`, in display-time order.
    points: Vec<(i64, f64)>,
    start_unix_ms: f64,
}

impl VideoClock {
    /// Read the video sidecar's frame stamps (`frames.jsonl`) and the stream's start
    /// (`stream.json`).
    pub fn parse(frames_jsonl: &str, stream_json: &str) -> Option<Self> {
        let stream: serde_json::Value = serde_json::from_str(stream_json).ok()?;
        let start_unix_ms = stream.get("video_start_unix_ms")?.as_f64()?;
        let mut points: Vec<(i64, f64)> = frames_jsonl
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|v| {
                Some((
                    v.get("predicted_display_time_ns")?.as_i64()?,
                    v.get("unix_ms")?.as_f64()?,
                ))
            })
            .collect();
        points.sort_by_key(|p| p.0);
        points.dedup_by_key(|p| p.0);
        (!points.is_empty()).then_some(Self {
            points,
            start_unix_ms,
        })
    }

    /// Seconds into the video for a frame at `display_time_ns`, interpolated between the stamps,
    /// and extrapolated at real time for at most [`EDGE_NS`] past either end.
    pub fn video_s(&self, display_time_ns: i64) -> Option<f64> {
        Some((self.unix_ms(display_time_ns)? - self.start_unix_ms) / 1000.0)
    }

    /// The wall-clock time of a frame at `display_time_ns`, milliseconds since the Unix epoch,
    /// placed as [`video_s`](Self::video_s) places it.
    pub fn unix_ms(&self, display_time_ns: i64) -> Option<f64> {
        let index = self.points.partition_point(|p| p.0 < display_time_ns);
        if index > 0 && index < self.points.len() {
            let ((t0, u0), (t1, u1)) = (self.points[index - 1], self.points[index]);
            let span = (t1 - t0) as f64;
            Some(u0 + (u1 - u0) * ((display_time_ns - t0) as f64 / span))
        } else {
            let (t, u) = if index == 0 {
                *self.points.first()?
            } else {
                *self.points.last()?
            };
            if (display_time_ns - t).abs() > EDGE_NS {
                return None;
            }
            Some(u + (display_time_ns - t) as f64 / 1.0e6)
        }
    }

    /// The wall-clock time of the video's first frame, milliseconds since the Unix epoch.
    pub fn start_unix_ms(&self) -> f64 {
        self.start_unix_ms
    }
}

/// How far past the sidecar's first or last stamp a frame is still placed on the video, nanoseconds.
const EDGE_NS: i64 = 100_000_000;

/// Run the rig over `recording`.
pub fn evaluate(
    recording: &Recording,
    options: &EvalOptions,
    clock: Option<&VideoClock>,
) -> (Vec<EvalRow>, EvalSummary) {
    let mut rig = GroundedRig::new(options.body);
    let mut summary = EvalSummary::default();
    let mut rows = Vec::new();
    let cases = body_cases(recording);
    let mut pelvis: Option<Vec2> = None;
    let mut velocity = Vec2::ZERO;
    let mut last_time: Option<u64> = None;
    let mut yaw_error_sum = 0.0f64;
    let mut offset_sum = 0.0f64;
    let mut was_turning = false;
    let mut last_grounded: Option<u64> = None;
    for frame in &recording.frames {
        let dt = last_time.map_or(0.0, |last| {
            (frame.time_ns.saturating_sub(last)) as f32 / 1.0e9
        });
        last_time = Some(frame.time_ns);
        let Some(head) = head_pose(frame, options.body.eye_height) else {
            continue;
        };
        for recenter in &frame.recenters {
            summary.recenters += 1;
            let turn = angle::yaw_of(recenter.turn * Vec3::NEG_Z).unwrap_or(0.0);
            rig.rotate(turn);
            if let Some(p) = pelvis.as_mut() {
                let turned = recenter.turn * Vec3::new(p.x, 0.0, p.y);
                *p = Vec2::new(turned.x, turned.z);
            }
            let snaps = match recenter.cause {
                RecenterCause::FollowBank => false,
                RecenterCause::Manual | RecenterCause::SeatTransition => true,
            } || recenter.kind == RecenterKind::Full;
            if snaps {
                rig.recenter(head.eyes, head.orientation);
                let neck = body_rig::body::neck_pivot(head.eyes, head.orientation);
                pelvis = Some(Vec2::new(neck.x, neck.z));
                velocity = Vec2::ZERO;
            }
        }
        if !grounded(frame, &cases) {
            velocity = Vec2::ZERO;
            // A short spell off the ground (a hop, a stair's bump) keeps the rig where it was; a
            // long one starts it again under the neck.
            if last_grounded.is_some_and(|at| frame.time_ns.saturating_sub(at) > RESUME_AFTER_NS) {
                pelvis = None;
            }
            continue;
        }
        last_grounded = Some(frame.time_ns);
        let dt = dt.clamp(0.0, 0.1);
        let neck = body_rig::body::neck_pivot(head.eyes, head.orientation);
        let start = *pelvis.get_or_insert_with(|| {
            rig.forget();
            Vec2::new(neck.x, neck.z)
        });
        let moving = frame
            .controllers
            .as_ref()
            .is_some_and(|c| c.hands[0].inputs.thumbstick.length() > MOVING_STICK);
        let output = rig.update(
            &RigFrame {
                eyes: head.eyes,
                head: head.orientation,
                hands: hands(frame),
                pelvis: start,
                moving,
                sample_height: true,
                dt,
            },
            &options.tuning,
        );
        let wanted = drive_velocity(output.phase, output.gap, dt, &options.tuning.step);
        let change = wanted - velocity;
        let max_change = options.accel_mps2 * dt;
        velocity += change.clamp_length_max(max_change);
        let moved = start + velocity * dt;
        pelvis = Some(moved);

        let head_yaw = angle::head_heading(head.orientation).unwrap_or(output.body_yaw);
        let yaw_error = angle::wrap(head_yaw - output.body_yaw).abs();
        summary.grounded_s += f64::from(dt);
        if output.phase.drives() {
            summary.driving_s += f64::from(dt);
        }
        if output.turning && !was_turning {
            summary.turns += 1;
        }
        was_turning = output.turning;
        yaw_error_sum += f64::from(yaw_error.to_degrees() * dt);
        let offset = output.solve.offset.length();
        offset_sum += f64::from(offset * dt);
        summary.max_offset_m = summary.max_offset_m.max(offset);
        summary.steps = output.steps;
        rows.push(EvalRow {
            time_s: frame.time_ns as f64 / 1.0e9,
            video_s: clock.and_then(|c| c.video_s(frame.display_time_ns)),
            output,
            head_yaw,
            pelvis: moved,
            speed_mps: velocity.length(),
            moving,
        });
    }
    if summary.grounded_s > 0.0 {
        summary.mean_yaw_error_deg = yaw_error_sum / summary.grounded_s;
        summary.mean_offset_m = offset_sum / summary.grounded_s;
    }
    (rows, summary)
}

/// Write `rows` as CSV.
pub fn write_csv(rows: &[EvalRow], out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "time_s,video_s,phase,steps,demand_m,offset_m,lean_reach_m,bend_deg,pitch_deg,roll_deg,\
         crouch_m,neck_y_m,standing_neck_m,head_yaw_deg,body_yaw_deg,yaw_target_deg,yaw_error_deg,\
         turning,pelvis_x,pelvis_z,neck_x,neck_z,speed_mps,moving,hands_reaching"
    )?;
    for row in rows {
        let o = &row.output;
        let s = &o.solve;
        writeln!(
            out,
            "{:.4},{},{},{},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},\
             {:.2},{},{:.4},{:.4},{:.4},{:.4},{:.3},{},{}",
            row.time_s,
            row.video_s.map_or(String::new(), |v| format!("{v:.3}")),
            phase_code(o.phase),
            o.steps,
            s.demand_m(),
            s.offset.length(),
            s.lean_reach_m,
            s.bend.angle().to_degrees(),
            s.bend.pitch.to_degrees(),
            s.bend.roll.to_degrees(),
            s.crouch_m,
            s.neck.y,
            o.standing_neck_m,
            row.head_yaw.to_degrees(),
            o.body_yaw.to_degrees(),
            o.yaw_target.to_degrees(),
            angle::wrap(row.head_yaw - o.body_yaw).to_degrees(),
            u8::from(o.turning),
            row.pelvis.x,
            row.pelvis.y,
            s.neck.x,
            s.neck.z,
            row.speed_mps,
            u8::from(row.moving),
            u8::from(s.hands_reaching),
        )?;
    }
    Ok(())
}

/// A spell off the ground longer than this starts the rig again under the neck, nanoseconds.
const RESUME_AFTER_NS: u64 = 1_000_000_000;

/// The off hand's stick past this deflection moves the body.
const MOVING_STICK: f32 = 0.2;

fn phase_code(phase: StepPhase) -> u8 {
    match phase {
        StepPhase::Idle => 0,
        StepPhase::Stepping => 1,
        StepPhase::Settling => 2,
    }
}

/// The eyes and the head's orientation in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
struct HeadPose {
    eyes: Vec3,
    orientation: Quat,
}

fn head_pose(frame: &Frame, eye_height: f32) -> Option<HeadPose> {
    let head = frame.head.as_ref()?;
    if !frame.session.head_tracked {
        return None;
    }
    let [left, right] = head.eyes.map(|eye| eye.pose);
    let mut eyes = (left.position + right.position) * 0.5;
    let orientation = left.orientation.slerp(right.orientation, 0.5).normalize();
    let floor = match frame.stage.head_height {
        Some(height) if height.is_finite() => eyes.y - height,
        _ => -eye_height,
    };
    eyes.y -= floor;
    (eyes.is_finite() && orientation.is_finite()).then_some(HeadPose { eyes, orientation })
}

fn hands(frame: &Frame) -> [Option<Vec3>; 2] {
    let Some(controllers) = frame.controllers.as_ref() else {
        return [None, None];
    };
    let floor = match (frame.stage.head_height, frame.head.as_ref()) {
        (Some(height), Some(head)) if height.is_finite() => {
            (head.eyes[0].pose.position.y + head.eyes[1].pose.position.y) * 0.5 - height
        }
        _ => return [None, None],
    };
    controllers.hands.each_ref().map(|hand| {
        (hand.active && hand.grip.position_valid && hand.grip.position_tracked)
            .then(|| hand.grip.pose.position - Vec3::Y * floor)
    })
}

/// The body's hand-over case at each time it changed, from the recording's body states.
fn body_cases(recording: &Recording) -> Vec<(u64, bool)> {
    recording
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::BodyState(state) => Some((event.time_ns, state.case == "on foot")),
            _ => None,
        })
        .collect()
}

fn grounded(frame: &Frame, cases: &[(u64, bool)]) -> bool {
    let on_foot_set = frame
        .controllers
        .as_ref()
        .is_some_and(|c| c.action_set == ActionSetKind::OnFoot);
    if cases.is_empty() {
        return on_foot_set;
    }
    let index = cases.partition_point(|(at, _)| *at <= frame.time_ns);
    on_foot_set && index > 0 && cases[index - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_clock_interpolates_between_stamps() {
        let frames = "{\"frame\":0,\"unix_ms\":1000.0,\"predicted_display_time_ns\":0}\n\
                      {\"frame\":1,\"unix_ms\":1100.0,\"predicted_display_time_ns\":100000000}\n";
        let clock = VideoClock::parse(frames, "{\"video_start_unix_ms\": 900.0}").unwrap();
        assert!((clock.video_s(50_000_000).unwrap() - 0.15).abs() < 1e-9);
        assert!((clock.video_s(0).unwrap() - 0.1).abs() < 1e-9);
        assert_eq!(clock.video_s(10_000_000_000), None);
    }
}
