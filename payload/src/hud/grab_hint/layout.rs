//! Where the hints sit: on the floating panel's plane, below its centre, where the flat game shows
//! its weapon selection, so they are in front of the player at the HUD's distance wherever the
//! hovering hand is. Two hints sit side by side, the left hand's on the left.

use glam::{Vec3, Vec4};

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

/// The corners of each hand's hint, by hand index (left first), from each hint's aspect (its
/// texture's width over its height; `None` for a hand without a hint). The hints are
/// [`HEIGHT`] of the panel's height tall, centred [`DROP`] of it below the panel's centre, and a
/// lone hint is centred across the panel.
pub(crate) fn hint_corners(
    frame: &PanelFrame,
    aspects: [Option<f32>; 2],
) -> [Option<[Vec4; 4]>; 2] {
    let height = frame.height * HEIGHT;
    let widths = aspects.map(|aspect| aspect.filter(|a| *a > 0.0).map(|a| a * height));
    let gap = frame.height * GAP;
    let total = widths.iter().flatten().sum::<f32>()
        + if widths.iter().all(Option::is_some) {
            gap
        } else {
            0.0
        };
    let row = frame.centre - frame.up * (frame.height * DROP);
    let mut left = -total * 0.5;
    widths.map(|width| {
        let width = width?;
        let centre = row + frame.right * (left + width * 0.5);
        left += width + gap;
        let (r, u) = (frame.right * (width * 0.5), frame.up * (height * 0.5));
        Some(
            [
                centre - r + u,
                centre + r + u,
                centre - r - u,
                centre + r - u,
            ]
            .map(|c| c.extend(1.0)),
        )
    })
}

/// A hint's height, as a share of the panel's height.
const HEIGHT: f32 = 0.11;

/// How far below the panel's centre the hints' centres sit, as a share of the panel's height,
/// clear of the reticle.
const DROP: f32 = 0.2;

/// The gap between two hints, as a share of the panel's height.
const GAP: f32 = 0.03;

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_lone_hint_is_centred_below_the_middle() {
        let frame = frame();
        for hands in [[Some(2.0), None], [None, Some(2.0)]] {
            let corners = hint_corners(&frame, hands);
            let quad = corners.iter().flatten().next().unwrap();
            let c = centre(quad);
            assert!(c.x.abs() < 1e-6);
            assert!((c.y + 2.0 * DROP).abs() < 1e-6);
            assert_eq!(c.z, -3.0);
            let height = (quad[0] - quad[2]).length();
            let width = (quad[1] - quad[0]).length();
            assert!((height - 2.0 * HEIGHT).abs() < 1e-6);
            assert!((width - 2.0 * height).abs() < 1e-5);
        }
    }

    #[test]
    fn two_hints_sit_side_by_side_with_the_left_hand_on_the_left() {
        let frame = frame();
        let [left, right] = hint_corners(&frame, [Some(2.0), Some(5.0)]);
        let (left, right) = (left.unwrap(), right.unwrap());
        assert!(centre(&left).x < 0.0 && centre(&right).x > 0.0);
        let gap = right[0].x - left[1].x;
        assert!((gap - 2.0 * GAP).abs() < 1e-5);
        // The pair is centred across the panel.
        assert!((left[0].x + right[1].x).abs() < 1e-5);
    }

    #[test]
    fn no_hint_has_no_corners() {
        assert_eq!(hint_corners(&frame(), [None, None]), [None, None]);
    }
}
