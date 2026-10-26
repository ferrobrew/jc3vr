//! Independent head-and-hand torso filter for the historical rig tests.
//!
//! The head heading and horizontal hand directions infer a torso heading, following Parger et al.
//! (2018). The filter applies the neck limit and critically damped heading response independently
//! of the simulation-time constrained body fit.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    body::{
        self, BodyDimensions,
        rig::{
            config::TorsoConfig,
            torso::{
                EYES_TO_NECK_PIVOT, EYES_TO_NECK_PIVOT_BACK, EYES_TO_NECK_PIVOT_DROP,
                HIP_SPACING_FRACTION, TorsoEstimate, head_heading, wrap, yaw_forward, yaw_of,
            },
        },
    },
    weapons::volumes::HIP_HEIGHT_FRACTION,
};

/// One update's inputs, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct TorsoInputs {
    /// The head (the eye midpoint).
    pub head_position: Vec3,
    pub head_orientation: Quat,
    /// The tracked hands' grip positions, left then right, `None` for a hand that is not tracked.
    pub hands: [Option<Vec3>; 2],
}

/// The estimator's state: the filtered heading and its rate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct TorsoEstimator {
    state: Option<State>,
}

impl TorsoEstimator {
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// Forget the held heading, so the next update starts from the target.
    pub fn reset(&mut self) {
        self.state = None;
    }

    /// Turn the held heading by `turn` about +Y, for a cockpit frame that turned by it (new frame
    /// from old), so that the torso stays where it is in the room.
    pub fn rotate(&mut self, turn: Quat) {
        if let Some(state) = self.state.as_mut()
            && let Some(yaw) = yaw_of(turn * yaw_forward(state.yaw))
        {
            state.yaw = yaw;
        }
    }

    /// Advance by `dt` seconds and return the estimate, or `None` when the inputs carry no
    /// heading (a non-finite head). `dims` are the player's body dimensions.
    pub fn update(
        &mut self,
        inputs: &TorsoInputs,
        dt: f32,
        dims: &BodyDimensions,
        config: &TorsoConfig,
    ) -> Option<TorsoEstimate> {
        let head = inputs.head_position;
        let orientation = inputs.head_orientation.normalize();
        if !head.is_finite() || !orientation.is_finite() {
            return None;
        }
        let head_yaw = head_heading(orientation)?;
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        let neck = head + orientation * EYES_TO_NECK_PIVOT;
        let limit = config.neck_limit_deg.clamp(0.0, 180.0).to_radians();

        let (hands, hands_weight) = hands_pull(inputs.hands, neck);
        let pull = flat(yaw_forward(head_yaw)) * config.head_weight.max(0.0) + hands;
        let target = yaw_of(Vec3::new(pull.x, 0.0, pull.y)).unwrap_or(head_yaw);
        let target = within_neck(target, head_yaw, limit);

        let state = self.state.get_or_insert(State {
            yaw: target,
            rate: 0.0,
        });
        follow(state, target, dt, config.follow_s);
        let twist = wrap(head_yaw - state.yaw);
        if twist.abs() > limit {
            // The neck limit is anatomy, not a preference: it holds at once, whatever the filter.
            state.yaw = wrap(head_yaw - twist.signum() * limit);
            state.rate = 0.0;
        }
        let yaw = state.yaw;

        let torso = Quat::from_rotation_y(yaw);
        let back = torso * Vec3::Z;
        let right = torso * Vec3::X;
        let chest = neck
            + Vec3::Y * (EYES_TO_NECK_PIVOT_DROP - (dims.eye_height - dims.shoulder_height))
            + back * (body::EYES_FORWARD_OF_SHOULDERS - EYES_TO_NECK_PIVOT_BACK);
        let half_shoulders = right * (dims.shoulder_width * 0.5);
        let shoulders = [chest - half_shoulders, chest + half_shoulders];
        let spine_length = (dims.shoulder_height - dims.height * HIP_HEIGHT_FRACTION).max(0.1);
        let pelvis = chest - Vec3::Y * spine_length;
        let half_hips = right * (dims.shoulder_width * HIP_SPACING_FRACTION * 0.5);
        let hips = [pelvis - half_hips, pelvis + half_hips];

        Some(TorsoEstimate {
            yaw,
            head_yaw,
            neck_twist: wrap(head_yaw - yaw),
            hands_yaw: (hands_weight > MIN_HANDS_WEIGHT)
                .then(|| yaw_of(Vec3::new(hands.x, 0.0, hands.y)))
                .flatten(),
            hands_weight,
            neck,
            shoulders,
            chest,
            pelvis,
            hips,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct State {
    yaw: f32,
    rate: f32,
}

/// The hands' weight below which they have no heading of their own.
const MIN_HANDS_WEIGHT: f32 = 1e-3;

/// The longest step the heading's filter integrates at once, seconds, so that it stays stable at
/// any frame rate.
const FILTER_STEP_S: f32 = 0.005;

/// The hands' horizontal pull on the heading, `(x, z)`, and its total weight: each tracked hand's
/// direction from the neck, normalized, flattened, and scaled by the square of its horizontal
/// share, so a hand held out counts nearly in full and a hand hanging down nearly not at all.
fn hands_pull(hands: [Option<Vec3>; 2], neck: Vec3) -> (Vec2, f32) {
    let mut pull = Vec2::ZERO;
    let mut weight = 0.0;
    for hand in hands.into_iter().flatten() {
        let direction = (hand - neck).normalize_or_zero();
        let horizontal = flat(direction);
        if !horizontal.is_finite() {
            continue;
        }
        let share = horizontal.length();
        pull += horizontal * (share * share);
        weight += share * share * share;
    }
    (pull, weight)
}

/// `target` held within `limit` of the head's heading.
fn within_neck(target: f32, head_yaw: f32, limit: f32) -> f32 {
    let off = wrap(target - head_yaw);
    wrap(head_yaw + off.clamp(-limit, limit))
}

/// Step the critically damped filter toward `target` over `dt`, with the time constant `tau`.
fn follow(state: &mut State, target: f32, dt: f32, tau: f32) {
    if !(tau.is_finite() && tau > 1.0e-4) {
        state.yaw = target;
        state.rate = 0.0;
        return;
    }
    let omega = 1.0 / tau;
    let steps = (dt / FILTER_STEP_S).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    for _ in 0..steps {
        let error = wrap(target - state.yaw);
        state.rate += (omega * omega * error - 2.0 * omega * state.rate) * h;
        state.yaw = wrap(state.yaw + state.rate * h);
    }
}

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}
