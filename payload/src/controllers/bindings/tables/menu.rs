//! The menu presses' latches ([`MenuLatch`]): each A, B, and stick press of a hand remembers whether
//! that hand's grip was held when it began, and the stick reads as one latched direction at a time.
//! Pure.
//!
//! The game repeats a held menu step itself, firing at once and then faster the longer the input
//! is held, and it ranks the stick's directions up, down, left, right. A stick read as raw half-axes
//! therefore steps up when pushed mostly sideways with a little lift. The latch takes the push's
//! larger axis instead, holds that one direction until the push falls back below half the push
//! threshold, and reports nothing else meanwhile.

use glam::Vec2;

use crate::controllers::bindings::{
    controls::{ButtonControl, HandControls},
    tables::sink::{Direction, MenuLatch},
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_larger_axis_wins_and_holds_until_the_push_halves() {
        let push = 0.5;
        assert_eq!(latch_stick(None, Vec2::new(0.3, 0.45), push, false), None);
        let up = latch_stick(None, Vec2::new(0.4, 0.6), push, false);
        assert_eq!(up, Some((Direction::Up, false)));
        // Swinging sideways while still pushed up keeps the direction.
        assert_eq!(latch_stick(up, Vec2::new(0.9, 0.3), push, true), up);
        // Below half the push it lets go, and the sideways push latches on its own.
        assert_eq!(
            latch_stick(up, Vec2::new(0.9, 0.2), push, true),
            Some((Direction::Right, true))
        );
        assert_eq!(latch_stick(up, Vec2::new(0.1, 0.2), push, false), None);
        assert_eq!(
            latch_stick(None, Vec2::new(-0.7, -0.2), push, false),
            Some((Direction::Left, false))
        );
        assert_eq!(latch_stick(up, Vec2::new(f32::NAN, 1.0), push, false), None);
    }

    #[test]
    fn a_button_keeps_the_grip_it_began_with() {
        let down = ButtonControl {
            down: true,
            pressed: true,
            released: false,
        };
        let held = ButtonControl {
            pressed: false,
            ..down
        };
        let latched = latch_button(None, down, true);
        assert_eq!(latched, Some(true));
        assert_eq!(latch_button(latched, held, false), Some(true));
        let repressed = ButtonControl {
            down: true,
            pressed: true,
            released: true,
        };
        assert_eq!(latch_button(latched, repressed, false), Some(false));
        assert_eq!(latch_button(latched, ButtonControl::default(), true), None);
    }
}
