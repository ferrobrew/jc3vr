//! The second weapon's magazine, shown as pips over its hand.
//!
//! The game's HUD reads out only the selected weapon's magazine, and the second weapon is never
//! selected ([`crate::controllers::weapons::hold::second`]): a slot's long gun or any weapon the VR
//! inventory carries, which is in no slot at all. So while it is held, a row of pips
//! floats above the hand holding it on the overlay, anchored to that hand's grip so that it rides
//! the controller at frame rate: a filled pip for each share of the magazine left, a ring for each
//! share spent, and the whole row red when the magazine is empty or out.

use std::time::Duration;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    overlay::{
        primitive::{
            Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
        },
        store,
    },
};

/// One pip of the row.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Pip {
    /// A share of the magazine left.
    Full,
    /// A share spent.
    Spent,
}

/// The row of pips for a magazine of `capacity` holding `magazine` rounds: one pip a round up to
/// [`MAX_PIPS`], and past that each pip a share of the magazine, a share counted as left while any
/// of it is. An empty row for a weapon without a magazine size.
pub fn row(magazine: i32, capacity: i32) -> Vec<Pip> {
    if capacity <= 0 {
        return Vec::new();
    }
    let pips = capacity.min(MAX_PIPS as i32);
    let left = magazine.clamp(0, capacity);
    // Rounded up, so that the last round still shows a pip.
    let full = (left * pips + capacity - 1) / capacity;
    (0..pips)
        .map(|i| if i < full { Pip::Full } else { Pip::Spent })
        .collect()
}

/// Show `hand`'s second weapon's magazine, or take the pips away with `None`.
pub(crate) fn submit(shown: Option<(Hand, i32, i32, bool)>) {
    let Some((hand, magazine, capacity, out)) = shown else {
        store::clear(Source::MagazinePips);
        return;
    };
    let pips = if out {
        row(0, capacity)
    } else {
        row(magazine, capacity)
    };
    let empty = out || magazine <= 0;
    let count = pips.len();
    let mut prims: Vec<Primitive> = pips
        .into_iter()
        .enumerate()
        .map(|(i, pip)| {
            // Centred over the fist, along the grip's forward (-Z) axis.
            let along = (i as f32 - (count as f32 - 1.0) * 0.5) * PIP_SPACING_M;
            Primitive::Sprite {
                at: Anchor::Hand {
                    hand,
                    pose: HandPose::Grip,
                    offset: Vec3::new(0.0, ABOVE_FIST_M, -along),
                },
                size: SpriteSize::World(PIP_SIZE_M),
                facing: Facing::Billboard,
                shape: match pip {
                    Pip::Full => Shape::Disc,
                    Pip::Spent => Shape::Ring { thickness: 0.35 },
                },
                color: match (empty, pip) {
                    (true, _) => EMPTY,
                    (false, Pip::Full) => FULL,
                    (false, Pip::Spent) => SPENT,
                },
                depth: DepthMode::Always,
            }
        })
        .collect();
    store::submit(Source::MagazinePips, &mut prims, TTL);
}

/// The most pips a row shows.
pub const MAX_PIPS: usize = 12;

const PIP_SIZE_M: f32 = 0.011;
const PIP_SPACING_M: f32 = 0.015;
/// How far above the fist's centre the row floats, along the grip's +Y.
const ABOVE_FIST_M: f32 = 0.085;
const TTL: Duration = Duration::from_millis(200);
const FULL: Rgba = [0.95, 0.95, 0.85, 0.9];
const SPENT: Rgba = [0.95, 0.95, 0.85, 0.35];
const EMPTY: Rgba = [1.0, 0.3, 0.25, 0.9];

#[cfg(test)]
mod tests {
    use super::*;

    fn full(row: &[Pip]) -> usize {
        row.iter().filter(|p| **p == Pip::Full).count()
    }

    #[test]
    fn a_small_magazine_has_a_pip_a_round() {
        let r = row(3, 6);
        assert_eq!(r.len(), 6);
        assert_eq!(full(&r), 3);
        assert_eq!(r[..3], [Pip::Full; 3]);
    }

    #[test]
    fn a_large_magazine_shows_shares_and_keeps_its_last_round() {
        let r = row(1, 100);
        assert_eq!(r.len(), MAX_PIPS);
        assert_eq!(full(&r), 1, "the last round still shows");
        assert_eq!(full(&row(100, 100)), MAX_PIPS);
        assert_eq!(full(&row(50, 100)), MAX_PIPS / 2);
        assert_eq!(full(&row(0, 100)), 0);
    }

    #[test]
    fn bad_counts_stay_in_range() {
        assert!(row(5, 0).is_empty());
        assert_eq!(full(&row(-3, 8)), 0);
        assert_eq!(full(&row(20, 8)), 8);
    }
}
