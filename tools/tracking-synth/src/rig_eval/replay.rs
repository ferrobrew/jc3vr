//! The rig over the recorded rig inputs (`Rig`, from version 8): one evaluation per frame the
//! payload's rig ran, on exactly the inputs it had, so a changed rig or tuning is compared with the
//! game's on the same frames. The pelvis is the game's closed loop's, not the candidate's.
//!
//! What the payload does to its rig between frames is taken from the recording: a recenter turns
//! the rig with the cockpit frame (the frames' recenters), a recenter's snap faces the head before
//! the frame that carries it (`snapped_to`), and a gap between rig frames is a spell off the ground,
//! which stops any step, and past [`RESUME_AFTER_NS`] forgets the heading as the payload does. A
//! teleport's forget leaves no trace in the recording.

use body_rig::{GroundedRig, RigFrame, angle};
use glam::{Vec2, Vec3};
use tracking_format::{Frame, Recording, event::EventKind};

use crate::rig_eval::{
    ComparedRow, EvalRow, EvalSummary, VideoClock,
    compare::{ComparisonAccumulator, RecordedOutput},
    config::ConfigResolver,
    summary::SummaryAccumulator,
};

/// Run the rig over `recording`'s recorded rig inputs, one row per rig frame.
pub(crate) fn run_recorded(
    recording: &Recording,
    configs: &ConfigResolver,
    clock: Option<&VideoClock>,
) -> (Vec<ComparedRow>, EvalSummary, ComparisonAccumulator) {
    let mut rig = GroundedRig::new(configs.at(0).0);
    let mut summary = SummaryAccumulator::default();
    let mut comparison = ComparisonAccumulator::default();
    let mut rows = Vec::new();
    let mut frames = recording.frames.iter().peekable();
    let mut last_frame: Option<&Frame> = None;
    let mut last: Option<(u64, Vec2)> = None;
    for event in &recording.events {
        let EventKind::Rig(sample) = &event.kind else {
            continue;
        };
        let time_ns = event.time_ns;
        while let Some(frame) = frames.next_if(|frame| frame.time_ns <= time_ns) {
            for recenter in &frame.recenters {
                summary.recenter();
                rig.rotate(angle::yaw_of(recenter.turn * Vec3::NEG_Z).unwrap_or(0.0));
            }
            last_frame = Some(frame);
        }
        let (body, tuning) = configs.at(time_ns);
        rig.set_body(body);
        let gap = last.map(|(at, _)| time_ns.saturating_sub(at));
        if gap.is_some_and(|gap| gap > RESUME_AFTER_NS) {
            rig.forget();
        } else if gap.is_some_and(|gap| gap > OFF_GROUND_NS) {
            rig.stand();
        }
        let input = &sample.input;
        if sample.snapped_to.is_some() {
            rig.recenter(input.eyes, input.head);
        }
        let output = rig.update(
            &RigFrame {
                eyes: input.eyes,
                head: input.head,
                hands: input.hands,
                pelvis: input.pelvis,
                slip: Vec2::ZERO,
                moving: input.moving,
                sample_height: input.sample_height,
                dt: input.dt_s,
            },
            &tuning,
        );
        let speed_mps = match (last, gap) {
            (Some((_, pelvis)), Some(gap)) if gap > 0 && gap <= OFF_GROUND_NS => {
                (input.pelvis - pelvis).length() / (gap as f32 / 1.0e9)
            }
            _ => 0.0,
        };
        last = Some((time_ns, input.pelvis));
        let head_yaw = angle::head_heading(input.head).unwrap_or(output.body_yaw);
        let dt = if input.dt_s.is_finite() {
            input.dt_s.max(0.0)
        } else {
            0.0
        };
        summary.observe(&output, head_yaw, dt);
        let recorded = RecordedOutput::from_result(&sample.output);
        comparison.observe(&output, &recorded);
        let video_s = clock.zip(last_frame).and_then(|(clock, frame)| {
            let since = i64::try_from(time_ns - frame.time_ns).ok()?;
            clock.video_s(frame.display_time_ns.checked_add(since)?)
        });
        rows.push(ComparedRow {
            row: EvalRow {
                time_s: time_ns as f64 / 1.0e9,
                video_s,
                output,
                head_yaw,
                head_pitch: angle::head_pitch(input.head),
                pelvis: input.pelvis,
                speed_mps,
                moving: input.moving,
            },
            recorded: Some(recorded),
        });
    }
    (rows, summary.finish(), comparison)
}

/// A gap between rig frames longer than this was a spell off the ground, nanoseconds: longer than
/// any rendered frame.
const OFF_GROUND_NS: u64 = 200_000_000;

/// A spell off the ground longer than this forgets the body's heading, nanoseconds, as the
/// payload's rig resumes.
const RESUME_AFTER_NS: u64 = 1_000_000_000;
