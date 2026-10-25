//! The values a [`Frame`](crate::Frame) carries: the controllers' poses and inputs, the STAGE
//! information, the recenters' kinds, and the semantic actions' states.
//!
//! These are the file's own types rather than the payload's, so the format does not change when
//! the payload's types do. The payload converts between the two exhaustively, so a field added on
//! either side fails to build until the format, and its version, account for it. The orders that
//! the encoding depends on ([`ActionSetKind::ALL`], [`Button::ALL`], [`SemanticSet::ALL`]) are the
//! payload's, and are part of the format.

use glam::{Quat, Vec2, Vec3};

/// A hand, and the index of its entry in the per-hand arrays. The left hand is index 0.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    /// Both hands, in index order.
    pub const BOTH: [Hand; 2] = [Hand::Left, Hand::Right];

    /// The index of this hand in the per-hand arrays.
    pub const fn index(self) -> usize {
        match self {
            Hand::Left => 0,
            Hand::Right => 1,
        }
    }
}

/// The mode action set that was active when a frame was sampled.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ActionSetKind {
    /// On foot, swimming, falling, and the parachute.
    OnFoot,
    Wingsuit,
    /// Seated in a car, a motorcycle, a boat, or a vehicle of no resolved class.
    LandVehicle,
    Helicopter,
    Plane,
    /// A game menu or the native map.
    Ui,
}

impl ActionSetKind {
    /// Every kind, in index order.
    pub const ALL: [ActionSetKind; 6] = [
        ActionSetKind::OnFoot,
        ActionSetKind::Wingsuit,
        ActionSetKind::LandVehicle,
        ActionSetKind::Helicopter,
        ActionSetKind::Plane,
        ActionSetKind::Ui,
    ];

    /// The index of this kind in [`ALL`](Self::ALL).
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The interaction profile the runtime bound a hand to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum InteractionProfile {
    /// No profile is bound.
    #[default]
    None,
    /// `/interaction_profiles/valve/index_controller`.
    ValveIndex,
    /// `/interaction_profiles/oculus/touch_controller`.
    OculusTouch,
    /// A profile the mod ships no suggested bindings for.
    Other,
}

/// A rigid transform in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Pose {
    /// Metres.
    pub position: Vec3,
    pub orientation: Quat,
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        position: Vec3::ZERO,
        orientation: Quat::IDENTITY,
    };
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A located controller space with its validity and tracking flags.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TrackedPose {
    pub pose: Pose,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_tracked: bool,
    pub orientation_tracked: bool,
    /// Metres per second in the cockpit frame, when the runtime reported it.
    pub linear_velocity: Option<Vec3>,
    /// Radians per second about each cockpit axis, when the runtime reported it.
    pub angular_velocity: Option<Vec3>,
}

/// Every raw input of one controller.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandInputs {
    pub trigger: f32,
    pub trigger_click: bool,
    pub trigger_touch: bool,
    /// How far the fingers close round the handle.
    pub squeeze: f32,
    /// How hard a force-sensing grip is squeezed.
    pub squeeze_force: f32,
    /// `(right, forward)`, each in `-1..=1`.
    pub thumbstick: Vec2,
    pub thumbstick_click: bool,
    pub thumbstick_touch: bool,
    pub trackpad: Vec2,
    pub trackpad_force: f32,
    pub trackpad_touch: bool,
    pub a_click: bool,
    pub a_touch: bool,
    pub b_click: bool,
    pub b_touch: bool,
    pub system_click: bool,
}

/// A digital input of one controller, as a bit in a [`ButtonSet`], including the presses derived
/// from the analog inputs.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    TriggerClick,
    TriggerTouch,
    TriggerPress,
    SqueezePress,
    ThumbstickClick,
    ThumbstickTouch,
    TrackpadTouch,
    AClick,
    ATouch,
    BClick,
    BTouch,
    SystemClick,
    TrackpadPressLeft,
    TrackpadPressRight,
}

impl Button {
    /// Every button, in bit order.
    pub const ALL: [Button; 14] = [
        Button::TriggerClick,
        Button::TriggerTouch,
        Button::TriggerPress,
        Button::SqueezePress,
        Button::ThumbstickClick,
        Button::ThumbstickTouch,
        Button::TrackpadTouch,
        Button::AClick,
        Button::ATouch,
        Button::BClick,
        Button::BTouch,
        Button::SystemClick,
        Button::TrackpadPressLeft,
        Button::TrackpadPressRight,
    ];

    const fn bit(self) -> u16 {
        1 << (self as u8)
    }
}

/// A set of [`Button`]s, one bit each.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct ButtonSet(u16);

impl ButtonSet {
    pub const EMPTY: ButtonSet = ButtonSet(0);

    pub const fn contains(self, button: Button) -> bool {
        self.0 & button.bit() != 0
    }

    pub fn set(&mut self, button: Button, down: bool) {
        if down {
            self.0 |= button.bit();
        } else {
            self.0 &= !button.bit();
        }
    }
}

/// The number of joints a tracked hand reports.
pub const HAND_JOINT_COUNT: usize = 26;

/// One tracked hand joint, in the `XrHandJointEXT` order.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandJoint {
    /// The joint pose in the cockpit frame.
    pub pose: Pose,
    /// The joint's radius, metres.
    pub radius: f32,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_tracked: bool,
    pub orientation_tracked: bool,
}

impl HandJoint {
    /// Whether both position and orientation are valid.
    pub const fn valid(&self) -> bool {
        self.position_valid && self.orientation_valid
    }

    /// Whether both position and orientation are tracked.
    pub const fn tracked(&self) -> bool {
        self.position_tracked && self.orientation_tracked
    }
}

/// One controller's snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandFrame {
    /// Whether the runtime reports this hand's controller as present.
    pub active: bool,
    pub profile: InteractionProfile,
    /// The OpenXR grip pose.
    pub grip: TrackedPose,
    /// The OpenXR aim pose.
    pub aim: TrackedPose,
    pub inputs: HandInputs,
    /// The digital state, including the derived presses.
    pub buttons: ButtonSet,
    /// The tracked hand skeleton, when hand tracking reported it.
    pub joints: Option<[HandJoint; HAND_JOINT_COUNT]>,
}

/// What the runtime reported about its STAGE space.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Stage {
    /// Whether the session has a STAGE space at all.
    pub available: bool,
    /// The play area's extent (width along X, depth along Z), metres, when the runtime reports
    /// bounds.
    pub bounds: Option<Vec2>,
    /// The head's height above the STAGE floor, metres, when the STAGE space was located.
    pub head_height: Option<f32>,
}

/// What a recenter re-based.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RecenterKind {
    /// Position and yaw.
    Full,
    /// Yaw and height only.
    YawAndHeight,
}

/// Who asked for a recenter.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RecenterCause {
    /// The player.
    Manual,
    /// A seat transition's re-base.
    SeatTransition,
}

/// A semantic action set.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum SemanticSet {
    Global,
    OnFoot,
    Wingsuit,
    LandVehicle,
    Helicopter,
    Plane,
    Menu,
}

impl SemanticSet {
    /// Every set, in creation and stash order.
    pub const ALL: [SemanticSet; 7] = [
        SemanticSet::Global,
        SemanticSet::OnFoot,
        SemanticSet::Wingsuit,
        SemanticSet::LandVehicle,
        SemanticSet::Helicopter,
        SemanticSet::Plane,
        SemanticSet::Menu,
    ];

    /// The index of this set in [`ALL`](Self::ALL).
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One action's value on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ActionValue {
    Bool(bool),
    Float(f32),
    Vec2(Vec2),
}

/// One action's state on one hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ActionState {
    pub value: ActionValue,
    /// Whether the runtime bound the action to an active source on this hand.
    pub active: bool,
    /// The digital state: a boolean action's value, or a float action past its press thresholds.
    pub down: bool,
}

/// Every action of one set, each per hand, indexed by the action's index in its set and then by
/// [`Hand::index`].
#[derive(Debug, Clone, PartialEq)]
pub struct SetState {
    pub set: SemanticSet,
    pub actions: Vec<[ActionState; 2]>,
}
