//! The physical inputs each semantic action is bound to on each hand ([`BoundSet`]), which the
//! binding tables' masks act on: a mechanism that claims a physical input on a hand (a chord, a
//! grab, the dual-wield split, the native UI pointer) masks every action that reads that input
//! there, wherever the player bound the action.
//!
//! The runtime's bound sources are opaque paths that OpenXR asks applications not to interpret. The
//! mod matches them, as text, against the inputs it knows on the shipped profiles, which is the
//! mildest reading available: a source it does not recognise is kept as unknown, and an action with
//! an unknown source is masked only with its whole hand. Until a set's sources have been
//! enumerated, or for an action whose enumeration failed, the default bindings stand in
//! ([`BoundSet::defaults`]).

use crate::{
    controllers::frame::{Hand, InteractionProfile},
    vr::input::{
        layout::{Profile, hand_path},
        semantic::{
            SemanticSet,
            defaults::{Component, DefaultBinding},
        },
    },
};

/// A physical input of one controller, as the masks name it: the trigger or grip as a whole, the
/// stick's axes apart from its click, and the face buttons by position.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum Physical {
    Trigger,
    Grip,
    /// The stick's axes and touch.
    Thumbstick,
    ThumbstickClick,
    Trackpad,
    /// A, or X on the left Touch controller.
    LowerFace,
    /// B, or Y on the left Touch controller.
    UpperFace,
    /// The system or menu button.
    System,
}

impl Physical {
    const fn bit(self) -> u16 {
        1 << (self as u8)
    }
}

/// A set of [`Physical`] inputs, plus whether a source matched none of them.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct PhysicalSet(u16);

impl PhysicalSet {
    pub(crate) const EMPTY: PhysicalSet = PhysicalSet(0);

    pub(crate) const fn of(physical: Physical) -> PhysicalSet {
        PhysicalSet(physical.bit())
    }

    pub(crate) const fn with(self, physical: Physical) -> PhysicalSet {
        PhysicalSet(self.0 | physical.bit())
    }

    pub(crate) const fn with_unknown(self) -> PhysicalSet {
        PhysicalSet(self.0 | UNKNOWN)
    }

    pub(crate) const fn contains(self, physical: Physical) -> bool {
        self.0 & physical.bit() != 0
    }
}

impl Component {
    /// The physical input the component belongs to.
    pub(crate) const fn physical(self) -> Physical {
        match self {
            Component::TriggerValue => Physical::Trigger,
            Component::GripPress => Physical::Grip,
            Component::Thumbstick | Component::ThumbstickX | Component::ThumbstickY => {
                Physical::Thumbstick
            }
            Component::ThumbstickClick => Physical::ThumbstickClick,
            Component::Trackpad | Component::TrackpadForce | Component::TrackpadTouch => {
                Physical::Trackpad
            }
            Component::LowerFace => Physical::LowerFace,
            Component::UpperFace => Physical::UpperFace,
            Component::System => Physical::System,
        }
    }
}

/// The hand a bound source path is under, and the physical input it names, or `None` for one the
/// mod does not recognise. A path under no hand is `None` altogether.
pub(crate) fn classify(path: &str) -> Option<(Hand, Option<Physical>)> {
    let (hand, rest) = Hand::BOTH.into_iter().find_map(|hand| {
        path.strip_prefix(hand_path(hand))
            .and_then(|rest| rest.strip_prefix('/'))
            .map(|rest| (hand, rest))
    })?;
    let mut segments = rest.strip_prefix("input/")?.split('/');
    let identifier = segments.next().unwrap_or("");
    let component = segments.next();
    let physical = match identifier {
        "trigger" => Some(Physical::Trigger),
        "squeeze" => Some(Physical::Grip),
        "thumbstick" if component == Some("click") => Some(Physical::ThumbstickClick),
        "thumbstick" => Some(Physical::Thumbstick),
        "trackpad" => Some(Physical::Trackpad),
        "a" | "x" => Some(Physical::LowerFace),
        "b" | "y" => Some(Physical::UpperFace),
        "system" | "menu" => Some(Physical::System),
        _ => None,
    };
    Some((hand, physical))
}

/// The physical inputs of the sources of one action, by [`Hand::index`].
pub(crate) fn of_sources<'a>(paths: impl IntoIterator<Item = &'a str>) -> [PhysicalSet; 2] {
    let mut out = [PhysicalSet::EMPTY; 2];
    for path in paths {
        match classify(path) {
            Some((hand, Some(physical))) => {
                out[hand.index()] = out[hand.index()].with(physical);
            }
            Some((hand, None)) => out[hand.index()] = out[hand.index()].with_unknown(),
            None => {
                for set in &mut out {
                    *set = set.with_unknown();
                }
            }
        }
    }
    out
}

/// A table of default bindings: action `index` of a set's default binding, or `None` when it ships
/// unbound. The shipped table is [`crate::vr::input::semantic::defaults::default_binding`]; the
/// tests also read the layout the tables used before the semantic actions.
pub(crate) type Layout = fn(SemanticSet, usize) -> Option<DefaultBinding>;

/// The physical inputs every action of one set reads on each hand.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundSet {
    pub(crate) set: SemanticSet,
    /// By action index, then [`Hand::index`].
    pub(crate) actions: Vec<[PhysicalSet; 2]>,
}

impl BoundSet {
    /// The inputs `layout` binds each action of `set` to, for a player whose dominant hand is
    /// `dominant` and whose hands report `profiles`.
    pub(crate) fn defaults(
        set: SemanticSet,
        profiles: [InteractionProfile; 2],
        dominant: Hand,
        layout: Layout,
    ) -> BoundSet {
        let actions = (0..set.actions().len())
            .map(|index| default_sources(set, index, profiles, dominant, layout))
            .collect();
        BoundSet { set, actions }
    }

    /// The inputs the runtime reported, as enumerated per action (`None` where the enumeration
    /// failed), with the defaults standing in for the gaps.
    pub(crate) fn merged(
        set: SemanticSet,
        enumerated: &[Option<[PhysicalSet; 2]>],
        profiles: [InteractionProfile; 2],
        dominant: Hand,
        layout: Layout,
    ) -> BoundSet {
        let actions = (0..set.actions().len())
            .map(|index| {
                enumerated
                    .get(index)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| default_sources(set, index, profiles, dominant, layout))
            })
            .collect();
        BoundSet { set, actions }
    }

    /// The inputs action `index` reads on `hand`.
    pub(crate) fn get(&self, index: usize, hand: Hand) -> PhysicalSet {
        self.actions
            .get(index)
            .map_or(PhysicalSet::EMPTY, |a| a[hand.index()])
    }
}

/// The shipped profile whose paths stand for a hand's: the Touch layout for a Touch controller,
/// and the Index layout for every other.
pub(crate) const fn profile_of(profile: InteractionProfile) -> Profile {
    match profile {
        InteractionProfile::OculusTouch => Profile::OculusTouch,
        InteractionProfile::None | InteractionProfile::ValveIndex | InteractionProfile::Other => {
            Profile::ValveIndex
        }
    }
}

const UNKNOWN: u16 = 1 << 15;

fn default_sources(
    set: SemanticSet,
    index: usize,
    profiles: [InteractionProfile; 2],
    dominant: Hand,
    layout: Layout,
) -> [PhysicalSet; 2] {
    let Some(binding) = layout(set, index) else {
        return [PhysicalSet::EMPTY; 2];
    };
    Hand::BOTH.map(|hand| {
        let present = binding.side.covers(hand, dominant)
            && binding
                .component
                .path(profile_of(profiles[hand.index()]), hand)
                .is_some();
        if present {
            PhysicalSet::of(binding.component.physical())
        } else {
            PhysicalSet::EMPTY
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_paths_classify_by_identifier() {
        let left = |rest: &str| classify(&format!("{}/{rest}", hand_path(Hand::Left)));
        let right = |rest: &str| classify(&format!("{}/{rest}", hand_path(Hand::Right)));
        assert_eq!(
            right("input/a/click"),
            Some((Hand::Right, Some(Physical::LowerFace)))
        );
        assert_eq!(
            left("input/x/click"),
            Some((Hand::Left, Some(Physical::LowerFace)))
        );
        assert_eq!(
            left("input/y"),
            Some((Hand::Left, Some(Physical::UpperFace)))
        );
        assert_eq!(
            right("input/thumbstick/click"),
            Some((Hand::Right, Some(Physical::ThumbstickClick)))
        );
        assert_eq!(
            right("input/thumbstick/x"),
            Some((Hand::Right, Some(Physical::Thumbstick)))
        );
        assert_eq!(
            right("input/thumbstick"),
            Some((Hand::Right, Some(Physical::Thumbstick)))
        );
        assert_eq!(
            left("input/squeeze/force"),
            Some((Hand::Left, Some(Physical::Grip)))
        );
        assert_eq!(
            left("input/trigger/value"),
            Some((Hand::Left, Some(Physical::Trigger)))
        );
        assert_eq!(
            left("input/menu/click"),
            Some((Hand::Left, Some(Physical::System)))
        );
        assert_eq!(left("input/pinch_ext/value"), Some((Hand::Left, None)));
        assert_eq!(left("output/haptic"), None);
        assert_eq!(classify("/user/gamepad/input/a/click"), None);
        assert_eq!(classify("/user/hand/leftover/input/a/click"), None);
    }

    #[test]
    fn unknown_sources_are_kept_as_unknown() {
        let left = format!("{}/input/x/click", hand_path(Hand::Left));
        let odd = format!("{}/input/pinch_ext/value", hand_path(Hand::Right));
        let sets = of_sources([left.as_str(), odd.as_str(), "/user/gamepad/input/a/click"]);
        assert_eq!(sets[0], PhysicalSet::of(Physical::LowerFace).with_unknown());
        assert_eq!(sets[1], PhysicalSet::EMPTY.with_unknown());
        assert_eq!(of_sources([]), [PhysicalSet::EMPTY; 2]);
    }
}
