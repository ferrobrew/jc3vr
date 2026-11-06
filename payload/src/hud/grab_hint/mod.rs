//! The grab hint: while a hand is within reach of a slot on the body, what its grip would take,
//! shown as the game's own icon in front of the player at the HUD's distance, so it reads while the
//! hand reaches over the shoulder or to the hip, where neither the slot nor the hand is in view.
//!
//! A take shows the item's icon: the weapon docked on a socket, the grenade or the C4 charge from
//! its pouch, or the gun a magazine from the belt pouch is for. Letting a held weapon go over a
//! taken socket on the back swaps it for the one docked there, which shows the weapon going in, an
//! arrow, and the weapon coming out, dimmed ([`paint`]). Above the middle of either, a small label
//! names the socket or pouch. The hint has no backing, only a soft dark halo. Each hand has its
//! own hint; two at once (each hand armed its own) sit apart toward their own hands' sides, the
//! left hand's on the left ([`layout`]).
//!
//! The weapon handling's input tick publishes each hand's hint ([`publish`]), with the icons named
//! by weapon id. The draw reads the icons from the archives ([`crate::hud::game_icons`]), paints a
//! texture per hand when its hint changes, and draws it on the panel's plane after the panel, with
//! a short fade. A hint whose icons are not all loaded is not drawn: it never falls back to text.

mod layout;
mod paint;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use glam::Vec4;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
pub(crate) use paint::{
    MARKER_TEXELS, TAG_TEXELS, marker as paint_marker, tag as paint_tag, tag_fits,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{frame::Hand, loadout::Socket, scheme, summary::glance::fade},
    hud::{
        game_icons::{self, IconId, Image, Lookup},
        grab_hint::{
            layout::{hint_corners, panel_frame},
            paint::{SWAP_TEXELS, TAKE_TEXELS},
        },
        log_once::LogOnce,
        quad::HudQuad,
        target::{HudTarget, KeyedTexture},
        world_quads_active,
    },
};

/// What one hand's grip would take, or swap, and where on the body.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Hint {
    /// The grip takes the item with this icon from `place`.
    Take { icon: IconId, place: Place },
    /// Letting go docks the held weapon (`incoming`) on `place`, and the weapon docked there
    /// (`outgoing`) falls clear.
    Swap {
        incoming: IconId,
        outgoing: IconId,
        place: Place,
    },
}

impl Hint {
    fn icons(self) -> impl Iterator<Item = IconId> {
        let (first, second) = match self {
            Hint::Take { icon, .. } => (icon, None),
            Hint::Swap {
                incoming, outgoing, ..
            } => (incoming, Some(outgoing)),
        };
        std::iter::once(first).chain(second)
    }

    /// The name of the place the hint is about, drawn above it.
    fn label(self) -> &'static str {
        match self {
            Hint::Take { place, .. } | Hint::Swap { place, .. } => place.name(),
        }
    }
}

/// A place on the body a hand takes from or docks on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Socket(Socket),
    GrenadePouch,
    C4Pouch,
    MagazinePouch,
}

impl Place {
    /// The place's name as the hint shows it, in sentence case.
    pub const fn name(self) -> &'static str {
        match self {
            Place::Socket(socket) => socket.display_name(),
            Place::GrenadePouch => "Grenade pouch",
            Place::C4Pouch => "C4 pouch",
            Place::MagazinePouch => "Magazine pouch",
        }
    }
}

/// Publish each hand's hint, by [`Hand::index`]. Called each input tick of the weapon handling.
pub(crate) fn publish(hints: [Option<Hint>; 2]) {
    *SAMPLE.lock() = Some((hints, Instant::now()));
}

/// Forget the hints: the weapon handling stood down.
pub(crate) fn clear() {
    *SAMPLE.lock() = None;
}

/// Draw the hints for `eye` over `target`, on the plane of the panel drawn this frame (`panel`, its
/// corners). Called from the HUD's draw after the panel, with the engine context mutex held.
pub(crate) fn draw(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
    panel: Option<[Vec4; 4]>,
) {
    let mut state = STATE.lock();
    if eye == 0 {
        let cfg = Config::lock_query(|c| c.hud);
        let active = cfg.grab_hint.enabled && world_quads_active(&cfg) && scheme::is_controllers();
        state.prepare(context, device, active.then_some(panel).flatten());
    }
    state.draw(context, device, target);
}

/// How long a published hint stays current without a newer one: the input tick stopped publishing
/// (a menu, or the weapon handling skipping its tick), so the hint no longer says what a grip does.
const STALE: Duration = Duration::from_millis(250);

/// The hint's fade in and out, seconds.
const FADE_S: f32 = 0.1;

static SAMPLE: Mutex<Option<([Option<Hint>; 2], Instant)>> = Mutex::new(None);

static STATE: Mutex<State> = Mutex::new(State::new());

struct State {
    last_prepare: Option<Instant>,
    hands: [HandHint; 2],
    gpu: Option<Gpu>,
}

/// One hand's hint as drawn: the hint painted (kept through the fade-out), its opacity, and its
/// corners this frame.
#[derive(Copy, Clone)]
struct HandHint {
    shown: Option<Hint>,
    alpha: f32,
    corners: Option<[Vec4; 4]>,
}

impl HandHint {
    const fn new() -> Self {
        Self {
            shown: None,
            alpha: 0.0,
            corners: None,
        }
    }
}

/// The quad pass and each hand's take and swap textures, created on the first active frame.
struct Gpu {
    quad: HudQuad,
    textures: [HintTextures; 2],
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            quad: HudQuad::new(device)?,
            textures: [HintTextures::new(device)?, HintTextures::new(device)?],
        })
    }
}

/// A hand's textures: one sized for a take and one for a swap, each holding the last hint of its
/// kind painted into it.
struct HintTextures {
    take: KeyedTexture<Hint>,
    swap: KeyedTexture<Hint>,
}

impl HintTextures {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            take: KeyedTexture::new(device, TAKE_TEXELS)
                .context("creating a grab hint's take texture")?,
            swap: KeyedTexture::new(device, SWAP_TEXELS)
                .context("creating a grab hint's swap texture")?,
        })
    }

    /// The texture `hint` is drawn from.
    fn target(&self, hint: Hint) -> &HudTarget {
        match hint {
            Hint::Take { .. } => self.take.target(),
            Hint::Swap { .. } => self.swap.target(),
        }
    }

    /// Paint `hint` from its loaded `icons`, if its texture does not show it already.
    fn paint(&mut self, context: &ID3D11DeviceContext, hint: Hint, icons: &[Arc<Image>]) {
        match (hint, icons) {
            (Hint::Take { .. }, [icon]) => self
                .take
                .update(context, hint, |_| paint::take(icon, hint.label())),
            (Hint::Swap { .. }, [incoming, outgoing]) => self.swap.update(context, hint, |_| {
                paint::swap(incoming, outgoing, hint.label())
            }),
            _ => {}
        }
    }
}

impl State {
    const fn new() -> Self {
        Self {
            last_prepare: None,
            hands: [HandHint::new(); 2],
            gpu: None,
        }
    }

    /// Eye 0: advance each hand's hint and fade, repaint what changed, and place the quads on the
    /// panel's plane (`panel`; `None` while the hints are not drawn).
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        panel: Option<[Vec4; 4]>,
    ) {
        let now = Instant::now();
        let dt = self
            .last_prepare
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        self.last_prepare = Some(now);
        let Some(frame) = panel.as_ref().and_then(panel_frame) else {
            self.hands = [HandHint::new(); 2];
            return;
        };
        let wanted = SAMPLE
            .lock()
            .filter(|(_, at)| now.duration_since(*at) <= STALE)
            .map_or([None; 2], |(hints, _)| hints);
        if wanted.iter().all(Option::is_none) && self.hands.iter().all(|h| h.alpha <= 0.0) {
            self.hands = [HandHint::new(); 2];
            return;
        }
        if self.gpu.is_none() {
            match Gpu::new(device) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => {
                    FAILURES.error(&e);
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        for hand in Hand::BOTH {
            let state = &mut self.hands[hand.index()];
            // A hint is drawn once all its icons are loaded; until then, or for a missing icon, the
            // hand shows nothing new.
            let ready = wanted[hand.index()].and_then(|hint| {
                let icons: Option<Vec<_>> = hint
                    .icons()
                    .map(|icon| match game_icons::lookup(icon) {
                        Lookup::Ready(image) => Some(image),
                        Lookup::Pending | Lookup::Missing => None,
                    })
                    .collect();
                Some((hint, icons?))
            });
            if let Some((hint, icons)) = &ready {
                gpu.textures[hand.index()].paint(context, *hint, icons);
                state.shown = Some(*hint);
            }
            state.alpha = fade(state.alpha, ready.is_some(), dt, FADE_S);
            if state.alpha <= 0.0 {
                state.shown = None;
            }
        }
        let sizes = self.hands.map(|state| {
            state.shown.map(|hint| match hint {
                Hint::Take { .. } => TAKE_TEXELS,
                Hint::Swap { .. } => SWAP_TEXELS,
            })
        });
        let corners = hint_corners(&frame, sizes);
        for (state, corners) in self.hands.iter_mut().zip(corners) {
            state.corners = corners;
        }
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        for (state, textures) in self.hands.iter().zip(&gpu.textures) {
            let (Some(hint), Some(corners)) = (state.shown, state.corners) else {
                continue;
            };
            if state.alpha > 0.0 {
                gpu.quad.draw_faded(
                    context,
                    device,
                    target,
                    textures.target(hint).color_srv(),
                    &corners,
                    state.alpha,
                );
            }
        }
    }
}

static FAILURES: LogOnce = LogOnce::new("hud grab hint");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::label;

    /// Every place's name fits across a take's texture, the narrower of the two, with room for its
    /// halo.
    #[test]
    fn every_place_name_fits_its_texture() {
        let places = Socket::ALL.map(Place::Socket).into_iter().chain([
            Place::GrenadePouch,
            Place::C4Pouch,
            Place::MagazinePouch,
        ]);
        for place in places {
            let name = place.name();
            assert!(name.starts_with(|c: char| c.is_uppercase()), "{name}");
            let width = label::rasterise(name, paint::LABEL_PX).width;
            assert!(width + 16 <= TAKE_TEXELS.0, "{name}: {width}");
        }
    }
}
