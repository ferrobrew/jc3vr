//! The free hand on the grapple's held line. While the lock holds the line
//! ([`crate::grapple::physical::lock`]) or the snatch's line swings its target
//! ([`crate::controllers::snatch`]), the free, empty hand can take hold of the line ahead of the
//! grapple hand's wrist ([`GrabVolume::GrappleLine`], provided by [`targets`]). The mod side is in
//! `docs/mod/input/grapple-line-grab.md`.
//!
//! On the lock's line, the held line pulled back toward the body takes line in by the pull, as the
//! grapple hand's own pull does ([`lock_hand`] hands the lock the free hand's grip and the line's
//! direction). On the snatch's line, the hand holding it becomes the line's anchor: the line runs
//! from that hand to the target through the same rope that swings it from the grapple hand, so the
//! hand's pull draws the target in and its swing across the line pulls the target the new way.
//! Letting go hands the line back to the grapple hand with what the pull took in
//! ([`handed_back_m`]).

use glam::Vec3;
use jc3vr_body::controllers::{
    aim::GRAPPLE_ORIGIN_M,
    grab::{GrabVolume, GripTarget, Mark, MarkAt, Provided, latch::GripLatch},
    hands,
    hands::ik,
    intent::yank::HandLine,
};
use jc3vr_input::frame::{Hand, Pose};
use jc3vr_view::headpose::xr;

use crate::{
    controllers::{physical_hands, snatch},
    grapple::{physical::lock, rope::held_floor},
};

/// Where the free hand may take the line, metres: how far ahead of the grapple hand's wrist along
/// the line, and how near the line the hand's palm must come.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct LineReach {
    /// The nearest point to the grapple hand's wrist the free hand takes the line at: past the
    /// reach of the snatch's brace at the wrist.
    pub min_ahead_m: f32,
    /// The farthest point ahead of the wrist: about an arm's length.
    pub max_ahead_m: f32,
    /// How near the far end the free hand may take the line.
    pub far_margin_m: f32,
    /// How near the line the palm must be.
    pub take_m: f32,
}

/// The free hand's reach for the line.
pub(crate) const REACH: LineReach = LineReach {
    min_ahead_m: 0.15,
    max_ahead_m: 1.0,
    far_margin_m: 0.5,
    take_m: 0.12,
};

/// Where the free hand would take the line.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct LineSpot {
    /// The point on the line nearest the palm, world space.
    pub point: Vec3,
    /// How far ahead of the line's origin on the grapple hand the point is, metres.
    pub ahead_m: f32,
    /// How far the palm is from the point, metres.
    pub distance_m: f32,
}

/// Where a palm at `palm` takes the line from `origin` on the grapple hand to `far`, when it is
/// within `reach` of the line ahead of the origin. `None` for a degenerate line or a sample that is
/// not finite.
pub(crate) fn spot(origin: Vec3, far: Vec3, palm: Vec3, reach: &LineReach) -> Option<LineSpot> {
    if !(origin.is_finite() && far.is_finite() && palm.is_finite()) {
        return None;
    }
    let line = far - origin;
    let length_m = line.length();
    let direction = line.try_normalize()?;
    let ahead_m = (palm - origin).dot(direction);
    let farthest = reach.max_ahead_m.min(length_m - reach.far_margin_m);
    if !(ahead_m >= reach.min_ahead_m && ahead_m <= farthest) {
        return None;
    }
    let point = origin + direction * ahead_m;
    let distance_m = palm.distance(point);
    (distance_m <= reach.take_m).then_some(LineSpot {
        point,
        ahead_m,
        distance_m,
    })
}

/// How far ahead of the grapple hand's line origin, along the line toward `far`, a hand at `palm`
/// holds it, metres. `None` for a degenerate line or a sample that is not finite.
pub(crate) fn ahead_m(origin: Vec3, far: Vec3, palm: Vec3) -> Option<f32> {
    let direction = (far - origin).try_normalize()?;
    let ahead = (palm - origin).dot(direction);
    ahead.is_finite().then_some(ahead)
}

/// The line's length from the grapple hand once the free hand lets go of it, metres: `held_m`, the
/// length from the free hand, plus how far ahead of the grapple hand the free hand lets go
/// (`released_ahead_m`). A hand pulled back past the grapple hand has taken in the difference. A
/// hand pushed out along the line pays none out: it gives back at most the `taken_ahead_m` it took
/// the line at. A release point that is not finite gives back what the take took. The length is
/// no shorter than [`held_floor`] of `floor_m`.
pub(crate) fn handed_back_m(
    held_m: f32,
    taken_ahead_m: f32,
    released_ahead_m: Option<f32>,
    floor_m: f32,
) -> f32 {
    let given = released_ahead_m
        .filter(|ahead| ahead.is_finite())
        .map_or(taken_ahead_m, |ahead| ahead.min(taken_ahead_m));
    (held_m + given).max(held_floor(floor_m))
}

/// The line's grab target for the free hand this tick, when the lock holds the line or the snatch's
/// line swings its target from `grapple_hand`, and the other hand is free and empty and within
/// [`REACH`] of the line. `world_scale` turns the reach into the player's scale for the resolver.
pub(crate) fn targets(grapple_hand: Hand, enabled: bool, world_scale: f32) -> Provided {
    let mut out = Provided::default();
    if !enabled {
        return out;
    }
    let Some(far) = far_end(grapple_hand) else {
        return out;
    };
    let free = grapple_hand.other();
    if !free_and_empty(free) {
        return out;
    }
    let records = ik::tick_records().current;
    let (Some(grapple), Some(other)) = (records[grapple_hand.index()], records[free.index()])
    else {
        return out;
    };
    let origin = line_origin(grapple.wrist);
    let Some(spot) = spot(origin, far, palm(other.wrist), &REACH) else {
        return out;
    };
    out.targets.push(GripTarget::measured(
        GrabVolume::GrappleLine,
        free,
        spot.distance_m / world_scale.max(1e-3),
        Mark::ring(MarkAt::World(spot.point), Mark::WORLD_SIZE_M),
    ));
    out
}

/// Whether the hand other than `grapple_hand` holds the line, by its grip `latch`.
pub(crate) fn holds_line(latch: GripLatch) -> bool {
    latch == GripLatch::Grab(GrabVolume::GrappleLine)
}

/// The free hand's grip in the cockpit frame, with the lock's line's direction in the cockpit frame
/// as its aim, while the lock holds the line and the free hand's grip `latch` holds it: the lock
/// measures the hand's pull along it. `None` while the hand or the line is untracked.
pub(crate) fn lock_hand(grapple_hand: Hand, latch: GripLatch) -> Option<HandLine> {
    if !lock::holds() || !holds_line(latch) {
        return None;
    }
    let free = grapple_hand.other();
    if !free_and_empty(free) {
        return None;
    }
    let grapple = ik::tick_records().current[grapple_hand.index()]?;
    let direction = (lock::status().hook - line_origin(grapple.wrist)).try_normalize()?;
    let aim = xr::body_rotation().inverse() * direction;
    let grip = hands::active_hands_raw()?.hands[free.index()]?
        .grip
        .position;
    (aim.is_finite() && grip.is_finite()).then_some(HandLine { grip, aim })
}

/// The line's origin on the grapple hand, from its wrist frame in the world.
pub(crate) fn line_origin(wrist: Pose) -> Vec3 {
    wrist.position + wrist.orientation * GRAPPLE_ORIGIN_M
}

/// Where a hand holds the line, from its wrist frame in the world: the palm, ahead of the wrist
/// toward the fingers.
pub(crate) fn palm(wrist: Pose) -> Vec3 {
    wrist.position + wrist.orientation * PALM_FROM_WRIST_M
}

/// Whether `hand` is free and holds nothing.
pub(crate) fn free_and_empty(hand: Hand) -> bool {
    physical_hands::free_hands()[hand.index()]
        && physical_hands::held_props()[hand.index()].is_none()
}

/// The far end of the held line from `grapple_hand`, world space: the lock's hook point, or the
/// hook's point on the snatch's swinging target.
fn far_end(grapple_hand: Hand) -> Option<Vec3> {
    if lock::holds() {
        Some(lock::status().hook)
    } else {
        snatch::swinging_far_end(grapple_hand)
    }
}

/// The palm's centre in the wrist's frame, metres: along the fingers' -Z.
const PALM_FROM_WRIST_M: Vec3 = Vec3::new(0.0, 0.0, -0.07);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_palm_beside_the_line_ahead_of_the_wrist_takes_it_at_its_nearest_point() {
        let far = Vec3::new(0.0, 0.0, -10.0);
        let spot = spot(Vec3::ZERO, far, Vec3::new(0.05, 0.0, -0.5), &REACH).unwrap();
        assert!(spot.point.distance(Vec3::new(0.0, 0.0, -0.5)) < 1e-6);
        assert!((spot.ahead_m - 0.5).abs() < 1e-6);
        assert!((spot.distance_m - 0.05).abs() < 1e-6);
    }

    #[test]
    fn the_line_is_out_of_reach_at_the_wrist_behind_it_past_an_arm_and_away_from_it() {
        let far = Vec3::new(0.0, 0.0, -10.0);
        for palm in [
            Vec3::new(0.0, 0.0, -0.1),
            Vec3::new(0.0, 0.0, 0.3),
            Vec3::new(0.0, 0.0, -1.2),
            Vec3::new(0.2, 0.0, -0.5),
        ] {
            assert_eq!(spot(Vec3::ZERO, far, palm, &REACH), None, "{palm}");
        }
    }

    #[test]
    fn a_short_line_keeps_the_spot_off_the_far_end() {
        let far = Vec3::new(0.0, 0.0, -0.8);
        assert!(spot(Vec3::ZERO, far, Vec3::new(0.0, 0.0, -0.25), &REACH).is_some());
        assert_eq!(
            spot(Vec3::ZERO, far, Vec3::new(0.0, 0.0, -0.4), &REACH),
            None
        );
    }

    #[test]
    fn degenerate_and_invalid_lines_are_never_taken() {
        let palm = Vec3::new(0.0, 0.0, -0.5);
        assert_eq!(spot(Vec3::ZERO, Vec3::ZERO, palm, &REACH), None);
        assert_eq!(spot(Vec3::NAN, Vec3::NEG_Z * 10.0, palm, &REACH), None);
        assert_eq!(
            spot(Vec3::ZERO, Vec3::NEG_Z * 10.0, Vec3::NAN, &REACH),
            None
        );
        assert_eq!(ahead_m(Vec3::ZERO, Vec3::ZERO, palm), None);
    }

    #[test]
    fn a_pull_back_past_the_grapple_hand_keeps_what_it_took_in() {
        // Taken 0.5 m ahead of a 5 m line, so 4.5 m from the free hand; let go 0.2 m behind the
        // grapple hand: 0.7 m was pulled in.
        assert!((handed_back_m(4.5, 0.5, Some(-0.2), 0.0) - 4.3).abs() < 1e-6);
    }

    #[test]
    fn a_hand_pushed_out_along_the_line_pays_none_out() {
        assert!((handed_back_m(4.5, 0.5, Some(0.9), 0.0) - 5.0).abs() < 1e-6);
        assert!((handed_back_m(4.5, 0.5, None, 0.0) - 5.0).abs() < 1e-6);
        assert!((handed_back_m(4.5, 0.5, Some(f32::NAN), 0.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn the_handed_back_line_is_never_shorter_than_its_floor() {
        assert_eq!(handed_back_m(0.2, 0.3, Some(-0.4), 1.5), 1.5);
        assert_eq!(handed_back_m(0.0, 0.0, Some(-1.0), 0.0), held_floor(0.0));
    }

    #[test]
    fn the_hand_reads_how_far_ahead_it_holds_the_line() {
        let ahead = ahead_m(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -5.0),
            Vec3::new(0.3, 0.1, -0.4),
        );
        assert!((ahead.unwrap() - 0.4).abs() < 1e-6);
    }
}
