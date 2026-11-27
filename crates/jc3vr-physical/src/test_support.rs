//! Fixtures the crate's tests share.

use glam::{Quat, Vec3};
use jc3vr_body::controllers::physics::rigid::{BodyPose, RigidBody, Velocities};

/// A box of `mass` kilograms and `size` metres, at rest at `centre`, turned by `orientation`.
pub(crate) fn crate_body(mass: f32, size: Vec3, centre: Vec3, orientation: Quat) -> RigidBody {
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
