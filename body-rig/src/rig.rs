//! The grounded rig: the pelvis solver, the step commit, the yaw, and the standing height,
//! advanced together once per frame.

use glam::{Quat, Vec2, Vec3};

use crate::{
    angle::{forward, head_heading, head_pitch},
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
    /// How far, `(x, z)`, the view's eye rig has carried Rico's pelvis from under the eyes it is
    /// read from, already in [`pelvis`](Self::pelvis): the view hangs the eyes from Rico's head in
    /// its own frame rather than the body's, so as the body turns under the view, Rico slides under
    /// the camera. The idle drift takes out each change of it whole, wherever it points, where it
    /// would leave a forward offset as a lean.
    pub slip: Vec2,
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
    /// The slip last seen, or `None` when the next frame's is the baseline.
    slip_seen: Option<Vec2>,
    /// The part of the neck's offset the slip's changes put there and no drift has taken out yet.
    slip_gap: Vec2,
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
            slip_seen: Some(Vec2::ZERO),
            slip_gap: Vec2::ZERO,
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
    /// starts facing the head, and the whole of its slip is still to be taken out, since the
    /// caller stood the eyes over the root without it.
    pub fn forget(&mut self) {
        self.step.stand();
        self.yaw = YawFollow::new();
        self.neck = None;
        self.head_speed_mps = 0.0;
        self.slip_seen = Some(Vec2::ZERO);
        self.slip_gap = Vec2::ZERO;
    }

    /// Turn the room frame by `turn` radians about +Y (new frame from old), leaving the body where
    /// it faces in the world. A turn of the frame re-bases the view's turn under the body, which
    /// changes the slip with the camera, so the next frame's slip is the baseline.
    pub fn rotate(&mut self, turn: f32) {
        self.yaw.rotate(turn);
        self.slip_seen = None;
        self.slip_gap = Vec2::ZERO;
    }

    /// Recenter on the head: the body faces the head's heading, the standing height starts again
    /// from the neck's height now, and any step stops. The caller puts the pelvis under the neck.
    pub fn recenter(&mut self, eyes: Vec3, head: Quat) {
        let neck = neck_pivot(eyes, head);
        self.height.restart_at(neck.y);
        self.step.stand();
        self.slip_seen = None;
        self.slip_gap = Vec2::ZERO;
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

    /// The idle drift of the neck's offset `offset` from the pelvis, for a body facing `body_yaw`,
    /// with the slip `slip`: the side and back of the offset the player made, as the solve gives
    /// it, and the whole of what the slip's changes added and nothing has taken out yet. That part
    /// shrinks as the offset along it does, whoever closes it.
    fn drift_with_slip(&mut self, slip: Vec2, offset: Vec2, body_yaw: f32) -> Vec2 {
        let slip = if slip.is_finite() { slip } else { Vec2::ZERO };
        let seen = self.slip_seen.unwrap_or(slip);
        self.slip_seen = Some(slip);
        // The pelvis moved by the change, so the offset to the neck changed by the opposite.
        self.slip_gap -= slip - seen;
        self.slip_gap = match self.slip_gap.try_normalize() {
            Some(along) if offset.is_finite() => {
                along * offset.dot(along).clamp(0.0, self.slip_gap.length())
            }
            _ => Vec2::ZERO,
        };
        let own = offset - self.slip_gap;
        let body_forward = forward(body_yaw);
        own - body_forward * own.dot(body_forward).max(0.0) + self.slip_gap
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
        let mut solve = solve(
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
        solve.drift = self.drift_with_slip(frame.slip, solve.offset, body_yaw);
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
                    head_pitch: head_pitch(frame.head),
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
            slip: Vec2::ZERO,
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

    /// Run the rig with a pelvis that follows the drive perfectly, standing with the neck at
    /// `neck`, while the view's eye rig carries the drawn pelvis `slip(t)` from the driven one.
    /// Returns the drawn pelvis and the last output.
    fn stand_with_slip(
        rig: &mut GroundedRig,
        seconds: f32,
        neck: Vec3,
        slip: impl Fn(f32) -> Vec2,
    ) -> (Vec2, RigOutput) {
        let tuning = RigTuning::new();
        let mut driven = Vec2::new(neck.x, neck.z);
        let mut out = RigOutput::default();
        for i in 0..(seconds / DT) as usize {
            let slip = slip(i as f32 * DT);
            let mut input = frame(neck, driven + slip);
            input.slip = slip;
            out = rig.update(&input, &tuning);
            driven += crate::step::drive_velocity(out.phase, out.gap, DT, &tuning.step) * DT;
        }
        (driven + slip(seconds), out)
    }

    #[test]
    fn a_slip_ahead_of_the_body_drifts_out_whole() {
        let mut rig = GroundedRig::new(body());
        let neck = Vec3::new(0.0, body().standing_neck(), 0.0);
        // The body faces -Z; the slip carries the pelvis 10 cm back over a second, which leaves the
        // neck ahead of it, as a lean would.
        let (pelvis, out) =
            stand_with_slip(&mut rig, 6.0, neck, |t| Vec2::new(0.0, 0.1 * t.min(1.0)));
        assert!(pelvis.length() < 0.03, "{pelvis:?}");
        assert_eq!(out.steps, 0);
    }

    #[test]
    fn a_lean_under_a_slip_keeps_the_lean() {
        let mut rig = GroundedRig::new(body());
        let lean = Vec3::new(0.0, body().standing_neck() - 0.01, -0.1);
        let (pelvis, _) = stand_with_slip(&mut rig, 3.0, lean, |_| Vec2::ZERO);
        let leaning = Vec2::new(lean.x, lean.z) - pelvis;
        let mut rig = GroundedRig::new(body());
        stand_with_slip(&mut rig, 3.0, lean, |_| Vec2::ZERO);
        let (pelvis, _) = stand_with_slip(&mut rig, 6.0, lean, |t| {
            Vec2::new(0.06 * t.min(1.0), 0.1 * t.min(1.0))
        });
        // Only the slip is taken out: the lean the player holds stays.
        let left = Vec2::new(lean.x, lean.z) - pelvis;
        assert!(
            (left - leaning).length() < 0.03,
            "{left:?} against {leaning:?}"
        );
    }

    #[test]
    fn a_slip_present_from_the_start_drifts_out() {
        let mut rig = GroundedRig::new(body());
        let neck = Vec3::new(0.0, body().standing_neck(), 0.0);
        let (pelvis, _) = stand_with_slip(&mut rig, 6.0, neck, |_| Vec2::new(-0.08, 0.08));
        assert!(pelvis.length() < 0.03, "{pelvis:?}");
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
