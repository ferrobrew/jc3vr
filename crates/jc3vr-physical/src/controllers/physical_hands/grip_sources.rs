//! The physical hands' grip sources ([`GripSource`]): what they take hold of, each with the pose the
//! hand takes on it. The grip has to keep holding each of them, so each holds as
//! [`Firmness::Grip`].

use jc3vr_body::controllers::fingers::grip::{
    Firmness, GripSource, bar, catalogue::GripPose, diameters,
};

/// A character taken hold of, or the ragdoll of one.
pub const CHARACTER: GripSource =
    GripSource::new("character", || GripPose::CollarGrab, Firmness::Grip);
/// A vehicle's door: its handle area while latched, or the door itself once open.
pub const DOOR: GripSource = GripSource::new("door", || GripPose::DoorHandle, Firmness::Grip);
/// An object hinged to the world or another body.
pub const ARTICULATED: GripSource = GripSource::new(
    "hinged object",
    || bar(diameters().articulated_m),
    Firmness::Grip,
);
/// A loose object picked up.
pub const PICKUP: GripSource = GripSource::new("loose object", || GripPose::Wrap, Firmness::Grip);
/// A stunt handhold: a helicopter's skid, gripped to hang from it, or held in its hang.
pub const STUNT: GripSource = GripSource::new(
    "stunt handhold",
    || bar(diameters().stunt_m),
    Firmness::Grip,
);
/// The world, or a moving body, gripped to climb.
pub const CLIMB: GripSource = GripSource::new("climb", || GripPose::LedgeHook, Firmness::Grip);
