//! The grounded rig as recording events: each frame's inputs and outputs as the payload ran them,
//! and the calibration and tuning in use, noted when they change and repeated now and then so a
//! stretch of a recording opens with them. With these, `tracking-synth rig-eval` can run a
//! candidate rig on the recorded inputs and diff it against what the game ran.

use std::time::{Duration, Instant};

use body_rig::{PlayerBody, RigFrame, RigOutput, RigTuning};
use glam::{Vec2, Vec3};
use parking_lot::Mutex;
use tracking_format::event::{Label, RigConfig, RigInput, RigResult, RigSample};

use crate::{
    controllers::body::BodyDimensions,
    recorder::tracking::events::{self, EventKind},
};

/// One grounded-rig frame as the payload ran it.
pub(crate) struct RigRun<'a> {
    pub frame: &'a RigFrame,
    pub output: &'a RigOutput,
    pub floor_m: f32,
    pub snapped_to: Option<Vec2>,
    pub body: PlayerBody,
    pub dimensions: &'a BodyDimensions,
    pub tuning: &'a RigTuning,
    pub now: Instant,
}

/// Note one grounded-rig frame, and the calibration and tuning when they changed or are due again.
pub(crate) fn note_rig(run: RigRun<'_>) {
    if !events::collecting() {
        return;
    }
    note_config(&run);
    let frame = run.frame;
    let output = run.output;
    let solve = &output.solve;
    events::note(EventKind::Rig(Box::new(RigSample {
        input: RigInput {
            eyes: frame.eyes,
            head: frame.head,
            hands: frame.hands,
            pelvis: frame.pelvis,
            moving: frame.moving,
            sample_height: frame.sample_height,
            dt_s: frame.dt,
        },
        output: RigResult {
            neck: solve.neck,
            waist: solve.waist,
            spine_m: solve.spine_m,
            offset: solve.offset,
            bend: Vec2::new(solve.bend.pitch, solve.bend.roll),
            lean_reach_m: solve.lean_reach_m,
            step_demand: solve.step_demand,
            crouch_m: solve.crouch_m,
            hands_reaching: solve.hands_reaching,
            phase: Label::Borrowed(output.phase.label()),
            gap: output.gap,
            body_yaw: output.body_yaw,
            chest_yaw: output.chest_yaw,
            yaw_target: output.yaw_target,
            turning: output.turning,
            turn_cause: output
                .turn_cause
                .map(|cause| Label::Borrowed(cause.label())),
            standing_neck_m: output.standing_neck_m,
            steps: output.steps,
        },
        floor_m: run.floor_m,
        snapped_to: run.snapped_to,
    })));
}

/// How often an unchanged calibration and tuning is noted again.
const CONFIG_REPEAT: Duration = Duration::from_secs(5);

/// The last noted calibration and tuning, and when.
static LAST_CONFIG: Mutex<Option<(Vec3, RigTuning, Instant)>> = Mutex::new(None);

fn note_config(run: &RigRun<'_>) {
    let key = Vec3::new(
        run.dimensions.height,
        run.dimensions.eye_height,
        run.body.hip_height,
    );
    let repeat = {
        let mut last = LAST_CONFIG.lock();
        let repeat = match *last {
            Some((k, t, at)) if k == key && t == *run.tuning => {
                if run.now.saturating_duration_since(at) < CONFIG_REPEAT {
                    return;
                }
                true
            }
            _ => false,
        };
        *last = Some((key, *run.tuning, run.now));
        repeat
    };
    let tuning_json = match serde_json::to_string(run.tuning) {
        Ok(json) => json,
        Err(error) => format!("{{\"error\":\"{error}\"}}"),
    };
    events::note(EventKind::RigConfig(Box::new(RigConfig {
        height_m: run.dimensions.height,
        eye_height_m: run.dimensions.eye_height,
        body_eye_height_m: run.body.eye_height,
        body_hip_height_m: run.body.hip_height,
        tuning_json: Label::Owned(tuning_json),
        repeat,
    })));
}
