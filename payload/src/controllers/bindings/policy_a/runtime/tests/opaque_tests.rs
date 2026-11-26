//! The arbiter on a runtime that names its bound sources under no user path (Monado's `/mnd/<n>`):
//! the default bindings stand in as the attribution, and the raw protected routes are taken at
//! their fixed location.

use super::*;
use crate::vr::input::semantic::bound::opaque;

/// The raw routes' sources, as the runtime reports them: each `hands` action is bound on both
/// hands, so every route names two opaque sources.
const RAW: [&[&str]; CONTROLS] = [
    &["/mnd/2367", "/mnd/2372"],
    &["/mnd/2367", "/mnd/2372"],
    &["/mnd/2355", "/mnd/2357"],
    &["/mnd/2351", "/mnd/2353"],
];

/// A complete on-foot group with opaque sources on every route, as a runtime reports the default
/// bindings: a semantic route is active where its default binding covers the hand, and pressed
/// where its default input is one of the raw controls `down` or it is listed in `pressed`.
fn opaque_group(
    ms: u64,
    sequence: u64,
    down: [bool; CONTROLS],
    pressed: &[(ActionId, Hand)],
) -> InputGroup {
    let mut group = group(ms, sequence, down, &[]);
    for sample in &mut group.samples {
        match sample.route {
            InputRoute::Raw(index) => sample.sources = paths(RAW[index as usize]),
            InputRoute::Semantic { set, action, hand } => {
                let set = SemanticSet::ALL[set as usize];
                let hand = Hand::BOTH[hand as usize];
                let id = ActionId {
                    set,
                    index: action as u8,
                };
                let binding = default_binding(set, id.index());
                let source = format!("/mnd/{}", 3000 + set.index() * 100 + id.index());
                sample.sources = binding.map_or_else(|| paths(&[]), |_| paths(&[&source]));
                let covered = binding.filter(|binding| binding.side.covers(hand, Hand::Right));
                sample.active = covered.is_some();
                let held = covered.is_some_and(|binding| {
                    Control::ALL.into_iter().any(|control| {
                        down[control.index()]
                            && raw_location(control) == (hand, binding.component.physical())
                    })
                }) || pressed.contains(&(id, hand));
                sample.down = held;
                sample.value = match sample.value {
                    ValueBits::Bool(_) => ValueBits::Bool(held),
                    ValueBits::Float(_) => {
                        ValueBits::Float(if held { 1.0f32.to_bits() } else { 0 })
                    }
                    ValueBits::Vec2(value) => ValueBits::Vec2(value),
                };
            }
        }
    }
    group
}

fn semantic_sample(group: &mut InputGroup, id: ActionId, hand: Hand) -> &mut RouteSample {
    group
        .samples
        .iter_mut()
        .find(|sample| {
            sample.route
                == InputRoute::Semantic {
                    set: id.set.index() as u8,
                    action: id.index() as u16,
                    hand: hand.index() as u8,
                }
        })
        .unwrap()
}

/// Two neutral groups, which arm every route.
fn armed() -> Runtime {
    let mut runtime = Runtime::default();
    for (ms, sequence) in [(1, 1), (2, 2)] {
        evaluate(
            &mut runtime,
            opaque_group(ms, sequence, [false; CONTROLS], &[]),
            true,
        );
    }
    runtime
}

#[test]
fn an_opaque_neutral_group_is_a_neutral_boundary() {
    let group = opaque_group(1, 1, [false; CONTROLS], &[]);
    assert!(group.samples.iter().all(|sample| match &sample.sources {
        SourceQuery::Paths(paths) => paths.is_empty() || opaque(paths.iter().map(String::as_str)),
        _ => false,
    }));
    assert!(group_neutral(&group, &BindingsConfig::new(), Hand::Right));
    let pressed = opaque_group(1, 1, [false, false, false, true], &[]);
    assert!(!group_neutral(
        &pressed,
        &BindingsConfig::new(),
        Hand::Right
    ));
}

#[test]
fn recognised_paths_that_contradict_still_fail() {
    let config = BindingsConfig::new();
    // A recognised source on the other hand leaves the active right-hand route unattributed.
    let mut wrong_hand = opaque_group(1, 1, [false; CONTROLS], &[]);
    semantic_sample(&mut wrong_hand, OnFootAction::Jump.id(), Hand::Right).sources =
        paths(&["/user/hand/left/input/a/click"]);
    assert!(!group_neutral(&wrong_hand, &config, Hand::Right));
    // A recognised raw source at the wrong location does not verify the protected route.
    let mut wrong_raw = opaque_group(1, 1, [false; CONTROLS], &[]);
    wrong_raw.samples[Control::RightA.index()].sources = paths(&[RB]);
    assert!(!group_neutral(&wrong_raw, &config, Hand::Right));
    // An enumeration that mixes an opaque source with a user path is read path by path.
    let mut mixed = opaque_group(1, 1, [false; CONTROLS], &[]);
    semantic_sample(&mut mixed, OnFootAction::Jump.id(), Hand::Right).sources =
        paths(&["/mnd/3108", A]);
    assert!(!group_neutral(&mixed, &config, Hand::Right));
}

#[test]
fn a_lone_gameplay_press_is_admitted() {
    let fire = OnFootAction::Fire.id();
    let mut runtime = armed();
    let tick = evaluate(
        &mut runtime,
        opaque_group(10, 3, [false; CONTROLS], &[(fire, Hand::Right)]),
        true,
    );
    assert!(button(&tick, fire).pressed);

    let jump = OnFootAction::Jump.id();
    let mut runtime = armed();
    let tick = evaluate(
        &mut runtime,
        opaque_group(10, 3, [false, false, false, true], &[]),
        true,
    );
    assert!(button(&tick, jump).pressed, "the A jumps at once alone");
    assert!(tick.commands.is_empty());
}

#[test]
fn the_panel_and_mark_chords_fire() {
    let mut runtime = armed();
    evaluate(
        &mut runtime,
        opaque_group(100, 3, [true, false, false, false], &[]),
        true,
    );
    let panel = evaluate(
        &mut runtime,
        opaque_group(150, 4, [true, false, true, false], &[]),
        true,
    );
    assert_eq!(panel.commands, [SystemCommand::TogglePanel]);

    let jump = OnFootAction::Jump.id();
    let mut runtime = armed();
    evaluate(
        &mut runtime,
        opaque_group(100, 3, [true, false, false, false], &[]),
        true,
    );
    let mark = evaluate(
        &mut runtime,
        opaque_group(150, 4, [true, false, false, true], &[]),
        true,
    );
    assert_eq!(mark.commands, [SystemCommand::Mark]);
    assert!(!button(&mark, jump).held(), "the chord consumes the A");
}

#[test]
fn recenter_fires_after_both_clicks_are_held() {
    let both = [true, true, false, false];
    let mut runtime = armed();
    let mut commands = Vec::new();
    for (step, ms) in (100..=1300).step_by(50).enumerate() {
        let tick = evaluate(
            &mut runtime,
            opaque_group(ms, 3 + step as u64, both, &[]),
            true,
        );
        commands.extend(tick.commands);
    }
    assert_eq!(commands, [SystemCommand::Recenter]);
}

/// Check a real recording's groups for neutral boundaries. Run it with
/// `JC3VR_TRACKING_RECORDING=<path to tracking.jc3trk>` and `--ignored --nocapture`.
#[test]
#[ignore = "reads a recording named by JC3VR_TRACKING_RECORDING"]
fn a_recording_produces_neutral_boundaries() {
    let path = std::env::var("JC3VR_TRACKING_RECORDING")
        .expect("JC3VR_TRACKING_RECORDING names a tracking recording");
    let recording = tracking_format::decode(&std::fs::read(&path).unwrap()).unwrap();
    let dominant = match recording.header.dominant_hand {
        tracking_format::model::Hand::Left => Hand::Left,
        tracking_format::model::Hand::Right => Hand::Right,
    };
    let config = BindingsConfig::new();
    let groups = recording
        .input
        .iter()
        .filter_map(|captured| match &captured.record {
            InputRecord::Group(group) => Some(group),
            _ => None,
        })
        .collect::<Vec<_>>();
    let neutral = groups
        .iter()
        .filter(|group| group_neutral(group, &config, dominant))
        .count();
    println!(
        "recording: {neutral} of {} groups are neutral",
        groups.len()
    );
    assert!(neutral > 0);
}
