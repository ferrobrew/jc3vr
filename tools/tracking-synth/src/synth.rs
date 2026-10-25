//! A script sampled into a recording: one frame per display refresh, each holding what a headset
//! session would have published at that moment.
//!
//! Every frame is a focused, rendering session with the head and both controllers tracked. The
//! controllers are Index controllers in the on-foot mode set. The velocities are the poses' own
//! rates, measured across a millisecond each side of the frame. The semantic actions are `global`
//! and `on_foot`, every action at rest but the two a stick drives with the default bindings of a
//! right-handed player: `move` from the left stick and `turn` from the right stick's horizontal
//! axis. The first frame carries a full recenter, so a replay starts the posture, the rig's
//! chase, and the torso estimate afresh from the scripted head.

use std::{
    f32::consts::{PI, TAU},
    fmt,
};

use glam::{Quat, Vec2, Vec3};
use tracking_format::{
    ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, Recording,
    SemanticSample, SessionSample, SessionState, StartMarker,
    model::{
        ActionSetKind, ActionValue, ButtonSet, Hand, HandFrame, InteractionProfile, Pose,
        RecenterCause, RecenterKind, SemanticSet, Stage, TrackedPose,
    },
};

use crate::{
    inputs,
    rig::{self, RigPose},
    schema::{ActionKind, Schema, SchemaError},
    script::Script,
};

/// How a script is sampled.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Options {
    /// Frames per second.
    pub rate_hz: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            rate_hz: DEFAULT_RATE_HZ,
        }
    }
}

/// The frame rate of an Index headset, the default.
pub const DEFAULT_RATE_HZ: f32 = 90.0;

/// Sample `script` into a recording whose semantic states index `schema`'s actions.
pub fn recording(
    script: &Script,
    options: Options,
    schema: &Schema,
) -> Result<Recording, SynthError> {
    if !(options.rate_hz.is_finite() && (MIN_RATE_HZ..=MAX_RATE_HZ).contains(&options.rate_hz)) {
        return Err(SynthError::Rate(options.rate_hz));
    }
    let semantic = SemanticLayout::new(schema)?;
    let rate = f64::from(options.rate_hz);
    let duration = f64::from(script.duration_s());
    let count = (duration * rate).ceil() as u64 + 1;
    let period_ns = (1e9 / rate).round() as i64;

    let mut previous = [ButtonSet::EMPTY; 2];
    let mut frames = Vec::with_capacity(count as usize);
    for index in 0..count {
        let time_ns = (index as f64 * 1e9 / rate).round() as u64;
        let t = (index as f64 / rate) as f32;
        let pose = rig::solve(script, t);
        let hands = Hand::BOTH.map(|hand| {
            let inputs = inputs::sample(&script.hands[hand.index()], t);
            let buttons = inputs::buttons(&inputs, previous[hand.index()]);
            previous[hand.index()] = buttons;
            HandFrame {
                active: true,
                profile: InteractionProfile::ValveIndex,
                grip: tracked(t, |t| rig::solve(script, t).grips[hand.index()]),
                aim: tracked(t, |t| {
                    rig::aim_from_grip(rig::solve(script, t).grips[hand.index()])
                }),
                inputs,
                buttons,
                joints: None,
            }
        });
        let semantic = semantic.sample(schema, &hands);
        frames.push(Frame {
            time_ns,
            display_time_ns: i64::try_from(time_ns).unwrap_or(i64::MAX) + period_ns,
            session: SessionSample {
                state: Some(SessionState::FOCUSED),
                should_render: true,
                head_tracked: true,
            },
            head: Some(head(&pose)),
            stage: Stage {
                available: true,
                bounds: Some(PLAY_AREA_M),
                head_height: Some(pose.eye_height),
            },
            controllers: Some(ControllerSample {
                focused: true,
                action_set: ActionSetKind::OnFoot,
                hands,
            }),
            semantic: Some(semantic),
            recenters: if index == 0 {
                vec![RecenterEvent {
                    kind: RecenterKind::Full,
                    cause: RecenterCause::Manual,
                    turn: Quat::IDENTITY,
                }]
            } else {
                Vec::new()
            },
        });
    }
    Ok(Recording {
        header: header(script, schema),
        frames,
        input: Vec::new(),
        version: tracking_format::VERSION,
        truncated: false,
        skipped_records: 0,
    })
}

/// The bytes of a recording's file.
pub fn encode(recording: &Recording) -> Vec<u8> {
    let mut bytes = tracking_format::encode_latest_preamble(&recording.header);
    for frame in &recording.frames {
        bytes.extend(tracking_format::encode_latest_frame(frame));
    }
    bytes
}

/// Why a script could not be sampled.
#[derive(Debug, Clone, PartialEq)]
pub enum SynthError {
    /// The frame rate is outside what a headset runs at.
    Rate(f32),
    Schema(SchemaError),
    /// An action the sticks drive has another type in the schema than the generator writes.
    ActionType {
        name: &'static str,
        expected: ActionKind,
        found: ActionKind,
    },
}

impl fmt::Display for SynthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "synthesis: ")?;
        match self {
            SynthError::Rate(rate) => write!(
                f,
                "the frame rate {rate} Hz is outside {MIN_RATE_HZ} to {MAX_RATE_HZ} Hz"
            ),
            SynthError::Schema(error) => write!(f, "{error}"),
            SynthError::ActionType {
                name,
                expected,
                found,
            } => write!(
                f,
                "the on_foot action `{name}` is a {found:?} action, where a {expected:?} one is \
                 written"
            ),
        }
    }
}

impl std::error::Error for SynthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SynthError::Schema(error) => Some(error),
            SynthError::Rate(_) | SynthError::ActionType { .. } => None,
        }
    }
}

impl From<SchemaError> for SynthError {
    fn from(error: SchemaError) -> Self {
        SynthError::Schema(error)
    }
}

/// The lowest and highest frame rates a recording is sampled at.
const MIN_RATE_HZ: f32 = 10.0;
const MAX_RATE_HZ: f32 = 1000.0;

/// The play area reported: a room-scale setup's, larger than any script walks.
const PLAY_AREA_M: Vec2 = Vec2::new(4.0, 4.0);

/// The fields of view of an Index's eyes, `left, right, up, down` in radians, wider outward.
const EYE_FOV: [[f32; 4]; 2] = [[-0.94, 0.80, 0.96, -0.96], [-0.80, 0.94, 0.96, -0.96]];

/// The interval a velocity is measured across, each side of the frame.
const VELOCITY_H_S: f32 = 0.001;

/// Where the semantic actions the sticks drive are in the schema.
struct SemanticLayout {
    /// `on_foot.move`, a vector from the left stick.
    on_foot_move: usize,
    /// `on_foot.turn`, a scalar from the right stick's horizontal axis.
    on_foot_turn: usize,
}

impl SemanticLayout {
    fn new(schema: &Schema) -> Result<SemanticLayout, SynthError> {
        let find = |name: &'static str, expected: ActionKind| {
            let (index, found) = schema.find(SemanticSet::OnFoot, name)?;
            if found == expected {
                Ok(index)
            } else {
                Err(SynthError::ActionType {
                    name,
                    expected,
                    found,
                })
            }
        };
        Ok(SemanticLayout {
            on_foot_move: find("move", ActionKind::Vec2)?,
            on_foot_turn: find("turn", ActionKind::Float)?,
        })
    }

    fn sample(&self, schema: &Schema, hands: &[HandFrame; 2]) -> SemanticSample {
        let mut mode = schema.rest(SemanticSet::OnFoot);
        let left = Hand::Left.index();
        let right = Hand::Right.index();
        let movement = &mut mode.actions[self.on_foot_move][left];
        movement.value = ActionValue::Vec2(hands[left].inputs.thumbstick);
        movement.active = true;
        let turn = &mut mode.actions[self.on_foot_turn][right];
        turn.value = ActionValue::Float(hands[right].inputs.thumbstick.x);
        turn.active = true;
        SemanticSample {
            action_set: ActionSetKind::OnFoot,
            global: schema.rest(SemanticSet::Global),
            mode,
        }
    }
}

fn head(pose: &RigPose) -> HeadSample {
    let [left, right] = rig::eyes(pose.head);
    HeadSample {
        eyes: [
            EyeSample {
                pose: left,
                fov: EYE_FOV[0],
            },
            EyeSample {
                pose: right,
                fov: EYE_FOV[1],
            },
        ],
    }
}

/// A tracked pose at `t`, with the rates of `pose_at` about it.
fn tracked(t: f32, pose_at: impl Fn(f32) -> Pose) -> TrackedPose {
    let (before, now, after) = (
        pose_at(t - VELOCITY_H_S),
        pose_at(t),
        pose_at(t + VELOCITY_H_S),
    );
    let span = 2.0 * VELOCITY_H_S;
    // The turn from before to after, in the cockpit frame, as the base space's angular velocity.
    let (axis, angle) = (after.orientation * before.orientation.inverse()).to_axis_angle();
    let angle = if angle > PI { angle - TAU } else { angle };
    TrackedPose {
        pose: now,
        position_valid: true,
        orientation_valid: true,
        position_tracked: true,
        orientation_tracked: true,
        linear_velocity: Some((after.position - before.position) / span),
        angular_velocity: Some(if angle.abs() < 1e-9 {
            Vec3::ZERO
        } else {
            axis * (angle / span)
        }),
    }
}

fn header(script: &Script, schema: &Schema) -> Header {
    let mut transform = [0.0; 16];
    for diagonal in [0, 5, 10, 15] {
        transform[diagonal] = 1.0;
    }
    Header {
        build: format!("tracking-synth {}", env!("CARGO_PKG_VERSION")),
        recorded_at: format!("synthetic {}", script.name),
        action_schema: schema.version,
        // A synthetic recording was made nowhere in the world: it replays in place, and the
        // marker is the identity at the world's origin.
        start: StartMarker {
            transform,
            bookmark: None,
        },
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    }
}
