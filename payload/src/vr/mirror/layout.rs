//! The mirror's layout math: where each eye lands in the back buffer for a [`MirrorView`],
//! compensated for the buffer→window stretch (see the parent module's framing note).

use crate::vr::MirrorView;

/// A width/height pair whose aspect (`width / height`) is preserved by the letterbox fit.
#[derive(Copy, Clone)]
pub(super) struct AspectSize {
    pub(super) width: u32,
    pub(super) height: u32,
}

/// How the desktop mirror frames an image (one eye, the stereo pair, or the UI panel) inside the
/// game window.
///
/// While a session runs the swapchain buffers are the per-eye render resolution, which is near-square
/// (often taller than wide), while the Win32 client rect keeps its original widescreen aspect. The two
/// aspects have to be reconciled, and the choice is the same one every VR game's companion view makes.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum MirrorFraming {
    /// Scale the eye image up until it covers the whole window, cropping whatever falls outside
    /// (centred). No bars; the periphery of the eye render -- most of which is outside the lens's
    /// useful area anyway -- is cut off. This is what other VR titles' desktop views do.
    Fill,
    /// Scale the eye image down until all of it is visible, letterboxing the remainder with black
    /// bars. Nothing is cropped, so it shows exactly what the eye rendered -- useful when checking
    /// coverage at the edges of the frame -- but a near-square eye in a widescreen window wastes most
    /// of the window.
    Fit,
}

/// The extra magnification applied to a single-eye mirror image on top of its fill, about the
/// centre. `1.0` is the plain fill; above that crops in further, tightening the desktop view onto
/// the middle of the eye render.
const MIRROR_ZOOM: f32 = 1.0;

/// One eye's draw in the mirror: which capture, and where it lands in the back buffer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) struct EyeDraw {
    pub(super) eye: usize,
    pub(super) viewport: Viewport,
}

/// The eye draws a [`MirrorView`] makes, in on-screen order from left to right.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum MirrorLayout {
    /// One eye, filling the window.
    Mono(EyeDraw),
    /// Both eyes side by side.
    Pair([EyeDraw; 2]),
}

impl MirrorLayout {
    pub(super) fn draws(&self) -> &[EyeDraw] {
        match self {
            Self::Mono(draw) => std::slice::from_ref(draw),
            Self::Pair(draws) => draws,
        }
    }
}

/// Lay `view` out in the back buffer. A single eye fills the window (cropping its overflow, like
/// other VR titles' desktop views) at [`MIRROR_ZOOM`]. The stereo pair is fitted as one image twice
/// the eye's width, so it is letterboxed and never cropped (a crop would cut the outer edge of each
/// eye differently and break the fusion), then split down the middle with the right eye in the
/// left half and the left eye in the right half: the cross-eyed order. Halving in buffer space
/// halves in window space, since the buffer→window stretch is linear per axis.
pub(super) fn mirror_layout(
    view: MirrorView,
    eye_size: AspectSize,
    buffer: (u32, u32),
    window: (u32, u32),
) -> MirrorLayout {
    let mono = |eye| {
        MirrorLayout::Mono(EyeDraw {
            eye,
            viewport: mirror_viewport(eye_size, buffer, window, MirrorFraming::Fill, MIRROR_ZOOM),
        })
    };
    match view {
        MirrorView::Left => mono(0),
        MirrorView::Right => mono(1),
        MirrorView::StereoPair => {
            let pair_size = AspectSize {
                width: eye_size.width.saturating_mul(2),
                height: eye_size.height,
            };
            let pair = mirror_viewport(pair_size, buffer, window, MirrorFraming::Fit, 1.0);
            let half = pair.width * 0.5;
            MirrorLayout::Pair([
                EyeDraw {
                    eye: 1,
                    viewport: Viewport {
                        width: half,
                        ..pair
                    },
                },
                EyeDraw {
                    eye: 0,
                    viewport: Viewport {
                        x: pair.x + half,
                        width: half,
                        ..pair
                    },
                },
            ])
        }
    }
}

/// A viewport rectangle within the swapchain back buffer, in buffer pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) struct Viewport {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

/// The range [`mirror_viewport`]'s `zoom` is clamped to. The upper bound keeps the (deliberately oversized) `Fill` viewport well inside D3D11's viewport bounds
/// and the rasterizer guard band for any plausible buffer size; the lower bound keeps the image
/// visible at all.
const MIRROR_ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// Compute the back-buffer viewport that, after DXGI stretches the whole buffer onto the window
/// client rect, places the source image at its own aspect, centered, framed per `framing`.
///
/// The mapping is: a fullscreen triangle fills the returned viewport with the source image, so in
/// *window* space the image must occupy the aspect-fit (or aspect-fill) rect of `source` inside
/// `window`. DXGI stretches buffer→window linearly and independently per axis, so a window rect
/// `(wx, wy, tw, th)` is produced by the buffer rect `(wx·bw/ww, wy·bh/wh, tw·bw/ww, th·bh/wh)`.
/// Substituting that rect gives the result.
///
/// Under [`MirrorFraming::Fill`] (and under any `zoom` above `1.0`) the returned viewport extends
/// beyond the buffer on the overflowing axis, with a negative origin: that is the crop, and it is
/// intentional. D3D11 rasterizes only within the render target regardless of the viewport, so the
/// out-of-bounds part is simply never shaded.
///
/// A degenerate size returns the full buffer (a straight, possibly-stretched draw -- the safe
/// fallback), as does a non-finite `zoom`.
pub(super) fn mirror_viewport(
    source: AspectSize,
    buffer: (u32, u32),
    window: (u32, u32),
    framing: MirrorFraming,
    zoom: f32,
) -> Viewport {
    let (bw, bh) = (buffer.0 as f32, buffer.1 as f32);
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let full = Viewport {
        x: 0.0,
        y: 0.0,
        width: bw,
        height: bh,
    };
    if source.width == 0
        || source.height == 0
        || buffer.0 == 0
        || buffer.1 == 0
        || window.0 == 0
        || window.1 == 0
    {
        return full;
    }
    let zoom = if zoom.is_finite() {
        zoom.clamp(*MIRROR_ZOOM_RANGE.start(), *MIRROR_ZOOM_RANGE.end())
    } else {
        1.0
    };

    let image_aspect = source.width as f32 / source.height as f32;
    let window_aspect = ww / wh;

    // Fit the shorter way round, fill the longer: an image wider than the window fits to width and
    // fills to height, and vice versa. Both keep the image's own aspect; they differ only in which
    // axis is matched exactly and which one gets bars (fit) or overflows and crops (fill).
    let match_width = (image_aspect > window_aspect) == matches!(framing, MirrorFraming::Fit);
    let (target_w, target_h) = if match_width {
        (ww, ww / image_aspect)
    } else {
        (wh * image_aspect, wh)
    };
    let (target_w, target_h) = (target_w * zoom, target_h * zoom);
    let target_x = (ww - target_w) * 0.5;
    let target_y = (wh - target_h) * 0.5;

    // Pre-compensate for the per-axis buffer→window stretch.
    let sx = bw / ww;
    let sy = bh / wh;
    Viewport {
        x: target_x * sx,
        y: target_y * sy,
        width: target_w * sx,
        height: target_h * sy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(x: f32, y: f32, width: f32, height: f32) -> Viewport {
        Viewport {
            x,
            y,
            width,
            height,
        }
    }

    fn approx(a: Viewport, b: Viewport) {
        let eps = 1e-3;
        assert!(
            (a.x - b.x).abs() < eps
                && (a.y - b.y).abs() < eps
                && (a.width - b.width).abs() < eps
                && (a.height - b.height).abs() < eps,
            "viewport mismatch: {a:?} != {b:?}",
        );
    }

    /// The core check: after the buffer→window stretch, the viewport must map back to the aspect-fit
    /// window rect. Re-derives the window rect from the viewport and compares against an independent
    /// aspect-fit, so the test does not just restate the formula.
    fn assert_letterboxes(source: AspectSize, buffer: (u32, u32), window: (u32, u32)) {
        let v = mirror_viewport(source, buffer, window, MirrorFraming::Fit, 1.0);
        let (bw, bh) = (buffer.0 as f32, buffer.1 as f32);
        let (ww, wh) = (window.0 as f32, window.1 as f32);
        // Stretch the viewport back into window space.
        let win_x = v.x * ww / bw;
        let win_y = v.y * wh / bh;
        let win_w = v.width * ww / bw;
        let win_h = v.height * wh / bh;

        // Independent aspect-fit in window space.
        let a = source.width as f32 / source.height as f32;
        let (want_w, want_h) = if a > ww / wh {
            (ww, ww / a)
        } else {
            (wh * a, wh)
        };
        let want_x = (ww - want_w) * 0.5;
        let want_y = (wh - want_h) * 0.5;

        let eps = 1e-2;
        assert!((win_x - want_x).abs() < eps, "x: {win_x} != {want_x}");
        assert!((win_y - want_y).abs() < eps, "y: {win_y} != {want_y}");
        assert!((win_w - want_w).abs() < eps, "w: {win_w} != {want_w}");
        assert!((win_h - want_h).abs() < eps, "h: {win_h} != {want_h}");
        // The presented image keeps the source aspect.
        assert!(
            (win_w / win_h - a).abs() < eps,
            "presented aspect {} != source {a}",
            win_w / win_h,
        );
        // The viewport must stay inside the buffer.
        assert!(
            v.x >= -eps && v.y >= -eps,
            "viewport origin outside buffer: {v:?}"
        );
        assert!(
            v.x + v.width <= bw + eps && v.y + v.height <= bh + eps,
            "viewport exceeds buffer: {v:?} in {bw}x{bh}",
        );
    }

    /// Stretch a viewport back into window space, undoing the buffer→window compensation, so a test
    /// can assert about what the user actually sees.
    fn to_window(v: Viewport, buffer: (u32, u32), window: (u32, u32)) -> Viewport {
        let (bw, bh) = (buffer.0 as f32, buffer.1 as f32);
        let (ww, wh) = (window.0 as f32, window.1 as f32);
        vp(
            v.x * ww / bw,
            v.y * wh / bh,
            v.width * ww / bw,
            v.height * wh / bh,
        )
    }

    /// The `Fill` counterpart to [`assert_letterboxes`]: the presented image must keep the source
    /// aspect, cover the window on both axes, and stay centred -- i.e. crop rather than bar.
    fn assert_fills(source: AspectSize, buffer: (u32, u32), window: (u32, u32), zoom: f32) {
        let v = mirror_viewport(source, buffer, window, MirrorFraming::Fill, zoom);
        let w = to_window(v, buffer, window);
        let (ww, wh) = (window.0 as f32, window.1 as f32);
        let a = source.width as f32 / source.height as f32;
        let eps = 1e-2;

        assert!(
            (w.width / w.height - a).abs() < eps,
            "presented aspect {} != source {a}",
            w.width / w.height,
        );
        // Covers the window: the image starts at or before the origin and ends at or past the far edge.
        assert!(
            w.x <= eps && w.y <= eps,
            "image does not reach the window origin: {w:?}",
        );
        assert!(
            w.x + w.width >= ww - eps && w.y + w.height >= wh - eps,
            "image does not cover the window: {w:?} in {ww}x{wh}",
        );
        // Centred: equal overflow on both sides of each axis.
        assert!(
            (w.x - (ww - w.width) * 0.5).abs() < eps && (w.y - (wh - w.height) * 0.5).abs() < eps,
            "image is not centred: {w:?} in {ww}x{wh}",
        );
    }

    #[test]
    fn square_eye_into_widescreen_window_letterboxes_sideways() {
        // Near-square per-eye buffer, 16:9 window: bars left and right.
        let v = mirror_viewport(
            AspectSize {
                width: 1600,
                height: 1600,
            },
            (1600, 1600),
            (1920, 1080),
            MirrorFraming::Fit,
            1.0,
        );
        // Fit to height: window rect is 1080x1080 centered, x=(1920-1080)/2=420. In buffer: the full
        // height (1600) and width 1080*1600/1920=900 at x=420*1600/1920=350.
        approx(v, vp(350.0, 0.0, 900.0, 1600.0));
    }

    #[test]
    fn matching_aspect_fills_the_buffer() {
        // When the source, buffer, and window all share an aspect there are no bars and the viewport
        // is the whole buffer (a straight full-buffer draw) -- under either framing.
        for framing in [MirrorFraming::Fit, MirrorFraming::Fill] {
            let v = mirror_viewport(
                AspectSize {
                    width: 1920,
                    height: 1080,
                },
                (1920, 1080),
                (1920, 1080),
                framing,
                1.0,
            );
            approx(v, vp(0.0, 0.0, 1920.0, 1080.0));
        }
    }

    #[test]
    fn square_eye_into_widescreen_window_fills_by_cropping_vertically() {
        // The real VR case: a near-square per-eye buffer in a 16:9 window. Filling scales to width, so
        // the image overflows top and bottom and the viewport leaves the buffer -- that overflow is
        // the crop, and it is what distinguishes this from the letterboxed fit.
        let source = AspectSize {
            width: 1600,
            height: 1600,
        };
        let v = mirror_viewport(source, (1600, 1600), (1920, 1080), MirrorFraming::Fill, 1.0);
        // Fit to width: window rect is 1920x1920 centred, y = (1080-1920)/2 = -420. In buffer space:
        // full width (1600), height 1920*1600/1080 = 2844.44 at y = -420*1600/1080 = -622.22.
        approx(v, vp(0.0, -622.222, 1600.0, 2844.444));
        assert_fills(source, (1600, 1600), (1920, 1080), 1.0);
    }

    #[test]
    fn fill_crops_whichever_axis_overflows() {
        // Portrait eye (the shape the engine actually resizes to) and an ultra-wide source, in a
        // couple of window aspects: the fill must crop, never bar, in every combination.
        for source in [
            AspectSize {
                width: 1832,
                height: 1920,
            },
            AspectSize {
                width: 3440,
                height: 1440,
            },
        ] {
            for window in [(1920, 1080), (2560, 1440), (1024, 768)] {
                for zoom in [1.0, 1.5] {
                    assert_fills(source, (1832, 1920), window, zoom);
                }
            }
        }
    }

    #[test]
    fn zoom_scales_about_the_centre_and_clamps() {
        let source = AspectSize {
            width: 1600,
            height: 1600,
        };
        let (buffer, window) = ((1600, 1600), (1920, 1080));
        let base = mirror_viewport(source, buffer, window, MirrorFraming::Fit, 1.0);
        let zoomed = mirror_viewport(source, buffer, window, MirrorFraming::Fit, 2.0);

        // Twice the size, same centre.
        approx(
            zoomed,
            vp(
                base.x - base.width * 0.5,
                base.y - base.height * 0.5,
                base.width * 2.0,
                base.height * 2.0,
            ),
        );

        // Out-of-range and non-finite zooms are tamed rather than producing a degenerate viewport.
        approx(
            mirror_viewport(source, buffer, window, MirrorFraming::Fit, 1e9),
            mirror_viewport(
                source,
                buffer,
                window,
                MirrorFraming::Fit,
                *MIRROR_ZOOM_RANGE.end(),
            ),
        );
        approx(
            mirror_viewport(source, buffer, window, MirrorFraming::Fit, f32::NAN),
            base,
        );
    }

    #[test]
    fn source_wider_than_window_bars_top_and_bottom() {
        // An ultra-wide source in a 4:3 window: fit to width, bars top and bottom.
        assert_letterboxes(
            AspectSize {
                width: 3440,
                height: 1440,
            },
            (2048, 2048),
            (1024, 768),
        );
    }

    #[test]
    fn stretched_buffer_recovers_correct_aspect() {
        // The buffer aspect differs from both the source and the window; the compensation must still
        // land the source at its own aspect on the window.
        assert_letterboxes(
            AspectSize {
                width: 1832,
                height: 1920,
            },
            (1832, 1920),
            (2560, 1440),
        );
        assert_letterboxes(
            AspectSize {
                width: 1440,
                height: 1600,
            },
            (1440, 1600),
            (1600, 900),
        );
    }

    /// With the mod owning the back buffer the swapchain matches the window, and the buffer→window
    /// pre-compensation degenerates to the identity: the viewport is simply the window-space rect.
    /// Pins that so the compensation can be removed once ownership is the only mode.
    #[test]
    fn equal_buffer_and_window_needs_no_compensation() {
        let source = AspectSize {
            width: 2015,
            height: 2240,
        };
        let window = (1714u32, 1405u32);
        for framing in [MirrorFraming::Fit, MirrorFraming::Fill] {
            let v = mirror_viewport(source, window, window, framing, 1.0);
            // `sx` and `sy` are both 1, so the buffer-space viewport equals the window-space one.
            let w = to_window(v, window, window);
            approx(v, w);
        }
    }

    #[test]
    fn mono_views_fill_with_their_own_eye() {
        let eye = AspectSize {
            width: 1832,
            height: 1920,
        };
        let (buffer, window) = ((1920, 1080), (1920, 1080));
        let fill = mirror_viewport(eye, buffer, window, MirrorFraming::Fill, MIRROR_ZOOM);
        for (view, want_eye) in [(MirrorView::Left, 0), (MirrorView::Right, 1)] {
            assert_eq!(
                mirror_layout(view, eye, buffer, window),
                MirrorLayout::Mono(EyeDraw {
                    eye: want_eye,
                    viewport: fill,
                }),
            );
        }
    }

    #[test]
    fn stereo_pair_is_cross_eyed_and_letterboxed_as_one_image() {
        // Portrait eyes in a 16:9 window, both with and without the buffer→window stretch.
        let eye = AspectSize {
            width: 1832,
            height: 1920,
        };
        for buffer in [(1920, 1080), (1832, 1920)] {
            let window = (1920, 1080);
            let MirrorLayout::Pair([left_half, right_half]) =
                mirror_layout(MirrorView::StereoPair, eye, buffer, window)
            else {
                panic!("the stereo pair must lay out two eyes");
            };
            // Cross-eyed: the right eye's image on the left, the left eye's on the right.
            assert_eq!((left_half.eye, right_half.eye), (1, 0));

            // The two halves abut, share a row, and are each one eye wide.
            let (l, r) = (left_half.viewport, right_half.viewport);
            assert!(
                (l.x + l.width - r.x).abs() < 1e-3,
                "halves do not abut: {l:?} {r:?}"
            );
            assert!((l.y - r.y).abs() < 1e-3 && (l.height - r.height).abs() < 1e-3);
            assert!((l.width - r.width).abs() < 1e-3);

            // Together they are the pair letterboxed whole: never cropped, each eye at its own
            // aspect on the window.
            let pair = vp(l.x, l.y, l.width + r.width, l.height);
            assert_letterboxes(
                AspectSize {
                    width: eye.width * 2,
                    height: eye.height,
                },
                buffer,
                window,
            );
            approx(
                pair,
                mirror_viewport(
                    AspectSize {
                        width: eye.width * 2,
                        height: eye.height,
                    },
                    buffer,
                    window,
                    MirrorFraming::Fit,
                    1.0,
                ),
            );
            let w = to_window(l, buffer, window);
            let a = eye.width as f32 / eye.height as f32;
            assert!(
                (w.width / w.height - a).abs() < 1e-2,
                "an eye of the pair is distorted: {} != {a}",
                w.width / w.height,
            );
        }
    }

    #[test]
    fn stereo_pair_in_a_window_narrower_than_the_pair_bars_top_and_bottom() {
        // Two near-square eyes make a 2:1 pair, wider than 16:9: fit to width, bars above and below.
        let eye = AspectSize {
            width: 1600,
            height: 1600,
        };
        let MirrorLayout::Pair([l, r]) =
            mirror_layout(MirrorView::StereoPair, eye, (1920, 1080), (1920, 1080))
        else {
            panic!("the stereo pair must lay out two eyes");
        };
        approx(l.viewport, vp(0.0, 60.0, 960.0, 960.0));
        approx(r.viewport, vp(960.0, 60.0, 960.0, 960.0));
    }

    #[test]
    fn degenerate_sizes_fall_back_to_full_buffer() {
        let full = vp(0.0, 0.0, 1280.0, 720.0);
        approx(
            mirror_viewport(
                AspectSize {
                    width: 0,
                    height: 0,
                },
                (1280, 720),
                (1920, 1080),
                MirrorFraming::Fill,
                1.0,
            ),
            full,
        );
        approx(
            mirror_viewport(
                AspectSize {
                    width: 100,
                    height: 100,
                },
                (1280, 720),
                (0, 1080),
                MirrorFraming::Fill,
                1.0,
            ),
            full,
        );
    }
}
