//! The hint's textures, painted on the CPU from the game's icons: for a take the item's icon, and
//! for a swap the weapon going in, an arrow, and the weapon coming out, dimmed, as the game's own
//! pickup panel pairs a pickup with the weapon it replaces. Above the middle of either (the icon
//! of a take, the arrow of a swap) a small label names the socket or pouch involved.
//!
//! There is no backing or frame: the icons, the arrow, and the label stand on their own, with a
//! soft dark halo around them so the game's white icons read against a bright sky or sand. The
//! lost-weapon markers ([`crate::hud::lost_weapons`]) use the same icon, alone ([`marker`]).

use crate::hud::{
    game_icons::Image,
    label::{self, Coverage},
    palette::{self, premultiply_linear as premultiply},
};

/// The cell an icon is fitted into, texels: the size of the game's weapon icons.
pub(crate) const ICON_TEXELS: (u32, u32) = (232, 80);

/// The marker texture's size, texels: one icon cell and its padding.
pub(crate) const MARKER_TEXELS: (u32, u32) = (ICON_TEXELS.0 + 2 * PAD, ICON_TEXELS.1 + 2 * PAD);

/// The take texture's size, texels: the label's row over one icon cell, and the padding.
pub(crate) const TAKE_TEXELS: (u32, u32) =
    (ICON_TEXELS.0 + 2 * PAD, LABEL_ROW + ICON_TEXELS.1 + 2 * PAD);

/// The swap texture's size, texels: the label's row over two icon cells and the arrow between
/// them, and the padding.
pub(crate) const SWAP_TEXELS: (u32, u32) = (
    2 * ICON_TEXELS.0 + ARROW_W + 2 * PAD,
    LABEL_ROW + ICON_TEXELS.1 + 2 * PAD,
);

/// The height of the icons' row and its padding, texels: what every texture here has below its
/// label, so the layout can size the textures alike.
pub(crate) const ICON_ROW_TEXELS: u32 = ICON_TEXELS.1 + 2 * PAD;

/// Paint a lost-weapon marker: `icon` alone, haloed. Premultiplied RGBA texels of
/// [`MARKER_TEXELS`], row by row.
pub(crate) fn marker(icon: &Image) -> Vec<u8> {
    let (width, height) = MARKER_TEXELS;
    let mut canvas = Canvas::clear(width, height);
    canvas.icon(icon, PAD, PAD, 1.0);
    canvas.haloed().into_texels()
}

/// Paint a take: `icon` with `label` above it, haloed. Premultiplied RGBA texels of
/// [`TAKE_TEXELS`], row by row.
pub(crate) fn take(icon: &Image, label: &str) -> Vec<u8> {
    let (width, height) = TAKE_TEXELS;
    let mut canvas = Canvas::clear(width, height);
    canvas.label(label, width / 2);
    canvas.icon(icon, PAD, LABEL_ROW + PAD, 1.0);
    canvas.haloed().into_texels()
}

/// Paint a swap: `incoming` (the weapon the hand docks), an arrow, and `outgoing` (the weapon that
/// falls clear), dimmed, with `label` above the arrow, haloed. Premultiplied RGBA texels of
/// [`SWAP_TEXELS`], row by row.
pub(crate) fn swap(incoming: &Image, outgoing: &Image, label: &str) -> Vec<u8> {
    let (width, height) = SWAP_TEXELS;
    let mut canvas = Canvas::clear(width, height);
    canvas.label(label, width / 2);
    canvas.icon(incoming, PAD, LABEL_ROW + PAD, 1.0);
    canvas.arrow(PAD + ICON_TEXELS.0, LABEL_ROW + PAD + ICON_TEXELS.1 / 2);
    canvas.icon(
        outgoing,
        PAD + ICON_TEXELS.0 + ARROW_W,
        LABEL_ROW + PAD,
        OUTGOING_OPACITY,
    );
    canvas.haloed().into_texels()
}

/// The tag texture's size, texels: one label and room for its halo.
pub(crate) const TAG_TEXELS: (u32, u32) = (192, LABEL_ROW + PAD);

/// Paint a tag: `label` alone, centred and haloed, for a marker's name
/// ([`crate::hud::mark_labels`]). Premultiplied RGBA texels of [`TAG_TEXELS`], row by row.
pub(crate) fn tag(label: &str) -> Vec<u8> {
    let (width, height) = TAG_TEXELS;
    let mut canvas = Canvas::clear(width, height);
    canvas.label(label, width / 2);
    canvas.haloed().into_texels()
}

/// Whether `text` fits across a tag ([`tag`]) with room for its halo either side.
pub(crate) fn tag_fits(text: &str) -> bool {
    label::rasterise(text, LABEL_PX).width + 2 * PAD <= TAG_TEXELS.0
}

/// The padding around the icons, texels: room for the halo.
const PAD: u32 = 12;

/// The label's row above the icons, texels.
const LABEL_ROW: u32 = 34;

/// The label's size, texels per em.
pub(crate) const LABEL_PX: f32 = 26.0;

/// The arrow's cell width, texels.
const ARROW_W: u32 = 64;

/// The outgoing weapon's opacity in a swap.
const OUTGOING_OPACITY: f32 = 0.5;

/// Straight RGBA colours in 0 to 1, premultiplied when painted.
const ARROW: [f32; 4] = palette::linear(palette::TEXT);
const LABEL: [f32; 4] = palette::linear(palette::TEXT);
const HALO: [f32; 4] = palette::linear(palette::HALO);

/// The halo's reach around what it surrounds, texels: the radius of each of its box blurs.
const HALO_RADIUS: u32 = 4;

/// How much stronger than the blurred coverage the halo is, so it is dense next to the marks and
/// fades out over its reach.
const HALO_GAIN: f32 = 2.5;

/// The arrow's shape, texels: the shaft's half thickness, the head's length and half height, and
/// the margin to the arrow cell's edges.
const SHAFT_HALF: f32 = 4.0;
const HEAD_LENGTH: f32 = 20.0;
const HEAD_HALF: f32 = 16.0;
const ARROW_MARGIN: f32 = 8.0;

/// The arrow's supersampling per axis, for smooth edges.
const SUPERSAMPLE: u32 = 4;

/// Premultiplied RGBA texels being painted, in linear 0 to 1.
struct Canvas {
    width: u32,
    height: u32,
    texels: Vec<[f32; 4]>,
}

impl Canvas {
    /// A transparent canvas.
    fn clear(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            texels: vec![[0.0; 4]; (width * height) as usize],
        }
    }

    /// Draw `text` in the label's row, centred on `centre_x`, clipped to the canvas.
    fn label(&mut self, text: &str, centre_x: u32) {
        if text.is_empty() {
            return;
        }
        let mask: Coverage = label::rasterise(text, LABEL_PX);
        let left = i64::from(centre_x) - i64::from(mask.width / 2);
        let top = i64::from(LABEL_ROW.saturating_sub(mask.height) / 2 + PAD / 2);
        let colour = premultiply(LABEL);
        for y in 0..mask.height {
            for x in 0..mask.width {
                let coverage = mask.at(x, y);
                let (cx, cy) = (left + i64::from(x), top + i64::from(y));
                if coverage <= 0.0 || cx < 0 || cy < 0 {
                    continue;
                }
                self.over(cx as u32, cy as u32, colour.map(|c| c * coverage));
            }
        }
    }

    /// The canvas over a soft dark halo of its own coverage: the alpha blurred by two box blurs,
    /// strengthened, and composited under every texel.
    fn haloed(self) -> Self {
        let alpha = self.texels.iter().map(|t| t[3]).collect::<Vec<_>>();
        let mut blurred = box_blur(&alpha, self.width, self.height, HALO_RADIUS);
        blurred = box_blur(&blurred, self.width, self.height, HALO_RADIUS);
        let halo = premultiply(HALO);
        let texels = self
            .texels
            .iter()
            .zip(blurred)
            .map(|(texel, strength)| {
                let under = halo.map(|c| c * (strength * HALO_GAIN).min(1.0));
                let keep = 1.0 - texel[3];
                std::array::from_fn(|i| texel[i] + under[i] * keep)
            })
            .collect();
        Self { texels, ..self }
    }

    /// Draw `image` fitted into the icon cell at (`left`, `top`), centred and keeping its aspect,
    /// at `opacity`.
    fn icon(&mut self, image: &Image, left: u32, top: u32, opacity: f32) {
        let (cell_w, cell_h) = ICON_TEXELS;
        if image.width == 0 || image.height == 0 {
            return;
        }
        let scale = (cell_w as f32 / image.width as f32).min(cell_h as f32 / image.height as f32);
        let (drawn_w, drawn_h) = (image.width as f32 * scale, image.height as f32 * scale);
        let (x0, y0) = (
            left as f32 + (cell_w as f32 - drawn_w) / 2.0,
            top as f32 + (cell_h as f32 - drawn_h) / 2.0,
        );
        for y in top..top + cell_h {
            for x in left..left + cell_w {
                let (u, v) = (
                    (x as f32 + 0.5 - x0) / scale - 0.5,
                    (y as f32 + 0.5 - y0) / scale - 0.5,
                );
                if u < -0.5 || v < -0.5 || u > image.width as f32 - 0.5 {
                    continue;
                }
                if v > image.height as f32 - 0.5 {
                    continue;
                }
                let colour = sample(image, u, v).map(|c| c * opacity);
                self.over(x, y, colour);
            }
        }
    }

    /// Draw a right-pointing arrow across the arrow cell starting at `left`, centred on `mid_y`.
    fn arrow(&mut self, left: u32, mid_y: u32) {
        let (start, tip) = (
            left as f32 + ARROW_MARGIN,
            (left + ARROW_W) as f32 - ARROW_MARGIN,
        );
        let base = tip - HEAD_LENGTH;
        let mid = mid_y as f32;
        let inside = |x: f32, y: f32| {
            let dy = (y - mid).abs();
            let shaft = x >= start && x <= base && dy <= SHAFT_HALF;
            let head = x >= base && x <= tip && dy <= HEAD_HALF * (tip - x) / HEAD_LENGTH;
            shaft || head
        };
        let top = mid_y.saturating_sub(HEAD_HALF as u32 + 1);
        for y in top..(mid_y + HEAD_HALF as u32 + 2).min(self.height) {
            for x in left..left + ARROW_W {
                let mut hits = 0;
                for sy in 0..SUPERSAMPLE {
                    for sx in 0..SUPERSAMPLE {
                        let step = 1.0 / SUPERSAMPLE as f32;
                        let (px, py) = (
                            x as f32 + (sx as f32 + 0.5) * step,
                            y as f32 + (sy as f32 + 0.5) * step,
                        );
                        hits += u32::from(inside(px, py));
                    }
                }
                if hits > 0 {
                    let coverage = hits as f32 / (SUPERSAMPLE * SUPERSAMPLE) as f32;
                    self.over(x, y, premultiply(ARROW).map(|c| c * coverage));
                }
            }
        }
    }

    /// Composite the premultiplied `colour` over the texel at (`x`, `y`).
    fn over(&mut self, x: u32, y: u32, colour: [f32; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let texel = &mut self.texels[(y * self.width + x) as usize];
        let keep = 1.0 - colour[3];
        for (dst, src) in texel.iter_mut().zip(colour) {
            *dst = src + *dst * keep;
        }
    }

    fn into_texels(self) -> Vec<u8> {
        self.texels
            .into_iter()
            .flat_map(|texel| texel.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect()
    }
}

/// `values` (a `width` by `height` grid, row by row) averaged over a square `2 * radius + 1` texels
/// across, horizontally then vertically, with zero outside the grid.
fn box_blur(values: &[f32], width: u32, height: u32, radius: u32) -> Vec<f32> {
    let (w, h, r) = (width as usize, height as usize, radius as usize);
    let span = (2 * r + 1) as f32;
    let pass = |source: &[f32], along_rows: bool| {
        let (lines, length) = if along_rows { (h, w) } else { (w, h) };
        let index = |line: usize, i: usize| {
            if along_rows {
                line * w + i
            } else {
                i * w + line
            }
        };
        let mut out = vec![0.0; source.len()];
        for line in 0..lines {
            let mut sum = (0..=r.min(length - 1))
                .map(|i| source[index(line, i)])
                .sum::<f32>();
            for i in 0..length {
                out[index(line, i)] = sum / span;
                if i + r + 1 < length {
                    sum += source[index(line, i + r + 1)];
                }
                if i >= r {
                    sum -= source[index(line, i - r)];
                }
            }
        }
        out
    };
    if w == 0 || h == 0 {
        return values.to_vec();
    }
    pass(&pass(values, true), false)
}

/// The premultiplied colour of `image` at texel coordinates (`u`, `v`), bilinearly filtered with
/// the edges clamped.
fn sample(image: &Image, u: f32, v: f32) -> [f32; 4] {
    let max_x = image.width as f32 - 1.0;
    let max_y = image.height as f32 - 1.0;
    let (u, v) = (u.clamp(0.0, max_x), v.clamp(0.0, max_y));
    let (x0, y0) = (u.floor() as u32, v.floor() as u32);
    let (x1, y1) = (
        (x0 + 1).min(image.width - 1),
        (y0 + 1).min(image.height - 1),
    );
    let (fx, fy) = (u - x0 as f32, v - y0 as f32);
    let at = |x: u32, y: u32| premultiply(image.texel(x, y).map(|c| f32::from(c) / 255.0));
    let lerp = |a: [f32; 4], b: [f32; 4], t: f32| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    lerp(
        lerp(at(x0, y0), at(x1, y0), fx),
        lerp(at(x0, y1), at(x1, y1), fx),
        fy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An icon-sized image, opaque white inside `lit` columns and transparent elsewhere; the
    /// transparent texels keep a white colour, as the game's icons do.
    fn icon(lit: std::ops::Range<u32>) -> Image {
        let (width, height) = ICON_TEXELS;
        let rgba = (0..height)
            .flat_map(|_| 0..width)
            .flat_map(|x| [255, 255, 255, if lit.contains(&x) { 255 } else { 0 }])
            .collect();
        Image {
            width,
            height,
            rgba,
        }
    }

    fn texel(texels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * width + x) * 4) as usize;
        [texels[at], texels[at + 1], texels[at + 2], texels[at + 3]]
    }

    fn alpha(texels: &[u8], width: u32, x: u32, y: u32) -> u8 {
        texel(texels, width, x, y)[3]
    }

    #[test]
    fn a_take_draws_the_icon_with_a_halo_and_no_backing() {
        let texels = take(&icon(100..140), "Right hip");
        let (width, height) = TAKE_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        let mid = LABEL_ROW + PAD + ICON_TEXELS.1 / 2;
        assert_eq!(texel(&texels, width, PAD + 120, mid), [255, 255, 255, 255]);
        // No backing or frame: the corners and the icon's clear texels far from its marks are
        // transparent.
        assert_eq!(texel(&texels, width, 0, 0), [0; 4]);
        assert_eq!(texel(&texels, width, width - 1, height - 1), [0; 4]);
        assert_eq!(texel(&texels, width, PAD + 20, mid), [0; 4]);
        // Beside the icon's marks, a dark halo.
        let halo = texel(&texels, width, PAD + 97, mid);
        assert!(halo[3] > 64, "{halo:?}");
        assert!(halo[0] < 32, "{halo:?}");
        assert!(alpha(&texels, width, PAD + 92, mid) < halo[3]);
    }

    #[test]
    fn a_take_names_its_place_above_the_icon() {
        let blank = icon(0..0);
        let named = take(&blank, "Left back");
        let unnamed = take(&blank, "");
        let (width, _) = TAKE_TEXELS;
        let lit_in_label = |texels: &[u8]| {
            (0..LABEL_ROW + PAD / 2)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .filter(|&(x, y)| texel(texels, width, x, y)[0] > 200)
                .collect::<Vec<_>>()
        };
        let lit = lit_in_label(&named);
        assert!(lit.len() > 50, "{}", lit.len());
        assert!(lit_in_label(&unnamed).is_empty());
        // Centred across the texture, over the icon.
        let (min, max) = lit
            .iter()
            .fold((u32::MAX, 0), |(lo, hi), &(x, _)| (lo.min(x), hi.max(x)));
        let centre = (min + max) as i64 / 2;
        assert!((centre - i64::from(width / 2)).abs() <= 3, "{min} {max}");
    }

    #[test]
    fn a_swap_draws_both_icons_and_an_arrow_and_dims_the_outgoing_one() {
        let whole = icon(0..ICON_TEXELS.0);
        let texels = swap(&whole, &whole, "Left back");
        let (width, height) = SWAP_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        let mid = LABEL_ROW + PAD + ICON_TEXELS.1 / 2;
        let incoming = texel(&texels, width, PAD + ICON_TEXELS.0 / 2, mid);
        let outgoing = texel(&texels, width, PAD + ICON_TEXELS.0 + ARROW_W + 100, mid);
        assert_eq!(incoming, [255, 255, 255, 255]);
        assert!(
            outgoing[0] < incoming[0] && outgoing[0] > 100,
            "{outgoing:?}"
        );
        let arrow = texel(&texels, width, PAD + ICON_TEXELS.0 + ARROW_W / 2, mid);
        assert!(arrow[3] > 250, "{arrow:?}");
        // Above the shaft, between the icons, only the dark halo.
        let above_shaft = texel(&texels, width, PAD + ICON_TEXELS.0 + 12, mid - 10);
        assert!(above_shaft[0] < 64, "{above_shaft:?}");
        // The label sits over the arrow, in the middle of the texture.
        let label_lit = (0..LABEL_ROW).any(|y| {
            (width / 2 - 20..width / 2 + 20).any(|x| texel(&texels, width, x, y)[0] > 200)
        });
        assert!(label_lit);
    }

    #[test]
    fn a_marker_is_the_icon_alone() {
        let texels = marker(&icon(100..140));
        let (width, height) = MARKER_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        assert_eq!(
            texel(&texels, width, PAD + 120, height / 2),
            [255, 255, 255, 255]
        );
        assert_eq!(texel(&texels, width, 0, 0), [0; 4]);
        assert!(alpha(&texels, width, PAD + 97, height / 2) > 64);
    }

    #[test]
    fn a_tag_is_its_label_alone_haloed() {
        let texels = tag("Nitrous");
        let (width, height) = TAG_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        let lit = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| texel(&texels, width, x, y)[0] > 200)
            .count();
        assert!(lit > 50, "{lit}");
        assert_eq!(texel(&texels, width, 0, height - 1), [0; 4]);
        assert!(tag("").iter().all(|&c| c == 0));
    }

    #[test]
    fn the_box_blur_spreads_and_keeps_the_mass_inside() {
        let mut values = vec![0.0; 9 * 9];
        values[4 * 9 + 4] = 1.0;
        let blurred = box_blur(&values, 9, 9, 1);
        assert!((blurred[4 * 9 + 4] - 1.0 / 9.0).abs() < 1e-6);
        assert!((blurred[3 * 9 + 3] - 1.0 / 9.0).abs() < 1e-6);
        assert_eq!(blurred[2 * 9 + 2], 0.0);
        assert!((blurred.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(box_blur(&[], 0, 0, 2).is_empty());
    }

    #[test]
    fn an_icon_of_another_size_is_fitted_into_its_cell() {
        let small = Image {
            width: 4,
            height: 4,
            rgba: vec![255; 64],
        };
        let texels = marker(&small);
        let (width, height) = MARKER_TEXELS;
        // Fitted by height: an 80-texel square centred in the cell.
        let centre = width / 2;
        assert_eq!(
            texel(&texels, width, centre, height / 2),
            [255, 255, 255, 255]
        );
        assert_ne!(texel(&texels, width, PAD + 10, height / 2)[0], 255);
    }
}
