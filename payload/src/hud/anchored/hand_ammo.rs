//! The ammunition counter the mod draws above a hand holding a gun: its magazine and its reserve,
//! painted from the input tick's sample of each hand's gun
//! ([`crate::controllers::weapons::hud_feed::hand_ammo`]).
//!
//! The game's weapon readout shows the wielded weapon alone, and the game wields nothing in the
//! seats whose update skips the wielded weapon. Each hand holds a weapon of its own under the
//! two-handed inventory, so every gun gets the mod's counter, in every mode, and the game's readout
//! is not drawn: the two hands' guns read alike. The HUD draws a counter above each hand holding
//! one: the wielded weapon, a gun armed beside it, and each gun of the game's own dual-wield pair,
//! which share one magazine. A magazine out of its gun reads as empty.

use crate::{
    controllers::weapons::hud_feed::hand_ammo::HandAmmo,
    hud::{
        label,
        palette::{ALERT, TEXT, premultiply, with_alpha},
    },
};

/// The counter texture's size, texels.
pub(crate) const COUNT_TEXELS: (u32, u32) = (192, 40);

/// What a counter shows, so it is repainted only when it changes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct CountKey {
    /// The rounds in the magazine, zero while the magazine is out.
    pub magazine: u32,
    pub reserve: u32,
}

impl CountKey {
    pub fn new(ammo: &HandAmmo) -> Self {
        let clamp = |n: i32| u32::try_from(n).unwrap_or(0).min(MAX_SHOWN);
        Self {
            magazine: if ammo.out { 0 } else { clamp(ammo.magazine) },
            reserve: clamp(ammo.reserve),
        }
    }

    /// The text drawn: the magazine, a slash, and the reserve.
    pub fn text(&self) -> String {
        format!("{}/{}", self.magazine, self.reserve)
    }
}

/// Paint centred, antialiased text into otherwise transparent premultiplied RGBA texels.
/// An empty or absent magazine reads in red.
pub(crate) fn paint(width: u32, height: u32, key: CountKey) -> Vec<u8> {
    let text = label::rasterise(&key.text(), FONT_PX);
    let colour = if key.magazine == 0 { ALERT } else { TEXT };
    let left = width.saturating_sub(text.width) / 2;
    let top = height.saturating_sub(text.height) / 2;
    let mut texels = vec![0; (width * height * 4) as usize];
    for y in 0..text.height.min(height) {
        for x in 0..text.width.min(width) {
            let alpha = (text.at(x, y) * f32::from(colour[3])).round() as u8;
            let offset = (((top + y) * width + left + x) * 4) as usize;
            texels[offset..offset + 4].copy_from_slice(&premultiply(with_alpha(colour, alpha)));
        }
    }
    texels
}

/// The largest count drawn; more shows as this.
const MAX_SHOWN: u32 = 9999;

/// The shared HUD label face's size, in texels per em.
const FONT_PX: f32 = 30.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_out_magazine_reads_as_empty() {
        let ammo = HandAmmo {
            weapon: 1,
            magazine: 12,
            out: true,
            reserve: 90,
            model: None,
        };
        let key = CountKey::new(&ammo);
        assert_eq!(key.magazine, 0);
        assert_eq!(key.text(), "0/90");
        let seated = CountKey::new(&HandAmmo { out: false, ..ammo });
        assert_eq!(seated.magazine, 12);
    }

    #[test]
    fn the_key_clamps_and_formats_the_counts() {
        let key = CountKey::new(&HandAmmo {
            weapon: 1,
            magazine: -3,
            out: false,
            reserve: 123_456,
            model: None,
        });
        assert_eq!(key.magazine, 0);
        assert_eq!(key.reserve, MAX_SHOWN);
        assert_eq!(key.text(), "0/9999");
    }

    #[test]
    fn the_widest_count_fits_its_texture() {
        for digit in '0'..='9' {
            let text = format!("{digit}{digit}{digit}{digit}/{digit}{digit}{digit}{digit}");
            let mask = label::rasterise(&text, FONT_PX);
            assert!(mask.width + 4 <= COUNT_TEXELS.0, "{text}: {}", mask.width);
            assert!(mask.height + 4 <= COUNT_TEXELS.1, "{}", mask.height);
        }
    }

    #[test]
    fn the_painted_text_lights_texels_in_the_counts_colour() {
        let (w, h) = COUNT_TEXELS;
        let full = paint(
            w,
            h,
            CountKey {
                magazine: 30,
                reserve: 450,
            },
        );
        assert_eq!(full.len(), (w * h * 4) as usize);
        let pixels = full.as_chunks::<4>().0;
        assert!(pixels.iter().any(|p| p[3] > 0 && p[3] < 255));
        assert!(pixels.iter().all(|p| p.iter().take(3).all(|c| *c <= p[3])));
        for y in 0..h {
            for x in 0..w {
                if x == 0 || x == w - 1 || y == 0 || y == h - 1 {
                    assert_eq!(pixels[(y * w + x) as usize], [0; 4]);
                }
            }
        }
        assert!(pixels.iter().filter(|p| p[3] == 0).count() > pixels.len() / 2);
        let lit = |texels: &[u8], colour: [u8; 4]| {
            let (texels, _) = texels.as_chunks::<4>();
            texels.iter().filter(|t| **t == premultiply(colour)).count()
        };
        assert!(lit(&full, TEXT) > 0);
        assert_eq!(lit(&full, ALERT), 0);
        let empty = paint(
            w,
            h,
            CountKey {
                magazine: 0,
                reserve: 450,
            },
        );
        assert!(lit(&empty, ALERT) > 0);
        assert_eq!(lit(&empty, TEXT), 0);
    }
}
