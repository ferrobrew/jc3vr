//! The vehicle handling's grip sources ([`GripSource`]) beside the physical vehicle controls, whose
//! sources are the body's ([`GripSource::of_control`]). Each is a round bar, and each hold is ended
//! by the vehicle handling, so each holds as [`Firmness::Held`].

use jc3vr_body::controllers::fingers::grip::{Firmness, GripSource, bar, diameters};

pub const PARACHUTE_HANDLE: GripSource = GripSource::new(
    "parachute handle",
    || bar(diameters().parachute_handle_m),
    Firmness::Held,
);
/// A mounted gun's handles.
pub const MOUNTED_GUN: GripSource = GripSource::new(
    "mounted gun",
    || bar(diameters().mounted_gun_m),
    Firmness::Held,
);
