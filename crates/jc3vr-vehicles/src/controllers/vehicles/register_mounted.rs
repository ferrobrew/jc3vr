//! Registers the vehicle handling's side of the mounted-gun fact
//! ([`jc3vr_body::controllers::facts::mounted`]): the mounted gun the hands aim, its aim point, and the
//! seated body's aim.

use glam::Vec3;
use jc3vr_body::controllers::{
    facts::{mounted, mounted::MountedSource},
    mode::VehicleSeat,
};

use crate::controllers::{vehicles, vehicles::passenger};

/// Register the source once; the controllers' install calls it before any hook is enabled.
pub fn register() {
    mounted::register_mounted_source(MountedSource {
        is_mounted_weapon: vehicles::is_mounted_weapon,
        target: vehicles::mounted_target,
        aim_point: vehicles::mounted_aim_point,
        seated_body_aim,
    });
}

/// The seated body's aim point, and the record of whether this update set one.
fn seated_body_aim(
    seat: Option<VehicleSeat>,
    mounted: bool,
    gun_targets: [Option<Vec3>; 2],
) -> Option<Vec3> {
    let point = passenger::body_aim_point(seat, mounted, gun_targets);
    passenger::note(point.is_some());
    point
}
