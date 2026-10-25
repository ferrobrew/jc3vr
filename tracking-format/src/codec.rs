//! The encoding of each of the tracking file's types, in both directions.

use crate::{
    ControllerSample, EyeSample, Frame, FrameSchema, HeadSample, Header, RecenterEvent,
    SemanticSample, SessionSample, SessionState, StartMarker,
    model::{
        ActionSetKind, ActionState, ActionValue, Button, ButtonSet, HAND_JOINT_COUNT, Hand,
        HandFrame, HandInputs, HandJoint, InteractionProfile, RecenterCause, RecenterKind,
        SemanticSet, SetState, Stage, TrackedPose,
    },
    wire::{DecodeError, DecodeErrorKind, Decoder, Encoder},
};

pub(crate) fn header(out: &mut Encoder, header: &Header) {
    out.str(&header.build);
    out.str(&header.recorded_at);
    out.u32(header.action_schema);
    for value in header.start.transform {
        out.f32(value);
    }
    match &header.start.bookmark {
        Some(name) => {
            out.bool(true);
            out.str(name);
        }
        None => out.bool(false),
    }
    out.u8(hand_code(header.dominant_hand));
    out.f32(header.world_scale);
    out.f32(header.time_scale);
}

pub(crate) fn read_header(input: &mut Decoder) -> Result<Header, DecodeError> {
    let build = input.str("the build stamp")?;
    let recorded_at = input.str("the recording stamp")?;
    let action_schema = input.u32()?;
    let mut transform = [0.0; 16];
    for value in &mut transform {
        *value = input.f32()?;
    }
    let bookmark = if input.bool("the bookmark flag")? {
        Some(input.str("the bookmark name")?)
    } else {
        None
    };
    let dominant_hand = read_hand(input, "the dominant hand")?;
    let world_scale = input.f32()?;
    let time_scale = input.f32()?;
    Ok(Header {
        build,
        recorded_at,
        action_schema,
        start: StartMarker {
            transform,
            bookmark,
        },
        dominant_hand,
        world_scale,
        time_scale,
    })
}

pub(crate) fn frame(out: &mut Encoder, frame: &Frame, schema: FrameSchema) {
    out.u64(frame.time_ns);
    out.i64(frame.display_time_ns);
    let mut flags = 0;
    set_bit(&mut flags, FRAME_HEAD, frame.head.is_some());
    set_bit(&mut flags, FRAME_CONTROLLERS, frame.controllers.is_some());
    set_bit(&mut flags, FRAME_SEMANTIC, frame.semantic.is_some());
    set_bit(&mut flags, FRAME_SHOULD_RENDER, frame.session.should_render);
    set_bit(&mut flags, FRAME_HEAD_TRACKED, frame.session.head_tracked);
    set_bit(
        &mut flags,
        FRAME_SESSION_STATE,
        frame.session.state.is_some(),
    );
    out.u8(flags);
    if let Some(state) = frame.session.state {
        out.i32(state.into_raw());
    }
    if let Some(head) = &frame.head {
        for eye in &head.eyes {
            out.pose(&eye.pose);
            for angle in eye.fov {
                out.f32(angle);
            }
        }
    }
    stage(out, &frame.stage);
    if let Some(controllers) = &frame.controllers {
        out.bool(controllers.focused);
        out.u8(controllers.action_set.index() as u8);
        for hand in &controllers.hands {
            hand_frame(out, hand, schema);
        }
    }
    if let Some(semantic) = &frame.semantic {
        out.u8(semantic.action_set.index() as u8);
        set_state(out, &semantic.global);
        set_state(out, &semantic.mode);
    }
    out.u8(frame.recenters.len().min(usize::from(u8::MAX)) as u8);
    for event in frame.recenters.iter().take(usize::from(u8::MAX)) {
        out.u8(match event.kind {
            RecenterKind::Full => 0,
            RecenterKind::YawAndHeight => 1,
        });
        out.u8(match event.cause {
            RecenterCause::Manual => 0,
            RecenterCause::SeatTransition => 1,
        });
        out.quat(event.turn);
    }
}

pub(crate) fn read_frame(input: &mut Decoder, schema: FrameSchema) -> Result<Frame, DecodeError> {
    let time_ns = input.u64()?;
    let display_time_ns = input.i64()?;
    let flags = input.u8()?;
    let state = if bit(flags, FRAME_SESSION_STATE) {
        Some(SessionState::from_raw(input.i32()?))
    } else {
        None
    };
    let head = if bit(flags, FRAME_HEAD) {
        let mut eye = || -> Result<EyeSample, DecodeError> {
            let pose = input.pose()?;
            let fov = [input.f32()?, input.f32()?, input.f32()?, input.f32()?];
            Ok(EyeSample { pose, fov })
        };
        Some(HeadSample {
            eyes: [eye()?, eye()?],
        })
    } else {
        None
    };
    let stage = read_stage(input)?;
    let controllers = if bit(flags, FRAME_CONTROLLERS) {
        let focused = input.bool("the focus flag")?;
        let action_set = read_action_set(input)?;
        let left = read_hand_frame(input, schema)?;
        let right = read_hand_frame(input, schema)?;
        Some(ControllerSample {
            focused,
            action_set,
            hands: [left, right],
        })
    } else {
        None
    };
    let semantic = if bit(flags, FRAME_SEMANTIC) {
        let action_set = read_action_set(input)?;
        let global = read_set_state(input)?;
        let mode = read_set_state(input)?;
        Some(SemanticSample {
            action_set,
            global,
            mode,
        })
    } else {
        None
    };
    let count = input.u8()?;
    if usize::from(count) > input.remaining() / 18 {
        return Err(input.error(crate::DecodeErrorKind::Truncated {
            needed: usize::from(count) * 18,
            available: input.remaining(),
        }));
    }
    let mut recenters = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let offset = input.offset();
        let kind = match input.u8()? {
            0 => RecenterKind::Full,
            1 => RecenterKind::YawAndHeight,
            value => return Err(invalid(offset, "a recenter's kind", value.into())),
        };
        let offset = input.offset();
        let cause = match input.u8()? {
            0 => RecenterCause::Manual,
            1 => RecenterCause::SeatTransition,
            value => return Err(invalid(offset, "a recenter's cause", value.into())),
        };
        let turn = input.quat()?;
        recenters.push(RecenterEvent { kind, cause, turn });
    }
    Ok(Frame {
        time_ns,
        display_time_ns,
        session: SessionSample {
            state,
            should_render: bit(flags, FRAME_SHOULD_RENDER),
            head_tracked: bit(flags, FRAME_HEAD_TRACKED),
        },
        head,
        stage,
        controllers,
        semantic,
        recenters,
    })
}

const FRAME_HEAD: u8 = 0;
const FRAME_CONTROLLERS: u8 = 1;
const FRAME_SEMANTIC: u8 = 2;
const FRAME_SHOULD_RENDER: u8 = 3;
const FRAME_HEAD_TRACKED: u8 = 4;
const FRAME_SESSION_STATE: u8 = 5;

fn set_bit<T>(flags: &mut T, index: u8, on: bool)
where
    T: Copy + std::ops::BitOrAssign + From<u8> + std::ops::Shl<u8, Output = T>,
{
    if on {
        *flags |= T::from(1) << index;
    }
}

fn bit<T>(flags: T, index: u8) -> bool
where
    T: Copy + std::ops::BitAnd<Output = T> + From<u8> + std::ops::Shl<u8, Output = T> + PartialEq,
{
    flags & (T::from(1) << index) != T::from(0)
}

fn invalid(offset: usize, field: &'static str, value: u64) -> DecodeError {
    DecodeError {
        offset,
        kind: DecodeErrorKind::InvalidValue { field, value },
    }
}

fn hand_code(hand: Hand) -> u8 {
    match hand {
        Hand::Left => 0,
        Hand::Right => 1,
    }
}

fn read_hand(input: &mut Decoder, field: &'static str) -> Result<Hand, DecodeError> {
    let offset = input.offset();
    match input.u8()? {
        0 => Ok(Hand::Left),
        1 => Ok(Hand::Right),
        value => Err(invalid(offset, field, value.into())),
    }
}

fn read_action_set(input: &mut Decoder) -> Result<ActionSetKind, DecodeError> {
    let offset = input.offset();
    let index = input.u8()?;
    ActionSetKind::ALL
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| invalid(offset, "the action set", index.into()))
}

fn stage(out: &mut Encoder, stage: &Stage) {
    let mut flags = 0u8;
    set_bit(&mut flags, 0, stage.available);
    set_bit(&mut flags, 1, stage.bounds.is_some());
    set_bit(&mut flags, 2, stage.head_height.is_some());
    out.u8(flags);
    if let Some(bounds) = stage.bounds {
        out.vec2(bounds);
    }
    if let Some(height) = stage.head_height {
        out.f32(height);
    }
}

fn read_stage(input: &mut Decoder) -> Result<Stage, DecodeError> {
    let flags = input.u8()?;
    let bounds = bit(flags, 1).then(|| input.vec2()).transpose()?;
    let head_height = bit(flags, 2).then(|| input.f32()).transpose()?;
    Ok(Stage {
        available: bit(flags, 0),
        bounds,
        head_height,
    })
}

fn hand_frame(out: &mut Encoder, hand: &HandFrame, schema: FrameSchema) {
    let mut flags = 0u8;
    set_bit(&mut flags, 0, hand.active);
    set_bit(&mut flags, 1, hand.joints.is_some());
    out.u8(flags);
    out.u8(match hand.profile {
        InteractionProfile::None => 0,
        InteractionProfile::ValveIndex => 1,
        InteractionProfile::OculusTouch => 2,
        InteractionProfile::Other => 3,
    });
    tracked_pose(out, &hand.grip);
    tracked_pose(out, &hand.aim);
    inputs(out, &hand.inputs);
    let mut buttons = 0u16;
    for (index, button) in Button::ALL.iter().enumerate() {
        set_bit(&mut buttons, index as u8, hand.buttons.contains(*button));
    }
    out.u16(buttons);
    if let Some(joints) = &hand.joints {
        for joint in joints {
            out.pose(&joint.pose);
            out.f32(joint.radius);
            let mut flags = 0u8;
            set_bit(&mut flags, 0, joint.valid());
            set_bit(&mut flags, 1, joint.tracked());
            if schema == FrameSchema::V4 {
                set_bit(&mut flags, 2, joint.position_valid);
                set_bit(&mut flags, 3, joint.orientation_valid);
                set_bit(&mut flags, 4, joint.position_tracked);
                set_bit(&mut flags, 5, joint.orientation_tracked);
            }
            out.u8(flags);
        }
    }
}

fn read_hand_frame(input: &mut Decoder, schema: FrameSchema) -> Result<HandFrame, DecodeError> {
    let flags = input.u8()?;
    let offset = input.offset();
    let profile = match input.u8()? {
        0 => InteractionProfile::None,
        1 => InteractionProfile::ValveIndex,
        2 => InteractionProfile::OculusTouch,
        3 => InteractionProfile::Other,
        value => {
            return Err(invalid(
                offset,
                "a hand's interaction profile",
                value.into(),
            ));
        }
    };
    let grip = read_tracked_pose(input)?;
    let aim = read_tracked_pose(input)?;
    let inputs = read_inputs(input)?;
    let offset = input.offset();
    let bits = input.u16()?;
    if bits >> Button::ALL.len() != 0 {
        return Err(invalid(offset, "a hand's buttons", bits.into()));
    }
    let mut buttons = ButtonSet::EMPTY;
    for (index, button) in Button::ALL.iter().enumerate() {
        buttons.set(*button, bit(bits, index as u8));
    }
    let joints = if bit(flags, 1) {
        let mut joints = [HandJoint::default(); HAND_JOINT_COUNT];
        for joint in &mut joints {
            let pose = input.pose()?;
            let radius = input.f32()?;
            let offset = input.offset();
            let flags = input.u8()?;
            let (position_valid, orientation_valid, position_tracked, orientation_tracked) =
                match schema {
                    FrameSchema::Legacy => {
                        (bit(flags, 0), bit(flags, 0), bit(flags, 1), bit(flags, 1))
                    }
                    FrameSchema::V4 => {
                        if flags & 0xc0 != 0 {
                            return Err(invalid(offset, "joint reserved flags", flags.into()));
                        }
                        let components =
                            (bit(flags, 2), bit(flags, 3), bit(flags, 4), bit(flags, 5));
                        if bit(flags, 0) != (components.0 && components.1)
                            || bit(flags, 1) != (components.2 && components.3)
                        {
                            return Err(invalid(offset, "joint aggregate flags", flags.into()));
                        }
                        components
                    }
                };
            *joint = HandJoint {
                pose,
                radius,
                position_valid,
                orientation_valid,
                position_tracked,
                orientation_tracked,
            };
        }
        Some(joints)
    } else {
        None
    };
    Ok(HandFrame {
        active: bit(flags, 0),
        profile,
        grip,
        aim,
        inputs,
        buttons,
        joints,
    })
}

fn tracked_pose(out: &mut Encoder, pose: &TrackedPose) {
    let mut flags = 0u8;
    set_bit(&mut flags, 0, pose.position_valid);
    set_bit(&mut flags, 1, pose.orientation_valid);
    set_bit(&mut flags, 2, pose.position_tracked);
    set_bit(&mut flags, 3, pose.orientation_tracked);
    set_bit(&mut flags, 4, pose.linear_velocity.is_some());
    set_bit(&mut flags, 5, pose.angular_velocity.is_some());
    out.u8(flags);
    out.pose(&pose.pose);
    if let Some(v) = pose.linear_velocity {
        out.vec3(v);
    }
    if let Some(v) = pose.angular_velocity {
        out.vec3(v);
    }
}

fn read_tracked_pose(input: &mut Decoder) -> Result<TrackedPose, DecodeError> {
    let flags = input.u8()?;
    let pose = input.pose()?;
    let linear_velocity = bit(flags, 4).then(|| input.vec3()).transpose()?;
    let angular_velocity = bit(flags, 5).then(|| input.vec3()).transpose()?;
    Ok(TrackedPose {
        pose,
        position_valid: bit(flags, 0),
        orientation_valid: bit(flags, 1),
        position_tracked: bit(flags, 2),
        orientation_tracked: bit(flags, 3),
        linear_velocity,
        angular_velocity,
    })
}

fn inputs(out: &mut Encoder, inputs: &HandInputs) {
    out.f32(inputs.trigger);
    out.f32(inputs.squeeze);
    out.f32(inputs.squeeze_force);
    out.vec2(inputs.thumbstick);
    out.vec2(inputs.trackpad);
    out.f32(inputs.trackpad_force);
    let mut flags = 0u16;
    for (index, value) in input_bools(inputs).into_iter().enumerate() {
        set_bit(&mut flags, index as u8, value);
    }
    out.u16(flags);
}

fn read_inputs(input: &mut Decoder) -> Result<HandInputs, DecodeError> {
    let trigger = input.f32()?;
    let squeeze = input.f32()?;
    let squeeze_force = input.f32()?;
    let thumbstick = input.vec2()?;
    let trackpad = input.vec2()?;
    let trackpad_force = input.f32()?;
    let offset = input.offset();
    let flags = input.u16()?;
    if flags >> INPUT_BOOLS != 0 {
        return Err(invalid(offset, "a hand's input switches", flags.into()));
    }
    let b = |index: u8| bit(flags, index);
    Ok(HandInputs {
        trigger,
        trigger_click: b(0),
        trigger_touch: b(1),
        squeeze,
        squeeze_force,
        thumbstick,
        thumbstick_click: b(2),
        thumbstick_touch: b(3),
        trackpad,
        trackpad_force,
        trackpad_touch: b(4),
        a_click: b(5),
        a_touch: b(6),
        b_click: b(7),
        b_touch: b(8),
        system_click: b(9),
    })
}

/// The number of boolean inputs [`input_bools`] lists.
const INPUT_BOOLS: usize = 10;

/// The boolean inputs, in their bit order.
fn input_bools(inputs: &HandInputs) -> [bool; INPUT_BOOLS] {
    [
        inputs.trigger_click,
        inputs.trigger_touch,
        inputs.thumbstick_click,
        inputs.thumbstick_touch,
        inputs.trackpad_touch,
        inputs.a_click,
        inputs.a_touch,
        inputs.b_click,
        inputs.b_touch,
        inputs.system_click,
    ]
}

fn set_state(out: &mut Encoder, state: &SetState) {
    out.u8(state.set.index() as u8);
    out.u16(state.actions.len().min(usize::from(u16::MAX)) as u16);
    for hands in state.actions.iter().take(usize::from(u16::MAX)) {
        for action in hands {
            match action.value {
                ActionValue::Bool(value) => {
                    out.u8(0);
                    out.bool(value);
                }
                ActionValue::Float(value) => {
                    out.u8(1);
                    out.f32(value);
                }
                ActionValue::Vec2(value) => {
                    out.u8(2);
                    out.vec2(value);
                }
            }
            let mut flags = 0u8;
            set_bit(&mut flags, 0, action.active);
            set_bit(&mut flags, 1, action.down);
            out.u8(flags);
        }
    }
}

fn read_set_state(input: &mut Decoder) -> Result<SetState, DecodeError> {
    let offset = input.offset();
    let index = input.u8()?;
    let set = SemanticSet::ALL
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| invalid(offset, "a semantic set", index.into()))?;
    let count = input.u16()?;
    if usize::from(count) > input.remaining() / 6 {
        return Err(input.error(crate::DecodeErrorKind::Truncated {
            needed: usize::from(count) * 6,
            available: input.remaining(),
        }));
    }
    let mut actions = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let mut action = || -> Result<ActionState, DecodeError> {
            let offset = input.offset();
            let value = match input.u8()? {
                0 => ActionValue::Bool(input.bool("a boolean action's value")?),
                1 => ActionValue::Float(input.f32()?),
                2 => ActionValue::Vec2(input.vec2()?),
                value => return Err(invalid(offset, "an action's type", value.into())),
            };
            let flags = input.u8()?;
            Ok(ActionState {
                value,
                active: bit(flags, 0),
                down: bit(flags, 1),
            })
        };
        actions.push([action()?, action()?]);
    }
    Ok(SetState { set, actions })
}
