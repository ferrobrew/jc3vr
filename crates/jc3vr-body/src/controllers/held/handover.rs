//! The hand-off of a loose object brought to a hand from out of reach (the grapple's snatch) to the
//! physical hands' hold: the next palms' tick takes hold of it as of a grab in reach, and the grip's
//! release lets go of it.
//!
//! The physical hands register their side at install
//! ([`crate::contributions::BodyContributions::prop_handover`]); calling through it before then is
//! a programming error and panics, and the install refuses to finish without it.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3vr_input::frame::Hand;

use crate::contributions;

/// A loose object arrived at a hand, to be held as a grab in reach takes it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandedProp {
    pub object: NonZeroUsize,
    pub body: u32,
    /// Where the hand takes it, world space.
    pub point: Vec3,
    pub mass_kg: f32,
}

/// Hand `prop` over to `hand`'s hold. The request replaces any the hand has waiting.
pub fn hand_over(hand: Hand, prop: HandedProp) {
    let hand_over = contributions::get()
        .and_then(|contributions| contributions.prop_handover)
        .expect("held: no prop hand-over is registered; the install registers it");
    hand_over(hand, prop);
}
