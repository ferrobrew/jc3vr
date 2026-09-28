//! The action layout: every raw input the mod reads, its OpenXR action name, and its suggested
//! binding on each interaction profile. Pure data; [`crate::vr::input::actions`] creates the OpenXR
//! objects from it.
//!
//! The order of the items here is the order of the persisted handle stash (see
//! [`crate::vr::input::stash`]), and [`fingerprint`] hashes it, so that a payload with a different
//! layout never re-wraps another layout's handles.

use crate::controllers::frame::{ActionSetKind, Hand};

/// A boolean input.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BoolInput {
    TriggerClick,
    TriggerTouch,
    ThumbstickClick,
    ThumbstickTouch,
    TrackpadTouch,
    AClick,
    ATouch,
    BClick,
    BTouch,
    SystemClick,
}

impl BoolInput {
    pub(crate) const ALL: [BoolInput; 10] = [
        BoolInput::TriggerClick,
        BoolInput::TriggerTouch,
        BoolInput::ThumbstickClick,
        BoolInput::ThumbstickTouch,
        BoolInput::TrackpadTouch,
        BoolInput::AClick,
        BoolInput::ATouch,
        BoolInput::BClick,
        BoolInput::BTouch,
        BoolInput::SystemClick,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            BoolInput::TriggerClick => "trigger_click",
            BoolInput::TriggerTouch => "trigger_touch",
            BoolInput::ThumbstickClick => "thumbstick_click",
            BoolInput::ThumbstickTouch => "thumbstick_touch",
            BoolInput::TrackpadTouch => "trackpad_touch",
            BoolInput::AClick => "a_click",
            BoolInput::ATouch => "a_touch",
            BoolInput::BClick => "b_click",
            BoolInput::BTouch => "b_touch",
            BoolInput::SystemClick => "system_click",
        }
    }

    pub(crate) const fn localized_name(self) -> &'static str {
        match self {
            BoolInput::TriggerClick => "Trigger click",
            BoolInput::TriggerTouch => "Trigger touch",
            BoolInput::ThumbstickClick => "Thumbstick click",
            BoolInput::ThumbstickTouch => "Thumbstick touch",
            BoolInput::TrackpadTouch => "Trackpad touch",
            BoolInput::AClick => "A (lower face button)",
            BoolInput::ATouch => "A touch",
            BoolInput::BClick => "B (upper face button)",
            BoolInput::BTouch => "B touch",
            BoolInput::SystemClick => "System or menu button",
        }
    }
}

/// A scalar input, `0..=1`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum FloatInput {
    Trigger,
    Squeeze,
    SqueezeForce,
    TrackpadForce,
}

impl FloatInput {
    pub(crate) const ALL: [FloatInput; 4] = [
        FloatInput::Trigger,
        FloatInput::Squeeze,
        FloatInput::SqueezeForce,
        FloatInput::TrackpadForce,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            FloatInput::Trigger => "trigger_value",
            FloatInput::Squeeze => "squeeze_value",
            FloatInput::SqueezeForce => "squeeze_force",
            FloatInput::TrackpadForce => "trackpad_force",
        }
    }

    pub(crate) const fn localized_name(self) -> &'static str {
        match self {
            FloatInput::Trigger => "Trigger pull",
            FloatInput::Squeeze => "Grip squeeze",
            FloatInput::SqueezeForce => "Grip force",
            FloatInput::TrackpadForce => "Trackpad force",
        }
    }
}

/// A two-axis input, each axis `-1..=1`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Vec2Input {
    Thumbstick,
    Trackpad,
}

impl Vec2Input {
    pub(crate) const ALL: [Vec2Input; 2] = [Vec2Input::Thumbstick, Vec2Input::Trackpad];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Vec2Input::Thumbstick => "thumbstick",
            Vec2Input::Trackpad => "trackpad",
        }
    }

    pub(crate) const fn localized_name(self) -> &'static str {
        match self {
            Vec2Input::Thumbstick => "Thumbstick",
            Vec2Input::Trackpad => "Trackpad",
        }
    }
}

/// The name of the always-active action set holding the poses and the haptic output.
pub(crate) const HANDS_SET_NAME: &str = "hands";
pub(crate) const HANDS_SET_LOCALIZED_NAME: &str = "Hands";

/// The pose and output actions of the hands set, in stash order.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HandsAction {
    GripPose,
    AimPose,
    Haptic,
}

impl HandsAction {
    pub(crate) const ALL: [HandsAction; 3] = [
        HandsAction::GripPose,
        HandsAction::AimPose,
        HandsAction::Haptic,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            HandsAction::GripPose => "grip_pose",
            HandsAction::AimPose => "aim_pose",
            HandsAction::Haptic => "haptic",
        }
    }

    pub(crate) const fn localized_name(self) -> &'static str {
        match self {
            HandsAction::GripPose => "Grip pose",
            HandsAction::AimPose => "Aim pose",
            HandsAction::Haptic => "Vibration",
        }
    }
}

/// Any action of the layout, for the binding table.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Input {
    Bool(BoolInput),
    Float(FloatInput),
    Vec2(Vec2Input),
    Hands(HandsAction),
}

/// An interaction profile the mod ships suggested bindings for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Profile {
    ValveIndex,
    OculusTouch,
}

impl Profile {
    /// In suggestion order: the primary profile first.
    pub(crate) const ALL: [Profile; 2] = [Profile::ValveIndex, Profile::OculusTouch];

    pub(crate) const fn path(self) -> &'static str {
        match self {
            Profile::ValveIndex => "/interaction_profiles/valve/index_controller",
            Profile::OculusTouch => "/interaction_profiles/oculus/touch_controller",
        }
    }
}

/// The top-level user path of a hand, the subaction path every action is created with.
pub(crate) const fn hand_path(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "/user/hand/left",
        Hand::Right => "/user/hand/right",
    }
}

/// The component path, relative to the hand's user path, that `input` suggests binding to on
/// `hand` of `profile`, or `None` when the profile lacks the input.
///
/// The Touch layout maps onto the Index names by position: the left controller's X and Y are the
/// lower and upper face buttons like A and B, and its menu button stands in for the system button.
/// Touch has no trigger click, so the boolean trigger click binds to the trigger value, which
/// OpenXR thresholds into a boolean. The right Touch controller's system button is reserved for the
/// runtime and is not suggested.
pub(crate) const fn binding(profile: Profile, hand: Hand, input: Input) -> Option<&'static str> {
    match profile {
        Profile::ValveIndex => Some(index_binding(input)),
        Profile::OculusTouch => touch_binding(hand, input),
    }
}

/// Every action's name in stash order, for [`fingerprint`] and the stash's handle count: the hands
/// set and its actions, then each mode set and its actions.
pub(crate) fn stash_order() -> impl Iterator<Item = &'static str> {
    std::iter::once(HANDS_SET_NAME)
        .chain(HandsAction::ALL.iter().map(|a| a.name()))
        .chain(ActionSetKind::ALL.iter().flat_map(|&kind| {
            std::iter::once(kind.name())
                .chain(BoolInput::ALL.iter().map(|i| i.name()))
                .chain(FloatInput::ALL.iter().map(|i| i.name()))
                .chain(Vec2Input::ALL.iter().map(|i| i.name()))
        }))
}

/// The number of handles the stash holds.
pub(crate) fn stash_len() -> usize {
    stash_order().count()
}

/// An FNV-1a hash of [`stash_order`], identifying this layout.
pub(crate) fn fingerprint() -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for name in stash_order() {
        for byte in name.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

const fn index_binding(input: Input) -> &'static str {
    match input {
        Input::Bool(b) => match b {
            BoolInput::TriggerClick => "input/trigger/click",
            BoolInput::TriggerTouch => "input/trigger/touch",
            BoolInput::ThumbstickClick => "input/thumbstick/click",
            BoolInput::ThumbstickTouch => "input/thumbstick/touch",
            BoolInput::TrackpadTouch => "input/trackpad/touch",
            BoolInput::AClick => "input/a/click",
            BoolInput::ATouch => "input/a/touch",
            BoolInput::BClick => "input/b/click",
            BoolInput::BTouch => "input/b/touch",
            BoolInput::SystemClick => "input/system/click",
        },
        Input::Float(f) => match f {
            FloatInput::Trigger => "input/trigger/value",
            FloatInput::Squeeze => "input/squeeze/value",
            FloatInput::SqueezeForce => "input/squeeze/force",
            FloatInput::TrackpadForce => "input/trackpad/force",
        },
        Input::Vec2(v) => match v {
            Vec2Input::Thumbstick => "input/thumbstick",
            Vec2Input::Trackpad => "input/trackpad",
        },
        Input::Hands(h) => hands_binding(h),
    }
}

const fn touch_binding(hand: Hand, input: Input) -> Option<&'static str> {
    let left = matches!(hand, Hand::Left);
    match input {
        Input::Bool(b) => match b {
            BoolInput::TriggerClick => Some("input/trigger/value"),
            BoolInput::TriggerTouch => Some("input/trigger/touch"),
            BoolInput::ThumbstickClick => Some("input/thumbstick/click"),
            BoolInput::ThumbstickTouch => Some("input/thumbstick/touch"),
            BoolInput::TrackpadTouch => None,
            BoolInput::AClick if left => Some("input/x/click"),
            BoolInput::AClick => Some("input/a/click"),
            BoolInput::ATouch if left => Some("input/x/touch"),
            BoolInput::ATouch => Some("input/a/touch"),
            BoolInput::BClick if left => Some("input/y/click"),
            BoolInput::BClick => Some("input/b/click"),
            BoolInput::BTouch if left => Some("input/y/touch"),
            BoolInput::BTouch => Some("input/b/touch"),
            BoolInput::SystemClick if left => Some("input/menu/click"),
            BoolInput::SystemClick => None,
        },
        Input::Float(f) => match f {
            FloatInput::Trigger => Some("input/trigger/value"),
            FloatInput::Squeeze => Some("input/squeeze/value"),
            FloatInput::SqueezeForce | FloatInput::TrackpadForce => None,
        },
        Input::Vec2(v) => match v {
            Vec2Input::Thumbstick => Some("input/thumbstick"),
            Vec2Input::Trackpad => None,
        },
        Input::Hands(h) => Some(hands_binding(h)),
    }
}

const fn hands_binding(action: HandsAction) -> &'static str {
    match action {
        HandsAction::GripPose => "input/grip/pose",
        HandsAction::AimPose => "input/aim/pose",
        HandsAction::Haptic => "output/haptic",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn every_input() -> Vec<Input> {
        BoolInput::ALL
            .iter()
            .map(|&b| Input::Bool(b))
            .chain(FloatInput::ALL.iter().map(|&f| Input::Float(f)))
            .chain(Vec2Input::ALL.iter().map(|&v| Input::Vec2(v)))
            .chain(HandsAction::ALL.iter().map(|&h| Input::Hands(h)))
            .collect()
    }

    #[test]
    fn index_binds_every_input_on_both_hands() {
        for hand in Hand::BOTH {
            for input in every_input() {
                assert!(
                    binding(Profile::ValveIndex, hand, input).is_some(),
                    "{input:?}"
                );
            }
        }
    }

    #[test]
    fn touch_binds_the_poses_haptics_and_core_inputs() {
        for hand in Hand::BOTH {
            for input in [
                Input::Hands(HandsAction::GripPose),
                Input::Hands(HandsAction::AimPose),
                Input::Hands(HandsAction::Haptic),
                Input::Float(FloatInput::Trigger),
                Input::Float(FloatInput::Squeeze),
                Input::Vec2(Vec2Input::Thumbstick),
                Input::Bool(BoolInput::AClick),
                Input::Bool(BoolInput::BClick),
            ] {
                assert!(
                    binding(Profile::OculusTouch, hand, input).is_some(),
                    "{input:?}"
                );
            }
        }
        let x = binding(
            Profile::OculusTouch,
            Hand::Left,
            Input::Bool(BoolInput::AClick),
        );
        assert_eq!(x, Some("input/x/click"));
    }

    #[test]
    fn binding_paths_are_well_formed_component_paths() {
        for profile in Profile::ALL {
            for hand in Hand::BOTH {
                for input in every_input() {
                    let Some(path) = binding(profile, hand, input) else {
                        continue;
                    };
                    assert!(!path.starts_with('/') && !path.ends_with('/'), "{path}");
                    assert!(
                        path.starts_with("input/") || path.starts_with("output/"),
                        "{path}"
                    );
                    assert!(
                        path.bytes()
                            .all(|c| c.is_ascii_lowercase() || c == b'/' || c == b'_'),
                        "{path}"
                    );
                }
            }
        }
    }

    #[test]
    fn action_names_are_valid_and_unique_per_set() {
        let valid = |name: &str| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        };
        let mode: Vec<&str> = BoolInput::ALL
            .iter()
            .map(|i| i.name())
            .chain(FloatInput::ALL.iter().map(|i| i.name()))
            .chain(Vec2Input::ALL.iter().map(|i| i.name()))
            .collect();
        assert_eq!(mode.iter().collect::<HashSet<_>>().len(), mode.len());
        assert!(mode.iter().all(|n| valid(n)));
        let hands: Vec<&str> = HandsAction::ALL.iter().map(|a| a.name()).collect();
        assert!(hands.iter().all(|n| valid(n)));
        let sets: HashSet<&str> = ActionSetKind::ALL
            .iter()
            .map(|k| k.name())
            .chain(std::iter::once(HANDS_SET_NAME))
            .collect();
        assert_eq!(sets.len(), ActionSetKind::ALL.len() + 1);
    }

    #[test]
    fn the_stash_holds_every_set_and_action() {
        assert_eq!(stash_len(), 1 + 3 + 4 * (1 + 10 + 4 + 2));
        assert_eq!(fingerprint(), fingerprint());
    }
}
