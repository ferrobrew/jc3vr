//! The world markers' pure geometry: where a stage point lands in the HUD texture, which texels a
//! marker's crop covers, how large its billboard is to keep its angular size, whether a world point
//! is inside the headset's field of view, and how the placement distance is resolved and smoothed.

use glam::{Quat, Vec2, Vec3, Vec4};

use crate::hud::config::CropExtents;

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

/// A rectangle of texels, half-open: `[x0, x1) x [y0, y1)`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TexelRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl TexelRect {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
}

/// A slot's size in texels for crops of `extents` (stage units) at `texels_per_unit`.
pub(crate) fn slot_size(extents: &CropExtents, texels_per_unit: f32) -> (u32, u32) {
    let w = ((extents.left + extents.right).max(0.0) * texels_per_unit).round() as u32;
    let h = ((extents.up + extents.down).max(0.0) * texels_per_unit).round() as u32;
    (w.max(1), h.max(1))
}

/// Where one marker's crop comes from and goes to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Crop {
    /// The texels copied out of the HUD texture (the crop clamped to the texture).
    pub source: TexelRect,
    /// Where `source` lands inside the slot: non-zero when the crop was clamped on its left or top.
    pub slot_offset: (u32, u32),
    /// The marker's world point inside the slot, in texels from the slot's top-left. The crop is
    /// centred on the clip's origin, which is the projected point plus the POI's 2D offset, so the
    /// world point sits that offset away from the origin.
    pub anchor: Vec2,
}

/// The crop for a clip whose origin is at `origin` (stage coordinates), with the POI's 2D offset
/// `offset` (stage units), into a slot of `slot` texels, from a texture of `texture` texels. The
/// crop's top-left is `extents.left` and `extents.up` from the origin. `None` when the crop misses
/// the texture entirely.
pub(crate) fn crop(
    frame: &StageFrame,
    texture: (u32, u32),
    slot: (u32, u32),
    origin: Vec2,
    offset: Vec2,
    extents: &CropExtents,
) -> Option<Crop> {
    let scale = frame.texels_per_unit(texture.0);
    let origin_texel = frame.to_uv(origin) * Vec2::new(texture.0 as f32, texture.1 as f32);
    let left = (extents.left * scale).round();
    let up = (extents.up * scale).round();
    // The slot's top-left in texture texels, before clamping.
    let x0 = (origin_texel.x - left).round() as i64;
    let y0 = (origin_texel.y - up).round() as i64;
    let x1 = x0 + i64::from(slot.0);
    let y1 = y0 + i64::from(slot.1);
    let clamp_x = |v: i64| v.clamp(0, i64::from(texture.0)) as u32;
    let clamp_y = |v: i64| v.clamp(0, i64::from(texture.1)) as u32;
    let source = TexelRect {
        x0: clamp_x(x0),
        y0: clamp_y(y0),
        x1: clamp_x(x1),
        y1: clamp_y(y1),
    };
    if source.width() == 0 || source.height() == 0 {
        return None;
    }
    Some(Crop {
        source,
        slot_offset: (
            (i64::from(source.x0) - x0) as u32,
            (i64::from(source.y0) - y0) as u32,
        ),
        anchor: Vec2::new(left, up) - offset * scale,
    })
}

/// The angle one HUD-texture texel subtends on the floating panel, in radians: the panel's
/// angular height over the texture's height. The panel is flat, so this is exact at its centre and
/// a close approximation across it.
pub(crate) fn radians_per_texel(panel_height_over_distance: f32, texture_height: u32) -> f32 {
    2.0 * (panel_height_over_distance * 0.5).atan() / texture_height.max(1) as f32
}

/// A billboard's placement: its world point, the head it faces, and its distance.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Billboard {
    /// The head (cyclopean eye) position.
    pub head: Vec3,
    /// The head orientation; the billboard's up follows the head's up, so it rolls with the view
    /// like the panel.
    pub head_rotation: Quat,
    /// The direction from the head to the marker's world point (unit length).
    pub direction: Vec3,
    /// The distance along `direction` the billboard is drawn at, in metres.
    pub distance: f32,
}

/// The billboard's world corners (top-left, top-right, bottom-left, bottom-right, as the quad
/// pass expects) for a slot of `slot` texels whose world point sits at `anchor` texels from its
/// top-left, keeping `radians_per_texel` of angular size per texel at any distance. `None` for a
/// degenerate direction or distance.
pub(crate) fn billboard_corners(
    billboard: &Billboard,
    slot: (u32, u32),
    anchor: Vec2,
    radians_per_texel: f32,
) -> Option<[Vec4; 4]> {
    if !(billboard.distance > 0.0 && billboard.distance.is_finite()) {
        return None;
    }
    let forward = billboard.direction.try_normalize()?;
    let back = -forward;
    let head_up = billboard.head_rotation * Vec3::Y;
    let right = head_up.cross(back).try_normalize().or_else(|| {
        // Looking straight along the head's up: fall back to the head's right.
        (billboard.head_rotation * Vec3::X).try_normalize()
    })?;
    let up = back.cross(right);
    let metres_per_texel = billboard.distance * radians_per_texel.tan();
    let point = billboard.head + forward * billboard.distance;
    let corner = |tx: f32, ty: f32| {
        (point + right * ((tx - anchor.x) * metres_per_texel)
            - up * ((ty - anchor.y) * metres_per_texel))
            .extend(1.0)
    };
    let (w, h) = (slot.0 as f32, slot.1 as f32);
    Some([
        corner(0.0, 0.0),
        corner(w, 0.0),
        corner(0.0, h),
        corner(w, h),
    ])
}

/// A field of view as the tangents of its four half-angles: `left` and `down` are negative.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FovTangents {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

impl FovTangents {
    /// A field of view from its angles in radians (`left` and `down` negative), shrunk on every
    /// side by `inset` radians. A side shrunk past the centre closes at the centre.
    pub fn from_angles(left: f32, right: f32, up: f32, down: f32, inset: f32) -> Self {
        let shrink_pos = |a: f32| (a - inset).max(0.0).tan();
        let shrink_neg = |a: f32| (a + inset).min(0.0).tan();
        Self {
            left: shrink_neg(left),
            right: shrink_pos(right),
            up: shrink_pos(up),
            down: shrink_neg(down),
        }
    }

    /// A symmetric field of view of `half` radians on every side, shrunk by `inset`.
    pub fn symmetric(half: f32, inset: f32) -> Self {
        Self::from_angles(-half, half, half, -half, inset)
    }

    /// The union of two fields of view (both eyes' together).
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            right: self.right.max(other.right),
            up: self.up.max(other.up),
            down: self.down.min(other.down),
        }
    }

    /// Whether a world point is inside this field of view from a camera at `position` with basis
    /// `right`, `up`, and `back` (the engine's camera transform rows).
    pub fn contains(&self, position: Vec3, right: Vec3, up: Vec3, back: Vec3, point: Vec3) -> bool {
        let offset = point - position;
        let forward = -offset.dot(back);
        if forward <= f32::EPSILON {
            return false;
        }
        let x = offset.dot(right) / forward;
        let y = offset.dot(up) / forward;
        (self.left..=self.right).contains(&x) && (self.down..=self.up).contains(&y)
    }
}

/// Clamp a stage point into a rectangle, for a marker reclassified on screen whose panel
/// projection fell outside the safe area.
pub(crate) fn clamp_into(point: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    if min.x > max.x || min.y > max.y {
        return (min + max) * 0.5;
    }
    point.clamp(min, max)
}

/// What a depth or ray probe says about the line of sight to a marker.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Sight {
    /// The nearest surface along the line of sight is this far from the eye, in metres.
    Surface(f32),
    /// Nothing blocks the line of sight before the point.
    Clear,
}

/// The distance to draw a marker at: its true distance when the line of sight is clear (or the
/// surface is not meaningfully nearer), otherwise the surface's distance pulled `pull` toward the
/// eye, never nearer than `min`.
pub(crate) fn placement_distance(true_distance: f32, sight: Sight, pull: f32, min: f32) -> f32 {
    match sight {
        Sight::Surface(surface) if surface + pull < true_distance => {
            (surface - pull).max(min).min(true_distance)
        }
        _ => true_distance,
    }
    .max(min)
}

/// Ease `current` toward `target` with a critically damped exponential of `halflife` seconds
/// over `dt`, in log space so near and far distances ease at the same relative rate. `None`
/// snaps to the target.
pub(crate) fn smooth_distance(current: Option<f32>, target: f32, dt: f32, halflife: f32) -> f32 {
    let Some(current) = current.filter(|c| *c > 0.0 && c.is_finite()) else {
        return target;
    };
    if !(target > 0.0 && target.is_finite()) {
        return current;
    }
    let alpha = if halflife <= 0.0 {
        1.0
    } else {
        (1.0 - 2.0_f32.powf(-dt.max(0.0) / halflife)).clamp(0.0, 1.0)
    };
    (current.ln() + (target.ln() - current.ln()) * alpha).exp()
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

    fn extents() -> CropExtents {
        CropExtents {
            left: 40.0,
            right: 60.0,
            up: 20.0,
            down: 30.0,
        }
    }

    /// At one texel per stage unit, an interior crop is the extents around the origin, and the
    /// world point is the origin minus the offset.
    #[test]
    fn interior_crop() {
        let frame = StageFrame::new(Vec2::new(1000.0, 1000.0), 1.0).unwrap();
        let ext = extents();
        let slot = slot_size(&ext, 1.0);
        assert_eq!(slot, (100, 50));
        let crop = crop(
            &frame,
            (1000, 1000),
            slot,
            Vec2::new(500.0, 400.0),
            Vec2::new(5.0, -3.0),
            &ext,
        )
        .unwrap();
        assert_eq!(
            crop.source,
            TexelRect {
                x0: 460,
                y0: 380,
                x1: 560,
                y1: 430
            }
        );
        assert_eq!(crop.slot_offset, (0, 0));
        assert_eq!(crop.anchor, Vec2::new(35.0, 23.0));
    }

    /// A crop over the texture's top-left edge is clamped, and the copy lands offset in the slot
    /// so the marker keeps its place.
    #[test]
    fn edge_crop_is_clamped() {
        let frame = StageFrame::new(Vec2::new(1000.0, 1000.0), 1.0).unwrap();
        let ext = extents();
        let slot = slot_size(&ext, 1.0);
        let crop = crop(
            &frame,
            (1000, 1000),
            slot,
            Vec2::new(10.0, 5.0),
            Vec2::ZERO,
            &ext,
        )
        .unwrap();
        assert_eq!(
            crop.source,
            TexelRect {
                x0: 0,
                y0: 0,
                x1: 70,
                y1: 35
            }
        );
        assert_eq!(crop.slot_offset, (30, 15));
    }

    #[test]
    fn crop_off_the_texture_is_none() {
        let frame = StageFrame::new(Vec2::new(1000.0, 1000.0), 1.0).unwrap();
        let ext = extents();
        let slot = slot_size(&ext, 1.0);
        assert_eq!(
            crop(
                &frame,
                (1000, 1000),
                slot,
                Vec2::new(-500.0, 5.0),
                Vec2::ZERO,
                &ext
            ),
            None
        );
    }

    /// The crop scales with the texture's texel density.
    #[test]
    fn crop_scales_with_texture_density() {
        let frame = StageFrame::new(Vec2::new(1000.0, 1000.0), 1.0).unwrap();
        let ext = extents();
        let scale = frame.texels_per_unit(2000);
        assert_eq!(scale, 2.0);
        let slot = slot_size(&ext, scale);
        assert_eq!(slot, (200, 100));
        let crop = crop(
            &frame,
            (2000, 2000),
            slot,
            Vec2::new(500.0, 500.0),
            Vec2::ZERO,
            &ext,
        )
        .unwrap();
        assert_eq!(crop.source.x0, 920);
        assert_eq!(crop.source.width(), 200);
    }

    /// The billboard subtends the same angle at any distance: its width over its distance is
    /// constant.
    #[test]
    fn billboard_keeps_its_angular_size() {
        for distance in [2.0, 50.0, 300.0] {
            let billboard = Billboard {
                head: Vec3::new(1.0, 2.0, 3.0),
                head_rotation: Quat::IDENTITY,
                direction: Vec3::NEG_Z,
                distance,
            };
            let rad = 0.001_f32;
            let corners =
                billboard_corners(&billboard, (100, 50), Vec2::new(50.0, 25.0), rad).unwrap();
            let width = (corners[1] - corners[0]).truncate().length();
            let height = (corners[2] - corners[0]).truncate().length();
            let expected = 100.0 * rad.tan();
            assert!((width / distance - expected).abs() < 1e-5, "{width}");
            assert!(
                (height / distance - expected * 0.5).abs() < 1e-5,
                "{height}"
            );
            // The anchor (the slot's centre here) is on the line of sight.
            let centre = (corners[0] + corners[3]).truncate() * 0.5;
            let on_axis = billboard.head + Vec3::NEG_Z * distance;
            assert!((centre - on_axis).length() < 1e-3 * distance.max(1.0));
        }
    }

    /// The billboard faces the head, with its top toward the head's up and its left on the left.
    #[test]
    fn billboard_faces_the_head() {
        let billboard = Billboard {
            head: Vec3::ZERO,
            head_rotation: Quat::IDENTITY,
            direction: Vec3::NEG_Z,
            distance: 10.0,
        };
        let c = billboard_corners(&billboard, (10, 10), Vec2::new(0.0, 0.0), 0.01).unwrap();
        // The anchor is the top-left corner: the world point.
        assert!((c[0].truncate() - Vec3::new(0.0, 0.0, -10.0)).length() < 1e-4);
        assert!(c[1].x > c[0].x, "top-right is right of top-left");
        assert!(c[2].y < c[0].y, "bottom-left is below top-left");
        assert!(
            close(c[0].z, c[3].z),
            "the quad is perpendicular to the view"
        );
    }

    #[test]
    fn degenerate_billboards_are_rejected() {
        let mut billboard = Billboard {
            head: Vec3::ZERO,
            head_rotation: Quat::IDENTITY,
            direction: Vec3::NEG_Z,
            distance: 0.0,
        };
        assert_eq!(
            billboard_corners(&billboard, (1, 1), Vec2::ZERO, 0.01),
            None
        );
        billboard.distance = 1.0;
        billboard.direction = Vec3::ZERO;
        assert_eq!(
            billboard_corners(&billboard, (1, 1), Vec2::ZERO, 0.01),
            None
        );
    }

    /// The panel's texels map to its angular height.
    #[test]
    fn radians_per_texel_spans_the_panel() {
        let per = radians_per_texel(1.0, 1000);
        assert!(close(per * 1000.0, 2.0 * 0.5_f32.atan()));
    }

    /// The camera basis of the identity transform: right +X, up +Y, back +Z.
    fn identity_basis() -> (Vec3, Vec3, Vec3, Vec3) {
        (Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z)
    }

    #[test]
    fn fov_classification() {
        let fov = FovTangents::symmetric(45.0_f32.to_radians(), 0.0);
        let (p, r, u, b) = identity_basis();
        for (name, point, inside) in [
            ("straight ahead", Vec3::new(0.0, 0.0, -10.0), true),
            ("inside the right edge", Vec3::new(9.0, 0.0, -10.0), true),
            ("outside the right edge", Vec3::new(11.0, 0.0, -10.0), false),
            ("inside the bottom edge", Vec3::new(0.0, -9.0, -10.0), true),
            ("behind", Vec3::new(0.0, 0.0, 10.0), false),
        ] {
            assert_eq!(fov.contains(p, r, u, b, point), inside, "{name}");
        }
    }

    /// A point between the panel's narrow subtense and the headset's wide view is on screen for
    /// the headset: the reason for classifying with the headset.
    #[test]
    fn headset_fov_is_wider_than_the_panel() {
        let (p, r, u, b) = identity_basis();
        let point = Vec3::new(8.0, 0.0, -10.0); // About 39 degrees right.
        let panel = FovTangents::symmetric(28.0_f32.to_radians(), 0.0);
        let headset = FovTangents::from_angles(-0.9, 0.8, 0.8, -0.9, 0.0)
            .union(FovTangents::from_angles(-0.8, 0.9, 0.8, -0.9, 0.0));
        assert!(!panel.contains(p, r, u, b, point));
        assert!(headset.contains(p, r, u, b, point));
    }

    /// The inset shrinks every side, and the union takes the wider side of each.
    #[test]
    fn fov_inset_and_union() {
        let fov = FovTangents::from_angles(-0.5, 0.6, 0.4, -0.3, 0.1);
        assert!(close(fov.left, (-0.4_f32).tan()));
        assert!(close(fov.right, 0.5_f32.tan()));
        assert!(close(fov.up, 0.3_f32.tan()));
        assert!(close(fov.down, (-0.2_f32).tan()));
        let closed = FovTangents::symmetric(0.05, 0.1);
        assert_eq!(closed.right, 0.0);
        let union = FovTangents::symmetric(0.2, 0.0)
            .union(FovTangents::from_angles(-0.1, 0.5, 0.1, -0.1, 0.0));
        assert!(close(union.right, 0.5_f32.tan()));
        assert!(close(union.left, (-0.2_f32).tan()));
    }

    #[test]
    fn clamp_into_the_safe_area() {
        let min = Vec2::new(48.0, 48.0);
        let max = Vec2::new(952.0, 952.0);
        assert_eq!(
            clamp_into(Vec2::new(-200.0, 500.0), min, max),
            Vec2::new(48.0, 500.0)
        );
        assert_eq!(
            clamp_into(Vec2::new(500.0, 500.0), min, max),
            Vec2::new(500.0, 500.0)
        );
        // An inverted rectangle collapses to its centre.
        assert_eq!(clamp_into(Vec2::ZERO, max, min), Vec2::new(500.0, 500.0));
    }

    #[test]
    fn placement() {
        for (name, sight, expected) in [
            ("clear line of sight", Sight::Clear, 100.0),
            ("a wall in front", Sight::Surface(20.0), 19.7),
            (
                "the surface is the point itself",
                Sight::Surface(99.9),
                100.0,
            ),
            (
                "the surface is behind the point",
                Sight::Surface(150.0),
                100.0,
            ),
            ("never nearer than the minimum", Sight::Surface(0.1), 0.5),
        ] {
            let placed = placement_distance(100.0, sight, 0.3, 0.5);
            assert!(close(placed, expected), "{name}: {placed}");
        }
    }

    #[test]
    fn smoothing_eases_in_log_space() {
        assert_eq!(smooth_distance(None, 10.0, 0.016, 0.15), 10.0);
        // One halflife covers half the log distance: from 10 toward 40 lands at 20.
        let eased = smooth_distance(Some(10.0), 40.0, 0.15, 0.15);
        assert!(close(eased, 20.0), "{eased}");
        // A zero halflife snaps.
        assert!(close(smooth_distance(Some(10.0), 40.0, 0.016, 0.0), 40.0));
        // A degenerate target keeps the current distance.
        assert_eq!(smooth_distance(Some(10.0), f32::NAN, 0.016, 0.15), 10.0);
    }
}
