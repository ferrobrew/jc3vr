//! The pure half of the on-GPU comparison: the per-pixel rule the diff shader applies (mirrored
//! here so it can be tested off the GPU), and the totals of the cells it writes.

use serde::Serialize;

/// How a plane's pixels are read and compared. Mirrors the `KIND_*` constants of
/// `snapshot_diff_cs.hlsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// 8-bit channels read as codes (the final image, the 8-bit G-buffers, the velocity buffer).
    Unorm8,
    /// 10-bit channels read as integers (the normals G-buffer).
    Uint4,
    /// R11G11B10 float channels.
    Float3,
    /// A 32-bit float depth, with or without an 8-bit stencil.
    Depth,
}

impl Kind {
    /// The shader's `Kind` constant.
    pub(crate) fn shader_constant(self) -> u32 {
        match self {
            Self::Unorm8 => 0,
            Self::Uint4 => 1,
            Self::Float3 => 2,
            Self::Depth => 3,
        }
    }
}

/// One pixel's values as the shader reads them, for [`compare_pixel`].
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pixel {
    /// Normalized channels in `[0, 1]`.
    Unorm([f32; 4]),
    /// Integer channels.
    Uint([u32; 4]),
    /// Float channels (the fourth is unused).
    Float([f32; 3]),
    /// Depth and stencil.
    Depth(f32, u8),
}

/// One pixel's comparison: its mean absolute channel difference, its largest channel difference,
/// whether it differs by the plane's rule, and whether its stored values change at all.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelResult {
    pub mean: f32,
    pub largest: f32,
    pub differs: bool,
    pub changed: bool,
}

/// The rule `compare_pixel` in `snapshot_diff_cs.hlsl` applies, for testing the totals off the
/// GPU. `channels` is how many 8-bit channels count (three for the final image, four for a stage);
/// `threshold` is the largest channel difference a pixel may have and still match, and for a depth
/// the largest distance in representable floats ([`depth_ulps`]). A mismatched pair of pixel kinds
/// compares as differing.
#[cfg(test)]
pub(crate) fn compare_pixel(a: Pixel, b: Pixel, channels: usize, threshold: f32) -> PixelResult {
    match (a, b) {
        (Pixel::Unorm(a), Pixel::Unorm(b)) => {
            let code = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round();
            let channels = channels.clamp(3, 4);
            let deltas: Vec<f32> = (0..channels)
                .map(|i| (code(a[i]) - code(b[i])).abs())
                .collect();
            let largest = deltas.iter().copied().fold(0.0, f32::max);
            PixelResult {
                mean: deltas.iter().sum::<f32>() / channels as f32,
                largest,
                differs: largest > threshold,
                changed: largest > 0.0,
            }
        }
        (Pixel::Uint(a), Pixel::Uint(b)) => {
            let deltas = [0, 1, 2, 3].map(|i| (a[i] as i64 - b[i] as i64).unsigned_abs() as f32);
            let largest = deltas.iter().copied().fold(0.0, f32::max);
            PixelResult {
                mean: deltas.iter().sum::<f32>() / 4.0,
                largest,
                differs: largest > threshold,
                changed: largest > 0.0,
            }
        }
        (Pixel::Float(a), Pixel::Float(b)) => {
            let differs = (0..3).any(|i| a[i].to_bits() != b[i].to_bits());
            let deltas = [0, 1, 2].map(|i| finite_or_zero((a[i] - b[i]).abs()));
            PixelResult {
                mean: deltas.iter().sum::<f32>() / 3.0,
                largest: deltas.iter().copied().fold(0.0, f32::max),
                differs,
                changed: differs,
            }
        }
        (Pixel::Depth(a, stencil_a), Pixel::Depth(b, stencil_b)) => {
            let delta = finite_or_zero((a - b).abs());
            let stencil_differs = stencil_a != stencil_b;
            PixelResult {
                mean: delta,
                largest: delta,
                differs: depth_ulps(a, b) as f32 > threshold || stencil_differs,
                changed: a.to_bits() != b.to_bits() || stencil_differs,
            }
        }
        _ => PixelResult {
            mean: 0.0,
            largest: 0.0,
            differs: true,
            changed: true,
        },
    }
}

/// The distance between two depths in representable floats, as `depth_ulps` in
/// `snapshot_diff_cs.hlsl` measures it: saturating when the signs differ, with the two zeros equal.
#[cfg(test)]
pub(crate) fn depth_ulps(a: f32, b: f32) -> u32 {
    let (ia, ib) = (a.to_bits() as i32, b.to_bits() as i32);
    if (ia ^ ib) < 0 {
        return if (ia | ib) & 0x7fff_ffff == 0 {
            0
        } else {
            u32::MAX
        };
    }
    ia.abs_diff(ib)
}

#[cfg(test)]
fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// One heat-map cell as the shader writes it: [`CELL_WORDS`] words.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cell {
    pub count: u32,
    pub sum: f32,
    pub largest: f32,
    /// The differing pixels' bounding box, inclusive, in region pixels; meaningless when `count` is
    /// zero.
    pub min: [u32; 2],
    pub max: [u32; 2],
    /// The pixels whose stored values change at all, whether or not they differ by the rule.
    pub changed: u32,
}

/// The words the shader writes per cell.
pub(crate) const CELL_WORDS: usize = 8;

impl Cell {
    /// Decode one cell from the shader's words.
    pub(crate) fn from_words(words: &[u32; CELL_WORDS]) -> Self {
        Self {
            count: words[0],
            sum: f32::from_bits(words[1]),
            largest: f32::from_bits(words[2]),
            min: [words[3], words[4]],
            max: [words[5], words[6]],
            changed: words[7],
        }
    }
}

/// The cells the shader would write for a `width` by `height` region on a `grid` heat map, with
/// `pixel(x, y)` giving the two planes' pixels there. The off-GPU twin of the shader's walk.
#[cfg(test)]
pub(crate) fn reference_cells(
    width: u32,
    height: u32,
    grid: [u32; 2],
    channels: usize,
    threshold: f32,
    pixel: impl Fn(u32, u32) -> (Pixel, Pixel),
) -> Vec<Cell> {
    let mut cells = Vec::with_capacity((grid[0] * grid[1]) as usize);
    for cy in 0..grid[1] {
        for cx in 0..grid[0] {
            let (x0, x1) = (cx * width / grid[0], (cx + 1) * width / grid[0]);
            let (y0, y1) = (cy * height / grid[1], (cy + 1) * height / grid[1]);
            let mut cell = Cell {
                count: 0,
                sum: 0.0,
                largest: 0.0,
                min: [u32::MAX; 2],
                max: [0; 2],
                changed: 0,
            };
            for y in y0..y1 {
                for x in x0..x1 {
                    let (a, b) = pixel(x, y);
                    let result = compare_pixel(a, b, channels, threshold);
                    cell.sum += result.mean;
                    cell.largest = cell.largest.max(result.largest);
                    cell.changed += u32::from(result.changed);
                    if result.differs {
                        cell.count += 1;
                        cell.min = [cell.min[0].min(x), cell.min[1].min(y)];
                        cell.max = [cell.max[0].max(x), cell.max[1].max(y)];
                    }
                }
            }
            cells.push(cell);
        }
    }
    cells
}

/// One plane comparison's totals: what the harness reports per eye and plane.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Totals {
    /// The pixels compared.
    pub pixels: u64,
    /// The pixels that differ by the plane's rule.
    pub differing: u64,
    /// The pixels whose stored values change at all. It equals `differing` for a plane whose rule is
    /// exact; for a depth compared with a tolerance it counts the changes the tolerance absorbs too.
    pub changed: u64,
    /// The mean over every compared pixel of its mean absolute channel difference.
    pub mean: f64,
    /// The largest channel difference anywhere.
    pub largest: f32,
    /// The differing pixels' bounding box as `[left, top, right, bottom]`, inclusive, or `None` when
    /// nothing differs.
    pub(crate) bbox: Option<[u32; 4]>,
    /// The heat map's cells across and down.
    pub grid: [u32; 2],
    /// The differing pixel count per cell, row by row.
    pub heat: Vec<u32>,
}

/// Total `cells` (row by row over a `grid` heat map) for a comparison of `pixels` pixels.
pub(crate) fn totals(cells: &[Cell], grid: [u32; 2], pixels: u64) -> Totals {
    let differing = cells.iter().map(|cell| u64::from(cell.count)).sum();
    let changed = cells.iter().map(|cell| u64::from(cell.changed)).sum();
    let sum: f64 = cells.iter().map(|cell| f64::from(cell.sum)).sum();
    let largest = cells.iter().map(|cell| cell.largest).fold(0.0, f32::max);
    let bbox =
        cells
            .iter()
            .filter(|cell| cell.count > 0)
            .fold(None, |bbox: Option<[u32; 4]>, cell| {
                Some(match bbox {
                    None => [cell.min[0], cell.min[1], cell.max[0], cell.max[1]],
                    Some([l, t, r, b]) => [
                        l.min(cell.min[0]),
                        t.min(cell.min[1]),
                        r.max(cell.max[0]),
                        b.max(cell.max[1]),
                    ],
                })
            });
    Totals {
        pixels,
        differing,
        changed,
        mean: if pixels == 0 {
            0.0
        } else {
            sum / pixels as f64
        },
        largest,
        bbox,
        grid,
        heat: cells.iter().map(|cell| cell.count).collect(),
    }
}

/// When a comparison counts as failed for the dump policy: more than `fraction` of its pixels
/// differ, or its mean difference exceeds `mean`. The same limits as the Python comparison's
/// `changed_fraction` and `mean` thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FailLimits {
    pub fraction: f64,
    pub mean: f64,
}

impl FailLimits {
    pub(crate) fn exceeded(self, totals: &Totals) -> bool {
        totals.pixels > 0
            && (totals.differing as f64 / totals.pixels as f64 > self.fraction
                || totals.mean > self.mean)
    }
}

/// `bbox` grown by `margin` pixels on every side and clamped to a `width` by `height` region, as
/// `(left, top, width, height)`.
pub(crate) fn crop(bbox: [u32; 4], margin: u32, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let left = bbox[0].saturating_sub(margin).min(width.saturating_sub(1));
    let top = bbox[1].saturating_sub(margin).min(height.saturating_sub(1));
    let right = bbox[2].saturating_add(margin).min(width.saturating_sub(1));
    let bottom = bbox[3].saturating_add(margin).min(height.saturating_sub(1));
    (
        left,
        top,
        right.max(left) - left + 1,
        bottom.max(top) - top + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unorm(code: [u8; 4]) -> Pixel {
        Pixel::Unorm(code.map(|c| f32::from(c) / 255.0))
    }

    #[test]
    fn final_image_rule_needs_more_than_three_codes() {
        let base = unorm([100, 100, 100, 255]);
        assert!(!compare_pixel(base, unorm([103, 100, 97, 255]), 3, 3.0).differs);
        assert!(compare_pixel(base, unorm([104, 100, 100, 255]), 3, 3.0).differs);
        // Alpha does not count for the final image.
        assert!(!compare_pixel(base, unorm([100, 100, 100, 0]), 3, 3.0).differs);
    }

    #[test]
    fn final_image_mean_is_over_rgb() {
        let result = compare_pixel(unorm([0, 0, 0, 0]), unorm([3, 6, 9, 200]), 3, 3.0);
        assert_eq!(result.mean, 6.0);
        assert_eq!(result.largest, 9.0);
    }

    #[test]
    fn stage_rules_are_exact() {
        assert!(compare_pixel(unorm([1, 2, 3, 4]), unorm([1, 2, 3, 5]), 4, 0.0).differs);
        assert!(!compare_pixel(unorm([1, 2, 3, 4]), unorm([1, 2, 3, 4]), 4, 0.0).differs);
        assert!(
            compare_pixel(Pixel::Uint([1, 2, 3, 0]), Pixel::Uint([1, 2, 4, 0]), 4, 0.0).differs
        );
        let float = compare_pixel(
            Pixel::Float([1.0, 2.0, 3.0]),
            Pixel::Float([1.0, 2.0, 3.0 + f32::EPSILON * 4.0]),
            3,
            0.0,
        );
        assert!(float.differs);
        assert!(compare_pixel(Pixel::Depth(0.5, 1), Pixel::Depth(0.5, 2), 1, 0.0).differs);
        assert!(!compare_pixel(Pixel::Depth(0.5, 1), Pixel::Depth(0.5, 1), 1, 0.0).differs);
        let depth = compare_pixel(Pixel::Depth(0.25, 0), Pixel::Depth(0.5, 0), 1, 0.0);
        assert_eq!((depth.mean, depth.largest), (0.25, 0.25));
    }

    #[test]
    fn depth_tolerance_counts_representable_floats() {
        let depth = 0.012_345f32;
        let next = |value: f32, steps: u32| f32::from_bits(value.to_bits() + steps);
        let within = compare_pixel(
            Pixel::Depth(depth, 0),
            Pixel::Depth(next(depth, 64), 0),
            1,
            64.0,
        );
        assert!(!within.differs);
        assert!(within.changed);
        let beyond = compare_pixel(
            Pixel::Depth(depth, 0),
            Pixel::Depth(next(depth, 65), 0),
            1,
            64.0,
        );
        assert!(beyond.differs);
        // A stencil change differs whatever the depth tolerance.
        assert!(compare_pixel(Pixel::Depth(depth, 0), Pixel::Depth(depth, 1), 1, 64.0).differs);
        assert_eq!(depth_ulps(0.0, -0.0), 0);
        assert_eq!(depth_ulps(f32::from_bits(1), -f32::from_bits(1)), u32::MAX);
        assert_eq!(depth_ulps(next(depth, 3), depth), 3);
    }

    #[test]
    fn cells_count_changes_under_the_tolerance() {
        let cells = reference_cells(4, 1, [1, 1], 1, 16.0, |x, _| {
            let a = 0.5f32;
            (
                Pixel::Depth(a, 0),
                Pixel::Depth(f32::from_bits(a.to_bits() + x * 10), 0),
            )
        });
        let totals = totals(&cells, [1, 1], 4);
        // Steps of 0, 10, 20, and 30 floats: three change, two beyond the tolerance of 16.
        assert_eq!((totals.changed, totals.differing), (3, 2));
    }

    #[test]
    fn non_finite_differences_count_but_stay_out_of_sums() {
        let result = compare_pixel(Pixel::Depth(f32::INFINITY, 0), Pixel::Depth(0.0, 0), 1, 0.0);
        assert!(result.differs);
        assert_eq!(result.mean, 0.0);
    }

    #[test]
    fn cells_total_to_the_whole_region() {
        // A 13 by 7 region (not a multiple of the grid) with a 3 by 2 patch that differs by ten
        // codes and one pixel that differs by two (under the threshold).
        let (width, height) = (13, 7);
        let cells = reference_cells(width, height, [4, 3], 3, 3.0, |x, y| {
            let a = unorm([50, 50, 50, 255]);
            let b = if (4..7).contains(&x) && (2..4).contains(&y) {
                unorm([60, 50, 50, 255])
            } else if (x, y) == (12, 6) {
                unorm([52, 50, 50, 255])
            } else {
                a
            };
            (a, b)
        });
        let totals = totals(&cells, [4, 3], u64::from(width * height));
        assert_eq!(totals.differing, 6);
        assert_eq!(totals.bbox, Some([4, 2, 6, 3]));
        assert_eq!(totals.largest, 10.0);
        let expected_mean = (6.0 * 10.0 / 3.0 + 2.0 / 3.0) / f64::from(width * height);
        assert!((totals.mean - expected_mean).abs() < 1e-6);
        assert_eq!(totals.heat.iter().sum::<u32>(), 6);
        assert_eq!(totals.heat.len(), 12);
    }

    #[test]
    fn identical_planes_total_to_nothing() {
        let cells = reference_cells(8, 8, [2, 2], 4, 0.0, |_, _| {
            (Pixel::Uint([1, 2, 3, 1]), Pixel::Uint([1, 2, 3, 1]))
        });
        let totals = totals(&cells, [2, 2], 64);
        assert_eq!((totals.differing, totals.bbox, totals.mean), (0, None, 0.0));
    }

    #[test]
    fn cells_decode_from_shader_words() {
        let cell = Cell::from_words(&[3, 1.5f32.to_bits(), 4.0f32.to_bits(), 1, 2, 5, 6, 9]);
        assert_eq!(cell.count, 3);
        assert_eq!(cell.changed, 9);
        assert_eq!(cell.sum, 1.5);
        assert_eq!(cell.largest, 4.0);
        assert_eq!((cell.min, cell.max), ([1, 2], [5, 6]));
    }

    #[test]
    fn fail_limits_follow_the_python_thresholds() {
        let limits = FailLimits {
            fraction: 0.001,
            mean: 0.25,
        };
        let mut totals = totals(&[], [1, 1], 10_000);
        assert!(!limits.exceeded(&totals));
        totals.differing = 10;
        assert!(!limits.exceeded(&totals));
        totals.differing = 11;
        assert!(limits.exceeded(&totals));
        totals.differing = 0;
        totals.mean = 0.3;
        assert!(limits.exceeded(&totals));
    }

    #[test]
    fn crops_grow_and_clamp() {
        assert_eq!(crop([10, 10, 20, 12], 4, 100, 100), (6, 6, 19, 11));
        assert_eq!(crop([0, 0, 99, 99], 8, 100, 100), (0, 0, 100, 100));
        assert_eq!(crop([98, 50, 98, 50], 8, 100, 60), (90, 42, 10, 17));
    }
}
