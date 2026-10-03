//! The published per-frame controller snapshot ([`ControllerFrame`]) and the input edge latch.
//!
//! The VR runtime ([`crate::vr::input`]) produces one [`ControllerFrame`] per rendered frame, on the
//! main thread inside the VR frame loop: it syncs the OpenXR actions, locates the grip and aim
//! spaces at the frame's predicted display time, re-bases them against the recenter baseline exactly
//! as the eye views are, and calls [`publish`]. Everything downstream reads the snapshot:
//!
//! - [`latest`] (a copy) or [`with_latest`] (a borrow under the slot lock) for the current state:
//!   poses, velocities, tracking validity, and every input value. Both hide a frame older than
//!   [`MAX_FRAME_AGE`], so a frame that stops being refreshed stops driving anything even when no
//!   path cleared it.
//! - [`take_edges`] for the press and release edges since the last take. Render frames outpace the
//!   engine's roughly 30 Hz sim tick, so a button pressed and released between two ticks would be
//!   invisible to a consumer that only compared states tick to tick. The latch accumulates edges
//!   across publishes until they are taken. The input tick is the single consumer: a second taker
//!   would steal edges from it.
//!
//! Poses are in the **cockpit frame**: the OpenXR LOCAL space re-based by the recenter baseline
//! (position and yaw), metres, OpenXR axes (+X right, +Y up, -Z forward). This is the frame the eye
//! views' `pose` uses, so a controller pose composes into the world through the same transform as
//! the head (`headpose::xr::compose`).
//!
//! The values are raw: the OpenXR actions are named for the physical inputs (trigger, squeeze, A,
//! B, ...), and mapping them to game actions happens in the bindings layer. The only derived states
//! are the digital presses ([`Button::TriggerPress`], [`Button::SqueezePress`],
//! [`Button::TrackpadPressLeft`], and [`Button::TrackpadPressRight`]), thresholded here with
//! hysteresis ([`PressRules`]) so that their edges are latched at frame rate like every other
//! button.

use std::time::{Duration, Instant};

use glam::{Quat, Vec2, Vec3};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::controllers::motion;

/// A hand, and the index of its entry in the per-hand arrays ([`ControllerFrame::hands`],
/// [`Edges`]). The left hand is index 0.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Hand {
    Left,
    #[default]
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

    /// The other hand.
    pub const fn other(self) -> Hand {
        match self {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    }

    /// A short lowercase label for logs and the debug UI.
    pub const fn label(self) -> &'static str {
        match self {
            Hand::Left => "left",
            Hand::Right => "right",
        }
    }
}

/// The OpenXR action set that was active when a frame was sampled. The mode detector selects one
/// per sim tick through [`crate::vr::input::set_active_action_set`]; each set carries the same raw
/// inputs, so the choice changes only which of the runtime's per-set bindings (and the user's
/// remaps of them) apply.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub enum ActionSetKind {
    /// On foot, swimming, and grapple traversal from the ground.
    #[default]
    OnFoot,
    /// Seated in a vehicle, including the entry and exit animations.
    Vehicle,
    /// Freefall, the wingsuit, and the parachute.
    Airborne,
    /// A game menu is up.
    Ui,
}

impl ActionSetKind {
    /// Every kind, in the order the action sets are created.
    pub const ALL: [ActionSetKind; 4] = [
        ActionSetKind::OnFoot,
        ActionSetKind::Vehicle,
        ActionSetKind::Airborne,
        ActionSetKind::Ui,
    ];

    /// The OpenXR action set name.
    pub const fn name(self) -> &'static str {
        match self {
            ActionSetKind::OnFoot => "onfoot",
            ActionSetKind::Vehicle => "vehicle",
            ActionSetKind::Airborne => "airborne",
            ActionSetKind::Ui => "ui",
        }
    }

    /// The localized action set name the runtime's binding interface shows.
    pub const fn localized_name(self) -> &'static str {
        match self {
            ActionSetKind::OnFoot => "On foot",
            ActionSetKind::Vehicle => "Vehicle",
            ActionSetKind::Airborne => "Airborne",
            ActionSetKind::Ui => "Menus",
        }
    }

    /// The index of this kind in [`ALL`](Self::ALL).
    pub const fn index(self) -> usize {
        match self {
            ActionSetKind::OnFoot => 0,
            ActionSetKind::Vehicle => 1,
            ActionSetKind::Airborne => 2,
            ActionSetKind::Ui => 3,
        }
    }
}

/// The interaction profile the runtime bound a hand to, which tells the consumer which physical
/// inputs exist (a Touch controller reports no trackpad and no squeeze force, for example).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum InteractionProfile {
    /// No profile is bound: the controller is off, or the session is not focused.
    #[default]
    None,
    /// `/interaction_profiles/valve/index_controller`.
    ValveIndex,
    /// `/interaction_profiles/oculus/touch_controller`.
    OculusTouch,
    /// A profile the mod ships no suggested bindings for, emulated by the runtime.
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

/// A located controller space: the pose, its validity and tracking flags, and the velocities the
/// runtime reported.
///
/// OpenXR separates *valid* (the value is usable) from *tracked* (the value is measured this frame
/// rather than inferred, as it is while a controller is briefly occluded). Consumers that need a
/// pose use [`is_valid`](Self::is_valid); consumers that must not act on an inferred pose (a melee
/// strike, a throw) also check [`is_tracked`](Self::is_tracked).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TrackedPose {
    /// The pose. Meaningful only for the components whose validity flag is set.
    pub pose: Pose,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub position_tracked: bool,
    pub orientation_tracked: bool,
    /// Linear velocity in the cockpit frame, metres per second, when the runtime reported one.
    pub linear_velocity: Option<Vec3>,
    /// Angular velocity in the cockpit frame, radians per second about each axis, when the runtime
    /// reported one.
    pub angular_velocity: Option<Vec3>,
}

impl TrackedPose {
    /// Whether both the position and the orientation are usable.
    pub fn is_valid(&self) -> bool {
        self.position_valid && self.orientation_valid
    }

    /// Whether both the position and the orientation are measured this frame.
    #[allow(dead_code, reason = "part of the pose validity contract")]
    pub fn is_tracked(&self) -> bool {
        self.is_valid() && self.position_tracked && self.orientation_tracked
    }
}

/// Every raw input of one controller, named for the Valve Index controller's inputs. Inputs a
/// controller lacks read as zero or false, and the Oculus Touch layout maps onto these names by
/// position: X and Y on the left Touch controller read as [`a_click`](Self::a_click) and
/// [`b_click`](Self::b_click), and its menu button as [`system_click`](Self::system_click).
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandInputs {
    /// The trigger's analog pull, `0..=1`.
    pub trigger: f32,
    pub trigger_click: bool,
    pub trigger_touch: bool,
    /// The grip's analog squeeze, `0..=1`.
    pub squeeze: f32,
    /// The grip's force beyond a full squeeze, `0..=1`, on force-sensing grips (Index only).
    pub squeeze_force: f32,
    /// The thumbstick, each axis `-1..=1`, +Y forward.
    pub thumbstick: Vec2,
    pub thumbstick_click: bool,
    pub thumbstick_touch: bool,
    /// The trackpad touch position, each axis `-1..=1` (Index only).
    pub trackpad: Vec2,
    /// The trackpad's press force, `0..=1` (Index only).
    pub trackpad_force: f32,
    pub trackpad_touch: bool,
    pub a_click: bool,
    pub a_touch: bool,
    pub b_click: bool,
    pub b_touch: bool,
    /// The system button (Index), or the menu button (left Touch controller). Runtimes commonly
    /// reserve the system button for their own dashboard and never report it.
    pub system_click: bool,
}

/// A digital input of one controller, as a bit in a [`ButtonSet`]. The touch buttons are
/// capacitive contact, not a press.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    TriggerClick,
    TriggerTouch,
    /// The trigger pulled past the press threshold, released past the release threshold
    /// (hysteresis; see [`PressThresholds`]). Available on every controller, unlike
    /// [`TriggerClick`](Self::TriggerClick).
    TriggerPress,
    /// The grip squeezed past the press threshold, released past the release threshold, read from
    /// the squeeze force on a force-sensing grip and from the squeeze value elsewhere (see
    /// [`GripPress`]).
    SqueezePress,
    ThumbstickClick,
    ThumbstickTouch,
    TrackpadTouch,
    AClick,
    ATouch,
    BClick,
    BTouch,
    SystemClick,
    /// The trackpad pressed past the press threshold on its left half, released past the release
    /// threshold. The half is decided at the press and kept until the release.
    TrackpadPressLeft,
    /// As [`TrackpadPressLeft`](Self::TrackpadPressLeft), on the right half.
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

    /// Whether this is a capacitive touch rather than a press. Touches change whenever the player
    /// shifts their grip, so they are not deliberate input.
    pub const fn is_touch(self) -> bool {
        matches!(
            self,
            Button::TriggerTouch
                | Button::ThumbstickTouch
                | Button::TrackpadTouch
                | Button::ATouch
                | Button::BTouch
        )
    }

    /// A short label for the debug UI.
    pub const fn label(self) -> &'static str {
        match self {
            Button::TriggerClick => "trigger click",
            Button::TriggerTouch => "trigger touch",
            Button::TriggerPress => "trigger press",
            Button::SqueezePress => "squeeze press",
            Button::ThumbstickClick => "stick click",
            Button::ThumbstickTouch => "stick touch",
            Button::TrackpadTouch => "trackpad touch",
            Button::AClick => "A",
            Button::ATouch => "A touch",
            Button::BClick => "B",
            Button::BTouch => "B touch",
            Button::SystemClick => "system",
            Button::TrackpadPressLeft => "pad press left",
            Button::TrackpadPressRight => "pad press right",
        }
    }

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

    pub const fn union(self, other: ButtonSet) -> ButtonSet {
        ButtonSet(self.0 | other.0)
    }

    /// The buttons in `self` that are not in `other`.
    pub const fn difference(self, other: ButtonSet) -> ButtonSet {
        ButtonSet(self.0 & !other.0)
    }

    /// The buttons in the set, in bit order.
    pub fn iter(self) -> impl Iterator<Item = Button> {
        Button::ALL.into_iter().filter(move |&b| self.contains(b))
    }
}

/// The analog thresholds that derive [`Button::TriggerPress`] and [`Button::SqueezePress`]. A
/// button presses when the value reaches `press` and releases when it falls below `release`, which
/// is lower, so a value resting near one threshold does not chatter.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct PressThresholds {
    pub press: f32,
    pub release: f32,
}

impl PressThresholds {
    /// Whether the button is down after observing `value`, given whether it was down before.
    pub fn apply(self, was_down: bool, value: f32) -> bool {
        if was_down {
            value >= self.release
        } else {
            value >= self.press
        }
    }
}

/// Which grip reading derives [`Button::SqueezePress`] on one controller, with its thresholds.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum GripPress {
    /// The squeeze value, [`HandInputs::squeeze`]: how far the fingers close round the handle, or
    /// how far a pull-style grip is pulled.
    Value(PressThresholds),
    /// The squeeze force, [`HandInputs::squeeze_force`]: how hard a force-sensing grip is squeezed
    /// once the hand is closed. A capacitive grip's value reaches its top with the hand merely
    /// resting round the handle, so only the force tells a deliberate squeeze from a held
    /// controller.
    Force(PressThresholds),
}

impl GripPress {
    /// The reading this press is derived from, and its thresholds.
    pub fn reading(self, inputs: &HandInputs) -> (f32, PressThresholds) {
        match self {
            GripPress::Value(thresholds) => (inputs.squeeze, thresholds),
            GripPress::Force(thresholds) => (inputs.squeeze_force, thresholds),
        }
    }
}

/// How one controller's derived presses are read from its analog inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PressRules {
    /// The trigger value thresholds of [`Button::TriggerPress`].
    pub trigger: PressThresholds,
    /// The reading and thresholds of [`Button::SqueezePress`].
    pub grip: GripPress,
    /// The trackpad force thresholds of [`Button::TrackpadPressLeft`] and
    /// [`Button::TrackpadPressRight`].
    pub trackpad: PressThresholds,
}

/// A half of the trackpad, left or right of its vertical centre line.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TrackpadHalf {
    Left,
    Right,
}

impl TrackpadHalf {
    /// The derived button of a press on this half.
    pub const fn button(self) -> Button {
        match self {
            TrackpadHalf::Left => Button::TrackpadPressLeft,
            TrackpadHalf::Right => Button::TrackpadPressRight,
        }
    }
}

/// How far from the trackpad's vertical centre line, in its `-1..=1` horizontal axis, a press must
/// land to count for a half. A press on the centre line names neither half, and waits for the thumb
/// to move off it.
pub const TRACKPAD_HALF_MARGIN: f32 = 0.2;

/// The half of the trackpad pressed after observing `inputs`, given the half pressed before, if
/// any. The half is decided at the press and kept, wherever the thumb slides, until the force falls
/// below the release threshold.
pub fn trackpad_press(
    previous: Option<TrackpadHalf>,
    inputs: &HandInputs,
    thresholds: PressThresholds,
) -> Option<TrackpadHalf> {
    let force = inputs.trackpad_force;
    if !force.is_finite() {
        return None;
    }
    if let Some(half) = previous {
        return thresholds.apply(true, force).then_some(half);
    }
    if !thresholds.apply(false, force) {
        return None;
    }
    let x = inputs.trackpad.x;
    if x <= -TRACKPAD_HALF_MARGIN {
        Some(TrackpadHalf::Left)
    } else if x >= TRACKPAD_HALF_MARGIN {
        Some(TrackpadHalf::Right)
    } else {
        None
    }
}

/// The digital state of every button of one controller: the raw booleans, plus the presses derived
/// from the analog values by `rules`, with hysteresis against `previous`.
pub fn derive_buttons(inputs: &HandInputs, previous: ButtonSet, rules: &PressRules) -> ButtonSet {
    let mut set = ButtonSet::EMPTY;
    set.set(Button::TriggerClick, inputs.trigger_click);
    set.set(Button::TriggerTouch, inputs.trigger_touch);
    set.set(
        Button::TriggerPress,
        rules
            .trigger
            .apply(previous.contains(Button::TriggerPress), inputs.trigger),
    );
    let (grip, grip_thresholds) = rules.grip.reading(inputs);
    set.set(
        Button::SqueezePress,
        grip.is_finite() && grip_thresholds.apply(previous.contains(Button::SqueezePress), grip),
    );
    let previous_half = [TrackpadHalf::Left, TrackpadHalf::Right]
        .into_iter()
        .find(|half| previous.contains(half.button()));
    if let Some(half) = trackpad_press(previous_half, inputs, rules.trackpad) {
        set.set(half.button(), true);
    }
    set.set(Button::ThumbstickClick, inputs.thumbstick_click);
    set.set(Button::ThumbstickTouch, inputs.thumbstick_touch);
    set.set(Button::TrackpadTouch, inputs.trackpad_touch);
    set.set(Button::AClick, inputs.a_click);
    set.set(Button::ATouch, inputs.a_touch);
    set.set(Button::BClick, inputs.b_click);
    set.set(Button::BTouch, inputs.b_touch);
    set.set(Button::SystemClick, inputs.system_click);
    set
}

/// The number of joints `XR_EXT_hand_tracking` reports per hand (its default joint set).
pub const HAND_JOINT_COUNT: usize = 26;

/// One tracked hand joint. The joint order is the `XrHandJointEXT` order: palm, wrist, then the
/// thumb (metacarpal, proximal, distal, tip) and each finger (metacarpal, proximal, intermediate,
/// distal, tip) from index to little.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandJoint {
    /// The joint pose in the cockpit frame.
    pub pose: Pose,
    /// The joint's radius, metres.
    pub radius: f32,
    pub valid: bool,
    pub tracked: bool,
}

/// One controller's snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandFrame {
    /// Whether the runtime reports this hand's controller as present: any of its actions is bound
    /// to a connected input source. A hand that is not active reads as all zeros.
    pub active: bool,
    /// The interaction profile the runtime bound this hand to.
    pub profile: InteractionProfile,
    /// The grip pose: the centre of the closed hand, +X out of the palm's thumb side on the right
    /// hand (see the OpenXR `grip` pose definition), -Z along the controller's handle axis.
    pub grip: TrackedPose,
    /// The aim pose: a ray origin at the controller's tip, pointing along -Z.
    pub aim: TrackedPose,
    pub inputs: HandInputs,
    /// The digital state, including the derived presses.
    pub buttons: ButtonSet,
    /// The tracked hand skeleton, when `XR_EXT_hand_tracking` is enabled, the runtime supports
    /// it, the hand-tracking toggle is on, and the runtime reports the hand as tracked this frame.
    pub joints: Option<[HandJoint; HAND_JOINT_COUNT]>,
}

/// The per-frame controller snapshot. See the module docs for the frame and the threading.
#[derive(Debug, Clone)]
pub struct ControllerFrame {
    /// Increments by one for every published frame since the payload loaded.
    pub sequence: u64,
    /// The OpenXR predicted display time the poses were located at, nanoseconds.
    pub display_time_ns: i64,
    /// When the frame was published, on the host clock.
    pub published_at: Instant,
    /// Whether the session had input focus. An unfocused session (the runtime's dashboard is up)
    /// reports every action inactive.
    pub focused: bool,
    /// The action set the inputs were read through.
    pub action_set: ActionSetKind,
    /// Indexed by [`Hand::index`].
    pub hands: [HandFrame; 2],
}

impl ControllerFrame {
    /// The snapshot of `hand`.
    pub fn hand(&self, hand: Hand) -> &HandFrame {
        &self.hands[hand.index()]
    }

    /// Whether any controller is present.
    pub fn any_active(&self) -> bool {
        self.hands.iter().any(|h| h.active)
    }

    /// How long ago the frame was published.
    pub fn age(&self) -> Duration {
        self.published_at.elapsed()
    }

    /// Whether the frame is recent enough at `now` to stand for the controllers' current state.
    pub fn is_fresh_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.published_at) <= MAX_FRAME_AGE
    }
}

/// The oldest a published frame may be and still be read as the controllers' current state. The
/// frame loop publishes every rendered frame, so an older frame means the loop stopped sampling.
/// The bound sits well above a frame interval because a main-thread hitch delays the publish too,
/// and the input tick can run after the hitch before the next frame is sampled; idling the bindings
/// for one such tick would drop whatever the hands hold.
pub const MAX_FRAME_AGE: Duration = Duration::from_millis(500);

/// The press and release edges of every button since the last [`take_edges`], per hand. A button
/// pressed and released between two takes appears in both sets.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Edges {
    pub pressed: [ButtonSet; 2],
    pub released: [ButtonSet; 2],
}

impl Edges {
    /// Whether `button` on `hand` was pressed since the last take.
    pub fn pressed(&self, hand: Hand, button: Button) -> bool {
        self.pressed[hand.index()].contains(button)
    }

    /// Whether `button` on `hand` was released since the last take.
    pub fn released(&self, hand: Hand, button: Button) -> bool {
        self.released[hand.index()].contains(button)
    }

    /// Fold the transition from `previous` to `current` into the edges.
    pub fn accumulate(&mut self, previous: [ButtonSet; 2], current: [ButtonSet; 2]) {
        for i in 0..2 {
            self.pressed[i] = self.pressed[i].union(current[i].difference(previous[i]));
            self.released[i] = self.released[i].union(previous[i].difference(current[i]));
        }
    }
}

/// A copy of the latest published frame, or `None` when no frame has been published since the
/// controllers were last cleared (no session, controllers disabled, or the payload just loaded) or
/// the latest is older than [`MAX_FRAME_AGE`].
pub fn latest() -> Option<ControllerFrame> {
    SLOT.lock().fresh(Instant::now()).cloned()
}

/// Run `f` against the latest published frame without copying it, under the same conditions as
/// [`latest`]. The slot lock is held for `f`, so `f` must be short and must not call back into
/// this module.
pub fn with_latest<R>(f: impl FnOnce(&ControllerFrame) -> R) -> Option<R> {
    SLOT.lock().fresh(Instant::now()).map(f)
}

/// Take the edges accumulated since the last take, and reset the latch. The input tick is the one
/// consumer.
pub fn take_edges() -> Edges {
    std::mem::take(&mut SLOT.lock().edges)
}

/// Publish a frame: fold its button transitions into the edge latch and make it the latest. The
/// caller leaves [`ControllerFrame::sequence`] and [`ControllerFrame::published_at`] to this
/// function, which stamps them.
pub(crate) fn publish(frame: ControllerFrame) {
    let mut slot = SLOT.lock();
    let frame = slot.publish(frame, Instant::now());
    motion::record(frame);
}

/// Drop the latest frame and any untaken edges: the controllers are disabled, the session is gone,
/// or the frame loop stopped sampling. Buttons held at the time produce no release edge; consumers
/// treat a missing frame as everything released. The first frame published after a clear has no
/// baseline, so it produces no edges either: a button held across the clear is not pressed anew.
pub(crate) fn clear() {
    SLOT.lock().clear();
    motion::clear();
}

/// The published frame and the edge latch, under one lock so that a publish and a take never
/// interleave.
static SLOT: Mutex<Slot> = Mutex::new(Slot {
    frame: None,
    edges: Edges {
        pressed: [ButtonSet::EMPTY; 2],
        released: [ButtonSet::EMPTY; 2],
    },
    sequence: 0,
});

struct Slot {
    frame: Option<ControllerFrame>,
    edges: Edges,
    sequence: u64,
}

impl Slot {
    fn fresh(&self, now: Instant) -> Option<&ControllerFrame> {
        self.frame.as_ref().filter(|f| f.is_fresh_at(now))
    }

    /// Stamp `frame`, fold its transitions from the previous frame into the edges, and store it.
    /// Without a previous frame (the first publish, or the first after a clear) there is no
    /// transition to fold. A change of action set starts a new input stream: the same physical
    /// button can be down in both sets, but those states are not comparable, and their difference
    /// must not manufacture an edge for the new set.
    fn publish(&mut self, mut frame: ControllerFrame, now: Instant) -> &ControllerFrame {
        self.sequence += 1;
        frame.sequence = self.sequence;
        frame.published_at = now;
        let current = [frame.hands[0].buttons, frame.hands[1].buttons];
        match &self.frame {
            Some(previous) if previous.action_set == frame.action_set => {
                let previous = [previous.hands[0].buttons, previous.hands[1].buttons];
                self.edges.accumulate(previous, current);
            }
            Some(_) => self.edges = Edges::default(),
            None => {}
        }
        self.frame.insert(frame)
    }

    fn clear(&mut self) {
        self.frame = None;
        self.edges = Edges::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLDS: PressThresholds = PressThresholds {
        press: 0.7,
        release: 0.5,
    };

    fn set(buttons: &[Button]) -> ButtonSet {
        let mut s = ButtonSet::EMPTY;
        for &b in buttons {
            s.set(b, true);
        }
        s
    }

    #[test]
    fn press_thresholds_have_hysteresis() {
        assert!(!THRESHOLDS.apply(false, 0.6));
        assert!(THRESHOLDS.apply(false, 0.7));
        assert!(THRESHOLDS.apply(true, 0.6));
        assert!(THRESHOLDS.apply(true, 0.5));
        assert!(!THRESHOLDS.apply(true, 0.49));
    }

    const FORCE: PressThresholds = PressThresholds {
        press: 0.3,
        release: 0.1,
    };

    fn rules(grip: GripPress) -> PressRules {
        PressRules {
            trigger: THRESHOLDS,
            grip,
            trackpad: FORCE,
        }
    }

    #[test]
    fn derived_presses_follow_the_analog_values() {
        let value = rules(GripPress::Value(THRESHOLDS));
        let mut inputs = HandInputs {
            trigger: 0.8,
            squeeze: 0.2,
            a_click: true,
            ..Default::default()
        };
        let first = derive_buttons(&inputs, ButtonSet::EMPTY, &value);
        assert_eq!(first, set(&[Button::TriggerPress, Button::AClick]));

        inputs.trigger = 0.6;
        let held = derive_buttons(&inputs, first, &value);
        assert!(held.contains(Button::TriggerPress));
        let fresh = derive_buttons(&inputs, ButtonSet::EMPTY, &value);
        assert!(!fresh.contains(Button::TriggerPress));
    }

    #[test]
    fn a_force_grip_presses_on_force_alone_with_hysteresis() {
        let force = rules(GripPress::Force(FORCE));
        // A hand resting round a capacitive grip reads a full squeeze value and no force.
        let mut inputs = HandInputs {
            squeeze: 1.0,
            squeeze_force: 0.05,
            ..Default::default()
        };
        let resting = derive_buttons(&inputs, ButtonSet::EMPTY, &force);
        assert!(!resting.contains(Button::SqueezePress));

        inputs.squeeze_force = 0.29;
        assert!(!derive_buttons(&inputs, resting, &force).contains(Button::SqueezePress));
        inputs.squeeze_force = 0.3;
        let pressed = derive_buttons(&inputs, resting, &force);
        assert!(pressed.contains(Button::SqueezePress));

        // Easing off to between the thresholds holds the press; below the release lets go, even
        // with the hand still closed round the grip.
        inputs.squeeze_force = 0.15;
        let held = derive_buttons(&inputs, pressed, &force);
        assert!(held.contains(Button::SqueezePress));
        inputs.squeeze_force = 0.09;
        assert!(!derive_buttons(&inputs, held, &force).contains(Button::SqueezePress));
        inputs.squeeze_force = f32::NAN;
        assert!(!derive_buttons(&inputs, held, &force).contains(Button::SqueezePress));
    }

    #[test]
    fn a_value_grip_ignores_the_force() {
        let value = rules(GripPress::Value(THRESHOLDS));
        let inputs = HandInputs {
            squeeze: 0.2,
            squeeze_force: 1.0,
            ..Default::default()
        };
        assert!(!derive_buttons(&inputs, ButtonSet::EMPTY, &value).contains(Button::SqueezePress));
        let inputs = HandInputs {
            squeeze: 0.8,
            ..Default::default()
        };
        assert!(derive_buttons(&inputs, ButtonSet::EMPTY, &value).contains(Button::SqueezePress));
    }

    fn pad(x: f32, force: f32) -> HandInputs {
        HandInputs {
            trackpad: Vec2::new(x, 0.0),
            trackpad_force: force,
            trackpad_touch: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_trackpad_press_picks_the_half_under_the_thumb() {
        assert_eq!(
            trackpad_press(None, &pad(-0.6, 0.5), FORCE),
            Some(TrackpadHalf::Left)
        );
        assert_eq!(
            trackpad_press(None, &pad(0.6, 0.5), FORCE),
            Some(TrackpadHalf::Right)
        );
        // A touch without force, or a press on the centre line, is no press.
        assert_eq!(trackpad_press(None, &pad(0.6, 0.2), FORCE), None);
        assert_eq!(trackpad_press(None, &pad(0.1, 1.0), FORCE), None);
        assert_eq!(trackpad_press(None, &pad(-0.1, 1.0), FORCE), None);
        assert_eq!(trackpad_press(None, &pad(0.6, f32::NAN), FORCE), None);
    }

    #[test]
    fn a_trackpad_press_keeps_its_half_until_released() {
        let left = Some(TrackpadHalf::Left);
        // The thumb slides across under pressure: the press stays on its half.
        assert_eq!(trackpad_press(left, &pad(0.8, 0.2), FORCE), left);
        assert_eq!(trackpad_press(left, &pad(0.8, 0.1), FORCE), left);
        assert_eq!(trackpad_press(left, &pad(0.8, 0.09), FORCE), None);
    }

    #[test]
    fn trackpad_presses_derive_their_buttons() {
        let rules = rules(GripPress::Value(THRESHOLDS));
        let right = derive_buttons(&pad(0.7, 0.5), ButtonSet::EMPTY, &rules);
        assert!(right.contains(Button::TrackpadPressRight));
        assert!(!right.contains(Button::TrackpadPressLeft));
        assert!(right.contains(Button::TrackpadTouch));
        let slid = derive_buttons(&pad(-0.7, 0.2), right, &rules);
        assert!(slid.contains(Button::TrackpadPressRight));
        assert!(!slid.contains(Button::TrackpadPressLeft));
        let released = derive_buttons(&pad(-0.7, 0.0), slid, &rules);
        assert!(!released.contains(Button::TrackpadPressRight));
        assert!(!Button::TrackpadPressLeft.is_touch());
    }

    #[test]
    fn edges_latch_a_tap_between_takes() {
        let mut edges = Edges::default();
        let down = [ButtonSet::EMPTY, set(&[Button::AClick])];
        let up = [ButtonSet::EMPTY; 2];
        edges.accumulate(up, down);
        edges.accumulate(down, up);
        assert!(edges.pressed(Hand::Right, Button::AClick));
        assert!(edges.released(Hand::Right, Button::AClick));
        assert!(!edges.pressed(Hand::Left, Button::AClick));
    }

    #[test]
    fn edges_ignore_held_buttons() {
        let mut edges = Edges::default();
        let held = [set(&[Button::BClick]), ButtonSet::EMPTY];
        edges.accumulate(held, held);
        assert_eq!(edges, Edges::default());
    }

    #[test]
    fn button_set_iterates_in_bit_order() {
        let s = set(&[Button::SystemClick, Button::TriggerClick]);
        let buttons: Vec<_> = s.iter().collect();
        assert_eq!(buttons, vec![Button::TriggerClick, Button::SystemClick]);
        assert_eq!(
            s.difference(set(&[Button::SystemClick])),
            set(&[Button::TriggerClick])
        );
    }

    fn empty_slot() -> Slot {
        Slot {
            frame: None,
            edges: Edges::default(),
            sequence: 0,
        }
    }

    fn frame_with(right: ButtonSet) -> ControllerFrame {
        frame_with_set(right, ActionSetKind::OnFoot)
    }

    fn frame_with_set(right: ButtonSet, action_set: ActionSetKind) -> ControllerFrame {
        let mut hands: [HandFrame; 2] = Default::default();
        hands[Hand::Right.index()].active = true;
        hands[Hand::Right.index()].buttons = right;
        ControllerFrame {
            sequence: 0,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set,
            hands,
        }
    }

    #[test]
    fn a_frame_goes_stale_past_the_age_bound() {
        let mut slot = empty_slot();
        let t0 = Instant::now();
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        assert!(slot.fresh(t0).is_some());
        assert!(slot.fresh(t0 + MAX_FRAME_AGE).is_some());
        assert!(
            slot.fresh(t0 + MAX_FRAME_AGE + Duration::from_millis(1))
                .is_none()
        );
        slot.publish(frame_with(ButtonSet::EMPTY), t0 + Duration::from_secs(5));
        assert!(slot.fresh(t0 + Duration::from_secs(5)).is_some());
    }

    #[test]
    fn a_clear_hides_the_frame() {
        let mut slot = empty_slot();
        let t0 = Instant::now();
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        slot.clear();
        assert!(slot.fresh(t0).is_none());
    }

    #[test]
    fn publishing_latches_press_edges() {
        let mut slot = empty_slot();
        let t0 = Instant::now();
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        slot.publish(frame_with(set(&[Button::TriggerPress])), t0);
        assert!(slot.edges.pressed(Hand::Right, Button::TriggerPress));
    }

    #[test]
    fn changing_action_sets_does_not_create_or_retain_edges() {
        let mut slot = empty_slot();
        let t0 = Instant::now();
        let held = set(&[Button::TriggerPress]);
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        slot.publish(frame_with(held), t0);
        assert!(slot.edges.pressed(Hand::Right, Button::TriggerPress));

        slot.publish(frame_with_set(held, ActionSetKind::Vehicle), t0);
        assert_eq!(slot.edges, Edges::default());
        slot.publish(frame_with_set(ButtonSet::EMPTY, ActionSetKind::Vehicle), t0);
        assert!(slot.edges.released(Hand::Right, Button::TriggerPress));
        assert!(!slot.edges.pressed(Hand::Right, Button::TriggerPress));
    }

    #[test]
    fn a_button_held_across_a_clear_is_not_pressed_again() {
        let mut slot = empty_slot();
        let t0 = Instant::now();
        let held = set(&[Button::TriggerPress]);
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        slot.publish(frame_with(held), t0);
        slot.edges = Edges::default();
        slot.clear();
        slot.publish(frame_with(held), t0);
        assert_eq!(slot.edges, Edges::default());
        slot.publish(frame_with(ButtonSet::EMPTY), t0);
        assert!(slot.edges.released(Hand::Right, Button::TriggerPress));
    }

    #[test]
    fn hands_index_and_mirror() {
        assert_eq!(Hand::Left.index(), 0);
        assert_eq!(Hand::Right.index(), 1);
        assert_eq!(Hand::Left.other(), Hand::Right);
    }
}
