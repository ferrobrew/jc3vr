//! The grab-volume query: whether a hand is inside a region where a grip press grabs rather than
//! performing the grip's default action (the off-hand's grapple). Pure.
//!
//! The volumes are the body holsters, a two-handed weapon's foregrip, the held gun's grip, the
//! grenade pouch, the held grenade's pin, a live grenade to catch, the reload pouch, a gun's action,
//! the C4 pouch, the vehicle controls, the parachute's handles, and an object or a surface to climb
//! in the physical hands' reach. The
//! subsystem that owns a volume places it each input tick as a [`Candidate`] (the weapons in
//! [`crate::controllers::weapons`], the vehicle controls and the parachute's handles in
//! [`crate::controllers::vehicles`]), and [`grab_volume`] picks the one a
//! hand's grab point is in. The pickup volume is not placed: the physical hands report it for a
//! hand that reaches a loose object or a weapon ([`crate::controllers::physical_hands::pickup`]),
//! and every placed volume wins over it; nor is the climb's, which the physical hands report for
//! a hand that reaches the static world ([`crate::controllers::physical_hands::climb`]), and which
//! the pickup volume wins over; nor is the stunt hold's, which the stunt positions report for
//! either hand while the player hangs from a helicopter, so that a grip there holds on rather
//! than firing the grapple. The tables latch the answer at the grip's press edge.

use glam::Vec3;

use crate::controllers::{
    frame::Hand, vehicles::physical::ControlKind, weapons::inventory::sockets::SocketId,
};

/// A region where a grip press grabs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GrabVolume {
    /// A weapon holster on the body.
    Holster(HolsterSite),
    /// The held two-handed weapon's foregrip.
    Foregrip,
    /// The held gun's grip, for the hand without the gun, which takes the gun from the other hand
    /// (see [`crate::controllers::weapons::hold`]).
    Handle,
    /// The grenade pouch.
    GrenadePouch,
    /// The universal reload pouch on the belt, for the hand without a gun, while the true reload
    /// reloads the held weapon (see [`crate::controllers::weapons::reload`]).
    MagazinePouch,
    /// The held gun's slide, charging handle, or bolt, for the hand without a gun, while its
    /// chamber waits on the action.
    WeaponAction,
    /// The C4 pouch on the chest, while the C4 throw is on.
    C4Pouch,
    /// A physical vehicle control of the driver's seat (see
    /// [`crate::controllers::vehicles::physical`]).
    VehicleControl(ControlKind),
    /// The parachute's handle on that side (see [`crate::controllers::vehicles::parachute`]).
    ParachuteHandle(Hand),
    /// The mounted gun's handle for that hand (see
    /// [`crate::controllers::vehicles::mounted_grip`]).
    MountedHandle(Hand),
    /// An object within the empty hand's reach (see
    /// [`crate::controllers::physical_hands::pickup`]).
    Pickup,
    /// The static world within the empty hand's reach, to climb (see
    /// [`crate::controllers::physical_hands::climb`]).
    Climb,
    /// The helicopter the player hangs from, held by either hand's grip (see
    /// [`crate::controllers::physical_hands::stunt`]).
    StuntHold,
    /// A weapon without a body in the world for the free hand to take: a gun in a character's
    /// hands, or a weapon on a rack (see [`crate::controllers::weapons::hold::take`]).
    WorldWeapon,
    /// The pin of the grenade the other hand holds (see
    /// [`crate::controllers::weapons::grenade::pin`]).
    GrenadePin,
    /// A live grenade in flight or on the ground near a free hand, to catch (see
    /// [`crate::controllers::weapons::grenade::catch`]).
    LiveGrenade,
    /// A magazine lying in the world that fits the held gun, for the hand without a gun while the
    /// gun's magazine is out (see [`crate::controllers::weapons::reload::loose`]).
    WorldMagazine,
    /// A socket on the body for a weapon carried beyond the slots: the carried weapon there, for a
    /// hand without a gun to draw, or the free socket, for the hand holding a weapon that fits it
    /// to put it there (see [`crate::controllers::weapons::inventory`]).
    Socket(SocketId),
}

impl GrabVolume {
    pub const fn label(self) -> &'static str {
        match self {
            GrabVolume::Holster(site) => site.label(),
            GrabVolume::Foregrip => "foregrip",
            GrabVolume::Handle => "gun grip",
            GrabVolume::GrenadePouch => "grenade pouch",
            GrabVolume::MagazinePouch => "reload pouch",
            GrabVolume::WeaponAction => "weapon action",
            GrabVolume::C4Pouch => "C4 pouch",
            GrabVolume::VehicleControl(kind) => kind.label(),
            GrabVolume::ParachuteHandle(Hand::Left) => "left parachute handle",
            GrabVolume::ParachuteHandle(Hand::Right) => "right parachute handle",
            GrabVolume::MountedHandle(Hand::Left) => "mounted gun's left handle",
            GrabVolume::MountedHandle(Hand::Right) => "mounted gun's right handle",
            GrabVolume::Pickup => "object in reach",
            GrabVolume::Climb => "surface to climb",
            GrabVolume::StuntHold => "helicopter to hang from",
            GrabVolume::WorldWeapon => "weapon to take",
            GrabVolume::GrenadePin => "grenade pin",
            GrabVolume::LiveGrenade => "live grenade",
            GrabVolume::WorldMagazine => "magazine in the world",
            GrabVolume::Socket(socket) => socket.label(),
        }
    }
}

/// Where a holster sits on the body. The sides are the body's own, not the dominant hand's.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HolsterSite {
    Hip(Hand),
    /// Over the shoulder, reached from above.
    Shoulder(Hand),
    Chest,
}

impl HolsterSite {
    pub const fn label(self) -> &'static str {
        match self {
            HolsterSite::Hip(Hand::Left) => "left hip holster",
            HolsterSite::Hip(Hand::Right) => "right hip holster",
            HolsterSite::Shoulder(Hand::Left) => "left shoulder holster",
            HolsterSite::Shoulder(Hand::Right) => "right shoulder holster",
            HolsterSite::Chest => "chest holster",
        }
    }
}

/// A placed volume: a sphere in the frame the grab points are given in, and the hands that may
/// grab it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Candidate {
    pub volume: GrabVolume,
    pub centre: Vec3,
    pub radius: f32,
    /// The one hand that may grab the volume, or `None` for either.
    pub hand: Option<Hand>,
}

impl Candidate {
    /// How deep `point` is in the volume: its distance from the centre as a fraction of the radius,
    /// below one inside. `None` outside or for a degenerate volume.
    pub fn depth(&self, point: Vec3) -> Option<f32> {
        if self.radius.is_nan() || self.radius <= 0.0 || !point.is_finite() {
            return None;
        }
        let depth = point.distance(self.centre) / self.radius;
        (depth <= 1.0).then_some(depth)
    }
}

/// The grab volume `hand`'s grab point `point` is in, among `candidates`: of the volumes that
/// contain it and that the hand may grab, the one it is deepest in, so that overlapping volumes
/// resolve to the one the hand is nearest the middle of.
pub fn grab_volume(point: Vec3, hand: Hand, candidates: &[Candidate]) -> Option<GrabVolume> {
    grab_volume_by(hand, candidates, |_| point)
}

/// [`grab_volume`] with each candidate tested against its own point of the hand, `point_for`:
/// the volumes on the body are reached by the player's own hand, before the body-scale remap,
/// while the volumes on a held weapon are reached by the hand the player sees on it.
pub fn grab_volume_by(
    hand: Hand,
    candidates: &[Candidate],
    point_for: impl Fn(&Candidate) -> Vec3,
) -> Option<GrabVolume> {
    candidates
        .iter()
        .filter(|c| c.hand.is_none_or(|h| h == hand))
        .filter_map(|c| c.depth(point_for(c)).map(|depth| (depth, c.volume)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, volume)| volume)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere(volume: GrabVolume, x: f32, radius: f32, hand: Option<Hand>) -> Candidate {
        Candidate {
            volume,
            centre: Vec3::new(x, 0.0, 0.0),
            radius,
            hand,
        }
    }

    #[test]
    fn a_point_outside_every_volume_grabs_nothing() {
        let c = [sphere(GrabVolume::Foregrip, 0.0, 0.1, None)];
        assert_eq!(grab_volume(Vec3::new(0.2, 0.0, 0.0), Hand::Left, &c), None);
        assert_eq!(grab_volume(Vec3::NAN, Hand::Left, &c), None);
    }

    #[test]
    fn overlapping_volumes_resolve_to_the_deepest() {
        let hip = GrabVolume::Holster(HolsterSite::Hip(Hand::Right));
        let c = [
            sphere(hip, 0.0, 0.2, None),
            sphere(GrabVolume::GrenadePouch, 0.15, 0.1, None),
        ];
        assert_eq!(
            grab_volume(Vec3::new(0.14, 0.0, 0.0), Hand::Right, &c),
            Some(GrabVolume::GrenadePouch)
        );
        assert_eq!(
            grab_volume(Vec3::new(0.02, 0.0, 0.0), Hand::Right, &c),
            Some(hip)
        );
    }

    #[test]
    fn a_volume_for_one_hand_ignores_the_other() {
        let c = [sphere(GrabVolume::Foregrip, 0.0, 0.1, Some(Hand::Left))];
        assert_eq!(grab_volume(Vec3::ZERO, Hand::Right, &c), None);
        assert_eq!(
            grab_volume(Vec3::ZERO, Hand::Left, &c),
            Some(GrabVolume::Foregrip)
        );
    }

    #[test]
    fn each_volume_may_be_tested_against_its_own_point() {
        let hip = GrabVolume::Holster(HolsterSite::Hip(Hand::Right));
        let c = [
            sphere(hip, 0.0, 0.1, None),
            sphere(GrabVolume::Foregrip, 1.0, 0.1, None),
        ];
        // The body volumes see one point and the weapon's volumes another.
        let point_for = |body: Vec3, held: Vec3| {
            move |c: &Candidate| {
                if matches!(c.volume, GrabVolume::Holster(_)) {
                    body
                } else {
                    held
                }
            }
        };
        assert_eq!(
            grab_volume_by(Hand::Right, &c, point_for(Vec3::ZERO, Vec3::splat(5.0))),
            Some(hip)
        );
        assert_eq!(
            grab_volume_by(Hand::Right, &c, point_for(Vec3::splat(5.0), Vec3::X)),
            Some(GrabVolume::Foregrip)
        );
        assert_eq!(
            grab_volume_by(Hand::Right, &c, point_for(Vec3::X, Vec3::ZERO)),
            None
        );
    }

    #[test]
    fn a_degenerate_volume_contains_nothing() {
        let c = [sphere(GrabVolume::Foregrip, 0.0, 0.0, None)];
        assert_eq!(grab_volume(Vec3::ZERO, Hand::Left, &c), None);
    }
}
