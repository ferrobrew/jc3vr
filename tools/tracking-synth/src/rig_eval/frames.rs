//! The rig over the tracking frames: the eyes, the head, and the hands from each frame, and the
//! pelvis either simulated under the step drive or taken from the game's pre-IK pose.

use body_rig::{GroundedRig, RigFrame, angle, drive_velocity};
use glam::{Quat, Vec2, Vec3};
use tracking_format::{
    Frame, Recording,
    event::EventKind,
    model::{ActionSetKind, RecenterCause, RecenterKind},
};

use crate::rig_eval::{
    ComparedRow, EvalOptions, EvalRow, EvalSummary, VideoClock,
    compare::{ComparisonAccumulator, RecordedTrack},
    config::ConfigResolver,
    pre_ik::PreIkTrack,
    summary::SummaryAccumulator,
};

/// Where the pelvis comes from.
#[derive(Debug, Copy, Clone)]
pub(crate) enum PelvisDrive<'a> {
    /// The game's character under the drive, at `accel_mps2`.
    Simulated,
    /// The game's own animated hips.
    PreIk(&'a PreIkTrack),
}

/// What a frame run needs beyond the recording.
pub(crate) struct FrameRun<'a> {
    pub(crate) options: &'a EvalOptions,
    pub(crate) configs: &'a ConfigResolver,
    pub(crate) drive: PelvisDrive<'a>,
    pub(crate) recorded: &'a RecordedTrack,
    pub(crate) clock: Option<&'a VideoClock>,
}

/// Run the rig over `recording`'s frames, one row per grounded frame.
pub(crate) fn run_frames(
    recording: &Recording,
    run: &FrameRun<'_>,
) -> (Vec<ComparedRow>, EvalSummary, ComparisonAccumulator) {
    let options = run.options;
    let mut rig = GroundedRig::new(run.configs.at(0).0);
    let mut summary = SummaryAccumulator::default();
    let mut comparison = ComparisonAccumulator::default();
    let mut rows = Vec::new();
    let cases = body_cases(recording);
    let mut pelvis: Option<Vec2> = None;
    let mut velocity = Vec2::ZERO;
    let mut last_time: Option<u64> = None;
    let mut last_grounded: Option<u64> = None;
    for frame in &recording.frames {
        let dt = last_time.map_or(0.0, |last| {
            (frame.time_ns.saturating_sub(last)) as f32 / 1.0e9
        });
        last_time = Some(frame.time_ns);
        let (body, tuning) = run.configs.at(frame.time_ns);
        rig.set_body(body);
        let Some(head) = head_pose(frame, body.eye_height) else {
            continue;
        };
        for recenter in &frame.recenters {
            summary.recenter();
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
                if let PelvisDrive::Simulated = run.drive {
                    let neck = body_rig::body::neck_pivot(head.eyes, head.orientation);
                    pelvis = Some(Vec2::new(neck.x, neck.z));
                }
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
        let start = match run.drive {
            PelvisDrive::Simulated => {
                let neck = body_rig::body::neck_pivot(head.eyes, head.orientation);
                *pelvis.get_or_insert_with(|| {
                    rig.forget();
                    Vec2::new(neck.x, neck.z)
                })
            }
            PelvisDrive::PreIk(track) => {
                let Some(point) = track.at(frame.time_ns) else {
                    continue;
                };
                velocity = match pelvis {
                    Some(last) if dt > 0.0 => (point - last) / dt,
                    Some(_) => velocity,
                    None => {
                        rig.forget();
                        Vec2::ZERO
                    }
                };
                pelvis = Some(point);
                point
            }
        };
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
            &tuning,
        );
        let moved = match run.drive {
            PelvisDrive::Simulated => {
                let wanted = drive_velocity(output.phase, output.gap, dt, &tuning.step);
                let change = wanted - velocity;
                let max_change = options.accel_mps2 * dt;
                velocity += change.clamp_length_max(max_change);
                let moved = start + velocity * dt;
                pelvis = Some(moved);
                moved
            }
            PelvisDrive::PreIk(_) => start,
        };

        let head_yaw = angle::head_heading(head.orientation).unwrap_or(output.body_yaw);
        summary.observe(&output, head_yaw, dt);
        let recorded = run.recorded.at(frame.time_ns);
        if let Some(recorded) = recorded.as_ref() {
            comparison.observe(&output, recorded);
        }
        rows.push(ComparedRow {
            row: EvalRow {
                time_s: frame.time_ns as f64 / 1.0e9,
                video_s: run.clock.and_then(|c| c.video_s(frame.display_time_ns)),
                output,
                head_yaw,
                pelvis: moved,
                speed_mps: velocity.length(),
                moving,
            },
            recorded,
        });
    }
    (rows, summary.finish(), comparison)
}

/// A spell off the ground longer than this starts the rig again under the neck, nanoseconds.
const RESUME_AFTER_NS: u64 = 1_000_000_000;

/// The off hand's stick past this deflection moves the body.
const MOVING_STICK: f32 = 0.2;

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
