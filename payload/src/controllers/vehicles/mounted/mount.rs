//! Matching a hand's held body to the mounted weapon it belongs to.
//!
//! A mounted gun's handles are parts of the object holding the gun, not of the weapon itself, so a
//! hand that takes a handle holds the holder or one of its parts. The runtime resolves the weapon
//! from that holder: a held body is the weapon's own game object, the object holding it, or a part
//! of the holder's vehicle at the gun.

use glam::Vec3;

/// A mounted weapon a hand can take: its own game object, the object holding it, and a point on
/// the gun the hand's body must be near when it holds a part of the holder's vehicle.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Mount {
    pub object: usize,
    pub holder: Option<usize>,
    pub centre: Vec3,
}

/// The body a hand holds, and its centre of mass when the hold reports one.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Held {
    pub object: usize,
    pub centre: Option<Vec3>,
}

/// Whether `held` is `mount`'s gun, its holder, or a part of the holder's vehicle at the gun.
/// `vehicle_of` resolves a game object to the vehicle it is or came from.
pub(crate) fn matches(
    mount: Mount,
    held: Held,
    vehicle_of: impl Fn(usize) -> Option<usize>,
) -> bool {
    if held.object == mount.object {
        return true;
    }
    let Some(holder) = mount.holder else {
        return false;
    };
    let holder_vehicle = vehicle_of(holder);
    // A holder that is a vehicle is taken by a part at the gun, not by the vehicle body held
    // anywhere on it; a holder that is a mount or a part is taken directly.
    if held.object == holder && holder_vehicle != Some(holder) {
        return true;
    }
    let Some(holder_vehicle) = holder_vehicle else {
        return false;
    };
    if vehicle_of(held.object) != Some(holder_vehicle) {
        return false;
    }
    // A part of the holder's vehicle is a handle only while the hand is at the gun. Holding the
    // vehicle itself elsewhere must not take the gun.
    let Some(centre) = held.centre.filter(|centre| centre.is_finite()) else {
        return false;
    };
    mount.centre.is_finite() && centre.distance(mount.centre) <= HANDLE_REACH_M
}

/// How far a held part of the holder's vehicle may be from the gun's centre to count as its
/// handle, metres.
const HANDLE_REACH_M: f32 = 1.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn mount() -> Mount {
        Mount {
            object: 1,
            holder: Some(2),
            centre: Vec3::ZERO,
        }
    }

    /// Vehicle 2 holds the gun; objects 20 and 21 are its parts, object 3 belongs to another
    /// vehicle, and object 30 is loose.
    fn vehicle_of(object: usize) -> Option<usize> {
        match object {
            2 | 20 | 21 => Some(2),
            3 => Some(3),
            _ => None,
        }
    }

    fn held(object: usize, centre: Vec3) -> Held {
        Held {
            object,
            centre: Some(centre),
        }
    }

    #[test]
    fn a_held_gun_matches_without_a_reported_centre() {
        assert!(matches(
            mount(),
            Held {
                object: 1,
                centre: None
            },
            vehicle_of
        ));
    }

    #[test]
    fn a_held_mount_that_is_not_a_vehicle_matches_without_a_reported_centre() {
        let part_mount = Mount {
            object: 1,
            holder: Some(20),
            centre: Vec3::ZERO,
        };
        assert!(matches(
            part_mount,
            Held {
                object: 20,
                centre: None
            },
            vehicle_of
        ));
    }

    #[test]
    fn a_held_vehicle_matches_only_at_the_gun() {
        assert!(matches(mount(), held(2, Vec3::ZERO), vehicle_of));
        assert!(!matches(
            mount(),
            held(2, Vec3::new(2.0, 0.0, 0.0)),
            vehicle_of
        ));
    }

    #[test]
    fn a_part_of_the_holders_vehicle_matches_only_at_the_gun() {
        assert!(matches(
            mount(),
            held(20, Vec3::new(1.0, 0.0, 0.0)),
            vehicle_of
        ));
        assert!(!matches(
            mount(),
            held(20, Vec3::new(2.0, 0.0, 0.0)),
            vehicle_of
        ));
        assert!(!matches(
            mount(),
            held(20, Vec3::new(f32::NAN, 0.0, 0.0)),
            vehicle_of
        ));
        assert!(!matches(
            mount(),
            Held {
                object: 20,
                centre: None
            },
            vehicle_of
        ));
    }

    #[test]
    fn a_part_of_another_vehicle_or_a_loose_body_does_not_match() {
        assert!(!matches(mount(), held(3, Vec3::ZERO), vehicle_of));
        assert!(!matches(mount(), held(30, Vec3::ZERO), vehicle_of));
    }

    #[test]
    fn a_holder_that_is_itself_a_part_resolves_through_its_vehicle() {
        let part_mount = Mount {
            object: 1,
            holder: Some(20),
            centre: Vec3::ZERO,
        };
        assert!(matches(part_mount, held(21, Vec3::ZERO), vehicle_of));
        assert!(!matches(part_mount, held(3, Vec3::ZERO), vehicle_of));
    }

    #[test]
    fn a_mount_without_a_holder_matches_only_its_gun() {
        let loose = Mount {
            object: 1,
            holder: None,
            centre: Vec3::ZERO,
        };
        assert!(matches(loose, held(1, Vec3::ZERO), vehicle_of));
        assert!(!matches(loose, held(2, Vec3::ZERO), vehicle_of));
    }
}
