//! The held guns' pure math: the gun body's mass, its place on the holding hand and how far it
//! strays from it, the gun's barrel and up axes in its own frame, and a shot's impulse.

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    frame::Pose,
    hands::pose as hand_pose,
    physical_hands::physics::config::{GunClassValues, HeldGunsConfig},
    weapons::heft::HeftClass,
};

/// The value of `values` for a gun of weight class `class`.
pub fn class_value(values: &GunClassValues, class: HeftClass) -> f32 {
    match class {
        HeftClass::Sidearm => values.sidearm,
        HeftClass::Smg => values.smg,
        HeftClass::Rifle => values.rifle,
        HeftClass::Heavy => values.heavy,
    }
}

/// Where the gun's mass comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MassSource {
    /// The gun's own body.
    Body,
    /// The class's fallback: the gun's own body gives no finite, positive mass.
    Class,
}

impl MassSource {
    pub const fn label(self) -> &'static str {
        match self {
            MassSource::Body => "its own body",
            MassSource::Class => "its class",
        }
    }
}

/// The mass a gun body is made with, kilograms, from the inverse mass of the gun's own body
/// (`inverse_mass`, zero for an infinite one), or the class's fallback when that gives no finite,
/// positive mass; either is held within the configured bounds.
pub fn gun_mass(inverse_mass: f32, class: HeftClass, config: &HeldGunsConfig) -> (f32, MassSource) {
    let lightest = config.min_mass_kg.max(0.01);
    let heaviest = config.max_mass_kg.max(lightest);
    let from_body = (inverse_mass.is_finite() && inverse_mass > 0.0).then(|| 1.0 / inverse_mass);
    let (mass, source) = match from_body.filter(|m| m.is_finite()) {
        Some(mass) => (mass, MassSource::Body),
        None => (
            class_value(&config.fallback_mass_kg, class),
            MassSource::Class,
        ),
    };
    let mass = if mass.is_finite() { mass } else { lightest };
    (mass.clamp(lightest, heaviest), source)
}

/// The gun body's place on the holding hand's body: the gun's pose `gun` in the frame of the hand
/// body's pose `hand`, both world space.
pub fn seat(hand: Pose, gun: Pose) -> Pose {
    hand_pose::mul(hand_pose::inverse(hand), gun)
}

/// Where the gun body sits on the holding hand's body at `seat`, world space.
pub fn seated(hand: Pose, seat: Pose) -> Pose {
    hand_pose::mul(hand, seat)
}

/// How far the gun body at `gun` strays from its seat on the hand body at `hand`, in the gun's own
/// frame: the transform that carries the seated gun onto the gun body. The identity while the
/// joint holds the gun exactly where it took it.
pub fn deviation(hand: Pose, seat: Pose, gun: Pose) -> Pose {
    hand_pose::mul(hand_pose::inverse(seated(hand, seat)), gun)
}

/// Whether a deviation is past what a held gun may stray before it is put back on its seat:
/// `break_distance_m` of shift, or a turn of [`MAX_TURN_RAD`].
pub fn strayed(deviation: Pose, break_distance_m: f32) -> bool {
    let finite = deviation.position.is_finite() && deviation.orientation.is_finite();
    !finite
        || deviation.position.length() > break_distance_m.max(0.0)
        || turn_rad(deviation.orientation) > MAX_TURN_RAD
}

/// The largest turn a gun body may stray from its seat, radians.
pub const MAX_TURN_RAD: f32 = 0.8;

/// The angle of a rotation, radians.
pub fn turn_rad(q: Quat) -> f32 {
    let q = q.normalize();
    let q = if q.w < 0.0 { -q } else { q };
    q.to_axis_angle().1
}

/// The gun's barrel and muzzle in its own frame: the barrel the axis of its fire point `fire_point`
/// (the fire point's -Z, as the sight line's), and the muzzle the fire point's origin.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunAxes {
    pub muzzle: Vec3,
    /// A unit vector.
    pub barrel: Vec3,
    /// A unit vector across the barrel: the gun's up, as it is held when the body is made.
    pub up: Vec3,
}

/// The gun's axes from its fire point `fire_point` in its own frame, with `up_in_gun` the world's
/// up in the gun's frame when the body is made. `None` for a degenerate fire point.
pub fn gun_axes(fire_point: Mat4, up_in_gun: Vec3) -> Option<GunAxes> {
    let barrel = fire_point.transform_vector3(Vec3::NEG_Z).try_normalize()?;
    let muzzle = fire_point.w_axis.truncate();
    // The world's up across the barrel; a gun made pointing straight up takes any axis across.
    let up = (up_in_gun - barrel * up_in_gun.dot(barrel))
        .try_normalize()
        .unwrap_or_else(|| barrel.any_orthonormal_vector());
    muzzle.is_finite().then_some(GunAxes { muzzle, barrel, up })
}

/// A shot's impulse on the gun body, world space, for a gun at `gun` with its axes `axes`: back
/// along the barrel by `impulse_ns`, and up across it by `muzzle_rise` of that, which, given at the
/// muzzle, also turns the gun's muzzle up about its centre of mass. Returns the impulse and the
/// point it is given at, game space.
pub fn shot_impulse(gun: Pose, axes: &GunAxes, impulse_ns: f32, muzzle_rise: f32) -> (Vec3, Vec3) {
    let back = -(gun.orientation * axes.barrel);
    let up = gun.orientation * axes.up;
    let impulse_ns = if impulse_ns.is_finite() {
        impulse_ns.max(0.0)
    } else {
        0.0
    };
    let rise = if muzzle_rise.is_finite() {
        muzzle_rise.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let at = gun.position + gun.orientation * axes.muzzle;
    ((back + up * rise) * impulse_ns, at)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn a_gun_takes_its_own_bodys_mass_within_bounds() {
        let config = HeldGunsConfig::new();
        assert_eq!(
            gun_mass(0.25, HeftClass::Rifle, &config),
            (4.0, MassSource::Body)
        );
        // An infinite mass, or a broken one, takes the class's.
        assert_eq!(
            gun_mass(0.0, HeftClass::Heavy, &config),
            (config.fallback_mass_kg.heavy, MassSource::Class)
        );
        assert_eq!(
            gun_mass(f32::NAN, HeftClass::Sidearm, &config).1,
            MassSource::Class
        );
        // Held within the bounds.
        assert_eq!(
            gun_mass(1000.0, HeftClass::Rifle, &config).0,
            config.min_mass_kg
        );
        assert_eq!(
            gun_mass(0.001, HeftClass::Rifle, &config).0,
            config.max_mass_kg
        );
        // Crossed bounds hold the lighter.
        let crossed = HeldGunsConfig {
            min_mass_kg: 5.0,
            max_mass_kg: 2.0,
            ..config
        };
        assert_eq!(gun_mass(0.25, HeftClass::Rifle, &crossed).0, 5.0);
    }

    #[test]
    fn a_seated_gun_has_no_deviation_and_follows_its_hand() {
        let hand = pose(Vec3::new(1.0, 1.5, -0.3), Quat::from_rotation_y(0.6));
        let gun = pose(
            Vec3::new(1.05, 1.45, -0.4),
            Quat::from_rotation_x(-0.2) * Quat::from_rotation_y(0.5),
        );
        let seat = seat(hand, gun);
        let d = deviation(hand, seat, gun);
        assert!(d.position.length() < 1e-5);
        assert!(turn_rad(d.orientation) < 1e-3);
        // The hand moves; the gun seated on it moves with it.
        let moved = pose(Vec3::new(-2.0, 0.5, 4.0), Quat::from_rotation_z(1.1));
        let carried = seated(moved, seat);
        let d = deviation(moved, seat, carried);
        assert!(d.position.length() < 1e-5);
        assert!(!strayed(d, 0.4));
    }

    #[test]
    fn a_deviation_is_in_the_guns_own_frame() {
        let hand = pose(
            Vec3::ZERO,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        let seat = Pose::IDENTITY;
        // The gun body sags 5 cm down in the world; in the gun's frame, also down, since the turn
        // is about the vertical.
        let gun = pose(Vec3::new(0.0, -0.05, 0.0), hand.orientation);
        let d = deviation(hand, seat, gun);
        assert!(d.position.abs_diff_eq(Vec3::new(0.0, -0.05, 0.0), 1e-5));
        // Composing the seated pose with the deviation is the gun body.
        let back = hand_pose::mul(seated(hand, seat), d);
        assert!(back.position.abs_diff_eq(gun.position, 1e-5));
        assert!(strayed(pose(Vec3::new(0.5, 0.0, 0.0), Quat::IDENTITY), 0.4));
        assert!(strayed(pose(Vec3::ZERO, Quat::from_rotation_x(1.0)), 0.4));
        assert!(strayed(pose(Vec3::NAN, Quat::IDENTITY), 0.4));
    }

    #[test]
    fn the_axes_come_from_the_fire_point() {
        let fire_point = Mat4::from_translation(Vec3::new(0.0, 0.05, -0.6));
        let axes = gun_axes(fire_point, Vec3::Y).unwrap();
        assert!(axes.barrel.abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!(axes.up.abs_diff_eq(Vec3::Y, 1e-6));
        assert!(axes.muzzle.abs_diff_eq(Vec3::new(0.0, 0.05, -0.6), 1e-6));
        // A tilted up is taken across the barrel.
        let axes = gun_axes(fire_point, Vec3::new(0.0, 1.0, 1.0)).unwrap();
        assert!(axes.up.dot(axes.barrel).abs() < 1e-6);
        assert!(axes.up.abs_diff_eq(Vec3::Y, 1e-6));
        // Pointing straight up still gives an up across it.
        let up = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let axes = gun_axes(up, Vec3::Y).unwrap();
        assert!(axes.up.dot(axes.barrel).abs() < 1e-5);
        assert!(gun_axes(Mat4::ZERO, Vec3::Y).is_none());
    }

    #[test]
    fn a_shot_pushes_back_along_the_barrel_and_lifts_the_muzzle() {
        let axes = GunAxes {
            muzzle: Vec3::new(0.0, 0.0, -0.5),
            barrel: Vec3::NEG_Z,
            up: Vec3::Y,
        };
        let gun = pose(
            Vec3::new(0.0, 1.5, 0.0),
            Quat::from_rotation_y(std::f32::consts::PI),
        );
        let (impulse, at) = shot_impulse(gun, &axes, 3.0, 0.5);
        // Turned half round, the barrel points along +Z, so back is -Z.
        assert!(impulse.abs_diff_eq(Vec3::new(0.0, 1.5, -3.0), 1e-5));
        assert!(at.abs_diff_eq(Vec3::new(0.0, 1.5, 0.5), 1e-5));
        let (none, _) = shot_impulse(gun, &axes, f32::NAN, 0.5);
        assert_eq!(none, Vec3::ZERO);
        let (flat, _) = shot_impulse(gun, &axes, 2.0, -1.0);
        assert!(flat.abs_diff_eq(Vec3::new(0.0, 0.0, -2.0), 1e-5));
    }

    #[test]
    fn class_values_pick_by_class() {
        let values = GunClassValues::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(
            HeftClass::ALL.map(|c| class_value(&values, c)),
            [1.0, 2.0, 3.0, 4.0]
        );
    }
}
