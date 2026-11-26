//! The colours of the mod's own HUD: the readouts it paints texel by texel (the ammunition counter,
//! the health bar, and the grab hint) and the egui panels (the controls summary and the restart
//! notice). They follow the game's HUD: white text and a light frame on a dark translucent backing,
//! and a red for what wants attention.

use egui::Color32;

/// A straight (not premultiplied) RGBA colour.
pub(crate) type Rgba8 = [u8; 4];

/// A readout's light frame.
pub(crate) const FRAME: Rgba8 = [235, 235, 235, 220];

/// A readout's dark translucent backing.
pub(crate) const BACKING: Rgba8 = [10, 12, 15, 190];

/// The soft dark halo around marks drawn without a backing (the grab hint, the lost-weapon
/// markers, a cockpit button's label), so white marks read against a bright sky or sand.
pub(crate) const HALO: Rgba8 = [0, 0, 0, 200];

/// A readout's text, fill, and marks.
pub(crate) const TEXT: Rgba8 = [240, 240, 240, 255];

/// A readout that wants attention: an empty magazine, or low health.
pub(crate) const ALERT: Rgba8 = [225, 45, 35, 255];

/// The accent red: the health bar's trail, and the panels' titles and rules.
pub(crate) const ACCENT: Rgba8 = [200, 40, 30, 255];

/// A panel's text.
pub(crate) const PANEL_TEXT: Color32 = Color32::WHITE;

/// A panel's secondary text: the game's unavailable-text grey.
pub(crate) const PANEL_DIM: Color32 = Color32::from_rgb(160, 160, 160);

/// A panel's dark translucent backing.
pub(crate) const PANEL_BACKGROUND: Color32 = Color32::from_rgba_premultiplied(10, 12, 15, 215);

/// The accent red on a panel.
pub(crate) const PANEL_ACCENT: Color32 = Color32::from_rgb(ACCENT[0], ACCENT[1], ACCENT[2]);

/// `colour` with its alpha replaced by `alpha`.
pub(crate) const fn with_alpha([r, g, b, _]: Rgba8, alpha: u8) -> Rgba8 {
    [r, g, b, alpha]
}

/// `colour` premultiplied by its alpha, rounded to the nearest step.
pub(crate) fn premultiply([r, g, b, a]: Rgba8) -> Rgba8 {
    let scale = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
    [scale(r), scale(g), scale(b), a]
}

/// `colour` as straight RGBA in 0 to 1.
pub(crate) const fn linear([r, g, b, a]: Rgba8) -> [f32; 4] {
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

/// Straight RGBA in 0 to 1 premultiplied by its alpha.
pub(crate) fn premultiply_linear([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [r * a, g * a, b * a, a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplying_scales_the_colour_by_its_coverage() {
        assert_eq!(premultiply([255, 255, 255, 255]), [255, 255, 255, 255]);
        assert_eq!(premultiply([255, 128, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(premultiply([200, 100, 50, 128]), [100, 50, 25, 128]);
        for colour in [FRAME, BACKING, TEXT, ALERT, with_alpha(ACCENT, 200)] {
            let [r, g, b, a] = premultiply(colour);
            assert!(r <= a && g <= a && b <= a, "{colour:?}");
        }
    }

    #[test]
    fn the_two_premultiplies_agree() {
        for colour in [FRAME, BACKING, TEXT, ALERT] {
            let bytes = premultiply(colour).map(f32::from);
            let floats = premultiply_linear(linear(colour)).map(|c| c * 255.0);
            for (b, f) in bytes.iter().zip(floats) {
                assert!((b - f).abs() <= 0.5, "{colour:?}");
            }
        }
    }
}
