//! The grounded rig: the pelvis solver, the step commit, the yaw, and the standing height,
//! advanced together once per frame.

use glam::{Quat, Vec2, Vec3};

use crate::{
    angle::{head_heading, head_pitch},
    body::{PlayerBody, neck_pivot},
    height::StandingHeight,
    pelvis::{PelvisInput, PelvisSolve, PelvisTuning, solve},
    step::{StepCommit, StepInput, StepPhase, StepTuning},
    yaw::{TurnCause, YawFollow, YawInput, YawTuning, hands_heading},
};

/// All of the rig's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RigTuning {
    pub pelvis: PelvisTuning,
    pub step: StepTuning,
    pub yaw: YawTuning,
}

impl RigTuning {
    pub const fn new() -> Self {
        Self {
            pelvis: PelvisTuning::new(),
            step: StepTuning::new(),
            yaw: YawTuning::new(),
        }
    }
}

/// The gaze is level enough to sample the standing height within this pitch, degrees: a look at
/// the floor carries the eyes down and forward about the neck, which the neck pivot takes out, but
/// a player looking down also tends to bow.
pub const LEVEL_GAZE_DEG: f32 = 20.0;

/// The smoothing time of the head's speed for the idle drift, seconds.
const HEAD_SPEED_SMOOTHING_S: f32 = 0.2;

/// One frame's inputs, in the room frame: metres over the floor, +Y up, -Z forward.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigFrame {
    /// The eyes.
    pub eyes: Vec3,
    /// The head's orientation.
    pub head: Quat,
    /// The tracked hands, left then right.
    pub hands: [Option<Vec3>; 2],
    /// The floor point under Rico's pelvis, `(x, z)`.
    pub pelvis: Vec2,
    /// Whether something else moves the body (the stick), which narrows the yaw's deadzone.
    pub moving: bool,
    /// Whether the frame may sample the standing height: on foot, in the standing posture.
    pub sample_height: bool,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// One frame's result.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RigOutput {
    pub solve: PelvisSolve,
    pub phase: StepPhase,
    /// What the drive closes, `(x, z)`: the pelvis's gap to the floor point under the neck, or the
    /// drift's part of it while drifting.
    pub gap: Vec2,
    /// The body's heading: the legs', which Rico faces.
    pub body_yaw: f32,
    /// The chest's heading, within its clamp of the body's.
    pub chest_yaw: f32,
    /// The heading the body turns toward: the head's.
    pub yaw_target: f32,
    /// Whether a body turn is under way.
    pub turning: bool,
    /// What started the body turn under way.
    pub turn_cause: Option<TurnCause>,
    /// The standing neck height, metres over the floor.
    pub standing_neck_m: f32,
    /// Steps committed so far.
    pub steps: u64,
}

/// The rig's state between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundedRig {
    body: PlayerBody,
    height: StandingHeight,
    step: StepCommit,
    yaw: YawFollow,
    /// The neck's last position and its smoothed horizontal speed, metres per second.
    neck: Option<Vec3>,
    head_speed_mps: f32,
}

impl GroundedRig {
    pub const fn new(body: PlayerBody) -> Self {
        Self {
            body,
            height: StandingHeight::new(body.eye_height + crate::body::EYES_TO_NECK.y),
            step: StepCommit::new(),
            yaw: YawFollow::new(),
            neck: None,
            head_speed_mps: 0.0,
        }
    }

    pub fn body(&self) -> PlayerBody {
        self.body
    }

    /// Take up a new calibration: the standing height is measured afresh against it.
    pub fn set_body(&mut self, body: PlayerBody) {
        if body != self.body {
            self.body = body;
            self.height.reseed(body.standing_neck());
        }
    }

    /// The standing eye height the rig has measured, metres over the floor: the standing neck
    /// height with a level head's eyes over it. The calibration's until enough samples are in.
    pub fn standing_eye_height(&self) -> f32 {
        self.height.reference() - crate::body::EYES_TO_NECK.y
    }

    /// The standing height's sample count.
    pub fn height_samples(&self) -> usize {
        self.height.samples()
    }

    /// The body's heading, once known.
    pub fn body_yaw(&self) -> Option<f32> {
        self.yaw.yaw()
    }

    /// Stop any step under way.
    pub fn stand(&mut self) {
        self.step.stand();
    }

    /// Forget the body's heading and any step, as after a teleport or a long gap: the next frame
    /// starts facing the head.
    pub fn forget(&mut self) {
        self.step.stand();
        self.yaw = YawFollow::new();
        self.neck = None;
        self.head_speed_mps = 0.0;
    }

    /// Turn the room frame by `turn` radians about +Y (new frame from old), leaving the body where
    /// it faces in the world.
    pub fn rotate(&mut self, turn: f32) {
        self.yaw.rotate(turn);
    }

    /// Recenter on the head: the body faces the head's heading, the standing height starts again
    /// from the neck's height now, and any step stops. The caller puts the pelvis under the neck.
    pub fn recenter(&mut self, eyes: Vec3, head: Quat) {
        let neck = neck_pivot(eyes, head);
        self.height.restart_at(neck.y);
        self.step.stand();
        match head_heading(head) {
            Some(yaw) => self.yaw.snap(yaw),
            None => self.yaw = YawFollow::new(),
        }
    }

    /// Fold the neck's horizontal motion since the last frame into the smoothed head speed.
    fn observe_head_speed(&mut self, neck: Vec3, dt: f32) {
        if !neck.is_finite() {
            return;
        }
        if let Some(last) = self.neck
            && dt.is_finite()
            && dt > 0.0
        {
            let speed = Vec2::new(neck.x - last.x, neck.z - last.z).length() / dt;
            let blend = (dt / (HEAD_SPEED_SMOOTHING_S + dt)).clamp(0.0, 1.0);
            if speed.is_finite() {
                self.head_speed_mps += (speed - self.head_speed_mps) * blend;
            }
        }
        self.neck = Some(neck);
    }

    /// Advance one frame.
    pub fn update(&mut self, frame: &RigFrame, tuning: &RigTuning) -> RigOutput {
        let neck = neck_pivot(frame.eyes, frame.head);
        self.observe_head_speed(neck, frame.dt);
        let level = head_pitch(frame.head).abs() <= LEVEL_GAZE_DEG.to_radians();
        let standing_neck = self
            .height
            .observe(neck.y, frame.sample_height && level, frame.dt);
        let head_yaw = head_heading(frame.head);
        let body_yaw = self.yaw.yaw().or(head_yaw).unwrap_or(0.0);
        let solve = solve(
            &PelvisInput {
                eyes: frame.eyes,
                head: frame.head,
                pelvis: frame.pelvis,
                body_yaw,
                hands: frame.hands,
                standing_neck_m: standing_neck,
                hip_height_m: self.body.hip_height,
            },
            &tuning.pelvis,
        );
        let phase = self.step.update(
            &StepInput {
                demand_m: solve.demand_m(),
                gap: solve.offset,
                drift: solve.drift,
                head_speed_mps: self.head_speed_mps,
                dt: frame.dt,
            },
            &tuning.step,
        );
        let gap = if phase == StepPhase::Drifting {
            solve.drift
        } else {
            solve.offset
        };
        let body_yaw = match head_yaw {
            Some(head_yaw) => self.yaw.update(
                &YawInput {
                    head_yaw,
                    hands: hands_heading(neck, &frame.hands, &tuning.yaw),
                    moving: frame.moving || phase.steps(),
                    dt: frame.dt,
                },
                &tuning.yaw,
            ),
            None => body_yaw,
        };
        RigOutput {
            solve,
            phase,
            gap,
            body_yaw,
            chest_yaw: self.yaw.chest().unwrap_or(body_yaw),
            yaw_target: self.yaw.target(),
            turning: self.yaw.turning(),
            turn_cause: self.yaw.turn_cause(),
            standing_neck_m: standing_neck,
            steps: self.step.steps(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::EYES_TO_NECK;

    const DT: f32 = 1.0 / 90.0;

    fn body() -> PlayerBody {
        PlayerBody::from_heights(1.8, 1.68)
    }

    fn frame(neck: Vec3, pelvis: Vec2) -> RigFrame {
        RigFrame {
            eyes: neck - EYES_TO_NECK,
            head: Quat::IDENTITY,
            hands: [None, None],
            pelvis,
            moving: false,
            sample_height: true,
            dt: DT,
        }
    }

    /// Run the rig with a pelvis that follows the drive perfectly, for `seconds`, with the neck
    /// given by `neck` at each time. Returns the pelvis and the last output.
    fn walk(
        rig: &mut GroundedRig,
        mut pelvis: Vec2,
        seconds: f32,
        neck: impl Fn(f32) -> Vec3,
    ) -> (Vec2, RigOutput) {
        let tuning = RigTuning::new();
        let mut out = RigOutput::default();
        for i in 0..(seconds / DT) as usize {
            out = rig.update(&frame(neck(i as f32 * DT), pelvis), &tuning);
            pelvis += crate::step::drive_velocity(out.phase, out.gap, DT, &tuning.step) * DT;
        }
        (pelvis, out)
    }

    #[test]
    fn a_walk_across_the_room_is_followed_and_ends_under_the_neck() {
        let mut rig = GroundedRig::new(body());
        let standing = body().standing_neck();
        // Walk a metre and a half forward at 0.8 m/s, then stand.
        let (pelvis, out) = walk(&mut rig, Vec2::ZERO, 5.0, |t| {
            Vec3::new(0.0, standing, -(t * 0.8).min(1.5))
        });
        assert!(
            (pelvis - Vec2::new(0.0, -1.5)).length() < 0.03,
            "{pelvis:?}"
        );
        assert_eq!(out.phase, StepPhase::Idle);
        assert!(out.steps >= 1);
    }

    #[test]
    fn a_lean_over_a_table_steps_nothing() {
        let mut rig = GroundedRig::new(body());
        let hip = body().hip_height;
        let spine = body().standing_neck() - hip;
        let (pelvis, out) = walk(&mut rig, Vec2::ZERO, 3.0, |t| {
            let tilt = (t * 0.5).min(0.9);
            Vec3::new(0.0, hip + spine * tilt.cos(), -spine * tilt.sin())
        });
        assert_eq!(pelvis, Vec2::ZERO);
        assert_eq!(out.steps, 0);
        assert!(out.solve.bend.pitch > 0.8);
    }

    #[test]
    fn a_weight_shift_held_still_drifts_back_under_the_neck() {
        let mut rig = GroundedRig::new(body());
        let standing = body().standing_neck();
        // Shift 12 cm to the side over half a second, then hold.
        let (pelvis, out) = walk(&mut rig, Vec2::ZERO, 4.0, |t| {
            Vec3::new((t * 0.24).min(0.12), standing, 0.0)
        });
        assert!(
            (pelvis - Vec2::new(0.12, 0.0)).length() < 0.03,
            "{pelvis:?}"
        );
        assert_eq!(out.steps, 0);
        assert!(out.solve.offset.length() < 0.03);
    }

    #[test]
    fn a_recenter_faces_the_head() {
        let mut rig = GroundedRig::new(body());
        let neck = Vec3::new(0.0, body().standing_neck(), 0.0);
        rig.update(&frame(neck, Vec2::ZERO), &RigTuning::new());
        let head = Quat::from_rotation_y(1.0);
        rig.recenter(neck - head * EYES_TO_NECK, head);
        assert!((rig.body_yaw().unwrap() - 1.0).abs() < 1e-5);
    }
}
