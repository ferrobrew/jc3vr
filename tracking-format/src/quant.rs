//! The quantised encodings of the dense per-tick samples a version 8 recording adds: rotations as
//! their smallest three components, and positions and velocities as fixed-point `i16`s.
//!
//! The model holds `f32`s; the codec quantises on the way out and expands on the way in, so a
//! decoded value is the nearest point of the encoding's grid, and encoding it again gives the same
//! bytes.

use glam::{Quat, Vec3};

use crate::wire::{DecodeError, DecodeErrorKind, Decoder, Encoder};

/// A unit quaternion in six bytes: the index of its largest component in the top two bits of a
/// 48-bit word, and the other three, sign-normalised so the largest is positive, as 15-bit fixed
/// point over `[-1/√2, 1/√2]`, largest-index order skipped. Its error is under 0.01 degrees.
pub(crate) fn rotation(out: &mut Encoder, value: Quat) {
    let q = if value.is_finite() && value.length_squared() > 0.0 {
        value.normalize()
    } else {
        Quat::IDENTITY
    };
    let components = q.to_array();
    let largest = (0..4)
        .max_by(|&a, &b| components[a].abs().total_cmp(&components[b].abs()))
        .unwrap_or(3);
    let sign = if components[largest] < 0.0 { -1.0 } else { 1.0 };
    let mut word = (largest as u64) << 45;
    let mut shift = 30;
    for (index, component) in components.into_iter().enumerate() {
        if index == largest {
            continue;
        }
        let unit = (component * sign / SMALLEST_RANGE).clamp(-1.0, 1.0);
        let code = ((unit + 1.0) * 0.5 * SMALLEST_MAX).round() as u64;
        word |= code << shift;
        shift -= 15;
    }
    out.bytes.extend_from_slice(&word.to_le_bytes()[..6]);
}

pub(crate) fn read_rotation(input: &mut Decoder) -> Result<Quat, DecodeError> {
    let offset = input.offset();
    let bytes = input.take(6)?;
    let mut word = [0u8; 8];
    word[..6].copy_from_slice(bytes);
    let word = u64::from_le_bytes(word);
    if word >> 47 != 0 {
        return Err(DecodeError {
            offset,
            kind: DecodeErrorKind::InvalidValue {
                field: "a quantised rotation",
                value: word,
            },
        });
    }
    let largest = ((word >> 45) & 0b11) as usize;
    let mut components = [0.0f32; 4];
    let mut shift = 30;
    let mut sum = 0.0;
    for (index, component) in components.iter_mut().enumerate() {
        if index == largest {
            continue;
        }
        let code = ((word >> shift) & 0x7fff) as f32;
        *component = (code / SMALLEST_MAX * 2.0 - 1.0) * SMALLEST_RANGE;
        sum += *component * *component;
        shift -= 15;
    }
    components[largest] = (1.0 - sum).max(0.0).sqrt();
    Ok(Quat::from_array(components).normalize())
}

/// A vector as three `i16`s of `step` units each, saturating at the grid's ends.
pub(crate) fn fixed3(out: &mut Encoder, value: Vec3, step: f32) {
    for component in value.to_array() {
        out.u16(fixed(component, step) as u16);
    }
}

pub(crate) fn read_fixed3(input: &mut Decoder, step: f32) -> Result<Vec3, DecodeError> {
    let mut read = || -> Result<f32, DecodeError> { Ok(f32::from(input.u16()? as i16) * step) };
    Ok(Vec3::new(read()?, read()?, read()?))
}

/// Whether every component of `value` lies on the `i16` grid of `step` without saturating.
pub(crate) fn fits(value: Vec3, step: f32) -> bool {
    value.is_finite()
        && value
            .to_array()
            .into_iter()
            .all(|c| (c / step).abs() <= f32::from(i16::MAX))
}

/// Model-space bone positions: a tenth of a millimetre, within 3.27 metres of the model's origin.
pub(crate) const BONE_POSITION_STEP: f32 = 1.0e-4;
/// A body's position relative to the sample's origin: a millimetre, within 32.7 metres.
pub(crate) const BODY_POSITION_STEP: f32 = 1.0e-3;
/// A body's linear velocity: a centimetre per second, up to 327 metres per second.
pub(crate) const LINEAR_VELOCITY_STEP: f32 = 1.0e-2;
/// A body's angular velocity: a hundredth of a radian per second, up to 327 radians per second.
pub(crate) const ANGULAR_VELOCITY_STEP: f32 = 1.0e-2;

/// The largest code of a smallest-three component, and the range its codes cover.
const SMALLEST_MAX: f32 = 32767.0;
const SMALLEST_RANGE: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn fixed(value: f32, step: f32) -> i16 {
    if !value.is_finite() {
        return 0;
    }
    (value / step)
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

/// The angle between two rotations, degrees, robust near zero where `acos` of a dot product loses
/// the f32's precision.
#[cfg(test)]
pub(crate) fn rotation_error_deg(a: Quat, b: Quat) -> f32 {
    let near = (a - b).length().min((a + b).length());
    (4.0 * (near * 0.5).asin()).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_rotation(q: Quat) -> (Quat, Vec<u8>) {
        let mut out = Encoder::default();
        rotation(&mut out, q);
        let bytes = out.bytes.clone();
        let decoded = read_rotation(&mut Decoder::new(&bytes)).unwrap();
        (decoded, bytes)
    }

    #[test]
    fn rotations_survive_to_a_hundredth_of_a_degree_and_re_encode_to_the_same_bytes() {
        let samples = [
            Quat::IDENTITY,
            Quat::from_rotation_y(2.9),
            Quat::from_euler(glam::EulerRot::YXZ, -1.2, 0.7, 0.3),
            Quat::from_xyzw(0.5, -0.5, 0.5, -0.5),
            -Quat::from_rotation_x(0.4),
        ];
        for q in samples {
            let (decoded, bytes) = round_trip_rotation(q);
            assert!(
                rotation_error_deg(decoded, q) < 0.01,
                "{q:?} came back as {decoded:?}"
            );
            assert_eq!(round_trip_rotation(decoded).1, bytes);
        }
    }

    #[test]
    fn a_degenerate_rotation_encodes_as_the_identity() {
        let (decoded, _) = round_trip_rotation(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0));
        assert!(decoded.angle_between(Quat::IDENTITY) < 1.0e-4);
    }

    #[test]
    fn a_rotation_word_with_its_spare_bit_set_is_refused() {
        let bytes = [0, 0, 0, 0, 0, 0x80];
        assert!(read_rotation(&mut Decoder::new(&bytes)).is_err());
    }

    #[test]
    fn fixed_point_saturates_and_reports_whether_it_fits() {
        let mut out = Encoder::default();
        fixed3(&mut out, Vec3::new(1.0, -40.0, 0.0004), BODY_POSITION_STEP);
        let decoded = read_fixed3(&mut Decoder::new(&out.bytes), BODY_POSITION_STEP).unwrap();
        assert_eq!(decoded, Vec3::new(1.0, -32.768, 0.0));
        assert!(fits(Vec3::new(1.0, 2.0, -3.0), BODY_POSITION_STEP));
        assert!(!fits(Vec3::new(1.0, -40.0, 0.0), BODY_POSITION_STEP));
    }
}
