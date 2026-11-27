//! The weapon handling's grip sources ([`GripSource`]): what it puts in a hand, each with the pose
//! the hand takes on it. Everything here stays in the hand after the grip lets go, so each holds as
//! [`Firmness::Held`].

use jc3vr_body::controllers::fingers::grip::{Firmness, GripSource, catalogue::GripPose};

/// A magazine or a round taken for the reload.
pub const MAGAZINE: GripSource =
    GripSource::new("magazine", || GripPose::MagazinePinch, Firmness::Held);
pub const GRENADE: GripSource =
    GripSource::new("grenade", || GripPose::GrenadePalm, Firmness::Held);
pub const C4: GripSource = GripSource::new("C4", || GripPose::ChargePalm, Firmness::Held);
/// A gun in the hand, by its grip.
pub const GUN: GripSource = GripSource::new("gun", || GripPose::PistolGrip, Firmness::Held);
/// The off-hand on the held gun's foregrip.
pub const FOREGRIP: GripSource = GripSource::new("foregrip", || GripPose::Foregrip, Firmness::Held);
