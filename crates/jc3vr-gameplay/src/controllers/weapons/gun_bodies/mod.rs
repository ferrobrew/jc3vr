//! The weapon handling's side of the held guns the hand bodies make bodies of. The store the
//! weapon handling publishes into and the hand bodies' step reports back to is the held-item
//! facts' ([`jc3vr_body::controllers::held::guns`]); the bodies themselves are the hand bodies'
//! own. The design is in `docs/mod/input/physical-guns.md`.
//!
//! - [`holds`]: the guns in the hands, read on the weapon handling's input tick, and the bracing
//!   hand's contact on the foregrip;
//! - [`apply`]: the support and weight targets the hands compose once for every hand, which the
//!   install registers with the store for the hand composition and the hand bodies' drive.

pub mod holds;

use glam::Vec3;
use jc3vr_body::controllers::hands::CockpitHand;
pub(crate) use jc3vr_body::controllers::held::guns::{
    GunHolds, deviation, publish, queue_shot, take_support_break,
};

pub(crate) use self::holds::read_holds;
use crate::controllers::weapons::{
    config::WEAPONS_CONFIG, foregrip, foregrip::FOREGRIP, heft, heft::Heft,
};

/// Compose support and weight once, before either driving a gun hand or drawing its body offset:
/// the held guns' support and weight targets
/// ([`jc3vr_body::controllers::held::guns::register_targets`]), with the weight's live
/// configuration. The physical drive keeps only the hands of current gun holds; ordinary
/// active-hand composition uses this same operation for every hand. The body supplies collisions
/// and recoil afterward.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Vec3) {
    let heft = WEAPONS_CONFIG.query(|w| w.heft);
    foregrip::apply(hands, &FOREGRIP);
    heft::apply(hands, head, &Heft::of(&heft));
}
