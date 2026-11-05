//! The hint's textures, painted on the CPU from the game's icons: one icon on a backing for a take,
//! and for a swap the weapon going in, an arrow, and the weapon coming out, dimmed, as the game's
//! own pickup panel pairs a pickup with the weapon it replaces.

use crate::hud::{
    game_icons::Image,
    palette::{self, premultiply_linear as premultiply},
};

/// The cell an icon is fitted into, texels: the size of the game's weapon icons.
pub(crate) const ICON_TEXELS: (u32, u32) = (232, 80);

/// The take texture's size, texels: one icon cell and its padding.
pub(crate) const TAKE_TEXELS: (u32, u32) = (ICON_TEXELS.0 + 2 * PAD, ICON_TEXELS.1 + 2 * PAD);

/// The swap texture's size, texels: two icon cells, the arrow between them, and the padding.
pub(crate) const SWAP_TEXELS: (u32, u32) = (
    2 * ICON_TEXELS.0 + ARROW_W + 2 * PAD,
    ICON_TEXELS.1 + 2 * PAD,
);

/// Paint a take: `icon` on the backing. Premultiplied RGBA texels of [`TAKE_TEXELS`], row by row.
pub(crate) fn take(icon: &Image) -> Vec<u8> {
    let (width, height) = TAKE_TEXELS;
    let mut canvas = Canvas::backed(width, height);
    canvas.icon(icon, PAD, PAD, 1.0);
    canvas.into_texels()
}

/// Paint a swap: `incoming` (the weapon the hand docks), an arrow, and `outgoing` (the weapon that
/// falls clear), dimmed. Premultiplied RGBA texels of [`SWAP_TEXELS`], row by row.
pub(crate) fn swap(incoming: &Image, outgoing: &Image) -> Vec<u8> {
    let (width, height) = SWAP_TEXELS;
    let mut canvas = Canvas::backed(width, height);
    canvas.icon(incoming, PAD, PAD, 1.0);
    canvas.arrow(PAD + ICON_TEXELS.0, height / 2);
    canvas.icon(
        outgoing,
        PAD + ICON_TEXELS.0 + ARROW_W,
        PAD,
        OUTGOING_OPACITY,
    );
    canvas.into_texels()
}

/// The padding around the icons, texels.
const PAD: u32 = 12;

/// The arrow's cell width, texels.
const ARROW_W: u32 = 64;

/// The outgoing weapon's opacity in a swap.
const OUTGOING_OPACITY: f32 = 0.5;

/// The frame's width, texels.
const BORDER: u32 = 2;

/// Straight RGBA colours in 0 to 1, premultiplied when painted.
const FRAME: [f32; 4] = palette::linear(palette::FRAME);
const BACKING: [f32; 4] = palette::linear(palette::BACKING);
const ARROW: [f32; 4] = palette::linear(palette::TEXT);

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
    /// A canvas filled with the backing inside the frame.
    fn backed(width: u32, height: u32) -> Self {
        let mut texels = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let border =
                    x < BORDER || y < BORDER || x >= width - BORDER || y >= height - BORDER;
                texels.push(premultiply(if border { FRAME } else { BACKING }));
            }
        }
        Self {
            width,
            height,
            texels,
        }
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

    #[test]
    fn a_take_draws_the_icon_over_the_backing() {
        let texels = take(&icon(100..140));
        let (width, height) = TAKE_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        let mid = height / 2;
        assert_eq!(texel(&texels, width, PAD + 120, mid), [255, 255, 255, 255]);
        // A transparent texel of the icon leaves the backing as it was: its white colour is not
        // added, since the icon is premultiplied before compositing.
        let backing = premultiply(BACKING).map(|c| (c * 255.0).round() as u8);
        assert_eq!(texel(&texels, width, PAD + 10, mid), backing);
        let frame = premultiply(FRAME).map(|c| (c * 255.0).round() as u8);
        assert_eq!(texel(&texels, width, 0, 0), frame);
    }

    #[test]
    fn a_swap_draws_both_icons_and_an_arrow_and_dims_the_outgoing_one() {
        let whole = icon(0..ICON_TEXELS.0);
        let texels = swap(&whole, &whole);
        let (width, height) = SWAP_TEXELS;
        assert_eq!(texels.len(), (width * height * 4) as usize);
        let mid = height / 2;
        let incoming = texel(&texels, width, PAD + ICON_TEXELS.0 / 2, mid);
        let outgoing = texel(&texels, width, PAD + ICON_TEXELS.0 + ARROW_W + 100, mid);
        assert_eq!(incoming, [255, 255, 255, 255]);
        assert!(
            outgoing[0] < incoming[0] && outgoing[0] > 100,
            "{outgoing:?}"
        );
        let arrow = texel(&texels, width, PAD + ICON_TEXELS.0 + ARROW_W / 2, mid);
        assert!(arrow[3] > 250, "{arrow:?}");
        let above_shaft = texel(&texels, width, PAD + ICON_TEXELS.0 + 12, mid - 10);
        assert!(above_shaft[0] < 64, "{above_shaft:?}");
    }

    #[test]
    fn an_icon_of_another_size_is_fitted_into_its_cell() {
        let small = Image {
            width: 4,
            height: 4,
            rgba: vec![255; 64],
        };
        let texels = take(&small);
        let (width, height) = TAKE_TEXELS;
        // Fitted by height: an 80-texel square centred in the cell.
        let centre = width / 2;
        assert_eq!(
            texel(&texels, width, centre, height / 2),
            [255, 255, 255, 255]
        );
        assert_ne!(texel(&texels, width, PAD + 10, height / 2)[0], 255);
    }
}
