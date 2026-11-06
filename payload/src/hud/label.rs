//! Short labels rasterised on the CPU, for the HUD textures the mod paints texel by texel (the grab
//! hint's socket name, a cockpit button's name). The text is set in Libre Franklin SemiBold (SIL
//! OFL, `payload/assets/fonts/libre-franklin`), the face the controls summary uses for its
//! headings, laid out and rasterised by egui's own text engine without a renderer: the glyphs it
//! packs into its font atlas are copied out of the atlas into a coverage mask.

use std::sync::Arc;

use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId,
    epaint::text::{Fonts, TextOptions},
};
use parking_lot::Mutex;

/// A label's coverage: one value in 0 to 1 per texel, row by row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Coverage {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<f32>,
}

impl Coverage {
    /// The coverage at (`x`, `y`), zero outside the mask.
    pub fn at(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.alpha[(y * self.width + x) as usize]
    }
}

/// `text` on one line at `size_px` texels per em, as tall as the font's line and as wide as the
/// text's advance. Characters the face lacks are left blank.
pub(crate) fn rasterise(text: &str, size_px: f32) -> Coverage {
    let mut guard = FONTS.lock();
    let fonts = guard.get_or_insert_with(|| Fonts::new(TextOptions::default(), definitions()));
    let mut view = fonts.with_pixels_per_point(1.0);
    let galley = view.layout_no_wrap(
        text.to_owned(),
        FontId::new(size_px, FontFamily::Proportional),
        Color32::WHITE,
    );
    let atlas = view.image();
    let width = galley.rect.width().ceil().max(1.0) as u32;
    let height = galley.rect.height().ceil().max(1.0) as u32;
    let mut alpha = vec![0.0_f32; (width * height) as usize];
    for row in &galley.rows {
        for glyph in &row.row.glyphs {
            let uv = glyph.uv_rect;
            if uv.is_nothing() {
                continue;
            }
            let left_top = row.pos + glyph.pos.to_vec2() + uv.offset;
            let (left, top) = (left_top.x.round() as i64, left_top.y.round() as i64);
            for v in uv.min[1]..uv.max[1] {
                for u in uv.min[0]..uv.max[0] {
                    let x = left + i64::from(u - uv.min[0]);
                    let y = top + i64::from(v - uv.min[1]);
                    if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
                        continue;
                    }
                    let texel = atlas.pixels[usize::from(v) * atlas.size[0] + usize::from(u)];
                    let cell = &mut alpha[(y as u32 * width + x as u32) as usize];
                    *cell = cell.max(f32::from(texel.a()) / 255.0);
                }
            }
        }
    }
    Coverage {
        width,
        height,
        alpha,
    }
}

/// The text engine, its fonts loaded once.
static FONTS: Mutex<Option<Fonts>> = Mutex::new(None);

/// Libre Franklin SemiBold alone: the labels are short English names.
fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        FACE.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/libre-franklin/LibreFranklin-SemiBold.ttf"
        ))),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec![FACE.to_owned()]);
    fonts
}

const FACE: &str = "libre-franklin-semibold";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_covers_its_text_and_leaves_the_rest_clear() {
        let label = rasterise("Left back", 24.0);
        assert!(label.height >= 24 && label.height < 48, "{}", label.height);
        assert!(label.width > 60 && label.width < 200, "{}", label.width);
        assert_eq!(label.alpha.len(), (label.width * label.height) as usize);
        let lit = label.alpha.iter().filter(|a| **a > 0.5).count();
        assert!(lit > 100, "{lit}");
        assert!(label.alpha.iter().all(|a| (0.0..=1.0).contains(a)));
        // The space between the words is clear from top to bottom somewhere in the middle.
        let clear_column = (label.width / 4..label.width * 3 / 4)
            .any(|x| (0..label.height).all(|y| label.at(x, y) == 0.0));
        assert!(clear_column);
        assert_eq!(label.at(label.width, 0), 0.0);
    }

    #[test]
    fn a_longer_label_is_wider() {
        let short = rasterise("Chest", 24.0);
        let long = rasterise("Magazine pouch", 24.0);
        assert!(long.width > short.width);
        assert_eq!(long.height, short.height);
    }
}
