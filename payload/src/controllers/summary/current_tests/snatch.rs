use super::*;
use crate::controllers::summary::eligibility::SnatchHint;

#[test]
fn unavailable_modifier_keeps_ordinary_snatch_without_swing() {
    let ctx = context(Hand::Right);
    let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
    snapshot.snatch = Some(SnatchHint {
        hand: Hand::Left,
        swinging: false,
        modifier_enabled: true,
        routes: Vec::new(),
    });
    for path in [
        None,
        Some("/user/hand/right/input/a/click"),
        Some("/user/hand/right/input/trigger/value"),
    ] {
        let paths: Vec<_> = path.into_iter().collect();
        let table = sources(&[(OnFootAction::Grapple, &paths)]);
        let summary = resolved(&ctx, &table, &snapshot, &[]);
        assert!(text(&summary).contains("Yank to snatch"));
        assert!(!text(&summary).contains("swing"));
        assert!(!text(&summary).contains("resume pull"));
    }
    snapshot.routing.ready[Hand::Left.index()] = false;
    assert!(!text(&resolved(&ctx, &SourceTable::default(), &snapshot, &[])).contains("snatch"));
}

#[test]
fn rebound_grapple_hint_uses_actual_button_and_explicit_release_with_motion_and_grip() {
    let ctx = context(Hand::Right);
    let table = sources(&[(OnFootAction::Grapple, &["/user/hand/right/input/a/click"])]);
    let mut snapshot = snapshot([true; 2], [TriggerUse::Free; 2]);
    snapshot.snatch = Some(SnatchHint {
        hand: Hand::Left,
        swinging: false,
        modifier_enabled: true,
        routes: vec![(OnFootAction::Grapple.id(), Hand::Right)],
    });
    let armed = resolved(&ctx, &table, &snapshot, &[]);
    let modifier = armed.cells[Hand::Right.index()]
        .iter()
        .find(|hint| hint.text == "Hold before yanking to swing")
        .unwrap();
    assert_eq!(modifier.sources[0].control, Control::A(Hand::Right));
    assert!(text(&armed).contains("Yank to snatch"));
    assert!(armed.cells[Hand::Left.index()].iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| source.control == Control::Grip(Hand::Left))
    }));
    snapshot.snatch.as_mut().unwrap().swinging = true;
    let swinging = resolved(&ctx, &table, &snapshot, &[]);
    let resume = swinging.cells[Hand::Right.index()]
        .iter()
        .find(|hint| hint.text == "Release grapple button to resume pull")
        .unwrap();
    assert_eq!(resume.sources[0].control, Control::A(Hand::Right));
    assert!(text(&swinging).contains("Release grip to throw"));
    assert!(swinging.cells[Hand::Left.index()].iter().any(|hint| {
        hint.sources
            .iter()
            .any(|source| source.control == Control::Motion(Hand::Left))
    }));
    assert!(
        !resume
            .sources
            .iter()
            .any(|source| source.control == Control::B(Hand::Left))
    );
}
