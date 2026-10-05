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
//!
//! The same matching, at the finer grain of a [`Component`], gives the controls summary its glyphs
//! ([`SourceTable`]): a recognised source is drawn with its glyph, and any other with the
//! runtime's localized name for it.

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
    classify_component(path).map(|(hand, component)| (hand, component.map(Component::physical)))
}

/// The hand a bound source path is under, and the [`Component`] it names, or `None` for one the
/// mod does not recognise. A path under no hand is `None` altogether. The component is as fine as
/// the summary's glyphs need: a stick's axis apart from the stick, and a trackpad's touch and
/// press apart from its surface.
pub(crate) fn classify_component(path: &str) -> Option<(Hand, Option<Component>)> {
    let (hand, rest) = Hand::BOTH.into_iter().find_map(|hand| {
        path.strip_prefix(hand_path(hand))
            .and_then(|rest| rest.strip_prefix('/'))
            .map(|rest| (hand, rest))
    })?;
    let mut segments = rest.strip_prefix("input/")?.split('/');
    let identifier = segments.next().unwrap_or("");
    let component = segments.next();
    let known = match (identifier, component) {
        ("trigger", _) => Some(Component::TriggerValue),
        ("squeeze", _) => Some(Component::GripPress),
        ("thumbstick", Some("click")) => Some(Component::ThumbstickClick),
        ("thumbstick", Some("x")) => Some(Component::ThumbstickX),
        ("thumbstick", Some("y")) => Some(Component::ThumbstickY),
        ("thumbstick", _) => Some(Component::Thumbstick),
        ("trackpad", Some("touch")) => Some(Component::TrackpadTouch),
        ("trackpad", Some("force" | "click")) => Some(Component::TrackpadForce),
        ("trackpad", _) => Some(Component::Trackpad),
        ("a" | "x", _) => Some(Component::LowerFace),
        ("b" | "y", _) => Some(Component::UpperFace),
        ("system" | "menu", _) => Some(Component::System),
        _ => None,
    };
    Some((hand, known))
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

/// A source's name in a [`SourceTable`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SourceName(u16);

/// One bound source of an action, as the controls summary shows it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct ShownSource {
    /// The hand whose user path the source is under.
    pub(crate) hand: Option<Hand>,
    /// The component the source names, where the mod recognises it.
    pub(crate) component: Option<Component>,
    /// The runtime's name for the source, for one no glyph shows.
    pub(crate) name: SourceName,
}

/// One source as the runtime enumerated it: its path, and its localized name where it could be
/// read.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct EnumeratedSource<'a> {
    pub(crate) path: &'a str,
    pub(crate) localized: Option<&'a str>,
}

/// The runtime's bound sources of the semantic actions, as the controls summary shows them: per
/// set that has been enumerated, each action's sources, matched to the components the mod knows
/// and named by the runtime.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SourceTable {
    /// By [`SemanticSet::index`], then action index; `None` for a set not enumerated yet, or for an
    /// action whose enumeration failed.
    sets: [Option<Vec<Option<Vec<ShownSource>>>>; SemanticSet::ALL.len()],
    names: Vec<String>,
}

impl SourceTable {
    /// Record the sources of every action of `set`, by action index, `None` for an action whose
    /// enumeration failed. A source's name is the runtime's localized name, or its path where the
    /// runtime gave none.
    pub(crate) fn insert<'a>(
        &mut self,
        set: SemanticSet,
        actions: impl IntoIterator<Item = Option<Vec<EnumeratedSource<'a>>>>,
    ) {
        let actions = actions
            .into_iter()
            .map(|sources| {
                sources.map(|sources| {
                    let mut shown: Vec<ShownSource> = Vec::with_capacity(sources.len());
                    for source in sources {
                        let (hand, component) = match classify_component(source.path) {
                            Some((hand, component)) => (Some(hand), component),
                            None => (None, None),
                        };
                        let name = self.intern(source.localized.unwrap_or(source.path));
                        let entry = ShownSource {
                            hand,
                            component,
                            name,
                        };
                        // A runtime may report one input under several paths; a recognised one is
                        // shown once.
                        let duplicate = shown.iter().any(|s| {
                            s.hand == entry.hand
                                && match (s.component, entry.component) {
                                    (Some(a), Some(b)) => a == b,
                                    _ => s.name == entry.name,
                                }
                        });
                        if !duplicate {
                            shown.push(entry);
                        }
                    }
                    shown
                })
            })
            .collect();
        self.sets[set.index()] = Some(actions);
    }

    /// The sources of action `index` of `set`, or `None` where they are not known: the set has not
    /// been enumerated, or the action's enumeration failed. An empty slice is an unbound action.
    pub(crate) fn sources(&self, set: SemanticSet, index: usize) -> Option<&[ShownSource]> {
        self.sets[set.index()].as_ref()?.get(index)?.as_deref()
    }

    /// The text of a source's name.
    pub(crate) fn name(&self, name: SourceName) -> &str {
        self.names
            .get(usize::from(name.0))
            .map_or("", String::as_str)
    }

    fn intern(&mut self, text: &str) -> SourceName {
        let index = match self.names.iter().position(|n| n == text) {
            Some(index) => index,
            None => {
                self.names.push(text.to_owned());
                self.names.len() - 1
            }
        };
        SourceName(u16::try_from(index).unwrap_or(u16::MAX))
    }
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

    #[test]
    fn components_classify_as_finely_as_the_glyphs() {
        let right = |rest: &str| classify_component(&format!("{}/{rest}", hand_path(Hand::Right)));
        let known = |component| Some((Hand::Right, Some(component)));
        assert_eq!(right("input/thumbstick/x"), known(Component::ThumbstickX));
        assert_eq!(right("input/thumbstick/y"), known(Component::ThumbstickY));
        assert_eq!(right("input/thumbstick"), known(Component::Thumbstick));
        assert_eq!(
            right("input/thumbstick/click"),
            known(Component::ThumbstickClick)
        );
        assert_eq!(
            right("input/trackpad/touch"),
            known(Component::TrackpadTouch)
        );
        assert_eq!(
            right("input/trackpad/force"),
            known(Component::TrackpadForce)
        );
        assert_eq!(
            right("input/trackpad/click"),
            known(Component::TrackpadForce)
        );
        assert_eq!(right("input/trackpad"), known(Component::Trackpad));
        assert_eq!(right("input/squeeze/value"), known(Component::GripPress));
        assert_eq!(right("input/b/click"), known(Component::UpperFace));
        assert_eq!(right("input/system/click"), known(Component::System));
        assert_eq!(right("input/pinch_ext/value"), Some((Hand::Right, None)));
        assert_eq!(classify_component("/user/gamepad/input/a/click"), None);
    }

    #[test]
    fn the_coarse_classification_follows_the_components() {
        for path in [
            "input/trigger/value",
            "input/squeeze/force",
            "input/thumbstick/click",
            "input/thumbstick/y",
            "input/trackpad/touch",
            "input/x/click",
            "input/y/click",
            "input/menu/click",
            "input/pinch_ext/value",
        ] {
            let path = format!("{}/{path}", hand_path(Hand::Left));
            assert_eq!(
                classify(&path),
                classify_component(&path).map(|(h, c)| (h, c.map(Component::physical))),
                "{path}"
            );
        }
    }

    #[test]
    fn the_source_table_names_and_matches_each_source() {
        let source = |path, localized| EnumeratedSource { path, localized };
        let mut table = SourceTable::default();
        assert_eq!(table.sources(SemanticSet::OnFoot, 0), None);
        table.insert(
            SemanticSet::OnFoot,
            [
                Some(vec![
                    source("/user/hand/left/input/trigger/value", Some("Left Trigger")),
                    // The same input under another path is shown once.
                    source(
                        "/user/hand/left/input/trigger/click",
                        Some("Left Trigger Click"),
                    ),
                    source("/user/hand/right/input/pinch_ext/value", None),
                    source("/user/gamepad/input/a/click", Some("Gamepad A")),
                ]),
                None,
                Some(Vec::new()),
            ],
        );
        let first = table.sources(SemanticSet::OnFoot, 0).unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(
            (first[0].hand, first[0].component),
            (Some(Hand::Left), Some(Component::TriggerValue))
        );
        assert_eq!(table.name(first[0].name), "Left Trigger");
        assert_eq!(
            (first[1].hand, first[1].component),
            (Some(Hand::Right), None)
        );
        assert_eq!(
            table.name(first[1].name),
            "/user/hand/right/input/pinch_ext/value"
        );
        assert_eq!((first[2].hand, first[2].component), (None, None));
        assert_eq!(table.name(first[2].name), "Gamepad A");
        assert_eq!(table.sources(SemanticSet::OnFoot, 1), None);
        assert_eq!(table.sources(SemanticSet::OnFoot, 2), Some(&[][..]));
        assert_eq!(table.sources(SemanticSet::OnFoot, 3), None);
        assert_eq!(table.sources(SemanticSet::Menu, 0), None);
    }
}
