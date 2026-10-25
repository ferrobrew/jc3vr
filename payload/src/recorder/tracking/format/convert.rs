//! The conversion between the payload's recording types and the `tracking-format` crate's.
//!
//! Every struct is destructured in full and every enum matched without a wildcard, so a field or
//! variant added to a payload type fails to build here until the file format accounts for it.

use tracking_format::{self as file, model as wire};

use crate::{
    controllers::frame::{
        ActionSetKind, Button, ButtonSet, Hand, HandFrame, HandInputs, HandJoint,
        InteractionProfile, Pose, TrackedPose,
    },
    recorder::tracking::format::{
        ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, Recording,
        SemanticSample, SessionSample, StartMarker,
    },
    vr::{
        RecenterCause, RecenterKind,
        input::semantic::{
            SemanticSet,
            frame::{ActionState, ActionValue, SetState},
        },
        stage::StageInfo,
    },
};

pub(super) fn header_to_file(header: &Header) -> file::Header {
    let Header {
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
    } = header;
    file::Header {
        build: build.clone(),
        recorded_at: recorded_at.clone(),
        action_schema: *action_schema,
        start: file::StartMarker {
            transform: *transform,
            bookmark: bookmark.clone(),
        },
        dominant_hand: hand_to_file(*dominant_hand),
        world_scale: *world_scale,
        time_scale: *time_scale,
    }
}

pub(super) fn frame_to_file(frame: &Frame) -> file::Frame {
    let Frame {
        time_ns,
        display_time_ns,
        session:
            SessionSample {
                state,
                should_render,
                head_tracked,
            },
        head,
        stage,
        controllers,
        semantic,
        recenters,
    } = frame;
    file::Frame {
        time_ns: *time_ns,
        display_time_ns: *display_time_ns,
        session: file::SessionSample {
            state: *state,
            should_render: *should_render,
            head_tracked: *head_tracked,
        },
        head: head.map(|HeadSample { eyes }| file::HeadSample {
            eyes: eyes.map(|EyeSample { pose, fov }| file::EyeSample {
                pose: pose_to_file(pose),
                fov,
            }),
        }),
        stage: stage_to_file(stage),
        controllers: controllers.as_ref().map(
            |ControllerSample {
                 focused,
                 action_set,
                 hands,
             }| file::ControllerSample {
                focused: *focused,
                action_set: action_set_to_file(*action_set),
                hands: [hand_frame_to_file(&hands[0]), hand_frame_to_file(&hands[1])],
            },
        ),
        semantic: semantic.as_ref().map(
            |SemanticSample {
                 action_set,
                 global,
                 mode,
             }| file::SemanticSample {
                action_set: action_set_to_file(*action_set),
                global: set_state_to_file(global),
                mode: set_state_to_file(mode),
            },
        ),
        recenters: recenters
            .iter()
            .map(|&RecenterEvent { kind, cause, turn }| file::RecenterEvent {
                kind: match kind {
                    RecenterKind::Full => wire::RecenterKind::Full,
                    RecenterKind::YawAndHeight => wire::RecenterKind::YawAndHeight,
                },
                cause: match cause {
                    RecenterCause::Manual => wire::RecenterCause::Manual,
                    RecenterCause::SeatTransition => wire::RecenterCause::SeatTransition,
                },
                turn,
            })
            .collect(),
    }
}

pub(super) fn recording_from_file(recording: file::Recording) -> Recording {
    let file::Recording {
        header,
        frames,
        input,
        version,
        truncated,
        skipped_records,
    } = recording;
    Recording {
        header: header_from_file(header),
        frames: frames.into_iter().map(frame_from_file).collect(),
        input,
        version,
        truncated,
        skipped_records,
    }
}

fn hand_to_file(hand: Hand) -> wire::Hand {
    match hand {
        Hand::Left => wire::Hand::Left,
        Hand::Right => wire::Hand::Right,
    }
}

fn pose_to_file(pose: Pose) -> wire::Pose {
    let Pose {
        position,
        orientation,
    } = pose;
    wire::Pose {
        position,
        orientation,
    }
}

fn stage_to_file(stage: &StageInfo) -> wire::Stage {
    // The floor shift and jump count are the session's bookkeeping, already taken out of the head
    // height, so they are not recorded.
    let StageInfo {
        available,
        bounds,
        head_height,
        floor_shift_m: _,
        floor_jumps: _,
    } = *stage;
    wire::Stage {
        available,
        bounds,
        head_height,
    }
}

fn action_set_to_file(kind: ActionSetKind) -> wire::ActionSetKind {
    match kind {
        ActionSetKind::OnFoot => wire::ActionSetKind::OnFoot,
        ActionSetKind::Wingsuit => wire::ActionSetKind::Wingsuit,
        ActionSetKind::LandVehicle => wire::ActionSetKind::LandVehicle,
        ActionSetKind::Helicopter => wire::ActionSetKind::Helicopter,
        ActionSetKind::Plane => wire::ActionSetKind::Plane,
        ActionSetKind::Ui => wire::ActionSetKind::Ui,
    }
}

fn hand_frame_to_file(hand: &HandFrame) -> wire::HandFrame {
    let HandFrame {
        active,
        profile,
        grip,
        aim,
        inputs,
        buttons,
        joints,
    } = hand;
    wire::HandFrame {
        active: *active,
        profile: match profile {
            InteractionProfile::None => wire::InteractionProfile::None,
            InteractionProfile::ValveIndex => wire::InteractionProfile::ValveIndex,
            InteractionProfile::OculusTouch => wire::InteractionProfile::OculusTouch,
            InteractionProfile::Other => wire::InteractionProfile::Other,
        },
        grip: tracked_pose_to_file(grip),
        aim: tracked_pose_to_file(aim),
        inputs: inputs_to_file(inputs),
        buttons: buttons_to_file(*buttons),
        joints: joints.map(|joints| {
            joints.map(
                |HandJoint {
                     pose,
                     radius,
                     valid,
                     tracked,
                 }| wire::HandJoint {
                    pose: pose_to_file(pose),
                    radius,
                    valid,
                    tracked,
                },
            )
        }),
    }
}

fn tracked_pose_to_file(pose: &TrackedPose) -> wire::TrackedPose {
    let TrackedPose {
        pose,
        position_valid,
        orientation_valid,
        position_tracked,
        orientation_tracked,
        linear_velocity,
        angular_velocity,
    } = *pose;
    wire::TrackedPose {
        pose: pose_to_file(pose),
        position_valid,
        orientation_valid,
        position_tracked,
        orientation_tracked,
        linear_velocity,
        angular_velocity,
    }
}

fn inputs_to_file(inputs: &HandInputs) -> wire::HandInputs {
    let HandInputs {
        trigger,
        trigger_click,
        trigger_touch,
        squeeze,
        squeeze_force,
        thumbstick,
        thumbstick_click,
        thumbstick_touch,
        trackpad,
        trackpad_force,
        trackpad_touch,
        a_click,
        a_touch,
        b_click,
        b_touch,
        system_click,
    } = *inputs;
    wire::HandInputs {
        trigger,
        trigger_click,
        trigger_touch,
        squeeze,
        squeeze_force,
        thumbstick,
        thumbstick_click,
        thumbstick_touch,
        trackpad,
        trackpad_force,
        trackpad_touch,
        a_click,
        a_touch,
        b_click,
        b_touch,
        system_click,
    }
}

fn buttons_to_file(buttons: ButtonSet) -> wire::ButtonSet {
    let mut out = wire::ButtonSet::EMPTY;
    for button in Button::ALL {
        out.set(button_to_file(button), buttons.contains(button));
    }
    out
}

fn button_to_file(button: Button) -> wire::Button {
    match button {
        Button::TriggerClick => wire::Button::TriggerClick,
        Button::TriggerTouch => wire::Button::TriggerTouch,
        Button::TriggerPress => wire::Button::TriggerPress,
        Button::SqueezePress => wire::Button::SqueezePress,
        Button::ThumbstickClick => wire::Button::ThumbstickClick,
        Button::ThumbstickTouch => wire::Button::ThumbstickTouch,
        Button::TrackpadTouch => wire::Button::TrackpadTouch,
        Button::AClick => wire::Button::AClick,
        Button::ATouch => wire::Button::ATouch,
        Button::BClick => wire::Button::BClick,
        Button::BTouch => wire::Button::BTouch,
        Button::SystemClick => wire::Button::SystemClick,
        Button::TrackpadPressLeft => wire::Button::TrackpadPressLeft,
        Button::TrackpadPressRight => wire::Button::TrackpadPressRight,
    }
}

fn set_state_to_file(state: &SetState) -> wire::SetState {
    let SetState { set, actions } = state;
    wire::SetState {
        set: match set {
            SemanticSet::Global => wire::SemanticSet::Global,
            SemanticSet::OnFoot => wire::SemanticSet::OnFoot,
            SemanticSet::Wingsuit => wire::SemanticSet::Wingsuit,
            SemanticSet::LandVehicle => wire::SemanticSet::LandVehicle,
            SemanticSet::Helicopter => wire::SemanticSet::Helicopter,
            SemanticSet::Plane => wire::SemanticSet::Plane,
            SemanticSet::Menu => wire::SemanticSet::Menu,
        },
        actions: actions
            .iter()
            .map(|hands| hands.map(action_state_to_file))
            .collect(),
    }
}

fn action_state_to_file(state: ActionState) -> wire::ActionState {
    let ActionState {
        value,
        active,
        down,
    } = state;
    wire::ActionState {
        value: match value {
            ActionValue::Bool(value) => wire::ActionValue::Bool(value),
            ActionValue::Float(value) => wire::ActionValue::Float(value),
            ActionValue::Vec2(value) => wire::ActionValue::Vec2(value),
        },
        active,
        down,
    }
}

fn header_from_file(header: file::Header) -> Header {
    let file::Header {
        build,
        recorded_at,
        action_schema,
        start: file::StartMarker {
            transform,
            bookmark,
        },
        dominant_hand,
        world_scale,
        time_scale,
    } = header;
    Header {
        build,
        recorded_at,
        action_schema,
        start: StartMarker {
            transform,
            bookmark,
        },
        dominant_hand: hand_from_file(dominant_hand),
        world_scale,
        time_scale,
    }
}

fn frame_from_file(frame: file::Frame) -> Frame {
    let file::Frame {
        time_ns,
        display_time_ns,
        session:
            file::SessionSample {
                state,
                should_render,
                head_tracked,
            },
        head,
        stage,
        controllers,
        semantic,
        recenters,
    } = frame;
    Frame {
        time_ns,
        display_time_ns,
        session: SessionSample {
            state,
            should_render,
            head_tracked,
        },
        head: head.map(|file::HeadSample { eyes }| HeadSample {
            eyes: eyes.map(|file::EyeSample { pose, fov }| EyeSample {
                pose: pose_from_file(pose),
                fov,
            }),
        }),
        stage: stage_from_file(stage),
        controllers: controllers.map(
            |file::ControllerSample {
                 focused,
                 action_set,
                 hands,
             }| ControllerSample {
                focused,
                action_set: action_set_from_file(action_set),
                hands: hands.map(hand_frame_from_file),
            },
        ),
        semantic: semantic.map(
            |file::SemanticSample {
                 action_set,
                 global,
                 mode,
             }| SemanticSample {
                action_set: action_set_from_file(action_set),
                global: set_state_from_file(global),
                mode: set_state_from_file(mode),
            },
        ),
        recenters: recenters
            .into_iter()
            .map(|file::RecenterEvent { kind, cause, turn }| RecenterEvent {
                kind: match kind {
                    wire::RecenterKind::Full => RecenterKind::Full,
                    wire::RecenterKind::YawAndHeight => RecenterKind::YawAndHeight,
                },
                cause: match cause {
                    wire::RecenterCause::Manual => RecenterCause::Manual,
                    wire::RecenterCause::SeatTransition => RecenterCause::SeatTransition,
                },
                turn,
            })
            .collect(),
    }
}

fn hand_from_file(hand: wire::Hand) -> Hand {
    match hand {
        wire::Hand::Left => Hand::Left,
        wire::Hand::Right => Hand::Right,
    }
}

fn pose_from_file(pose: wire::Pose) -> Pose {
    let wire::Pose {
        position,
        orientation,
    } = pose;
    Pose {
        position,
        orientation,
    }
}

fn stage_from_file(stage: wire::Stage) -> StageInfo {
    let wire::Stage {
        available,
        bounds,
        head_height,
    } = stage;
    StageInfo {
        available,
        bounds,
        head_height,
        ..StageInfo::default()
    }
}

fn action_set_from_file(kind: wire::ActionSetKind) -> ActionSetKind {
    match kind {
        wire::ActionSetKind::OnFoot => ActionSetKind::OnFoot,
        wire::ActionSetKind::Wingsuit => ActionSetKind::Wingsuit,
        wire::ActionSetKind::LandVehicle => ActionSetKind::LandVehicle,
        wire::ActionSetKind::Helicopter => ActionSetKind::Helicopter,
        wire::ActionSetKind::Plane => ActionSetKind::Plane,
        wire::ActionSetKind::Ui => ActionSetKind::Ui,
    }
}

fn hand_frame_from_file(hand: wire::HandFrame) -> HandFrame {
    let wire::HandFrame {
        active,
        profile,
        grip,
        aim,
        inputs,
        buttons,
        joints,
    } = hand;
    HandFrame {
        active,
        profile: match profile {
            wire::InteractionProfile::None => InteractionProfile::None,
            wire::InteractionProfile::ValveIndex => InteractionProfile::ValveIndex,
            wire::InteractionProfile::OculusTouch => InteractionProfile::OculusTouch,
            wire::InteractionProfile::Other => InteractionProfile::Other,
        },
        grip: tracked_pose_from_file(grip),
        aim: tracked_pose_from_file(aim),
        inputs: inputs_from_file(inputs),
        buttons: buttons_from_file(buttons),
        joints: joints.map(|joints| {
            joints.map(
                |wire::HandJoint {
                     pose,
                     radius,
                     valid,
                     tracked,
                 }| HandJoint {
                    pose: pose_from_file(pose),
                    radius,
                    valid,
                    tracked,
                },
            )
        }),
    }
}

fn tracked_pose_from_file(pose: wire::TrackedPose) -> TrackedPose {
    let wire::TrackedPose {
        pose,
        position_valid,
        orientation_valid,
        position_tracked,
        orientation_tracked,
        linear_velocity,
        angular_velocity,
    } = pose;
    TrackedPose {
        pose: pose_from_file(pose),
        position_valid,
        orientation_valid,
        position_tracked,
        orientation_tracked,
        linear_velocity,
        angular_velocity,
    }
}

fn inputs_from_file(inputs: wire::HandInputs) -> HandInputs {
    let wire::HandInputs {
        trigger,
        trigger_click,
        trigger_touch,
        squeeze,
        squeeze_force,
        thumbstick,
        thumbstick_click,
        thumbstick_touch,
        trackpad,
        trackpad_force,
        trackpad_touch,
        a_click,
        a_touch,
        b_click,
        b_touch,
        system_click,
    } = inputs;
    HandInputs {
        trigger,
        trigger_click,
        trigger_touch,
        squeeze,
        squeeze_force,
        thumbstick,
        thumbstick_click,
        thumbstick_touch,
        trackpad,
        trackpad_force,
        trackpad_touch,
        a_click,
        a_touch,
        b_click,
        b_touch,
        system_click,
    }
}

fn buttons_from_file(buttons: wire::ButtonSet) -> ButtonSet {
    let mut out = ButtonSet::EMPTY;
    for button in Button::ALL {
        out.set(button, buttons.contains(button_to_file(button)));
    }
    out
}

fn set_state_from_file(state: wire::SetState) -> SetState {
    let wire::SetState { set, actions } = state;
    SetState {
        set: match set {
            wire::SemanticSet::Global => SemanticSet::Global,
            wire::SemanticSet::OnFoot => SemanticSet::OnFoot,
            wire::SemanticSet::Wingsuit => SemanticSet::Wingsuit,
            wire::SemanticSet::LandVehicle => SemanticSet::LandVehicle,
            wire::SemanticSet::Helicopter => SemanticSet::Helicopter,
            wire::SemanticSet::Plane => SemanticSet::Plane,
            wire::SemanticSet::Menu => SemanticSet::Menu,
        },
        actions: actions
            .into_iter()
            .map(|hands| hands.map(action_state_from_file))
            .collect(),
    }
}

fn action_state_from_file(state: wire::ActionState) -> ActionState {
    let wire::ActionState {
        value,
        active,
        down,
    } = state;
    ActionState {
        value: match value {
            wire::ActionValue::Bool(value) => ActionValue::Bool(value),
            wire::ActionValue::Float(value) => ActionValue::Float(value),
            wire::ActionValue::Vec2(value) => ActionValue::Vec2(value),
        },
        active,
        down,
    }
}
