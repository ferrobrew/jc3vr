//! The raw-input tables' menu latches, frozen.

use glam::Vec2;

use crate::controllers::bindings::{
    controls::{ButtonControl, HandControls},
    tables::tests::legacy::sink::{Direction, MenuLatch},
};

/// Advance `latch` by one tick of `hand`'s controls. `push` is how far the stick must be pushed
/// along one axis to latch a direction, `0..=1`.
pub fn step(latch: &mut MenuLatch, hand: &HandControls, push: f32) {
    let gripped = hand.grip.held();
    latch.a = latch_button(latch.a, hand.a, gripped);
    latch.b = latch_button(latch.b, hand.b, gripped);
    latch.stick = latch_stick(latch.stick, hand.stick_raw, push, gripped);
}

/// A button's latch: whether its press began with the grip held, kept while it is held. A release
/// and a new press between two ticks begins a new press.
fn latch_button(latched: Option<bool>, button: ButtonControl, gripped: bool) -> Option<bool> {
    if !button.held() {
        return None;
    }
    let repressed = button.pressed && button.released && button.down;
    match latched {
        Some(latched) if !repressed => Some(latched),
        _ => Some(gripped),
    }
}

/// The stick's latch: the latched direction holds while the push along it stays at or above half of
/// `push`; otherwise a push of at least `push` along the stick's larger axis latches that axis's
/// direction, with the grip as it is now.
fn latch_stick(
    latched: Option<(Direction, bool)>,
    stick: Vec2,
    push: f32,
    gripped: bool,
) -> Option<(Direction, bool)> {
    let push = push.clamp(0.1, 1.0);
    if !stick.is_finite() {
        return None;
    }
    if let Some((direction, _)) = latched
        && along(stick, direction) >= push * 0.5
    {
        return latched;
    }
    let direction = if stick.y.abs() >= stick.x.abs() {
        if stick.y >= 0.0 {
            Direction::Up
        } else {
            Direction::Down
        }
    } else if stick.x >= 0.0 {
        Direction::Right
    } else {
        Direction::Left
    };
    (along(stick, direction) >= push).then_some((direction, gripped))
}

/// How far `stick` (+X right, +Y forward) is pushed toward `direction`.
fn along(stick: Vec2, direction: Direction) -> f32 {
    match direction {
        Direction::Up => stick.y,
        Direction::Down => -stick.y,
        Direction::Left => -stick.x,
        Direction::Right => stick.x,
    }
}
