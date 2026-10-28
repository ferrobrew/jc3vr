use jc3gi::input::input_action_map::Action;

use super::*;
use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls},
            summary::grapple_routes,
            tables::{
                Bindings,
                describe::{Control as ShownControl, Effect, ModAction},
            },
            trigger_ownership::{self, Owner, TriggerMask},
        },
        grab::GrabVolume,
        loadout::Socket,
        mode::{Mode, OnFootVariant},
        summary::{current::CurrentContext, eligibility::SnatchHint, snatch},
        weapons::capabilities::{GunHand, GunOwner},
    },
    vr::input::semantic::bound::{EnumeratedSource, SourceTable, of_sources},
};

const MODE: Mode = Mode::OnFoot(OnFootVariant::Ground);
const TRIGGER: &str = "/user/hand/right/input/trigger/value";
const GRIP: &str = "/user/hand/left/input/squeeze/click";
const LEFT_A: &str = "/user/hand/left/input/a/click";

struct Fixture {
    input: SemanticInput,
    policy: Tick,
    table: SourceTable,
}

fn fixture(entries: &[(ActionId, &[&str])]) -> Fixture {
    let make_group = |sequence, pressed| {
        let semantics: Vec<_> = entries
            .iter()
            .map(|(id, sources)| (*id, pressed, paths(sources)))
            .collect();
        let mut group = group(sequence, sequence, [false; 3], &semantics);
        for sample in &mut group.samples {
            if let InputRoute::Semantic { set, action, hand } = sample.route
                && let Some((_, sources)) = entries
                    .iter()
                    .find(|(id, _)| id.set.index() == set as usize && id.index() == action as usize)
            {
                let bound = of_sources(sources.iter().copied());
                sample.sources = paths(sources);
                sample.active = bound[hand as usize] != Default::default();
                sample.down = pressed && sample.active;
                sample.value = match sample.value {
                    ValueBits::Bool(_) => ValueBits::Bool(sample.down),
                    ValueBits::Float(_) => {
                        ValueBits::Float(if sample.down { 1.0f32.to_bits() } else { 0 })
                    }
                    ValueBits::Vec2(_) => ValueBits::Vec2([0; 2]),
                };
            }
        }
        group
    };
    let mut runtime = Runtime::default();
    evaluate(&mut runtime, make_group(1, false), true);
    evaluate(&mut runtime, make_group(2, false), true);
    let policy = evaluate(&mut runtime, make_group(3, true), true);
    let mut input = SemanticInput::rest(SemanticSet::OnFoot, Default::default(), Hand::Right);
    for set in [&mut input.global, &mut input.mode] {
        for action in &mut set.actions {
            action.bound = Default::default();
        }
    }
    let mut shown = vec![Some(Vec::new()); OnFootAction::ALL.len()];
    for &(id, sources) in entries {
        input.get_mut(id).unwrap().bound = of_sources(sources.iter().copied());
        shown[id.index()] = Some(
            sources
                .iter()
                .map(|path| EnumeratedSource {
                    path,
                    localized: Some("Runtime source"),
                })
                .collect(),
        );
    }
    policy.filter_semantic(&mut input);
    let mut table = SourceTable::default();
    table.insert(SemanticSet::OnFoot, shown);
    Fixture {
        input,
        policy,
        table,
    }
}

fn assert_hints(fixture: &Fixture, routes: Vec<(ActionId, Hand)>, expected: &[ShownControl]) {
    let mut current = CurrentContext::default();
    current.routing.ready = [true; 2];
    snatch::extend(
        &mut current,
        &fixture.table,
        Some(&SnatchHint {
            hand: Hand::Left,
            swinging: false,
            modifier_enabled: true,
            routes,
        }),
    );
    assert!(
        current
            .usable
            .iter()
            .any(|row| row.effect == Effect::Mod(ModAction::SnatchYank))
    );
    assert!(
        current
            .usable
            .iter()
            .any(|row| row.effect == Effect::Mod(ModAction::SnatchGrip))
    );
    let controls: Vec<_> = current
        .usable
        .iter()
        .filter(|row| row.effect == Effect::Mod(ModAction::SnatchModifier))
        .map(|row| row.control)
        .collect();
    assert_eq!(controls, expected);
    assert_eq!(
        current
            .usable
            .iter()
            .any(|row| row.effect == Effect::Mod(ModAction::SnatchSwing)),
        !expected.is_empty()
    );
}

#[test]
fn snatch_mixed_face_trigger_uses_whole_action_mask_and_keeps_other_hand_alternative() {
    let id = OnFootAction::Grapple.id();
    let owners = [
        Owner {
            mode: MODE,
            gun: None,
        },
        Owner {
            mode: MODE,
            gun: Some(GunOwner {
                weapon: 1,
                role: GunHand::Grip,
            }),
        },
    ];
    let triggers = trigger_ownership::masks(owners, [false; 2]);
    for alternative in [false, true] {
        let paths: &[&str] = if alternative {
            &[A, TRIGGER, LEFT_A]
        } else {
            &[A, TRIGGER]
        };
        let mut f = fixture(&[(id, paths)]);
        let controls = Controls {
            dominant: Hand::Right,
            ..Default::default()
        };
        let mut bindings = Bindings::new();
        assert!(f.policy.permits(id, Hand::Right));
        let routes = grapple_routes(&bindings, MODE, &controls, &f.input, &f.policy, triggers);
        assert!(!routes.contains(&(id, Hand::Right)));
        trigger_ownership::mask_routes(&mut f.input, MODE, triggers);
        let step = bindings.step(MODE, &controls, &f.input, 0.01, &BindingsConfig::new());
        assert_eq!(step.grapple_held, alternative);
        assert_eq!(
            step.desired
                .iter()
                .any(|d| d.target.action == Action::FIRE_GRAPPLE),
            alternative
        );
        assert_hints(
            &f,
            routes,
            if alternative {
                &[ShownControl::A(Hand::Left)]
            } else {
                &[]
            },
        );
    }
}

#[test]
fn snatch_mixed_grip_face_uses_latched_grip_mask() {
    let id = OnFootAction::Grapple.id();
    let f = fixture(&[(id, &[GRIP, LEFT_A])]);
    let mut bindings = Bindings::new();
    let mut controls = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    controls.hands[0].grip = ButtonControl {
        down: true,
        pressed: true,
        released: false,
    };
    controls.hands[0].grab_volume = Some(GrabVolume::Socket(Socket::Hip(Hand::Left)));
    let step = bindings.step(MODE, &controls, &f.input, 0.01, &BindingsConfig::new());
    assert!(bindings.grip_latches()[0].grabbing());
    assert!(!step.grapple_held);
    assert!(
        !step
            .desired
            .iter()
            .any(|d| d.target.action == Action::FIRE_GRAPPLE)
    );
    let routes = grapple_routes(
        &bindings,
        MODE,
        &controls,
        &f.input,
        &f.policy,
        [TriggerMask::default(); 2],
    );
    assert!(routes.is_empty());
    assert_hints(&f, routes, &[]);
}

#[test]
fn snatch_separate_action_alternative_survives_another_actions_aggregate_mask() {
    let mut f = fixture(&[
        (OnFootAction::Grapple.id(), &[A, TRIGGER]),
        (OnFootAction::FireGrapple.id(), &[LEFT_A]),
    ]);
    let controls = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    let triggers = [
        TriggerMask::default(),
        TriggerMask {
            reserved: true,
            ..Default::default()
        },
    ];
    let mut bindings = Bindings::new();
    let routes = grapple_routes(&bindings, MODE, &controls, &f.input, &f.policy, triggers);
    assert_eq!(routes, vec![(OnFootAction::FireGrapple.id(), Hand::Left)]);
    trigger_ownership::mask_routes(&mut f.input, MODE, triggers);
    let step = bindings.step(MODE, &controls, &f.input, 0.01, &BindingsConfig::new());
    assert!(step.grapple_held);
    assert!(
        step.desired
            .iter()
            .any(|desired| desired.target.action == Action::FIRE_GRAPPLE)
    );
    assert_hints(&f, routes, &[ShownControl::A(Hand::Left)]);
}

#[test]
fn snatch_unresolved_hand_and_nonhand_paths_never_advertise_modifier() {
    let id = OnFootAction::Grapple.id();
    for unknown in [
        "/user/hand/right/input/unknown/click",
        "/user/head/input/select/click",
    ] {
        for paths in [vec![unknown], vec![A, unknown]] {
            let f = fixture(&[(id, &paths)]);
            assert!(!f.policy.permits(id, Hand::Right));
            assert!(!f.policy.permits(id, Hand::Left));
            assert!(
                !f.input
                    .get(id)
                    .unwrap()
                    .hands
                    .iter()
                    .any(|intent| intent.button.held())
            );
            let routes = grapple_routes(
                &Bindings::new(),
                MODE,
                &Controls::default(),
                &f.input,
                &f.policy,
                [TriggerMask::default(); 2],
            );
            assert!(routes.is_empty());
            assert_hints(&f, routes, &[]);
        }
    }
}
