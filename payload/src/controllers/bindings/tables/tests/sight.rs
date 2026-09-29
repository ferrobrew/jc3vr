//! Sight-chord and crouch control regressions.

use super::*;

#[test]
fn a_crouch_flick_fires_once_until_the_stick_returns() {
    let mut fired = false;
    assert!(!flick_down(&mut fired, -0.5, 0.8));
    assert!(flick_down(&mut fired, -0.9, 0.8));
    assert!(!flick_down(&mut fired, -1.0, 0.8));
    assert!(!flick_down(&mut fired, -0.3, 0.8));
    assert!(flick_down(&mut fired, -0.85, 0.8));
    assert!(!flick_down(&mut fired, f32::NAN, 0.8));
}

#[test]
fn the_sight_chord_captures_without_jumping_or_grappling() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = down();
    });
    c.sight_chord = true;
    let step = b.step(ON_FOOT, &c, DT, &CONFIG);
    assert!(step.sight_capture);
    assert_eq!(player(&step, JUMP), None);
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
}

#[test]
fn the_sight_chord_captures_only_on_the_press() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = ButtonControl {
            down: true,
            ..Default::default()
        };
    });
    c.sight_chord = true;
    let step = b.step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), None);
}

#[test]
fn a_disarmed_sight_chord_leaves_the_bindings_alone() {
    let mut b = Bindings::default();
    let c = controls(Hand::Right, |dom, off| {
        dom.grip = down();
        off.grip = down();
        dom.a = down();
    });
    let step = b.step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
}

#[test]
fn the_sight_chord_keeps_the_foregrip() {
    let chord = |armed: bool| {
        let mut c = controls(Hand::Right, |dom, off| {
            dom.grip = ButtonControl {
                down: true,
                ..Default::default()
            };
            off.grip = down();
            off.grab_volume = Some(GrabVolume::Foregrip);
        });
        c.sight_chord = armed;
        c
    };
    let mut b = Bindings::default();
    let step = b.step(ON_FOOT, &chord(true), DT, &CONFIG);
    assert_eq!(step.grabs, [None; 2]);
    assert_eq!(player(&step, FIRE_GRAPPLE), None);
    // Without the chord armed, the same press is a foregrip grab.
    let mut b = Bindings::default();
    let step = b.step(ON_FOOT, &chord(false), DT, &CONFIG);
    assert_eq!(step.grabs[Hand::Left.index()], Some(GrabVolume::Foregrip));
}

#[test]
fn the_sight_chord_needs_both_grips() {
    let mut b = Bindings::default();
    let mut c = controls(Hand::Right, |_, off| {
        off.grip = down();
    });
    c.hands[1].a = down();
    c.sight_chord = true;
    let step = b.step(ON_FOOT, &c, DT, &CONFIG);
    assert!(!step.sight_capture);
    assert_eq!(player(&step, JUMP), Some(Want::Hold(1.0)));
    assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
}
