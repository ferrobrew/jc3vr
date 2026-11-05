//! The grab hint: while a hand is within reach of a slot on the body, what its grip would take,
//! shown as the game's own icon in front of the player at the HUD's distance, so it reads while the
//! hand reaches over the shoulder or to the hip, where neither the slot nor the hand is in view.
//!
//! A take shows the item's icon: the weapon docked on a socket, the grenade or the C4 charge from
//! its pouch, or the gun a magazine from the belt pouch is for. Letting a held weapon go over a
//! taken socket on the back swaps it for the one docked there, which shows the weapon going in, an
//! arrow, and the weapon coming out, dimmed ([`paint`]). Each hand has its own hint; two sit side
//! by side, the left hand's on the left ([`layout`]).
//!
//! The weapon handling's input tick publishes each hand's hint ([`publish`]), with the icons named
//! by weapon id. The draw reads the icons from the archives ([`crate::hud::game_icons`]), paints a
//! texture per hand when its hint changes, and draws it on the panel's plane after the panel, with
//! a short fade. A hint whose icons are not all loaded is not drawn: it never falls back to text.

mod layout;
mod paint;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context as _;
use glam::Vec4;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
pub(crate) use paint::{TAKE_TEXELS, take as paint_take};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{frame::Hand, scheme, summary::glance::fade},
    hud::{
        HudMode,
        game_icons::{self, IconId, Image, Lookup},
        grab_hint::{
            layout::{hint_corners, panel_frame},
            paint::SWAP_TEXELS,
        },
        quad::HudQuad,
        target::{HudTarget, KeyedTexture},
    },
};

/// What one hand's grip would take, or swap.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Hint {
    /// The grip takes the item with this icon.
    Take(IconId),
    /// Letting go docks the held weapon (`incoming`) and the weapon docked there (`outgoing`) falls
    /// clear.
    Swap { incoming: IconId, outgoing: IconId },
}

impl Hint {
    fn icons(self) -> impl Iterator<Item = IconId> {
        let (first, second) = match self {
            Hint::Take(icon) => (icon, None),
            Hint::Swap { incoming, outgoing } => (incoming, Some(outgoing)),
        };
        std::iter::once(first).chain(second)
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
        let active = cfg.grab_hint.enabled
            && cfg.redirect
            && cfg.quad
            && crate::hud::current_mode() == HudMode::Hud
            && scheme::is_controllers()
            && !crate::director::clean_view::hides_everything();
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
            Hint::Take(_) => self.take.target(),
            Hint::Swap { .. } => self.swap.target(),
        }
    }

    /// Paint `hint` from its loaded `icons`, if its texture does not show it already.
    fn paint(&mut self, context: &ID3D11DeviceContext, hint: Hint, icons: &[Arc<Image>]) {
        match (hint, icons) {
            (Hint::Take(_), [icon]) => self.take.update(context, hint, |_| paint::take(icon)),
            (Hint::Swap { .. }, [incoming, outgoing]) => self
                .swap
                .update(context, hint, |_| paint::swap(incoming, outgoing)),
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
                    log_once(&e);
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
        let aspects = self.hands.map(|state| {
            state.shown.map(|hint| {
                let (width, height) = match hint {
                    Hint::Take(_) => TAKE_TEXELS,
                    Hint::Swap { .. } => SWAP_TEXELS,
                };
                width as f32 / height as f32
            })
        });
        let corners = hint_corners(&frame, aspects);
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

/// Log a resource failure once per session; the next frame retries regardless.
fn log_once(error: &anyhow::Error) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("hud grab hint: {error:#}");
    }
}
