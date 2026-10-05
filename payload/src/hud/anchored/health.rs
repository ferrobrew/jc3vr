//! The wrist's health bar: the player character's health as a bar the mod draws, with a trail
//! that shows the last hit before it drains. The game has no compact health readout (its health is
//! the full-screen vignette, which the floating HUD suppresses), so the bar is the player's only
//! health display in VR.

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::hud::target::HudTarget;

/// The bar texture's size, texels. Small: the bar is a few centimetres on the wrist.
pub(crate) const BAR_TEXELS: (u32, u32) = (320, 32);

/// The trail behind the health: where the bar stood before the last hit, held briefly and then
/// drained toward the health, so a hit reads as the piece it took.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct HealthTrail {
    trail: Option<f32>,
    /// Seconds the trail has held since the last drop.
    held_s: f32,
}

impl HealthTrail {
    /// How long the trail holds after a hit before draining, seconds.
    pub const HOLD_S: f32 = 0.5;
    /// How fast the trail drains, in health fractions per second.
    pub const DRAIN_PER_S: f32 = 0.6;

    pub const fn new() -> Self {
        Self {
            trail: None,
            held_s: 0.0,
        }
    }

    /// Advance by `dt` seconds with the health fraction `health`, and return the trail, never below
    /// the health.
    pub fn step(&mut self, health: f32, dt: f32) -> f32 {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let trail = match self.trail {
            Some(trail) if trail > health => {
                if self.held_s < Self::HOLD_S {
                    self.held_s += dt;
                    trail
                } else {
                    (trail - Self::DRAIN_PER_S * dt).max(health)
                }
            }
            _ => {
                self.held_s = 0.0;
                health
            }
        };
        self.trail = Some(trail);
        trail
    }

    /// Restart the hold: health just dropped.
    pub fn hit(&mut self) {
        self.held_s = 0.0;
    }
}

/// What the bar shows, quantised to its texels, so it is repainted only when a texel changes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct BarKey {
    pub fill: u32,
    pub trail: u32,
    pub low: bool,
}

impl BarKey {
    /// The key for a bar of `width` texels showing `health` and `trail` (fractions), drawn as low
    /// when `low`.
    pub fn new(width: u32, health: f32, trail: f32, low: bool) -> Self {
        let inner = width.saturating_sub(2 * (BORDER + PAD));
        let texels = |f: f32| (f.clamp(0.0, 1.0) * inner as f32).round() as u32;
        let fill = texels(health);
        Self {
            fill,
            trail: texels(trail).max(fill),
            low,
        }
    }
}

/// Paint the bar into premultiplied RGBA texels, row by row: a dark translucent backing with a
/// light border, the health in white (red while low), and the trail in the HUD's accent red.
pub(crate) fn paint(width: u32, height: u32, key: BarKey) -> Vec<u8> {
    let mut texels = Vec::with_capacity((width * height * 4) as usize);
    let inset = BORDER + PAD;
    for y in 0..height {
        for x in 0..width {
            let border = x < BORDER || y < BORDER || x >= width - BORDER || y >= height - BORDER;
            let inside = x >= inset && y >= inset && x < width - inset && y < height - inset;
            let colour = if border {
                FRAME
            } else if inside && x - inset < key.fill {
                if key.low { LOW } else { FILL }
            } else if inside && x - inset < key.trail {
                TRAIL
            } else {
                BACKING
            };
            texels.extend_from_slice(&premultiply(colour));
        }
    }
    texels
}

/// The bar's texture, repainted when its key changes.
pub(crate) struct Bar {
    target: HudTarget,
    key: Option<BarKey>,
}

impl Bar {
    pub fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            target: HudTarget::new(device, BAR_TEXELS.0, BAR_TEXELS.1)
                .context("creating the health bar texture")?,
            key: None,
        })
    }

    pub fn target(&self) -> &HudTarget {
        &self.target
    }

    /// Repaint the bar for `key` if it changed.
    pub fn update(&mut self, context: &ID3D11DeviceContext, key: BarKey) {
        if self.key == Some(key) {
            return;
        }
        let (width, height) = BAR_TEXELS;
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

/// The frame's width, texels.
const BORDER: u32 = 2;
/// The gap between the frame and the bar, texels.
const PAD: u32 = 2;

/// Straight RGBA colours, premultiplied when painted.
const FRAME: [u8; 4] = [235, 235, 235, 220];
const BACKING: [u8; 4] = [10, 12, 15, 190];
const FILL: [u8; 4] = [240, 240, 240, 255];
const LOW: [u8; 4] = [225, 45, 35, 255];
const TRAIL: [u8; 4] = [200, 40, 30, 200];

fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let scale = |c: u8| ((u32::from(c) * u32::from(a) + 127) / 255) as u8;
    [scale(r), scale(g), scale(b), a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trail_holds_then_drains_to_the_health() {
        let mut trail = HealthTrail::new();
        assert_eq!(trail.step(1.0, 0.016), 1.0);
        trail.hit();
        // The hit: the trail holds where the bar stood.
        assert_eq!(trail.step(0.6, 0.1), 1.0);
        assert_eq!(trail.step(0.6, HealthTrail::HOLD_S), 1.0);
        // Then drains.
        let drained = trail.step(0.6, 0.1);
        assert!((drained - (1.0 - HealthTrail::DRAIN_PER_S * 0.1)).abs() < 1e-5);
        for _ in 0..100 {
            trail.step(0.6, 0.1);
        }
        assert_eq!(trail.step(0.6, 0.1), 0.6);
        // Healing moves the trail with the health.
        assert_eq!(trail.step(0.9, 0.1), 0.9);
    }

    #[test]
    fn the_key_quantises_to_texels() {
        let inner = (320 - 8) as f32;
        let key = BarKey::new(320, 0.5, 0.75, false);
        assert_eq!(key.fill, (0.5 * inner).round() as u32);
        assert_eq!(key.trail, (0.75 * inner).round() as u32);
        // The trail is never shorter than the fill, and fractions clamp.
        assert_eq!(
            BarKey::new(320, 0.8, 0.2, false).trail,
            BarKey::new(320, 0.8, 0.2, false).fill
        );
        assert_eq!(BarKey::new(320, 2.0, 2.0, false).fill, inner as u32);
    }

    fn texel(texels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        texels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn the_bar_paints_fill_trail_and_backing() {
        let (w, h) = (100, 12);
        let key = BarKey {
            fill: 40,
            trail: 60,
            low: false,
        };
        let texels = paint(w, h, key);
        assert_eq!(texels.len(), (w * h * 4) as usize);
        let mid = h / 2;
        assert_eq!(texel(&texels, w, 0, mid), premultiply(FRAME));
        assert_eq!(texel(&texels, w, 4 + 10, mid), premultiply(FILL));
        assert_eq!(texel(&texels, w, 4 + 50, mid), premultiply(TRAIL));
        assert_eq!(texel(&texels, w, 4 + 70, mid), premultiply(BACKING));
        let low = paint(w, h, BarKey { low: true, ..key });
        assert_eq!(texel(&low, w, 4 + 10, mid), premultiply(LOW));
    }

    /// Premultiplied texels never carry more colour than coverage.
    #[test]
    fn texels_are_premultiplied() {
        let texels = paint(
            64,
            8,
            BarKey {
                fill: 20,
                trail: 30,
                low: true,
            },
        );
        let (texels, rest) = texels.as_chunks::<4>();
        assert!(rest.is_empty());
        for [r, g, b, a] in texels {
            assert!(r <= a && g <= a && b <= a, "{r} {g} {b} {a}");
        }
    }
}
