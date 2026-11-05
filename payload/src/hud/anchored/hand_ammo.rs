//! The ammunition readout of a gun in a hand: its magazine and its reserve, as a small counter the
//! mod draws above that hand.
//!
//! The game's weapon readout shows the wielded weapon alone, and the game wields nothing in the
//! seats whose update skips the wielded weapon. Each hand holds a weapon of its own under the
//! two-handed inventory, so every gun gets the mod's counter, in every mode, and the game's readout
//! is not drawn: the two hands' guns read alike. The input tick samples each hand's gun
//! ([`observe`], on the game thread where the table keeps the guns alive), and the HUD draws a
//! counter above each hand holding one ([`counted_hands`]).

use anyhow::Context as _;
use jc3gi::{
    character::character::Character, equipment::weapon::WeaponBase, graphics_engine::device::Device,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::hud::target::HudTarget;

/// The counter texture's size, texels.
pub(crate) const COUNT_TEXELS: (u32, u32) = (192, 40);

/// One hand's gun as the input tick read it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct HandAmmo {
    /// The gun's address.
    pub weapon: usize,
    /// The rounds in its magazine.
    pub magazine: i32,
    /// The rounds of its ammunition its user carries.
    pub reserve: i32,
}

/// The input tick's sample: each hand's gun, by [`Hand::index`](crate::controllers::frame::Hand::index).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct HandsAmmo {
    pub hands: [Option<HandAmmo>; 2],
}

/// Sample the gun in each hand, by [`Hand::index`](crate::controllers::frame::Hand::index), for the HUD. Called each input tick of the
/// weapon handling.
///
/// # Safety
///
/// Each address must be a live weapon of the local player, read on the game thread before the
/// world update.
pub(crate) unsafe fn observe(guns: [Option<usize>; 2]) {
    let hands = guns.map(|gun| {
        let weapon = unsafe { (gun? as *const WeaponBase).as_ref() }?;
        // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character.
        let reserve = unsafe { (weapon.m_User[0] as *const Character).as_ref() }
            .map_or(0, |user| unsafe {
                user.m_Inventory.GetAmmoCount(weapon.m_AmmunitionType)
            });
        Some(HandAmmo {
            weapon: gun?,
            magazine: weapon.m_MagazineAmmo,
            reserve,
        })
    });
    *SAMPLE.lock() = Some(HandsAmmo { hands });
}

/// Forget the sample: the weapon handling stood down.
pub(crate) fn clear() {
    *SAMPLE.lock() = None;
}

/// The last sample.
pub(crate) fn sampled() -> Option<HandsAmmo> {
    *SAMPLE.lock()
}

/// The hands, by [`Hand::index`](crate::controllers::frame::Hand::index), that get a counter: every hand holding a gun, the wielded weapon
/// as well as a gun armed beside it, and each gun of the game's own dual-wield pair, which share
/// one magazine.
pub(crate) fn counted_hands(sample: &HandsAmmo) -> [bool; 2] {
    sample.hands.map(|gun| gun.is_some())
}

/// What a counter shows, so it is repainted only when it changes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct CountKey {
    pub magazine: u32,
    pub reserve: u32,
}

impl CountKey {
    pub fn new(ammo: &HandAmmo) -> Self {
        let clamp = |n: i32| u32::try_from(n).unwrap_or(0).min(MAX_SHOWN);
        Self {
            magazine: clamp(ammo.magazine),
            reserve: clamp(ammo.reserve),
        }
    }

    /// The text drawn: the magazine, a slash, and the reserve.
    pub fn text(&self) -> String {
        format!("{}/{}", self.magazine, self.reserve)
    }
}

/// Paint the counter into premultiplied RGBA texels, row by row: a dark translucent backing with a
/// light border and the text centred on it, in white, or in red for an empty magazine.
pub(crate) fn paint(width: u32, height: u32, key: CountKey) -> Vec<u8> {
    let text = key.text();
    let colour = if key.magazine == 0 { EMPTY } else { TEXT };
    let glyphs = text.chars().filter_map(glyph).collect::<Vec<_>>();
    let advance = (GLYPH_W + 1) * SCALE;
    let text_w = (glyphs.len() as u32 * advance).saturating_sub(SCALE);
    let text_h = GLYPH_H * SCALE;
    let left = width.saturating_sub(text_w) / 2;
    let top = height.saturating_sub(text_h) / 2;
    let lit = |x: u32, y: u32| -> bool {
        if x < left || y < top || y >= top + text_h {
            return false;
        }
        let (gx, gy) = ((x - left) / SCALE, (y - top) / SCALE);
        let (index, column) = ((gx / (GLYPH_W + 1)) as usize, gx % (GLYPH_W + 1));
        column < GLYPH_W
            && glyphs
                .get(index)
                .is_some_and(|rows| rows[gy as usize] & (1 << (GLYPH_W - 1 - column)) != 0)
    };
    let mut texels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let border = x < BORDER || y < BORDER || x >= width - BORDER || y >= height - BORDER;
            let c = if border {
                FRAME
            } else if lit(x, y) {
                colour
            } else {
                BACKING
            };
            texels.extend_from_slice(&premultiply(c));
        }
    }
    texels
}

/// A counter's texture, repainted when its key changes.
pub(crate) struct Count {
    target: HudTarget,
    key: Option<CountKey>,
}

impl Count {
    pub fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            target: HudTarget::new(device, COUNT_TEXELS.0, COUNT_TEXELS.1)
                .context("creating an ammunition counter texture")?,
            key: None,
        })
    }

    pub fn target(&self) -> &HudTarget {
        &self.target
    }

    /// Repaint the counter for `key` if it changed.
    pub fn update(&mut self, context: &ID3D11DeviceContext, key: CountKey) {
        if self.key == Some(key) {
            return;
        }
        let (width, height) = COUNT_TEXELS;
        let texels = paint(width, height, key);
        // SAFETY: the texture is ours, of the default usage, and `texels` holds `width * height`
        // RGBA texels; the caller holds the engine context mutex.
        unsafe {
            context.UpdateSubresource(
                self.target.color_texture(),
                0,
                None,
                texels.as_ptr().cast(),
                width * 4,
                0,
            );
        }
        self.key = Some(key);
    }
}

static SAMPLE: Mutex<Option<HandsAmmo>> = Mutex::new(None);

/// The largest count drawn; more shows as this.
const MAX_SHOWN: u32 = 9999;

/// A glyph's size in font cells.
const GLYPH_W: u32 = 3;
const GLYPH_H: u32 = 5;

/// The texels per font cell.
const SCALE: u32 = 5;

/// The frame's width, texels.
const BORDER: u32 = 2;

/// Straight RGBA colours, premultiplied when painted; the health bar's palette.
const FRAME: [u8; 4] = [235, 235, 235, 220];
const BACKING: [u8; 4] = [10, 12, 15, 190];
const TEXT: [u8; 4] = [240, 240, 240, 255];
const EMPTY: [u8; 4] = [225, 45, 35, 255];

/// The rows of a character's 3-by-5 glyph, top first, each row's bits left to right.
fn glyph(c: char) -> Option<[u8; GLYPH_H as usize]> {
    Some(match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        _ => return None,
    })
}

fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let scale = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
    [scale(r), scale(g), scale(b), a]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun(weapon: usize) -> Option<HandAmmo> {
        Some(HandAmmo {
            weapon,
            magazine: 12,
            reserve: 90,
        })
    }

    #[test]
    fn every_hand_holding_a_gun_gets_a_counter() {
        let both = HandsAmmo {
            hands: [gun(0x20), gun(0x10)],
        };
        assert_eq!(counted_hands(&both), [true, true]);
        // The native pair is one weapon in both hands; each hand shows its shared magazine.
        let pair = HandsAmmo {
            hands: [gun(0x10), gun(0x10)],
        };
        assert_eq!(counted_hands(&pair), [true, true]);
        let one = HandsAmmo {
            hands: [None, gun(0x10)],
        };
        assert_eq!(counted_hands(&one), [false, true]);
        assert_eq!(counted_hands(&HandsAmmo::default()), [false, false]);
    }

    #[test]
    fn the_key_clamps_and_formats_the_counts() {
        let key = CountKey::new(&HandAmmo {
            weapon: 1,
            magazine: -3,
            reserve: 123_456,
        });
        assert_eq!(key.magazine, 0);
        assert_eq!(key.reserve, MAX_SHOWN);
        assert_eq!(key.text(), "0/9999");
    }

    #[test]
    fn the_widest_count_fits_its_texture() {
        let advance = (GLYPH_W + 1) * SCALE;
        let widest = "9999/9999".len() as u32 * advance - SCALE;
        assert!(widest + 2 * BORDER <= COUNT_TEXELS.0);
        assert!(GLYPH_H * SCALE + 2 * BORDER <= COUNT_TEXELS.1);
        assert!(
            "9999/9999"
                .chars()
                .chain("0123456789".chars())
                .all(|c| glyph(c).is_some())
        );
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
        let lit = |texels: &[u8], colour: [u8; 4]| {
            let (texels, _) = texels.as_chunks::<4>();
            texels.iter().filter(|t| **t == premultiply(colour)).count()
        };
        assert!(lit(&full, TEXT) > 0);
        assert_eq!(lit(&full, EMPTY), 0);
        let empty = paint(
            w,
            h,
            CountKey {
                magazine: 0,
                reserve: 450,
            },
        );
        assert!(lit(&empty, EMPTY) > 0);
        assert_eq!(lit(&empty, TEXT), 0);
    }
}
