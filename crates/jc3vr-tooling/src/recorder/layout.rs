//! Where each eye lands in the recorded frame, and how big the frame is.
//!
//! The recorder draws both eyes into one frame ([`crate::recorder::convert`]). Each eye keeps its
//! render aspect ratio and is scaled to a chosen width, so a 1920-wide eye still holds a 1920x1080
//! single-eye crop with the eye's full height around it. [`Arrangement::SideBySide`] places the two
//! eyes at that width next to each other, left eye on the left. [`Arrangement::LeftWithHalfRight`]
//! keeps the left eye at the full width and puts the right eye beside it at half size, at the top,
//! for a host that cannot take the bandwidth of the full pair: about 40 percent fewer pixels.
//!
//! Every rectangle has an even origin and an even size, since 4:2:0 chroma covers 2x2 luma blocks
//! and a block must not straddle an eye's edge.

use std::fmt;

use serde::{Deserialize, Serialize};

/// How the two eyes share the recorded frame.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arrangement {
    /// Both eyes at the chosen width, side by side.
    #[default]
    SideBySide,
    /// The left eye at the chosen width, and the right eye at half size beside it, at the top.
    LeftWithHalfRight,
}

impl Arrangement {
    /// The name the control protocol and the debug UI use.
    pub fn label(self) -> &'static str {
        match self {
            Self::SideBySide => "side_by_side",
            Self::LeftWithHalfRight => "left_with_half_right",
        }
    }
}

/// One eye's rectangle in the frame, in luma pixels.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EyeRect {
    pub eye: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The recorded frame's size and where each eye is in it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StreamLayout {
    pub width: u32,
    pub height: u32,
    pub arrangement: Arrangement,
    /// The left eye's rectangle, then the right eye's.
    pub eyes: [EyeRect; 2],
}

/// The narrowest eye the recorder accepts.
pub const MIN_EYE_WIDTH: u32 = 320;
/// The widest eye the recorder accepts. Two eyes at this width make a 4096-wide frame, the widest
/// H.264 frame the hardware encoders take.
pub const MAX_EYE_WIDTH: u32 = 2048;
/// The default eye width: wide enough for a 1920x1080 crop of one eye.
pub const DEFAULT_EYE_WIDTH: u32 = 1920;

/// The eye render size gave no frame.
#[derive(Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// An eye render dimension is zero.
    EmptyEye { width: u32, height: u32 },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEye { width, height } => write!(
                f,
                "recorder layout: the eye render size {width}x{height} is empty"
            ),
        }
    }
}

impl std::error::Error for LayoutError {}

impl StreamLayout {
    /// The layout for eyes rendered at `eye_width` x `eye_height`, with each eye scaled to
    /// `target_width` (clamped to [`MIN_EYE_WIDTH`]..=[`MAX_EYE_WIDTH`] and rounded to even).
    pub fn new(
        eye_width: u32,
        eye_height: u32,
        target_width: u32,
        arrangement: Arrangement,
    ) -> Result<Self, LayoutError> {
        if eye_width == 0 || eye_height == 0 {
            return Err(LayoutError::EmptyEye {
                width: eye_width,
                height: eye_height,
            });
        }
        let width = clamp_eye_width(target_width);
        let height = round_even(f64::from(eye_height) * f64::from(width) / f64::from(eye_width));
        let left = EyeRect {
            eye: 0,
            x: 0,
            y: 0,
            width,
            height,
        };
        let layout = match arrangement {
            Arrangement::SideBySide => Self {
                width: width * 2,
                height,
                arrangement,
                eyes: [
                    left,
                    EyeRect {
                        eye: 1,
                        x: width,
                        ..left
                    },
                ],
            },
            Arrangement::LeftWithHalfRight => {
                let half_width = round_even(f64::from(width) / 2.0);
                let half_height = round_even(f64::from(height) / 2.0);
                Self {
                    width: width + half_width,
                    height,
                    arrangement,
                    eyes: [
                        left,
                        EyeRect {
                            eye: 1,
                            x: width,
                            y: 0,
                            width: half_width,
                            height: half_height,
                        },
                    ],
                }
            }
        };
        Ok(layout)
    }

    /// The byte length of one planar 4:2:0 frame.
    pub fn frame_len(&self) -> usize {
        self.width as usize * self.height as usize * 3 / 2
    }
}

/// Clamp a requested eye width to the accepted range, rounded to even.
pub fn clamp_eye_width(width: u32) -> u32 {
    width.clamp(MIN_EYE_WIDTH, MAX_EYE_WIDTH) & !1
}

/// The nearest even integer to `value`, at least two.
fn round_even(value: f64) -> u32 {
    ((value / 2.0).round() as u32).max(1) * 2
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::*;

    /// The headsets in the capture plan, at their runtime-recommended eye sizes, each eye scaled to
    /// 1920 wide.
    #[test_case(2016, 2240, 3840, 2134 ; "valve index")]
    #[test_case(2064, 2208, 3840, 2054 ; "quest 3")]
    #[test_case(1832, 1920, 3840, 2012 ; "quest 2, scaled up")]
    #[test_case(3584, 3561, 3840, 1908 ; "beyond 2e")]
    fn side_by_side_keeps_each_eyes_aspect_at_1920_wide(
        eye_width: u32,
        eye_height: u32,
        width: u32,
        height: u32,
    ) {
        let layout = StreamLayout::new(eye_width, eye_height, 1920, Arrangement::SideBySide)
            .expect("a layout");
        assert_eq!((layout.width, layout.height), (width, height));
        assert_eq!(
            layout.eyes,
            [
                EyeRect {
                    eye: 0,
                    x: 0,
                    y: 0,
                    width: 1920,
                    height,
                },
                EyeRect {
                    eye: 1,
                    x: 1920,
                    y: 0,
                    width: 1920,
                    height,
                },
            ]
        );
    }

    #[test]
    fn a_1920_wide_eye_holds_a_1920_by_1080_crop() {
        for (w, h) in [(2016, 2240), (2064, 2208), (1832, 1920), (3584, 3561)] {
            let layout = StreamLayout::new(w, h, DEFAULT_EYE_WIDTH, Arrangement::SideBySide)
                .expect("a layout");
            assert!(layout.eyes[0].width >= 1920 && layout.eyes[0].height >= 1080);
        }
    }

    #[test]
    fn left_with_half_right_puts_a_half_size_right_eye_at_the_top() {
        let layout =
            StreamLayout::new(2016, 2240, 1920, Arrangement::LeftWithHalfRight).expect("a layout");
        assert_eq!((layout.width, layout.height), (2880, 2134));
        assert_eq!(
            layout.eyes[1],
            EyeRect {
                eye: 1,
                x: 1920,
                y: 0,
                width: 960,
                height: 1068,
            }
        );
        // About 9.2 MB a frame, against 12.3 MB for the full pair.
        assert_eq!(layout.frame_len(), 2880 * 2134 * 3 / 2);
    }

    #[test]
    fn rectangles_are_even_and_inside_the_frame() {
        for target in [MIN_EYE_WIDTH, 777, 1280, 1920, 1921, MAX_EYE_WIDTH, 9000] {
            for (w, h) in [(2016, 2240), (3584, 3561), (1001, 999), (64, 4000)] {
                for arrangement in [Arrangement::SideBySide, Arrangement::LeftWithHalfRight] {
                    let layout = StreamLayout::new(w, h, target, arrangement).expect("a layout");
                    assert_eq!(layout.width % 2, 0);
                    assert_eq!(layout.height % 2, 0);
                    for rect in layout.eyes {
                        for v in [rect.x, rect.y, rect.width, rect.height] {
                            assert_eq!(v % 2, 0, "{rect:?} in {layout:?}");
                        }
                        assert!(rect.x + rect.width <= layout.width);
                        assert!(rect.y + rect.height <= layout.height);
                        assert!(rect.width > 0 && rect.height > 0);
                    }
                }
            }
        }
    }

    #[test]
    fn the_eye_width_is_clamped_and_even() {
        assert_eq!(clamp_eye_width(0), MIN_EYE_WIDTH);
        assert_eq!(clamp_eye_width(1281), 1280);
        assert_eq!(clamp_eye_width(100_000), MAX_EYE_WIDTH);
    }

    #[test]
    fn an_empty_eye_is_refused() {
        assert_eq!(
            StreamLayout::new(0, 100, 1920, Arrangement::SideBySide),
            Err(LayoutError::EmptyEye {
                width: 0,
                height: 100
            })
        );
    }
}
