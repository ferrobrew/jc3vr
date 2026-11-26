//! Reaching a vehicle's occupant through one of its windows the glass brought down
//! ([`crate::controllers::glass`]). The glass has no collision with the hands, so the hand's way in
//! is judged by casting along it: from the player's eyes to the palm, which a hand reached in
//! through the opening crosses, and on from the palm to the point it reaches on the occupant, which
//! a hand still outside crosses.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::{aim::raycast::CastOptions, glass::bullets, physical_hands::query};

/// Whether a palm at `palm` that reaches `target` on an occupant of `vehicle` came in through one
/// of the vehicle's panes the glass brought down ([`query::crosses_hidden_pane`]), with the
/// player's eyes at `eyes`.
///
/// # Safety
///
/// The game thread, outside the physics step; `vehicle` must be the live vehicle the occupant is
/// attached to.
pub(crate) unsafe fn reached_through(
    vehicle: NonZeroUsize,
    eyes: Vec3,
    palm: Vec3,
    target: Vec3,
) -> bool {
    if !bullets::any_hidden() {
        return false;
    }
    // SAFETY: forwarded from the caller.
    let options = unsafe { CastOptions::ignoring_player(0) };
    any_leg_crosses(&[eyes, palm, target], |from, to| {
        // A cast the engine refuses shows no way in, which leaves the occupant out of reach as it
        // was.
        unsafe { query::crosses_hidden_pane(from, to, vehicle, &options) }.unwrap_or(false)
    })
}

/// The point the camera renders the player's eyes from, where the hand's way in starts.
pub(crate) fn eyes() -> Vec3 {
    jc3vr_view::hooks::camera::eye_point()
}

/// Whether any leg of the path through `points` crosses, by `crosses`, skipping a leg too short or
/// too broken to cast.
fn any_leg_crosses(points: &[Vec3], mut crosses: impl FnMut(Vec3, Vec3) -> bool) -> bool {
    points
        .windows(2)
        .filter(|leg| {
            leg[0].is_finite()
                && leg[1].is_finite()
                && leg[0].distance_squared(leg[1]) >= MIN_LEG_M * MIN_LEG_M
        })
        .any(|leg| crosses(leg[0], leg[1]))
}

/// The shortest leg of the hand's way in that is cast, metres.
const MIN_LEG_M: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eyes_to_palm_leg_is_tried_before_the_palm_to_target_leg() {
        let path = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, 1.0)];
        let mut legs = Vec::new();
        assert!(!any_leg_crosses(&path, |from, to| {
            legs.push((from, to));
            false
        }));
        assert_eq!(legs, [(path[0], path[1]), (path[1], path[2])]);
        // The first leg that crosses settles it.
        legs.clear();
        assert!(any_leg_crosses(&path, |from, to| {
            legs.push((from, to));
            true
        }));
        assert_eq!(legs, [(path[0], path[1])]);
    }

    #[test]
    fn a_degenerate_leg_is_never_cast() {
        let cast = |from: Vec3, to: Vec3| -> bool { panic!("cast {from} to {to}") };
        assert!(!any_leg_crosses(&[Vec3::ZERO, Vec3::splat(0.001)], cast));
        assert!(!any_leg_crosses(&[Vec3::NAN, Vec3::X], cast));
        assert!(!any_leg_crosses(&[Vec3::ZERO, Vec3::INFINITY], cast));
        assert!(!any_leg_crosses(&[Vec3::ZERO], cast));
        // A hand with no gap to the target still has its way in from the eyes judged.
        let mut legs = 0;
        any_leg_crosses(&[Vec3::ZERO, Vec3::X, Vec3::X], |_, _| {
            legs += 1;
            false
        });
        assert_eq!(legs, 1);
    }
}
