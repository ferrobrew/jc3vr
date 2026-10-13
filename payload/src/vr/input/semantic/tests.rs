use std::collections::{HashMap, HashSet};

use super::*;
use crate::{
    controllers::frame::Hand,
    vr::input::{
        layout::{self, BoolInput, FloatInput, HandsAction, Input, Profile, Vec2Input},
        semantic::{
            defaults::{
                Side, Suggestion, default_binding, default_path, mirror, resolve, suggestions,
            },
            vocabulary::{
                GlobalAction as G, HelicopterAction as H, LandVehicleAction as L, MenuAction as M,
                OnFootAction as F, PlaneAction as P,
            },
        },
    },
};

/// The schema's version, then every set, action, and type of the schema, in stash order. Changing
/// it changes the players' saved bindings: bump [`SCHEMA_VERSION`] and add a changelog entry (see
/// its documentation). It is a file of its own because `tools/tracking-synth` reads it to index the
/// semantic actions of the recordings it writes.
const GOLDEN: &str = include_str!("schema.golden");

#[test]
fn the_schema_matches_the_golden_snapshot() {
    let mut set = "";
    let mut lines = String::new();
    for (name, tag) in layout::stash_order() {
        if tag == "set" {
            set = name;
            lines.push_str(&format!("{name}: set\n"));
        } else {
            lines.push_str(&format!("{set}.{name}: {tag}\n"));
        }
    }
    let (version, golden) = GOLDEN
        .split_once('\n')
        .expect("the snapshot starts with its version line");
    assert_eq!(version, format!("version: {SCHEMA_VERSION}"));
    assert_eq!(lines, golden);
}

#[test]
fn semantic_names_are_valid_and_unique_per_set() {
    let valid = |name: &str| {
        !name.is_empty()
            && name.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    };
    let mut sets = HashSet::new();
    for set in SemanticSet::ALL {
        assert!(valid(set.name()), "{}", set.name());
        assert!(sets.insert(set.name()), "{}", set.name());
        let mut names = HashSet::new();
        for def in set.actions() {
            assert!(valid(def.name), "{}.{}", set.name(), def.name);
            assert!(names.insert(def.name), "{}.{}", set.name(), def.name);
            assert!(!def.localized_name.is_empty());
        }
    }
}

#[test]
fn every_mode_selects_a_distinct_set_and_global_is_none_of_them() {
    let selected: HashSet<SemanticSet> = ActionSetKind::ALL
        .iter()
        .map(|&k| SemanticSet::of(k))
        .collect();
    assert_eq!(selected.len(), ActionSetKind::ALL.len());
    assert!(!selected.contains(&SemanticSet::Global));
    for kind in ActionSetKind::ALL {
        assert_eq!(kind.name(), SemanticSet::of(kind).name());
        assert_eq!(ActionSetKind::ALL[kind.index()], kind);
    }
    for (i, set) in SemanticSet::ALL.iter().enumerate() {
        assert_eq!(set.index(), i);
    }
}

/// The component paths each shipped profile defines on `hand`, from the OpenXR specification's
/// interaction profile paths.
fn allowed(profile: Profile, hand: Hand) -> &'static [&'static str] {
    const INDEX: &[&str] = &[
        "input/system/click",
        "input/system/touch",
        "input/a/click",
        "input/a/touch",
        "input/b/click",
        "input/b/touch",
        "input/squeeze/value",
        "input/squeeze/force",
        "input/trigger/click",
        "input/trigger/value",
        "input/trigger/touch",
        "input/thumbstick",
        "input/thumbstick/x",
        "input/thumbstick/y",
        "input/thumbstick/click",
        "input/thumbstick/touch",
        "input/trackpad",
        "input/trackpad/x",
        "input/trackpad/y",
        "input/trackpad/force",
        "input/trackpad/touch",
        "input/grip/pose",
        "input/aim/pose",
        "output/haptic",
    ];
    const TOUCH_LEFT: &[&str] = &[
        "input/x/click",
        "input/x/touch",
        "input/y/click",
        "input/y/touch",
        "input/menu/click",
        "input/squeeze/value",
        "input/trigger/value",
        "input/trigger/touch",
        "input/thumbstick",
        "input/thumbstick/x",
        "input/thumbstick/y",
        "input/thumbstick/click",
        "input/thumbstick/touch",
        "input/thumbrest/touch",
        "input/grip/pose",
        "input/aim/pose",
        "output/haptic",
    ];
    const TOUCH_RIGHT: &[&str] = &[
        "input/a/click",
        "input/a/touch",
        "input/b/click",
        "input/b/touch",
        "input/system/click",
        "input/squeeze/value",
        "input/trigger/value",
        "input/trigger/touch",
        "input/thumbstick",
        "input/thumbstick/x",
        "input/thumbstick/y",
        "input/thumbstick/click",
        "input/thumbstick/touch",
        "input/thumbrest/touch",
        "input/grip/pose",
        "input/aim/pose",
        "output/haptic",
    ];
    match (profile, hand) {
        (Profile::ValveIndex, _) => INDEX,
        (Profile::OculusTouch, Hand::Left) => TOUCH_LEFT,
        (Profile::OculusTouch, Hand::Right) => TOUCH_RIGHT,
    }
}

/// Whether an action of `kind` may bind to `path`: a two-axis action only to a parent with x and
/// y, a scalar to a scalar or an axis, and a boolean to a click, a touch, or a thresholded scalar.
fn type_fits(kind: ActionKind, path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or_default();
    match kind {
        ActionKind::Vec2 => matches!(last, "thumbstick" | "trackpad"),
        ActionKind::Float => matches!(last, "value" | "force" | "x" | "y"),
        ActionKind::Bool => matches!(last, "click" | "touch" | "value" | "force"),
    }
}

#[test]
fn every_suggested_path_is_a_component_of_its_profile_and_fits_the_type() {
    for profile in Profile::ALL {
        for dominant in Hand::BOTH {
            for s in suggestions(profile, dominant) {
                let def = s.set.actions()[s.index];
                assert!(
                    allowed(profile, s.hand).contains(&s.path),
                    "{}.{} on {:?} {:?}: {}",
                    s.set.name(),
                    def.name,
                    profile,
                    s.hand,
                    s.path
                );
                assert!(
                    type_fits(def.kind, s.path),
                    "{}.{}: {:?} on {}",
                    s.set.name(),
                    def.name,
                    def.kind,
                    s.path
                );
            }
        }
        for hand in Hand::BOTH {
            let raw = HandsAction::ALL
                .iter()
                .map(|&a| (Input::Hands(a), None))
                .chain(
                    BoolInput::ALL
                        .iter()
                        .map(|&b| (Input::Bool(b), Some(ActionKind::Bool))),
                )
                .chain(
                    FloatInput::ALL
                        .iter()
                        .map(|&f| (Input::Float(f), Some(ActionKind::Float))),
                )
                .chain(
                    Vec2Input::ALL
                        .iter()
                        .map(|&v| (Input::Vec2(v), Some(ActionKind::Vec2))),
                );
            for (input, kind) in raw {
                let Some(path) = layout::binding(profile, hand, input) else {
                    continue;
                };
                assert!(
                    allowed(profile, hand).contains(&path),
                    "{input:?} on {profile:?} {hand:?}: {path}"
                );
                if let Some(kind) = kind {
                    assert!(type_fits(kind, path), "{input:?}: {path}");
                }
            }
        }
    }
}

fn sorted(mut table: Vec<Suggestion>) -> Vec<(usize, usize, usize, &'static str)> {
    let mut keys: Vec<_> = table
        .drain(..)
        .map(|s| (s.set.index(), s.index, s.hand.index(), s.path))
        .collect();
    keys.sort();
    keys
}

#[test]
fn the_left_handed_table_is_the_right_handed_one_with_the_hands_swapped() {
    for profile in Profile::ALL {
        let right = resolve(profile, Hand::Right);
        let mirrored = mirror(profile, &right);
        assert_eq!(
            sorted(mirrored.clone()),
            sorted(resolve(profile, Hand::Left))
        );
        assert_eq!(
            sorted(suggestions(profile, Hand::Left)),
            sorted(mirrored.clone())
        );
        assert_eq!(
            sorted(suggestions(profile, Hand::Right)),
            sorted(right.clone())
        );
    }
    // The Index controllers are alike, so there the mirror moves each handed row to the other hand
    // at the same path, and leaves the rest alone.
    let right = resolve(Profile::ValveIndex, Hand::Right);
    let swapped: Vec<Suggestion> = right
        .iter()
        .map(|s| match s.binding.side {
            Side::Dom | Side::Off => Suggestion {
                hand: s.hand.other(),
                ..*s
            },
            Side::Both | Side::Left | Side::Right => *s,
        })
        .collect();
    assert_eq!(sorted(swapped), sorted(mirror(Profile::ValveIndex, &right)));
}

/// What decides which of the actions sharing an input is read: the state the binding tables
/// branch on (`controllers::bindings::tables::declare::TableCtx`) and the seat.
#[derive(Debug, Copy, Clone)]
struct ReadCtx {
    dual_wield: bool,
    weapon_cycle: bool,
    driver: bool,
    pointer_hand: Option<Hand>,
    trackpad_paging: bool,
}

fn contexts() -> Vec<ReadCtx> {
    let mut out = Vec::new();
    for bits in 0..16u8 {
        for pointer_hand in [None, Some(Hand::Left), Some(Hand::Right)] {
            out.push(ReadCtx {
                dual_wield: bits & 1 != 0,
                weapon_cycle: bits & 2 != 0,
                driver: bits & 4 != 0,
                trackpad_paging: bits & 8 != 0,
                pointer_hand,
            });
        }
    }
    out
}

/// Whether the mod reads action `index` of `set` on `hand` in `ctx`: the contexts the vocabulary
/// documents for the actions that share a default input with another.
fn reads(set: SemanticSet, index: usize, hand: Hand, ctx: ReadCtx) -> bool {
    match set {
        SemanticSet::OnFoot => match F::ALL[index] {
            F::FireOffHandPistol | F::RetractTethersAlt => ctx.dual_wield,
            F::RetractTethers => !ctx.dual_wield,
            F::NextWeapon => ctx.weapon_cycle && !ctx.dual_wield,
            _ => true,
        },
        SemanticSet::LandVehicle => match L::ALL[index] {
            L::Accelerate | L::Reverse => ctx.driver,
            L::Fire => !ctx.driver,
            L::FireOffHandPistol => !ctx.driver && ctx.dual_wield,
            _ => true,
        },
        SemanticSet::Helicopter => match H::ALL[index] {
            H::CollectiveUp | H::CollectiveDown => ctx.driver,
            H::Fire => !ctx.driver,
            H::FireOffHandPistol => !ctx.driver && ctx.dual_wield,
            _ => true,
        },
        SemanticSet::Plane => match P::ALL[index] {
            P::ThrustUp | P::ThrustDown => ctx.driver,
            P::Fire => !ctx.driver,
            P::FireOffHandPistol => !ctx.driver && ctx.dual_wield,
            _ => true,
        },
        SemanticSet::Menu => match M::ALL[index] {
            M::MenuPagePrev | M::MenuPageNext => ctx.pointer_hand != Some(hand),
            M::PointerSelect => ctx.pointer_hand == Some(hand),
            M::MenuTabPad | M::MenuTabPress => ctx.trackpad_paging,
            _ => true,
        },
        // The cutscene skip shares the right B with the mode's own action there on purpose, as
        // the gamepad's B carries both: the skip acts only in a skippable cutscene, where the
        // gameplay actions do not, and the B's own action only outside one.
        SemanticSet::Global => G::ALL[index] != G::SkipCutscene,
        SemanticSet::Wingsuit => true,
    }
}

/// Which part of its input an action reads: a signed axis can carry one action on each half.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum Half {
    Whole,
    Negative,
    Positive,
}

impl Half {
    const ALL: [Half; 3] = [Half::Whole, Half::Negative, Half::Positive];

    fn overlaps(self, other: Half) -> bool {
        self == other || self == Half::Whole || other == Half::Whole
    }
}

/// The half of its input action `index` of `set` reads: the crouch flick only pushes down the
/// dominant stick's vertical axis, and the weapon wheel only up.
fn half_read(set: SemanticSet, index: usize) -> Half {
    match set {
        SemanticSet::OnFoot => match F::ALL[index] {
            F::CrouchFlick => Half::Negative,
            F::WeaponWheel => Half::Positive,
            _ => Half::Whole,
        },
        _ => Half::Whole,
    }
}

#[test]
fn actions_sharing_a_default_input_are_never_read_together() {
    for mode in ActionSetKind::ALL {
        let sets = [SemanticSet::Global, SemanticSet::of(mode)];
        for profile in Profile::ALL {
            for dominant in Hand::BOTH {
                for hand in Hand::BOTH {
                    for ctx in contexts() {
                        let mut seen: HashMap<(&str, Half), String> = HashMap::new();
                        for set in sets {
                            for (index, def) in set.actions().iter().enumerate() {
                                let Some(path) = default_path(set, index, profile, hand, dominant)
                                else {
                                    continue;
                                };
                                if !reads(set, index, hand, ctx) {
                                    continue;
                                }
                                let name = format!("{}.{}", set.name(), def.name);
                                let half = half_read(set, index);
                                let other = Half::ALL
                                    .into_iter()
                                    .filter(|h| h.overlaps(half))
                                    .find_map(|h| seen.get(&(path, h)));
                                if let Some(other) = other {
                                    panic!(
                                        "{other} and {name} both read {path} on {hand:?} of \
                                         {profile:?} ({dominant:?}-handed) in {ctx:?}"
                                    );
                                }
                                seen.insert((path, half), name);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_defaults_mirror_the_gamepad_by_side() {
    let path = |set, index, hand| default_path(set, index, Profile::ValveIndex, hand, Hand::Right);
    let on_foot = |a: F, hand| path(SemanticSet::OnFoot, a as usize, hand);
    assert_eq!(on_foot(F::Jump, Hand::Right), Some("input/a/click"));
    assert_eq!(on_foot(F::Use, Hand::Right), Some("input/b/click"));
    assert_eq!(on_foot(F::Reload, Hand::Left), Some("input/a/click"));
    assert_eq!(on_foot(F::Use, Hand::Left), None);
    let land = |a: L, hand| path(SemanticSet::LandVehicle, a as usize, hand);
    // A vehicle is exited with the button it was entered with.
    assert_eq!(
        land(L::ExitVehicle, Hand::Right),
        on_foot(F::Use, Hand::Right)
    );
    // The grapple takes the off-hand's upper face in every mode, and nitrous the dominant stick's
    // click.
    assert_eq!(land(L::Grapple, Hand::Left), Some("input/b/click"));
    assert_eq!(
        land(L::VehicleMod, Hand::Right),
        Some("input/thumbstick/click")
    );
    assert_eq!(on_foot(F::Grapple, Hand::Left), Some("input/b/click"));
    assert_eq!(on_foot(F::FireGrapple, Hand::Left), None);
    assert_eq!(on_foot(F::ReleaseTethers, Hand::Left), None);
    assert_eq!(land(L::Handbrake, Hand::Left), Some("input/a/click"));
    for (set, exit) in [
        (SemanticSet::Helicopter, H::ExitVehicle as usize),
        (SemanticSet::Plane, P::ExitVehicle as usize),
    ] {
        assert_eq!(path(set, exit, Hand::Right), Some("input/b/click"));
    }
    assert_eq!(
        on_foot(F::WeaponSelectPress, Hand::Left),
        Some("input/trackpad/force")
    );
    assert_eq!(
        on_foot(F::WeaponSelectPad, Hand::Left),
        Some("input/trackpad")
    );
    for profile in Profile::ALL {
        assert_eq!(
            default_path(
                SemanticSet::OnFoot,
                F::WeaponWheel as usize,
                profile,
                Hand::Right,
                Hand::Right
            ),
            Some("input/thumbstick/y")
        );
    }
    assert_eq!(
        default_path(
            SemanticSet::OnFoot,
            F::WeaponSelectPress as usize,
            Profile::OculusTouch,
            Hand::Left,
            Hand::Right
        ),
        None
    );
    assert_eq!(
        on_foot(F::Detonate, Hand::Right),
        Some("input/thumbstick/click")
    );
    // The grips are free on foot: nothing ships on them.
    for action in F::ALL {
        for hand in Hand::BOTH {
            assert_ne!(
                on_foot(*action, hand),
                Some("input/squeeze/force"),
                "{action:?}"
            );
        }
    }
    // On the left Touch controller the grapple is Y.
    assert_eq!(
        default_path(
            SemanticSet::OnFoot,
            F::Grapple as usize,
            Profile::OculusTouch,
            Hand::Left,
            Hand::Right
        ),
        Some("input/y/click")
    );
    assert_eq!(
        default_binding(SemanticSet::OnFoot, F::ThrowGrenade as usize),
        None
    );
    for unbound in [G::OpenCommlink, G::RecenterView, G::ToggleDebugPanel] {
        assert_eq!(default_binding(SemanticSet::Global, unbound as usize), None);
    }
    let pause = G::Pause as usize;
    for hand in Hand::BOTH {
        assert_eq!(
            path(SemanticSet::Global, pause, hand),
            Some("input/system/click")
        );
    }
    let touch_pause = |hand| {
        default_path(
            SemanticSet::Global,
            pause,
            Profile::OculusTouch,
            hand,
            Hand::Left,
        )
    };
    assert_eq!(touch_pause(Hand::Left), Some("input/menu/click"));
    assert_eq!(touch_pause(Hand::Right), None);
}
