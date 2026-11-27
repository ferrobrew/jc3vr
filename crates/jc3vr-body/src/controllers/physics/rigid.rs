//! A rigid body's response to impulses, pure: its place, its velocities, and its mass properties
//! as the Havok motion holds them, and what an impulse at a point does to it.
//!
//! The motion keeps its inverse inertia as three principal values about the axes of its own frame,
//! so the world-space inverse inertia is that diagonal turned by the motion's orientation. An
//! impulse at a point changes the linear velocity by the impulse over the mass and the angular
//! velocity by the inverse inertia times the impulse's moment about the centre of mass, as the
//! engine's own impulse does ([`crate::controllers::physics::bodies::push`]).

use body_rig::math::geometry::point_velocity;
use glam::{Mat3, Quat, Vec3};

/// A body's place: its centre of mass and the orientation of its motion's frame, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyPose {
    pub centre: Vec3,
    pub orientation: Quat,
}

impl BodyPose {
    /// The world point at `local` in the body's frame, about its centre of mass.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.centre + self.orientation * local
    }

    /// The body-frame position of the world point `at`, about the centre of mass.
    pub fn local(&self, at: Vec3) -> Vec3 {
        self.orientation.inverse() * (at - self.centre)
    }
}

/// A body's velocities, world space: linear in metres per second, angular in radians per second.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Velocities {
    pub linear: Vec3,
    pub angular: Vec3,
}

/// A rigid body as an impulse sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RigidBody {
    pub pose: BodyPose,
    pub velocities: Velocities,
    /// One over the mass, per kilogram; zero for an infinite mass.
    pub inverse_mass: f32,
    /// The inverse inertia about the axes of the motion's frame; zero on an axis that does not
    /// turn.
    pub inverse_inertia: Vec3,
}

impl RigidBody {
    /// The inverse inertia, world space.
    pub fn inverse_inertia_world(&self) -> Mat3 {
        let rotation = Mat3::from_quat(self.pose.orientation);
        rotation * Mat3::from_diagonal(sanitise(self.inverse_inertia)) * rotation.transpose()
    }

    /// The inertia, world space: the inverse inertia's inverse, with an axis that does not turn
    /// left at zero.
    pub fn inertia_world(&self) -> Mat3 {
        let rotation = Mat3::from_quat(self.pose.orientation);
        let inertia = sanitise(self.inverse_inertia)
            .to_array()
            .map(|i| if i > MIN_INVERSE { 1.0 / i } else { 0.0 });
        rotation * Mat3::from_diagonal(Vec3::from_array(inertia)) * rotation.transpose()
    }

    /// The velocity of the body's material point at the world point `at`.
    #[inline]
    pub fn point_velocity(&self, at: Vec3) -> Vec3 {
        point_velocity(
            self.pose.centre,
            self.velocities.linear,
            self.velocities.angular,
            at,
        )
    }

    /// Apply the linear impulse `impulse` at the world point `at`.
    pub fn apply_impulse(&mut self, impulse: Vec3, at: Vec3) {
        if !(impulse.is_finite() && at.is_finite()) {
            return;
        }
        self.velocities.linear += impulse * self.mass_scale();
        let moment = (at - self.pose.centre).cross(impulse);
        self.velocities.angular += self.inverse_inertia_world() * moment;
    }

    /// Apply the angular impulse `impulse` about the centre of mass.
    pub fn apply_angular_impulse(&mut self, impulse: Vec3) {
        if impulse.is_finite() {
            self.velocities.angular += self.inverse_inertia_world() * impulse;
        }
    }

    /// The mass the point at `at` has along the unit `direction`: the impulse along `direction`
    /// that changes the point's velocity along it by one metre per second. Zero for a point that
    /// does not move.
    pub fn mass_along(&self, at: Vec3, direction: Vec3) -> f32 {
        let moment = (at - self.pose.centre).cross(direction);
        let k = self.mass_scale() + moment.dot(self.inverse_inertia_world() * moment);
        if k.is_finite() && k > MIN_INVERSE {
            1.0 / k
        } else {
            0.0
        }
    }

    /// The point at `at`'s mass matrix: the impulse at `at` that changes its velocity by a given
    /// vector is this matrix times the vector. Zero for a point that does not move.
    pub fn point_mass(&self, at: Vec3) -> Mat3 {
        let skew = skew(at - self.pose.centre);
        let k = Mat3::from_diagonal(Vec3::splat(self.mass_scale()))
            - skew * self.inverse_inertia_world() * skew;
        let determinant = k.determinant();
        if determinant.is_finite() && determinant.abs() > MIN_DETERMINANT {
            k.inverse()
        } else {
            Mat3::ZERO
        }
    }

    fn mass_scale(&self) -> f32 {
        if self.inverse_mass.is_finite() {
            self.inverse_mass.max(0.0)
        } else {
            0.0
        }
    }
}

/// The impulses, one per body of `bodies` (each a mass in kilograms and a linear velocity), that
/// damp an assembly's motion within itself over a step of `dt` seconds: each body's velocity eases
/// toward the assembly's mass-weighted velocity at a rate of `per_s`. The impulses add up to
/// nothing, so the assembly as a whole keeps its momentum and whatever holds it is not fought; only
/// the bodies' motion against each other dies down. Empty for a step or a rate that is not a
/// positive number, or for bodies whose masses are not all positive and finite.
pub fn internal_damping(bodies: &[(f32, Vec3)], per_s: f32, dt: f32) -> Vec<Vec3> {
    let valid = bodies
        .iter()
        .all(|(mass, linear)| mass.is_finite() && *mass > 0.0 && linear.is_finite());
    if !(valid && per_s.is_finite() && per_s > 0.0 && dt.is_finite() && dt > 0.0) {
        return Vec::new();
    }
    let total_kg: f32 = bodies.iter().map(|(mass, _)| mass).sum();
    if total_kg <= 0.0 {
        return Vec::new();
    }
    let momentum: Vec3 = bodies.iter().map(|(mass, linear)| *linear * *mass).sum();
    let whole = momentum / total_kg;
    let share = 1.0 - (-per_s * dt).exp();
    bodies
        .iter()
        .map(|(mass, linear)| (whole - *linear) * share * *mass)
        .collect()
}

/// The smallest inverse mass or inertia that counts as moving.
const MIN_INVERSE: f32 = 1e-9;
/// The smallest mass matrix determinant that is inverted.
const MIN_DETERMINANT: f32 = 1e-18;

fn sanitise(v: Vec3) -> Vec3 {
    if v.is_finite() {
        v.max(Vec3::ZERO)
    } else {
        Vec3::ZERO
    }
}

/// The matrix of the cross product `v × _`.
fn skew(v: Vec3) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(0.0, v.z, -v.y),
        Vec3::new(-v.z, 0.0, v.x),
        Vec3::new(v.y, -v.x, 0.0),
    )
}

/// Fixtures for the tests here and in the crates above.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;

    /// A box of `mass` kilograms and `size` metres, at rest at `centre`, turned by `orientation`.
    pub fn crate_body(mass: f32, size: Vec3, centre: Vec3, orientation: Quat) -> RigidBody {
        let [x, y, z] = (size * size).to_array();
        let inertia = Vec3::new(y + z, x + z, x + y) * (mass / 12.0);
        RigidBody {
            pose: BodyPose {
                centre,
                orientation,
            },
            velocities: Velocities::default(),
            inverse_mass: 1.0 / mass,
            inverse_inertia: Vec3::ONE / inertia,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::physics::rigid::test_support::crate_body;

    #[test]
    fn the_skew_matrix_is_the_cross_product() {
        let a = Vec3::new(0.3, -1.2, 2.0);
        let b = Vec3::new(-0.7, 0.4, 1.1);
        assert!((skew(a) * b - a.cross(b)).length() < 1e-6);
    }

    #[test]
    fn an_impulse_through_the_centre_only_moves() {
        let mut body = crate_body(10.0, Vec3::ONE, Vec3::ZERO, Quat::IDENTITY);
        body.apply_impulse(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO);
        assert!((body.velocities.linear - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-6);
        assert!(body.velocities.angular.length() < 1e-6);
    }

    #[test]
    fn an_impulse_off_the_centre_also_turns() {
        let mut body = crate_body(12.0, Vec3::ONE, Vec3::ZERO, Quat::IDENTITY);
        // Inertia of a unit cube of 12 kg: 2 about each axis.
        body.apply_impulse(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.5, 0.0, 0.0));
        assert!((body.velocities.angular - Vec3::new(0.0, 0.25, 0.0)).length() < 1e-5);
    }

    #[test]
    fn the_inertia_turns_with_the_body() {
        let long = crate_body(
            6.0,
            Vec3::new(2.0, 0.1, 0.1),
            Vec3::ZERO,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        // The long axis now lies along Z: the body turns easily about Z and hardly about Y.
        let inverse = long.inverse_inertia_world();
        assert!((inverse * Vec3::Z).length() > 10.0 * (inverse * Vec3::Y).length());
        let product = long.inertia_world() * inverse;
        assert!((product - Mat3::IDENTITY).abs_diff_eq(Mat3::ZERO, 1e-3));
    }

    #[test]
    fn the_point_mass_gives_the_wanted_point_velocity() {
        let body = crate_body(
            8.0,
            Vec3::new(1.0, 0.5, 0.3),
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_x(0.4),
        );
        let at = body.pose.point(Vec3::new(0.5, 0.2, -0.1));
        let wanted = Vec3::new(0.3, -0.2, 1.0);
        let mut moved = body;
        moved.apply_impulse(body.point_mass(at) * wanted, at);
        assert!((moved.point_velocity(at) - wanted).length() < 1e-4);
        // Along one direction, the mass gives the change along it.
        let direction = Vec3::new(1.0, 1.0, 0.0).normalize();
        let mut pushed = body;
        pushed.apply_impulse(direction * body.mass_along(at, direction), at);
        assert!((pushed.point_velocity(at).dot(direction) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn an_immovable_body_has_no_point_mass() {
        let body = RigidBody {
            inverse_mass: 0.0,
            inverse_inertia: Vec3::ZERO,
            ..crate_body(1.0, Vec3::ONE, Vec3::ZERO, Quat::IDENTITY)
        };
        assert_eq!(body.point_mass(Vec3::X), Mat3::ZERO);
        assert_eq!(body.mass_along(Vec3::X, Vec3::Y), 0.0);
        let mut moved = body;
        moved.apply_impulse(Vec3::new(100.0, 0.0, 0.0), Vec3::Y);
        assert_eq!(moved.velocities, Velocities::default());
    }

    #[test]
    fn internal_damping_keeps_the_momentum_and_eases_each_body_toward_the_whole() {
        let bodies = [
            (25.0, Vec3::new(3.0, 0.0, -1.0)),
            (60.0, Vec3::new(-0.5, 0.2, 0.0)),
            (5.0, Vec3::new(0.0, 4.0, 0.0)),
        ];
        let impulses = internal_damping(&bodies, 4.0, 1.0 / 60.0);
        assert_eq!(impulses.len(), bodies.len());
        let total: Vec3 = impulses.iter().sum();
        assert!(total.length() < 1e-4, "no net push: {total}");
        let momentum: Vec3 = bodies.iter().map(|(m, v)| *v * *m).sum();
        let whole = momentum / bodies.iter().map(|(m, _)| m).sum::<f32>();
        for ((mass, linear), impulse) in bodies.iter().zip(&impulses) {
            let after = *linear + *impulse / *mass;
            assert!(after.distance(whole) < linear.distance(whole));
        }
    }

    #[test]
    fn internal_damping_leaves_a_lone_body_and_bad_input_alone() {
        let lone = internal_damping(&[(25.0, Vec3::new(2.0, 0.0, 0.0))], 4.0, 1.0 / 60.0);
        assert_eq!(lone.len(), 1);
        assert!(lone[0].length() < 1e-6);
        assert!(internal_damping(&[], 4.0, 1.0 / 60.0).is_empty());
        let pair = [(10.0, Vec3::X), (10.0, Vec3::ZERO)];
        assert!(internal_damping(&pair, 0.0, 1.0 / 60.0).is_empty());
        assert!(internal_damping(&pair, 4.0, f32::NAN).is_empty());
        assert!(internal_damping(&[(0.0, Vec3::X), (10.0, Vec3::ZERO)], 4.0, 0.1).is_empty());
    }
}
