//! Scoped weapons: the plan of record's "Scopes" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`suppress`]: the game's precision aim (its camera zoom, its sniper overlay, and its hiding of
//!   the character) refused under the controller scheme;
//! - [`lens`]: the lens's geometry: the eyepiece on the sight line, the sighting eye, and the
//!   magnification;
//! - [`renderer`]: the D3D11 pipeline that copies the eyes' images and draws the lens.
//!
//! The lens is a magnified resample of a rendered eye image, drawn as a disc at the scope's eyepiece
//! while an eye is within [`ScopeConfig::max_eye_distance_m`] of it. The scene is not rendered a
//! third time: the scope is at the eye and looks along almost the same line, so the region around
//! the sight line's projection, magnified, is the scope's view, correct in parallax at the aim
//! point. The sighting eye's own image has the scope's model in front of that region, so by default
//! the lens reads the other eye's image, which the scope does not block
//! ([`LensSource::OtherEye`](crate::controllers::weapons::config::LensSource::OtherEye)). The narrow-field scene render
//! ([`LensPath::SceneRender`](crate::controllers::weapons::config::LensPath::SceneRender)) is not
//! built, and selecting it uses the resample.
//!
//! The frame runs in three steps at the end of `RenderEngine::PostDraw`, beside the world-space
//! overlay:
//!
//! 1. [`tick`], once per frame on the first eye, outside the engine's context lock: the scoped
//!    weapon, its sight line anchored to the holding wrist, the lens placed from the render-time
//!    wrist, and the sighting eye;
//! 2. [`capture`], every dispatch, before the overlay draws: the dispatch's eye images copied while a
//!    scoped weapon is held;
//! 3. [`draw`], for each eye after the overlay: the lens, on the sighting eye only.
//!
//! The scoped weapon is identified on the input tick ([`input_tick`]) by its class, the sniper
//! class, which is what the game gives its scoped rifles.

pub mod lens;
mod renderer;
pub mod suppress;

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponClass},
    graphics_engine::{device::Device, texture::Texture},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext};

use crate::{
    config::Config,
    controllers::{
        aim::{self, HandSource, sights::SightSource},
        frame::Hand,
        hands::late,
        overlay::view::{self, EyeView},
        scheme,
        weapons::{
            config::{LensPath, ScopeConfig},
            scope::{
                lens::{EyeCamera, Lens, SightAnchor},
                renderer::{EyeRegion, Renderer},
            },
        },
    },
    hooks,
    stereo::{STEREO_STATE, single_pass::collapse_active},
};

/// The scope hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    suppress::hook_library()
}

/// Why no lens is drawn.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LensIdle {
    /// The controllers or the lens are off.
    Disabled,
    /// The gamepad scheme is in effect, outside gameplay, or in cinematic mode.
    Inactive,
    /// The wielded weapon is not a scoped one.
    NoScope,
    /// The scoped weapon's sight line is not aimed by a tracked hand.
    NoSightLine,
    /// No eye is at the eyepiece.
    NotAtEye,
}

impl LensIdle {
    pub fn label(self) -> &'static str {
        match self {
            LensIdle::Disabled => "off",
            LensIdle::Inactive => "gamepad scheme, outside gameplay, or cinematic",
            LensIdle::NoScope => "no scoped weapon",
            LensIdle::NoSightLine => "no sight line",
            LensIdle::NotAtEye => "no eye at the eyepiece",
        }
    }
}

/// The scoped weapon as the lens sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ScopeReadout {
    /// The weapon type's name hash.
    pub hash: u32,
    /// The hand that holds it.
    pub hand: Hand,
    /// Where its sight correction comes from; on the fire point the sight line runs along the
    /// barrel, below the scope.
    pub sight: SightSource,
}

/// The scopes' readout for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScopeStatus {
    /// The scoped weapon, when one is held and aimed.
    pub scope: Option<ScopeReadout>,
    /// The nearest eye's distance from the eyepiece, metres.
    pub eyepiece_distance_m: Option<f32>,
    /// Why no lens is drawn, when none is.
    pub idle: Option<LensIdle>,
    /// The eye that sights through the lens and the eye whose image it reads.
    pub eyes: Option<(usize, usize)>,
    /// The scene render was asked for; the resample is drawn instead.
    pub scene_render_fallback: bool,
    /// Frames the lens was drawn in, since the payload loaded.
    pub frames: u64,
    /// The last failure to build the pipeline, copy an eye, or draw.
    pub error: Option<String>,
}

/// The latest readout.
pub fn status() -> ScopeStatus {
    STATUS.lock().clone()
}

/// Identify the wielded weapon, at `weapon`, as scoped or not. Called on the input tick, on the main
/// thread outside the world update, while the bindings inject.
pub(crate) fn input_tick(weapon: Option<usize>) {
    // SAFETY: the address is the local player's wielded weapon, read on the main thread outside
    // the world update; the class is read as its raw value.
    let scoped = weapon.filter(|&address| {
        unsafe { (address as *const WeaponBase).as_ref() }.is_some_and(|w| {
            let class = unsafe { (&raw const w.m_Class).cast::<i32>().read() };
            class == WeaponClass::E_WEAPON_CLASS_SNIPER as i32
        })
    });
    *SCOPED.lock() = scoped.map(|address| (address, Instant::now()));
}

/// Plan the frame's lens. Called once per render dispatch at the end of `RenderEngine::PostDraw`,
/// before the engine's context lock is taken; it acts only on the frame's first eye.
pub(crate) fn tick(eye: usize) {
    if eye != 0 || crate::stereo::far_phase() {
        return;
    }
    let (enabled, config) = Config::lock_query(|c| {
        (
            c.controllers.enabled && c.controllers.weapons.scopes,
            c.controllers.weapons.scope,
        )
    });
    let mut state = STATE.lock();
    let plan = plan(enabled, &config, &mut state.anchor);
    let mut status = STATUS.lock();
    status.scope = plan.scope;
    status.eyepiece_distance_m = plan.eyepiece_distance_m;
    status.idle = plan.idle;
    status.scene_render_fallback = config.path == LensPath::SceneRender;
    if plan.idle.is_some() {
        status.eyes = None;
    }
    state.plan = plan;
    state.config = config;
}

/// Copy the dispatch's eye images for the lens while a scoped weapon is held. Called under the
/// engine's context lock, before the world-space overlay draws, with `target` the dispatch's final
/// back buffer.
pub(crate) fn capture(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    if crate::stereo::far_phase() {
        return;
    }
    let mut state = STATE.lock();
    if !state.plan.armed {
        return;
    }
    let Some(renderer) = state.renderer(device) else {
        return;
    };
    let width = u32::from(target.m_Width);
    let result = if collapse_active() {
        let half = width / 2;
        (0..2).try_for_each(|e| {
            let Some(camera) = collapse_camera(e) else {
                return Ok(());
            };
            let region = EyeRegion {
                left: e as u32 * half,
                width: half,
            };
            renderer.capture(&device.m_Device, context, target, region, e, camera)
        })
    } else {
        match view::current(eye) {
            Some((v, _)) => renderer.capture(
                &device.m_Device,
                context,
                target,
                EyeRegion { left: 0, width },
                eye,
                camera_of(&v),
            ),
            None => Ok(()),
        }
    };
    if let Err(error) = result {
        report(error);
    }
}

/// Draw the lens onto `target` for `eye`, when `eye` sights through it. Called under the engine's
/// context lock after the world-space overlay; under the collapse, the caller has selected the eye
/// with `set_collapse_ui_eye`.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if crate::stereo::far_phase() || crate::director::cinematic::active() {
        return;
    }
    let mut state = STATE.lock();
    let (Some(lens), Some(sighting)) = (state.plan.lens, state.plan.sighting) else {
        return;
    };
    if sighting != eye {
        return;
    }
    let config = state.config;
    let Some((v, viewport)) = view::current(eye) else {
        return;
    };
    let Some(renderer) = state.renderer(device) else {
        return;
    };
    let fresh = |e: usize| {
        renderer
            .copy(e)
            .filter(|c| c.taken.elapsed() < MAX_COPY_AGE)
            .map(|c| c.camera)
    };
    let other = fresh(1 - eye.min(1));
    let source = lens::source_eye(eye, config.source, other.is_some());
    let source_camera = if source == eye {
        // The own image is this dispatch's copy, taken with this eye's camera.
        fresh(eye).map(|_| camera_of(&v))
    } else {
        other
    };
    let Some(source_camera) = source_camera else {
        return;
    };
    let constants = lens::constants(&lens, &camera_of(&v), &source_camera, &config);
    let viewport = viewport.unwrap_or(D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: f32::from(target.m_Width),
        Height: f32::from(target.m_Height),
        MinDepth: 0.0,
        MaxDepth: 1.0,
    });
    let drawn = renderer.draw(
        &device.m_Device,
        context,
        target,
        viewport,
        &constants,
        source,
    );
    drop(state);
    match drawn {
        Ok(()) => {
            let mut status = STATUS.lock();
            status.eyes = Some((eye, source));
            status.frames += 1;
        }
        Err(error) => report(error),
    }
}

/// The frame's lens plan.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Plan {
    /// Whether a scoped weapon is held and aimed, so the eyes' images are copied.
    armed: bool,
    /// The lens, while armed.
    lens: Option<Lens>,
    /// The eye at the eyepiece.
    sighting: Option<usize>,
    scope: Option<ScopeReadout>,
    eyepiece_distance_m: Option<f32>,
    idle: Option<LensIdle>,
}

struct State {
    renderer: Option<Renderer>,
    /// Set when the pipeline could not be built, so that it is not retried every frame.
    failed: bool,
    /// The sight line in the holding wrist's frame, and the aim update it was taken from.
    anchor: Option<(u64, SightAnchor)>,
    plan: Plan,
    config: ScopeConfig,
}

impl State {
    /// The pipeline, built on first use.
    fn renderer(&mut self, device: &Device) -> Option<&mut Renderer> {
        if self.failed {
            return None;
        }
        if self.renderer.is_none() {
            match Renderer::new(&device.m_Device) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    self.failed = true;
                    report(error);
                    return None;
                }
            }
        }
        self.renderer.as_mut()
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    renderer: None,
    failed: false,
    anchor: None,
    plan: Plan {
        armed: false,
        lens: None,
        sighting: None,
        scope: None,
        eyepiece_distance_m: None,
        idle: Some(LensIdle::Disabled),
    },
    config: ScopeConfig::new(),
});

static STATUS: Mutex<ScopeStatus> = Mutex::new(ScopeStatus {
    scope: None,
    eyepiece_distance_m: None,
    idle: None,
    eyes: None,
    scene_render_fallback: false,
    frames: 0,
    error: None,
});

/// The wielded weapon the last input tick found scoped, and when.
static SCOPED: Mutex<Option<(usize, Instant)>> = Mutex::new(None);

/// How long an input tick's identification of the scoped weapon stands. The input tick runs at the
/// simulation rate while the bindings inject, so an older one belongs to a menu or the gamepad
/// scheme.
const MAX_SCOPED_AGE: Duration = Duration::from_millis(250);

/// How old an eye's copy may be and still be read: a frame or two at any usable frame rate. The
/// other eye's copy is the previous frame's when the first eye sights in two-pass stereo.
const MAX_COPY_AGE: Duration = Duration::from_millis(100);

/// Plan the frame's lens, re-anchoring the sight line to the holding wrist when the aim has run
/// since the anchor was taken.
fn plan(enabled: bool, config: &ScopeConfig, anchor: &mut Option<(u64, SightAnchor)>) -> Plan {
    let idle = |idle| Plan {
        idle: Some(idle),
        ..Plan::default()
    };
    if !enabled {
        return idle(LensIdle::Disabled);
    }
    if !scheme::is_controllers() || !hooks::in_gameplay() || crate::director::cinematic::active() {
        return idle(LensIdle::Inactive);
    }
    let scoped = SCOPED
        .lock()
        .filter(|(_, at)| at.elapsed() < MAX_SCOPED_AGE)
        .map(|(address, _)| address);
    let aim = aim::status();
    let Some(weapon) = aim.weapon.filter(|w| Some(w.address) == scoped) else {
        *anchor = None;
        return idle(LensIdle::NoScope);
    };
    let Some(hand) = weapon.gun_hands.iter().flatten().next().copied() else {
        return idle(LensIdle::NoSightLine);
    };
    let scope = Some(ScopeReadout {
        hash: weapon.hash,
        hand,
        sight: weapon.sight.source,
    });
    let dtf = hooks::camera::last_dtf();
    let Some(wrist) = late::render_hands()[hand.index()].map(|h| h.wrist.at(dtf)) else {
        return Plan {
            scope,
            ..idle(LensIdle::NoSightLine)
        };
    };
    let hand_aim = aim.hands[hand.index()].filter(|h| matches!(h.source, HandSource::Gun { .. }));
    if let Some(h) = hand_aim
        && anchor.is_none_or(|(updates, a)| updates != aim.updates || a.hand != hand)
    {
        let focus = h.ray.origin.distance(h.target);
        *anchor = Some((aim.updates, SightAnchor::new(hand, &h.ray, wrist, focus)));
    }
    let Some((_, a)) = anchor.filter(|(_, a)| a.hand == hand) else {
        return Plan {
            scope,
            ..idle(LensIdle::NoSightLine)
        };
    };
    let Some(sight) = a.place(wrist) else {
        return Plan {
            scope,
            ..idle(LensIdle::NoSightLine)
        };
    };
    let lens = Lens::new(&sight, a.focus_m, config);
    let eyes = eye_positions();
    let eyepiece_distance_m = lens::nearest_eye(lens.eyepiece, eyes).map(|(_, d)| d);
    let sighting = lens::sighting_eye(&lens, eyes, config.max_eye_distance_m);
    Plan {
        armed: true,
        lens: Some(lens),
        sighting,
        scope,
        eyepiece_distance_m,
        idle: sighting.is_none().then_some(LensIdle::NotAtEye),
    }
}

/// Both eyes' world positions this frame: the centre camera moved by each eye's offset. `None`
/// for an eye without a VR frame's render parameters.
fn eye_positions() -> [Option<Vec3>; 2] {
    let Some((head, _)) = crate::hud::render_camera_pose() else {
        return [None; 2];
    };
    [0, 1].map(|eye| crate::vr::render_params(eye).map(|p| head + p.world_offset))
}

/// An eye's camera under the collapse, which keeps the render camera centred.
fn collapse_camera(eye: usize) -> Option<EyeCamera> {
    let center = STEREO_STATE.lock().center_transform?;
    let params = crate::vr::render_params(eye)?;
    let eye_world = view::collapse_eye_world(
        Mat4::from(center),
        params.world_offset,
        params.orientation_delta,
    );
    Some(camera_of(&EyeView::from_eye_world(
        eye_world,
        Mat4::from(params.projection_reverse_z),
    )))
}

fn camera_of(view: &EyeView) -> EyeCamera {
    EyeCamera {
        position: view.position,
        offset_view_projection: view.offset_view_projection,
    }
}

fn report(error: anyhow::Error) {
    let message = format!("{error:#}");
    let mut status = STATUS.lock();
    if status.error.as_deref() != Some(message.as_str()) {
        tracing::error!("{message}");
    }
    status.error = Some(message);
}
