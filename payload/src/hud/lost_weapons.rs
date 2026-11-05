//! Markers over the weapons the player let go of: each weapon the loadout table has lying in the
//! world (dropped, thrown, knocked from a hand, ejected by a swap, or brought back to the feet)
//! gets the game's own icon for it above where it lies, so a weapon thrown or dropped in the heat of
//! a fight can be found again.
//!
//! The game's own pickup hints are no help here: they mark only the nearest useful weapon per slot,
//! and only while that slot is low on ammunition (`docs/engine/rendering/ui-rendering.md`). The
//! marker is the icon those hints show ([`crate::hud::game_icons`]), on the grab hint's backing
//! ([`crate::hud::grab_hint`]), drawn by the mod as an upright billboard at the weapon's own world
//! point. Like every HUD quad it is drawn without a depth test, at a stereo-correct depth, so it
//! shows through what is in front of the weapon and still reads as being where the weapon is.
//!
//! The inventory's input tick publishes the weapons and their places ([`publish`]); the marker is
//! kept for as long as the table keeps the weapon, which forgets one left far away (the pins'
//! forget distance). It fades in past arm's length, where the grip's own markers take over, keeps
//! a constant angular size, and only the nearest few are drawn.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use glam::{Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::scheme,
    hud::{
        HudMode,
        anchored::place::upright_corners,
        game_icons::{self, IconId, Lookup},
        grab_hint::{TAKE_TEXELS, paint_take},
        log_once::LogOnce,
        quad::HudQuad,
        target::HudTarget,
    },
};

/// A weapon the table has lying in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct LostWeapon {
    /// Where it lies, in the world.
    pub position: Vec3,
    pub icon: IconId,
}

/// Publish the weapons lying in the world. Called each input tick of the inventory.
pub(crate) fn publish(weapons: Vec<LostWeapon>) {
    *SAMPLE.lock() = Some((weapons, Instant::now()));
}

/// Draw the markers for `eye` over `target`. Called from the HUD's draw, with the engine context
/// mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut state = STATE.lock();
    if eye == 0 {
        let cfg = Config::lock_query(|c| c.hud);
        let active = cfg.lost_weapons.enabled
            && cfg.redirect
            && cfg.quad
            && crate::hud::current_mode() == HudMode::Hud
            && scheme::is_controllers()
            && !crate::director::clean_view::hides_everything();
        let head = active.then(crate::hud::render_camera_pose).flatten();
        state.prepare(context, device, head);
    }
    state.draw(context, device, target);
}

/// One marker as placed this frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Placed {
    pub icon: IconId,
    pub corners: [Vec3; 4],
    pub alpha: f32,
}

/// Place the markers for `weapons` seen from `head` (with `head_up`, the head's up): the nearest
/// [`MAX_DRAWN`] beyond [`NEAR_M`], each an upright card [`ANGULAR_HEIGHT`] tall at its distance,
/// its lower edge [`RAISE_M`] above the weapon, fading in over [`FADE_M`] past [`NEAR_M`]. The
/// nearest is first.
pub(crate) fn place(head: Vec3, head_up: Vec3, weapons: &[LostWeapon]) -> Vec<Placed> {
    let mut near: Vec<(f32, &LostWeapon)> = weapons
        .iter()
        .filter(|w| w.position.is_finite())
        .map(|w| (w.position.distance(head), w))
        .filter(|(distance, _)| *distance > NEAR_M)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter()
        .take(MAX_DRAWN)
        .filter_map(|(distance, weapon)| {
            let height = distance * ANGULAR_HEIGHT;
            let width = height * TAKE_TEXELS.0 as f32 / TAKE_TEXELS.1 as f32;
            let centre = weapon.position + Vec3::Y * (RAISE_M + height * 0.5);
            Some(Placed {
                icon: weapon.icon,
                corners: upright_corners(centre, head, head_up, width, height)?,
                alpha: ((distance - NEAR_M) / FADE_M).clamp(0.0, 1.0),
            })
        })
        .collect()
}

/// A marker is not drawn this near the eye, metres: within reach the grip's own markers show the
/// weapon.
const NEAR_M: f32 = 1.5;

/// How far past [`NEAR_M`] a marker takes to fade in, metres.
const FADE_M: f32 = 1.0;

/// The most markers drawn, the nearest first.
const MAX_DRAWN: usize = 8;

/// A marker's height over its distance: its angular height, radians.
const ANGULAR_HEIGHT: f32 = 0.03;

/// How far above the weapon a marker's lower edge sits, metres.
const RAISE_M: f32 = 0.3;

/// The most distinct icons kept as textures.
const MAX_TEXTURES: usize = 16;

/// How long a published list stays current without a newer one.
const STALE: Duration = Duration::from_millis(500);

static SAMPLE: Mutex<Option<(Vec<LostWeapon>, Instant)>> = Mutex::new(None);

static STATE: Mutex<State> = Mutex::new(State {
    placed: Vec::new(),
    gpu: None,
});

struct State {
    placed: Vec<Placed>,
    gpu: Option<Gpu>,
}

/// The quad pass and a painted texture per icon, created on the first frame with a marker.
struct Gpu {
    quad: HudQuad,
    textures: HashMap<IconId, HudTarget>,
}

impl State {
    /// Eye 0: place the markers seen from `head`, and paint the icons not painted yet.
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        head: Option<(Vec3, glam::Quat)>,
    ) {
        self.placed.clear();
        let Some((head, rotation)) = head else {
            return;
        };
        let now = Instant::now();
        let placed = {
            let sample = SAMPLE.lock();
            let Some((weapons, _)) = sample
                .as_ref()
                .filter(|(_, at)| now.duration_since(*at) <= STALE)
            else {
                return;
            };
            place(head, rotation * Vec3::Y, weapons)
        };
        if placed.is_empty() {
            return;
        }
        if self.gpu.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => {
                    self.gpu = Some(Gpu {
                        quad,
                        textures: HashMap::new(),
                    })
                }
                Err(e) => {
                    FAILURES.error(&e);
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        for marker in placed {
            if !gpu.textures.contains_key(&marker.icon) {
                // An icon still loading, or missing, has no marker: never a stand-in.
                let Lookup::Ready(image) = game_icons::lookup(marker.icon) else {
                    continue;
                };
                if gpu.textures.len() >= MAX_TEXTURES {
                    continue;
                }
                match paint(context, device, &paint_take(&image)) {
                    Ok(texture) => {
                        gpu.textures.insert(marker.icon, texture);
                    }
                    Err(e) => {
                        FAILURES.error(&e);
                        continue;
                    }
                }
            }
            self.placed.push(marker);
        }
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        // The farthest first, so a nearer card covers a farther one.
        for marker in self.placed.iter().rev() {
            let Some(texture) = gpu.textures.get(&marker.icon) else {
                continue;
            };
            let corners: [Vec4; 4] = marker.corners.map(|c| c.extend(1.0));
            gpu.quad.draw_faded(
                context,
                device,
                target,
                texture.color_srv(),
                &corners,
                marker.alpha,
            );
        }
    }
}

/// A texture of [`TAKE_TEXELS`] holding `texels`.
fn paint(
    context: &ID3D11DeviceContext,
    device: &Device,
    texels: &[u8],
) -> anyhow::Result<HudTarget> {
    let (width, height) = TAKE_TEXELS;
    let target =
        HudTarget::new(device, width, height).context("creating a lost weapon marker's texture")?;
    target.upload(context, texels);
    Ok(target)
}

static FAILURES: LogOnce = LogOnce::new("hud lost weapons");

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon(x: f32, z: f32, icon: i16) -> LostWeapon {
        LostWeapon {
            position: Vec3::new(x, 0.0, z),
            icon: IconId(icon),
        }
    }

    #[test]
    fn markers_skip_the_reach_and_order_the_nearest_first() {
        // Standing at the origin, the eye a little above the ground the weapons lie on.
        let head = Vec3::new(0.0, 0.5, 0.0);
        let placed = place(
            head,
            Vec3::Y,
            &[
                weapon(0.0, -20.0, 1),
                weapon(0.3, -0.3, 2),
                weapon(0.0, -5.0, 3),
            ],
        );
        let icons: Vec<i16> = placed.iter().map(|p| p.icon.0).collect();
        assert_eq!(icons, [3, 1]);
        assert!(placed.iter().all(|p| p.alpha == 1.0));
    }

    #[test]
    fn a_marker_keeps_its_angular_size_and_sits_above_the_weapon() {
        let head = Vec3::new(0.0, 0.0, 0.0);
        for distance in [4.0f32, 40.0] {
            let placed = place(head, Vec3::Y, &[weapon(0.0, -distance, 7)]);
            let corners = placed[0].corners;
            let height = (corners[0] - corners[2]).length();
            assert!(
                (height / distance - ANGULAR_HEIGHT).abs() < 1e-3,
                "{distance}"
            );
            let bottom = corners[2].y.min(corners[3].y);
            assert!(bottom >= RAISE_M - 1e-3);
        }
    }

    #[test]
    fn a_marker_fades_in_past_arms_length() {
        let head = Vec3::ZERO;
        let half = place(head, Vec3::Y, &[weapon(0.0, -(NEAR_M + FADE_M * 0.5), 1)]);
        assert!((half[0].alpha - 0.5).abs() < 1e-3);
        assert!(place(head, Vec3::Y, &[weapon(f32::NAN, 0.0, 1)]).is_empty());
    }

    #[test]
    fn only_the_nearest_few_are_drawn() {
        let weapons: Vec<LostWeapon> = (0..20).map(|i| weapon(0.0, -3.0 - i as f32, 1)).collect();
        assert_eq!(place(Vec3::ZERO, Vec3::Y, &weapons).len(), MAX_DRAWN);
    }
}
