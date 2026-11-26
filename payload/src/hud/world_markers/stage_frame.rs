//! How the movie stage maps onto the HUD texture: the stage is scaled to fit the texture and
//! centred, so the frame extends past the stage on the texture's relatively longer axis. Pure.

use glam::Vec2;

/// How the movie stage maps onto the HUD texture.
///
/// The movie is shown with the stage scaled to fit the texture and centred, so the visible frame
/// (the texture) extends past the stage on whichever axis the texture is relatively longer: a
/// texture taller than the stage's aspect shows bands above and below the stage, a wider one
/// bands at its sides. `UIManager::Convert3DCoords` maps a world point into this same frame
/// (stage coordinates, with the frame running outside `0..stage` on the longer axis).
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct StageFrame {
    /// The visible frame's top-left corner, in stage coordinates.
    pub origin: Vec2,
    /// The visible frame's size, in stage units.
    pub size: Vec2,
}

impl StageFrame {
    /// The frame for a `stage` size shown in a texture of `aspect` (width / height), or `None` for
    /// a degenerate stage or aspect.
    pub fn new(stage: Vec2, aspect: f32) -> Option<Self> {
        if !(stage.x > 0.0 && stage.y > 0.0 && aspect > 0.0 && aspect.is_finite()) {
            return None;
        }
        let size = if aspect <= stage.x / stage.y {
            Vec2::new(stage.x, stage.x / aspect)
        } else {
            Vec2::new(stage.y * aspect, stage.y)
        };
        Some(Self {
            origin: (stage - size) * 0.5,
            size,
        })
    }

    /// The texture UV of a stage point.
    pub fn to_uv(self, stage: Vec2) -> Vec2 {
        (stage - self.origin) / self.size
    }

    /// The texels per stage unit for a texture `texture_width` texels wide. The stage scales
    /// uniformly, so this holds on both axes.
    pub fn texels_per_unit(&self, texture_width: u32) -> f32 {
        texture_width as f32 / self.size.x
    }

    /// The empty band above and below the stage when the HUD lays out inside the stage only
    /// (`(height of one band, in stage units)`), or zero when the frame does not extend past the
    /// stage vertically.
    pub fn vertical_band(&self) -> f32 {
        (-self.origin.y).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Vec2 = Vec2::new(1920.0, 1080.0);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// A texture taller than the stage's aspect shows bands above and below the stage, matching
    /// `Convert3DCoords`' y scaling by the viewport ratio over the stage ratio.
    #[test]
    fn taller_texture_extends_vertically() {
        let frame = StageFrame::new(STAGE, 1.3).unwrap();
        assert!(close(frame.size.x, 1920.0));
        assert!(close(frame.size.y, 1920.0 / 1.3));
        assert!(close(frame.origin.x, 0.0));
        let band = (1920.0 / 1.3 - 1080.0) * 0.5;
        assert!(close(frame.vertical_band(), band));
        // The stage's own corners land inside the texture, the bands outside them.
        let top = frame.to_uv(Vec2::new(0.0, 0.0));
        assert!(close(top.x, 0.0));
        assert!(close(top.y, band / frame.size.y));
        let centre = frame.to_uv(STAGE * 0.5);
        assert!(close(centre.x, 0.5) && close(centre.y, 0.5));
    }

    /// The inverse of `Convert3DCoords`: a point at panel NDC `(x, y)` lands at the same UV.
    #[test]
    fn uv_inverts_convert_3d_coords() {
        for (ndc_x, ndc_y) in [(0.0, 0.0), (1.0, 1.0), (-0.5, 0.75)] {
            let aspect = 1.3_f32;
            let stage_ratio = STAGE.y / STAGE.x;
            let viewport_ratio = 1.0 / aspect;
            // Convert3DCoords with the viewport relatively taller than the stage.
            let y_scaled = ndc_y * viewport_ratio / stage_ratio;
            let stage = Vec2::new(
                (ndc_x + 1.0) * 0.5 * STAGE.x,
                (1.0 - y_scaled) * 0.5 * STAGE.y,
            );
            let uv = StageFrame::new(STAGE, aspect).unwrap().to_uv(stage);
            assert!(close(uv.x, (ndc_x + 1.0) * 0.5), "{uv}");
            assert!(close(uv.y, (1.0 - ndc_y) * 0.5), "{uv}");
        }
    }

    #[test]
    fn wider_texture_extends_horizontally() {
        let frame = StageFrame::new(STAGE, 2.5).unwrap();
        assert!(close(frame.size.y, 1080.0));
        assert!(close(frame.size.x, 2700.0));
        assert!(close(frame.origin.x, -390.0));
        assert_eq!(frame.vertical_band(), 0.0);
    }

    #[test]
    fn degenerate_frames_are_rejected() {
        assert_eq!(StageFrame::new(Vec2::ZERO, 1.0), None);
        assert_eq!(StageFrame::new(STAGE, 0.0), None);
        assert_eq!(StageFrame::new(STAGE, f32::NAN), None);
    }
}
