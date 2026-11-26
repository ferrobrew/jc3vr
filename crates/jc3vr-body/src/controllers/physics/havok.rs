//! Conversions between glam's types and Havok's vectors and transforms.

use glam::{Affine3A, Mat3A, Vec3, Vec3A};
use jc3gi::{havok::hkTransformf, types::vector_math::AlignedVector3};

/// A Havok transform as an affine transform: its rotation's columns are the images of the local
/// axes.
#[inline]
pub fn affine(t: &hkTransformf) -> Affine3A {
    let [x, y, z] = t.m_rotation.map(|c| Vec3A::from_array(c.data));
    Affine3A {
        matrix3: Mat3A::from_cols(x, y, z),
        translation: Vec3A::from_array(t.m_translation.data),
    }
}

/// A vector as a Havok vector, its padding lane zero.
pub fn vector(v: Vec3) -> AlignedVector3 {
    AlignedVector3 {
        data: v.to_array(),
        _w: 0.0,
    }
}

/// An affine transform as a Havok transform.
pub fn transform(a: Affine3A) -> hkTransformf {
    let column = |v: Vec3A| vector(Vec3::from(v));
    hkTransformf {
        m_rotation: [
            column(a.matrix3.x_axis),
            column(a.matrix3.y_axis),
            column(a.matrix3.z_axis),
        ],
        m_translation: column(a.translation),
    }
}
