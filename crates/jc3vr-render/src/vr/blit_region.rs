//! What the eye blit samples: the whole source, or one eye's half of the single-pass double-wide
//! back buffer, as the horizontal mapping the blit's pixel shader (`vr_blit_ps.hlsl`) applies, and
//! the constant-buffer words that carry it.

/// Which part of the back buffer an eye's direct blit samples.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum BlitSource {
    /// The whole back buffer: a dispatch that rendered one eye.
    Whole,
    /// One eye's half of the double-wide back buffer the single-pass collapse renders, left half
    /// eye 0.
    Half(u32),
}

/// The horizontal mapping the blit's pixel shader applies to its fullscreen UV: `u * scale + offset`,
/// clamped to `[min, max]`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) struct SourceRegion {
    scale: f32,
    offset: f32,
    min: f32,
    max: f32,
}

impl SourceRegion {
    /// The whole source: the identity, and a clamp the fullscreen UV never leaves.
    pub(super) const WHOLE: Self = Self {
        scale: 1.0,
        offset: 0.0,
        min: 0.0,
        max: 1.0,
    };

    /// The region `source` names in a source `width` texels wide. A half is clamped to its own
    /// outermost texel centres, so a bilinear tap at the seam never reaches the other eye, as the
    /// clamping sampler keeps a blit from an eye-sized capture inside it.
    pub(super) fn of(source: BlitSource, width: u32) -> Self {
        match source {
            BlitSource::Whole => Self::WHOLE,
            BlitSource::Half(eye) => {
                let width = width.max(2) as f32;
                let eye = eye.min(1) as f32;
                Self {
                    scale: 0.5,
                    offset: 0.5 * eye,
                    min: 0.5 * eye + 0.5 / width,
                    max: 0.5 * (eye + 1.0) - 0.5 / width,
                }
            }
        }
    }
}

/// The blit's constant buffer as words: the gamma mode, the brightness (clamped to `[0, 1]`, `1`
/// when not finite), two words of padding, and the source region.
pub(super) fn params_words(linearize: bool, brightness: f32, region: SourceRegion) -> [u32; 8] {
    let brightness = if brightness.is_finite() {
        brightness.clamp(0.0, 1.0)
    } else {
        1.0
    };
    [
        u32::from(linearize),
        brightness.to_bits(),
        0,
        0,
        region.scale.to_bits(),
        region.offset.to_bits(),
        region.min.to_bits(),
        region.max.to_bits(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_source_is_the_identity() {
        assert_eq!(
            SourceRegion::of(BlitSource::Whole, 7168),
            SourceRegion::WHOLE
        );
    }

    #[test]
    fn each_half_maps_onto_its_texel_centres() {
        let width = 7168;
        for eye in 0..2u32 {
            let region = SourceRegion::of(BlitSource::Half(eye), width);
            // The first and last texel centres of an eye-sized target land on the half's own.
            let eye_width = (width / 2) as f32;
            let first = (0.5 / eye_width) * region.scale + region.offset;
            let last = ((eye_width - 0.5) / eye_width) * region.scale + region.offset;
            let texel = |u: f32| u * width as f32 - 0.5;
            assert!((texel(first) - (eye as f32 * eye_width)).abs() < 1e-2);
            assert!((texel(last) - ((eye as f32 + 1.0) * eye_width - 1.0)).abs() < 1e-2);
            assert!(region.min <= first && last <= region.max);
            assert!((texel(region.min) - eye as f32 * eye_width).abs() < 1e-2);
            assert!((texel(region.max) - ((eye as f32 + 1.0) * eye_width - 1.0)).abs() < 1e-2);
        }
    }
}
