//! Where the hints sit: on the floating panel's plane, below its centre, where the flat game shows
//! its weapon selection, so they are in front of the player at the HUD's distance wherever the
//! hovering hand is. Two hints sit apart, each toward its own hand's side.

use glam::{Vec3, Vec4};

use crate::hud::{grab_hint::paint::ICON_ROW_TEXELS, tuning::HUD};

/// The panel's plane, from its world corners.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PanelFrame {
    pub centre: Vec3,
    /// Unit vectors along the panel's width and height.
    pub right: Vec3,
    pub up: Vec3,
    /// The panel's height, metres.
    pub height: f32,
}

/// The panel's plane from its corners (top left, top right, bottom left, bottom right, as the quad
/// pass takes them), or `None` for a degenerate panel.
pub(crate) fn panel_frame(corners: &[Vec4; 4]) -> Option<PanelFrame> {
    let [tl, tr, bl, br] = corners.map(|c| c.truncate());
    let centre = (tl + tr + bl + br) * 0.25;
    let up = tl - bl;
    let height = up.length();
    let frame = PanelFrame {
        centre,
        right: (tr - tl).try_normalize()?,
        up: up.try_normalize()?,
        height,
    };
    (height > 0.0 && centre.is_finite()).then_some(frame)
}

/// The corners of each hand's hint, by hand index (left first), from each hint's texture size in
/// texels (`None` for a hand without a hint), by the tuning's `grab_hint` section. The icons' row of
/// every hint is `height` of the panel's height tall, so the textures share one scale, and is
/// centred `drop` of it below the panel's centre, a label's row rising above it. A lone hint is
/// centred across the panel; two sit apart toward their hands' sides, each centred `spread` of the
/// panel's height from the middle, or farther when they are too wide for that (`gap` apart at the
/// least), so it is plain which hand's each is.
pub(crate) fn hint_corners(
    frame: &PanelFrame,
    sizes: [Option<(u32, u32)>; 2],
) -> [Option<[Vec4; 4]>; 2] {
    let tuning = &HUD.get().grab_hint;
    let texel = frame.height * tuning.height / ICON_ROW_TEXELS as f32;
    let extents = sizes.map(|size| {
        size.filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| (w as f32 * texel, h as f32 * texel))
    });
    let gap = frame.height * tuning.gap;
    let offsets = match extents {
        [Some((left, _)), Some((right, _))] => {
            let apart = (left + right) * 0.5 + gap;
            let half = (frame.height * tuning.spread).max(apart * 0.5);
            [-half, half]
        }
        _ => [0.0; 2],
    };
    let icon_row = frame.height * tuning.height;
    let row = frame.centre - frame.up * (frame.height * tuning.drop);
    let mut corners = [None; 2];
    for (slot, (extent, offset)) in corners.iter_mut().zip(extents.into_iter().zip(offsets)) {
        let Some((width, height)) = extent else {
            continue;
        };
        // The icons' row stays on the row; whatever the texture has above it rises.
        let centre = row + frame.right * offset + frame.up * ((height - icon_row) * 0.5);
        let (r, u) = (frame.right * (width * 0.5), frame.up * (height * 0.5));
        *slot = Some(
            [
                centre - r + u,
                centre + r + u,
                centre - r - u,
                centre + r - u,
            ]
            .map(|c| c.extend(1.0)),
        );
    }
    corners
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::tuning::GrabHintTuning;

    fn tuning() -> &'static GrabHintTuning {
        &HUD.get().grab_hint
    }

    fn frame() -> PanelFrame {
        // A panel 2 m tall, 3 m in front, facing the origin.
        panel_frame(&[
            Vec4::new(-1.5, 1.0, -3.0, 1.0),
            Vec4::new(1.5, 1.0, -3.0, 1.0),
            Vec4::new(-1.5, -1.0, -3.0, 1.0),
            Vec4::new(1.5, -1.0, -3.0, 1.0),
        ])
        .unwrap()
    }

    fn centre(corners: &[Vec4; 4]) -> Vec3 {
        corners.iter().map(|c| c.truncate()).sum::<Vec3>() * 0.25
    }

    #[test]
    fn the_panel_frame_reads_the_corners() {
        let frame = frame();
        assert_eq!(frame.centre, Vec3::new(0.0, 0.0, -3.0));
        assert_eq!(frame.right, Vec3::X);
        assert_eq!(frame.up, Vec3::Y);
        assert_eq!(frame.height, 2.0);
        assert!(panel_frame(&[Vec4::ZERO; 4]).is_none());
    }

    /// A texture whose icons' row is `aspect` times as wide as it is tall, with `label` texels
    /// above it.
    fn size(aspect: f32, label: u32) -> Option<(u32, u32)> {
        Some((
            (aspect * ICON_ROW_TEXELS as f32).round() as u32,
            ICON_ROW_TEXELS + label,
        ))
    }

    #[test]
    fn a_lone_hint_is_centred_below_the_middle() {
        let frame = frame();
        for hands in [[size(2.0, 0), None], [None, size(2.0, 0)]] {
            let corners = hint_corners(&frame, hands);
            let quad = corners.iter().flatten().next().unwrap();
            let c = centre(quad);
            assert!(c.x.abs() < 1e-6);
            assert!((c.y + 2.0 * tuning().drop).abs() < 1e-6);
            assert_eq!(c.z, -3.0);
            let height = (quad[0] - quad[2]).length();
            let width = (quad[1] - quad[0]).length();
            assert!((height - 2.0 * tuning().height).abs() < 1e-6);
            assert!((width - 2.0 * height).abs() < 1e-2);
        }
    }

    /// A label above the icons makes the quad taller upward: the icons' row keeps its height and
    /// its place.
    #[test]
    fn a_label_rises_above_the_icons_row() {
        let frame = frame();
        let label = ICON_ROW_TEXELS / 2;
        let [quad, _] = hint_corners(&frame, [size(2.0, label), None]);
        let quad = quad.unwrap();
        let bottom = quad[2].y;
        let top = quad[0].y;
        let row_height = 2.0 * tuning().height;
        assert!((bottom - (-2.0 * tuning().drop - row_height * 0.5)).abs() < 1e-5);
        assert!((top - bottom - row_height * 1.5).abs() < 1e-5);
    }

    #[test]
    fn two_hints_sit_apart_toward_their_hands() {
        let frame = frame();
        let [left, right] = hint_corners(&frame, [size(2.0, 0), size(2.4, 0)]);
        let (left, right) = (left.unwrap(), right.unwrap());
        assert!((centre(&left).x + 2.0 * tuning().spread).abs() < 1e-5);
        assert!((centre(&right).x - 2.0 * tuning().spread).abs() < 1e-5);
        assert!(right[0].x - left[1].x >= 2.0 * tuning().gap - 1e-5);
    }

    #[test]
    fn two_wide_hints_keep_the_gap_between_them() {
        let frame = frame();
        let [left, right] = hint_corners(&frame, [size(9.0, 0), size(9.0, 0)]);
        let (left, right) = (left.unwrap(), right.unwrap());
        assert!(centre(&left).x < 0.0 && centre(&right).x > 0.0);
        let gap = right[0].x - left[1].x;
        assert!((gap - 2.0 * tuning().gap).abs() < 1e-4, "{gap}");
        assert!((left[0].x + right[1].x).abs() < 1e-4);
    }

    #[test]
    fn no_hint_has_no_corners() {
        assert_eq!(hint_corners(&frame(), [None, None]), [None, None]);
        assert_eq!(hint_corners(&frame(), [Some((0, 10)), None]), [None, None]);
    }
}
