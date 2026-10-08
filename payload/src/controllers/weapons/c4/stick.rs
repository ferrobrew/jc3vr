//! Sticking a held charge onto a surface by hand.
//!
//! While a charge from the pouch is held, each aim update (the simulation phase outside the physics
//! step, where the game casts its own aim ray) casts short rays from the held charge along the six
//! axes of the hand's grip frame ([`probe`]). A surface within [`C4StickConfig::contact_m`] of the
//! charge's centre is touching it: the charge is pressed onto it. The press is a placement rather
//! than a throw: the charge is made live as a throw makes it, but it leaves from just off the
//! surface ([`C4StickConfig::standoff_m`]) and is pushed onto it along the surface's normal
//! ([`C4StickConfig::push_mps`]), so that the charge's own collision handler sticks it where it
//! touches, oriented by the surface and attached to the object it touched, as a thrown charge
//! sticks where it lands. The player's own character is never a surface.
//!
//! [`C4StickConfig::contact_m`]: crate::controllers::weapons::c4::config::C4StickConfig::contact_m
//! [`C4StickConfig::standoff_m`]: crate::controllers::weapons::c4::config::C4StickConfig::standoff_m
//! [`C4StickConfig::push_mps`]: crate::controllers::weapons::c4::config::C4StickConfig::push_mps

use glam::Vec3;
use jc3gi::character::character::Character;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    config::ControllersConfig,
    frame::Pose,
    weapons::c4::{self, config::C4StickConfig},
};

/// A surface touching the held charge.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Contact {
    /// Where the probe met the surface, world space.
    pub point: Vec3,
    /// The surface's normal there, facing the charge.
    pub normal: Vec3,
    /// How far the point is from the charge's centre, metres.
    pub distance: f32,
    /// Whether the surface belongs to a game object rather than the world's geometry.
    pub on_object: bool,
}

/// Where a pressed charge leaves from, and how it is pushed onto the surface.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StickAt {
    /// The charge's start, just off the surface, world space.
    pub position: Vec3,
    /// The push onto the surface, world axes, metres per second.
    pub push: Vec3,
    pub on_object: bool,
}

/// Probe for a surface touching the held charge, and press the charge onto the nearest one.
///
/// # Safety
///
/// The simulation thread outside the physics step, with `owner` the live local player character.
pub(crate) unsafe fn probe(owner: &Character, config: &ControllersConfig) {
    let tuning = &config.weapons.c4;
    if !(config.weapons.c4_throw && tuning.stick.enabled) {
        return;
    }
    let Some((hand, grip)) = c4::stick_candidate(tuning) else {
        return;
    };
    let centre = probe_origin(grip, c4::hold_offset(hand, tuning));
    let options = CastOptions {
        ignore: [owner as *const Character as usize, 0],
        see_through: false,
    };
    let reach = tuning.stick.contact_m.max(0.0);
    let hits = PROBE_AXES.iter().filter_map(|&axis| {
        let ray = Ray::new(centre, grip.orientation * axis)?;
        // SAFETY: forwarded from the caller; the owner is live.
        let hit = unsafe { raycast::cast(&ray, reach, &options) }.ok()??;
        Some((
            hit.position,
            hit.normal,
            ray.direction,
            hit.object.is_some(),
        ))
    });
    let Some(contact) = nearest_contact(centre, hits, reach) else {
        return;
    };
    c4::press(hand, placement(contact, &tuning.stick));
}

/// The probes' directions in the grip frame: its six axes.
const PROBE_AXES: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];

/// The nearest of `hits` (each a point, a normal, the probe's direction, and whether it is on an
/// object) within `reach` of `centre`. A normal that is not finite, or that faces away from the
/// charge, is taken as the probe's direction reversed.
pub fn nearest_contact(
    centre: Vec3,
    hits: impl IntoIterator<Item = (Vec3, Vec3, Vec3, bool)>,
    reach: f32,
) -> Option<Contact> {
    hits.into_iter()
        .filter(|(point, ..)| point.is_finite())
        .map(|(point, normal, direction, on_object)| {
            let normal = normal
                .try_normalize()
                .filter(|n| n.dot(direction) < 0.0)
                .unwrap_or(-direction);
            Contact {
                point,
                normal,
                distance: point.distance(centre),
                on_object,
            }
        })
        .filter(|c| c.distance <= reach)
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// Where a charge pressed onto `contact` leaves from, and its push onto the surface.
pub fn placement(contact: Contact, tuning: &C4StickConfig) -> StickAt {
    StickAt {
        position: contact.point + contact.normal * tuning.standoff_m.max(0.0),
        push: -contact.normal * tuning.push_mps.max(0.0),
        on_object: contact.on_object,
    }
}

/// Whether a held charge may stick: it has been in the hand `held_s` seconds, and the hand has not
/// opened.
pub fn may_stick(held_s: f32, released: bool, tuning: &C4StickConfig) -> bool {
    !released && held_s >= tuning.after_prime_s
}

/// The held charge's centre, where the probes start: the grip moved by the charge's offset in the
/// grip frame.
pub fn probe_origin(grip: Pose, offset: Vec3) -> Vec3 {
    grip.position + grip.orientation * offset
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_nearest_surface_in_reach_is_the_contact() {
        let centre = Vec3::ZERO;
        let hits = [
            (Vec3::new(0.03, 0.0, 0.0), Vec3::NEG_X, Vec3::X, false),
            (Vec3::new(0.0, -0.02, 0.0), Vec3::Y, Vec3::NEG_Y, true),
            (Vec3::new(0.0, 0.0, 0.1), Vec3::NEG_Z, Vec3::Z, false),
        ];
        let c = nearest_contact(centre, hits, 0.04).unwrap();
        assert_eq!(c.point, Vec3::new(0.0, -0.02, 0.0));
        assert_eq!(c.normal, Vec3::Y);
        assert!(c.on_object);
        assert!(nearest_contact(centre, [hits[2]], 0.04).is_none());
    }

    #[test]
    fn a_normal_facing_away_is_taken_from_the_probe() {
        let hits = [(Vec3::new(0.02, 0.0, 0.0), Vec3::X, Vec3::X, false)];
        let c = nearest_contact(Vec3::ZERO, hits, 0.04).unwrap();
        assert_eq!(c.normal, Vec3::NEG_X);
        let hits = [(Vec3::new(0.02, 0.0, 0.0), Vec3::NAN, Vec3::X, false)];
        assert_eq!(
            nearest_contact(Vec3::ZERO, hits, 0.04).unwrap().normal,
            Vec3::NEG_X
        );
    }

    #[test]
    fn the_charge_leaves_off_the_surface_pushed_onto_it() {
        let tuning = C4StickConfig::new();
        let at = placement(
            Contact {
                point: Vec3::new(1.0, 0.0, 0.0),
                normal: Vec3::Y,
                distance: 0.02,
                on_object: false,
            },
            &tuning,
        );
        assert!((at.position - Vec3::new(1.0, tuning.standoff_m, 0.0)).length() < 1e-6);
        assert!(at.push.dot(Vec3::Y) < 0.0);
        assert!((at.push.length() - tuning.push_mps).abs() < 1e-6);
    }

    #[test]
    fn a_charge_sticks_only_after_it_leaves_the_pouch() {
        let tuning = C4StickConfig::new();
        assert!(!may_stick(0.1, false, &tuning));
        assert!(may_stick(tuning.after_prime_s, false, &tuning));
        assert!(!may_stick(1.0, true, &tuning));
    }

    #[test]
    fn the_probe_starts_at_the_held_charge() {
        let grip = Pose {
            position: Vec3::new(0.0, 1.0, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let at = probe_origin(grip, Vec3::new(0.0, 0.0, -0.1));
        assert!(at.distance(Vec3::new(-0.1, 1.0, 0.0)) < 1e-5);
    }
}
