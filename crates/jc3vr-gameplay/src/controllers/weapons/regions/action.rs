//! The action's region: where a hand works a gun's slide, charging handle, bolt, or pump.
//!
//! [`classify`](crate::controllers::weapons::regions::classify) puts a hand at the action's handle
//! (the wells table's, in the gun's frame) in [`GunRegion::Action`]. While the gun's chamber waits
//! on its action, the reload ([`crate::controllers::weapons::reload`]) places this region's grab
//! volume on the handle where the part is along its travel, and a grip press there works the
//! action rather than bracing the gun; a hand in the action's region at any other time braces the
//! gun there.
//!
//! [`GunRegion::Action`]: crate::controllers::weapons::regions::GunRegion::Action

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
};

/// The action's grab volume for `hand`: a sphere of `radius` at the action's handle `handle`, in
/// the cockpit frame, carried `back` cockpit metres along the gun's `rearward` axis with the part.
pub fn action_volume(
    handle: Vec3,
    rearward: Vec3,
    back: f32,
    radius: f32,
    hand: Hand,
) -> Candidate {
    Candidate {
        volume: GrabVolume::WeaponAction,
        centre: handle + rearward * back,
        radius,
        hand: Some(hand),
    }
}

/// How far back along its travel the action's part is, cockpit metres: `position` (a share of the
/// travel, `0..=1`) of a travel of `travel_m` gun metres, in a frame of `world_scale` gun metres
/// per cockpit metre.
pub fn action_back(position: f32, travel_m: f32, world_scale: f32) -> f32 {
    position * travel_m / world_scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_volume_rides_the_part_along_its_travel() {
        let handle = Vec3::new(0.0, 0.08, 0.05);
        let rearward = Vec3::Z;
        let home = action_volume(handle, rearward, 0.0, 0.07, Hand::Left);
        assert_eq!(home.centre, handle);
        assert_eq!(home.volume, GrabVolume::WeaponAction);
        assert_eq!(home.hand, Some(Hand::Left));
        let back = action_back(0.5, 0.1, 2.0);
        assert!((back - 0.025).abs() < 1e-6);
        let held_back = action_volume(handle, rearward, back, 0.07, Hand::Left);
        assert!((held_back.centre - Vec3::new(0.0, 0.08, 0.075)).length() < 1e-6);
    }
}
