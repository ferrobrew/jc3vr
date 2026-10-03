//! The grab-volume query: whether a hand is inside a region where a grip press grabs rather than
//! performing the grip's default action (the off-hand's grapple). Pure.
//!
//! The volumes are the body holsters, a two-handed weapon's foregrip, the grenade pouch, the
//! magazine pouch, the vehicle controls, and the parachute's handles. The subsystem
//! that owns a volume places it each input tick as a [`Candidate`] (the weapons in
//! [`crate::controllers::weapons`], the vehicle controls and the parachute's handles in
//! [`crate::controllers::vehicles`]), and
//! [`grab_volume`] picks the one a hand's grab point is in. The tables latch the answer at the
//! grip's press edge.

use glam::Vec3;

use crate::controllers::{frame::Hand, vehicles::physical::ControlKind};

/// A region where a grip press grabs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GrabVolume {
    /// A weapon holster on the body.
    Holster(HolsterSite),
    /// The held two-handed weapon's foregrip.
    Foregrip,
    /// The grenade pouch.
    GrenadePouch,
    /// The off-hand's magazine pouch, while the magazine reload gesture is on.
    MagazinePouch,
    /// A physical vehicle control of the driver's seat (see
    /// [`crate::controllers::vehicles::physical`]).
    VehicleControl(ControlKind),
    /// The parachute's handle on that side (see [`crate::controllers::vehicles::parachute`]).
    ParachuteHandle(Hand),
}

impl GrabVolume {
    pub const fn label(self) -> &'static str {
        match self {
            GrabVolume::Holster(site) => site.label(),
            GrabVolume::Foregrip => "foregrip",
            GrabVolume::GrenadePouch => "grenade pouch",
            GrabVolume::MagazinePouch => "magazine pouch",
            GrabVolume::VehicleControl(kind) => kind.label(),
            GrabVolume::ParachuteHandle(Hand::Left) => "left parachute handle",
            GrabVolume::ParachuteHandle(Hand::Right) => "right parachute handle",
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
    candidates
        .iter()
        .filter(|c| c.hand.is_none_or(|h| h == hand))
        .filter_map(|c| c.depth(point).map(|depth| (depth, c.volume)))
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
    fn a_degenerate_volume_contains_nothing() {
        let c = [sphere(GrabVolume::Foregrip, 0.0, 0.0, None)];
        assert_eq!(grab_volume(Vec3::ZERO, Hand::Left, &c), None);
    }
}
