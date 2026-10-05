//! The body-anchored HUD readouts, under the controller scheme: the weapon readout above the gun
//! hand, the tether readout above the off-hand's forearm, the health bar and heat readout on the
//! inside of the off-hand's wrist, and the weapon selection cross where the hand steering the
//! weapon wheel was when it opened.
//!
//! The readouts above the hand and the forearm stand upright and face the eye. They are drawn
//! without a depth test, like every HUD quad, so they are placed clear of the hand and the gun
//! rather than tested against them: a readout near the hand that the scene's depth could hide
//! would vanish into the gun or a wall the gun is pushed against.
//!
//! The weapon, tether, and heat readouts are the game's own clips, moved into atlas cells for the capture
//! ([`crate::hud::atlas`]) and drawn here from their cells' copies, so they keep the game's values,
//! icons, and fades: the game shows the weapon readout while a gun is out and fades it out when it
//! is holstered, and the quad carries that fade. The health bar is the mod's own
//! ([`health`]), from the player character's health, sampled at the capture seam.
//!
//! On eye 0 the anchors are placed from the render-time hand poses and the head
//! ([`place`]); both eyes then draw the same corners through the shared quad pass, after the
//! floating panel, since the readouts are nearer than it.

mod health;
pub(crate) mod place;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use glam::{Vec3, Vec4};
use jc3gi::{
    character::character::Character,
    graphics_engine::{device::Device, texture::Texture},
    heat_manager::HeatManager,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{
        aim,
        frame::{Hand, Pose},
        hands::late,
        scheme,
        summary::glance::{Glance, fade, looking},
        weapons::wheel::{self, CROSS_LINGER},
    },
    hooks,
    hud::{
        HudMode,
        anchored::{
            health::{BAR_TEXELS, Bar, BarKey, HealthTrail},
            place::{
                AmmoHand, Attention, ammo_anchor, facing_corners, fade_out, follow, forearm_anchor,
                gun_hand, inner_wrist_pose, pull_toward, stacked_corners, upright_corners,
                wants_attention, wrist_wanted,
            },
        },
        atlas::{self, Part},
        config::{AtlasConfig, HudConfig},
        quad::HudQuad,
    },
};

/// Sample the player's health and heat for the wrist (the capture seam, after the HUD's update on
/// the same thread, which reads the same state).
///
/// # Safety
/// Must be called from the `MovieImpl::Capture` detour for the main movie.
pub(crate) unsafe fn sample_status(cfg: &HudConfig) {
    let wanted =
        cfg.atlas.enabled && cfg.atlas.wrist.enabled && crate::hud::current_mode() == HudMode::Hud;
    let sample = wanted
        .then(|| {
            // SAFETY: the local player character and the heat manager are live in gameplay; the
            // HUD's own update reads both on this thread.
            unsafe {
                let character = Character::GetLocalPlayerCharacter().as_ref()?;
                Some(StatusSample {
                    health: character.GetHealth(),
                    max_health: character.m_MaxHealth,
                    heat_level: HeatManager::get().map_or(0, |heat| heat.m_Level),
                })
            }
        })
        .flatten();
    *SAMPLE.lock() = sample;
}

/// Draw the anchored readouts for `eye` over `target`. Called from the HUD's draw after the panel,
/// with the engine context mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut state = STATE.lock();
    if eye == 0 {
        let (cfg, dominant) = Config::lock_query(|c| (c.hud, c.controllers.dominant_hand));
        let active = cfg.atlas.enabled
            && cfg.redirect
            && cfg.quad
            && crate::hud::current_mode() == HudMode::Hud
            && scheme::is_controllers()
            && !crate::director::cinematic::active()
            && crate::vr::render_params(0).is_some();
        state.prepare(context, device, active, dominant, &cfg.atlas);
    }
    state.draw(context, device, target);
}

/// The readouts' state, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct AnchoredStatus {
    /// The hand the weapon readout rides, when it is drawn.
    pub ammo_hand: Option<Hand>,
    /// The health fraction and the trail, when sampled.
    pub health: Option<(f32, f32)>,
    pub heat_level: i32,
    pub wrist_alpha: f32,
    pub glance: bool,
    pub attention: bool,
    /// Whether the heat readout's cell was drawn.
    pub heat_drawn: bool,
    /// The selection cross's opacity, while it is drawn at the steering hand.
    pub cross_alpha: Option<f32>,
    /// Whether the tether readout's quad was placed above the forearm.
    pub tether_drawn: bool,
}

pub(crate) fn status() -> AnchoredStatus {
    STATE.lock().status
}

/// The player's state as the capture seam read it.
#[derive(Debug, Copy, Clone, PartialEq)]
struct StatusSample {
    health: i16,
    max_health: i16,
    heat_level: i32,
}

static SAMPLE: Mutex<Option<StatusSample>> = Mutex::new(None);

static STATE: Mutex<State> = Mutex::new(State::new());

/// How far the hand may jump in a frame before the weapon readout snaps instead of easing, metres.
const SNAP_M: f32 = 0.5;

/// The gap between the health bar and the heat readout on the wrist, metres.
const WRIST_GAP_M: f32 = 0.006;

struct State {
    last_prepare: Option<Instant>,
    ammo_hand: AmmoHand,
    ammo_position: Option<Vec3>,
    ammo_corners: Option<[Vec4; 4]>,
    glance: Glance,
    wrist_alpha: f32,
    trail: HealthTrail,
    last_health: Option<i16>,
    last_damage: Option<Instant>,
    bar_corners: Option<[Vec4; 4]>,
    heat_corners: Option<[Vec4; 4]>,
    /// The selection cross's place relative to the head, fixed when the wheel opens.
    cross_offset: Option<Vec3>,
    cross_open: bool,
    cross_corners: Option<[Vec4; 4]>,
    cross_alpha: f32,
    tether_position: Option<Vec3>,
    tether_corners: Option<[Vec4; 4]>,
    status: AnchoredStatus,
    gpu: Option<Gpu>,
}

/// The quad pass and the health bar's texture, created on the first active frame.
struct Gpu {
    quad: HudQuad,
    bar: Bar,
}

impl State {
    const fn new() -> Self {
        Self {
            last_prepare: None,
            ammo_hand: AmmoHand::new(),
            ammo_position: None,
            ammo_corners: None,
            glance: Glance::new(),
            wrist_alpha: 0.0,
            trail: HealthTrail::new(),
            last_health: None,
            last_damage: None,
            bar_corners: None,
            heat_corners: None,
            cross_offset: None,
            cross_open: false,
            cross_corners: None,
            cross_alpha: 0.0,
            tether_position: None,
            tether_corners: None,
            status: AnchoredStatus {
                ammo_hand: None,
                health: None,
                heat_level: 0,
                wrist_alpha: 0.0,
                glance: false,
                attention: false,
                heat_drawn: false,
                cross_alpha: None,
                tether_drawn: false,
            },
            gpu: None,
        }
    }

    /// Eye 0: place the weapon readout and the wrist, advance the glance and the fade, and repaint
    /// the health bar.
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        active: bool,
        dominant: Hand,
        cfg: &AtlasConfig,
    ) {
        let now = Instant::now();
        let dt = self
            .last_prepare
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        self.last_prepare = Some(now);
        self.ammo_corners = None;
        self.bar_corners = None;
        self.heat_corners = None;
        self.cross_corners = None;
        self.tether_corners = None;
        let sample = *SAMPLE.lock();
        if !active {
            self.ammo_position = None;
            self.cross_offset = None;
            self.cross_open = false;
            self.tether_position = None;
            self.wrist_alpha = 0.0;
            self.status = AnchoredStatus::default();
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
        let head = crate::hud::render_camera_pose().map(|(position, orientation)| Pose {
            position,
            orientation,
        });
        let dtf = hooks::camera::last_dtf();
        let hands = late::render_hands();
        let grip = |hand: Hand| hands[hand.index()].map(|h| h.grip.at(dtf));

        // The weapon readout, above the gun hand.
        let holding = aim::status().weapon.map_or([false; 2], |weapon| {
            let mut holding = [false; 2];
            for hand in weapon.gun_hands.into_iter().flatten() {
                holding[hand.index()] = true;
            }
            holding
        });
        let ammo_hand = self
            .ammo_hand
            .step(gun_hand(holding, dominant), dt, cfg.ammo.linger_s);
        self.status.ammo_hand = None;
        if cfg.ammo.enabled
            && let (Some(hand), Some(head)) = (ammo_hand, head)
            && let Some(grip) = grip(hand)
            && let Some(cell) = atlas::part_texture(Part::Ammo, |part| part.cell)
        {
            let width = cfg.ammo.width_m.max(0.01);
            let height = width * cell.rect.size.y / cell.rect.size.x.max(1.0);
            let target = ammo_anchor(
                grip.position,
                head.position,
                cfg.ammo.clearance_m,
                height,
                cfg.ammo.pull_m,
            );
            let position = follow(
                self.ammo_position,
                target,
                dt,
                cfg.ammo.follow_halflife_s,
                SNAP_M,
            );
            self.ammo_position = Some(position);
            self.ammo_corners = upright_corners(
                position,
                head.position,
                head.orientation * Vec3::Y,
                width,
                height,
            )
            .map(|corners| corners.map(|c| c.extend(1.0)));
            self.status.ammo_hand = Some(hand);
        } else {
            self.ammo_position = None;
        }

        // The tether readout, above the off-hand's forearm, where the grapple fires from.
        self.status.tether_drawn = false;
        let off = dominant.other();
        if cfg.tether.enabled
            && let Some(head) = head
            && let Some(wrist) = hands[off.index()].map(|h| h.wrist.at(dtf))
            && let Some(cell) = atlas::part_texture(Part::Tether, |part| part.cell)
        {
            let width = cfg.tether.width_m.max(0.01);
            let height = width * cell.rect.size.y / cell.rect.size.x.max(1.0);
            let target = forearm_anchor(wrist, cfg.tether.back_m, cfg.tether.clearance_m, height);
            let position = follow(
                self.tether_position,
                target,
                dt,
                cfg.tether.follow_halflife_s,
                SNAP_M,
            );
            self.tether_position = Some(position);
            self.tether_corners = upright_corners(
                position,
                head.position,
                head.orientation * Vec3::Y,
                width,
                height,
            )
            .map(|corners| corners.map(|c| c.extend(1.0)));
            self.status.tether_drawn = self.tether_corners.is_some();
        } else {
            self.tether_position = None;
        }

        // The selection cross, where the steering hand was when the wheel opened, relative to the
        // head so it moves with the player; it fades out over the cross's hide animation.
        let anchor = cfg.cross.enabled.then(wheel::cross_anchor).flatten();
        let cell = atlas::part_texture(Part::Cross, |part| part.cell);
        self.status.cross_alpha = None;
        match (anchor, head, cell) {
            (Some(anchor), Some(head), Some(cell)) => {
                if anchor.open && (!self.cross_open || self.cross_offset.is_none()) {
                    self.cross_offset = grip(anchor.hand).map(|grip| {
                        pull_toward(grip.position, head.position, cfg.cross.pull_m) - head.position
                    });
                }
                self.cross_open = anchor.open;
                self.cross_alpha = if anchor.open {
                    1.0
                } else {
                    fade_out(anchor.closed_s, CROSS_LINGER.as_secs_f32())
                };
                if let Some(offset) = self.cross_offset {
                    let width = cfg.cross.width_m.max(0.01);
                    let height = width * cell.rect.size.y / cell.rect.size.x.max(1.0);
                    self.cross_corners = facing_corners(
                        head.position + offset,
                        head.position,
                        head.orientation * Vec3::Y,
                        width,
                        height,
                    )
                    .map(|corners| corners.map(|c| c.extend(1.0)));
                    self.status.cross_alpha = Some(self.cross_alpha);
                }
            }
            _ => {
                self.cross_offset = None;
                self.cross_open = false;
            }
        }

        // The wrist: the health bar and the heat readout.
        let wrist = &cfg.wrist;
        let pose = grip(off).map(|grip| inner_wrist_pose(grip, off, wrist.offset_m));
        let fraction = sample
            .filter(|s| s.max_health > 0)
            .map(|s| (f32::from(s.health) / f32::from(s.max_health)).clamp(0.0, 1.0));
        if let Some(sample) = sample {
            if self.last_health.is_some_and(|last| sample.health < last) {
                self.last_damage = Some(now);
                self.trail.hit();
            }
            self.last_health = Some(sample.health);
        }
        let attention = wants_attention(
            &Attention {
                health: fraction,
                since_damage_s: self
                    .last_damage
                    .map(|at| now.duration_since(at).as_secs_f32()),
                heat_level: sample.map_or(0, |s| s.heat_level),
            },
            wrist.low_health,
            wrist.after_damage_s,
        );
        let looked = match (head, pose) {
            (Some(head), Some(pose)) => looking(head, &pose, &wrist.glance, self.glance.shown()),
            _ => false,
        };
        let glance = self.glance.step(looked, dt, &wrist.glance);
        let shown =
            wrist.enabled && pose.is_some() && wrist_wanted(wrist.visibility, glance, attention);
        self.wrist_alpha = fade(self.wrist_alpha, shown, dt, wrist.fade_s);
        let trail = fraction.map(|f| self.trail.step(f, dt));
        self.status.health = fraction.zip(trail);
        self.status.heat_level = sample.map_or(0, |s| s.heat_level);
        self.status.wrist_alpha = self.wrist_alpha;
        self.status.glance = glance;
        self.status.attention = attention;
        self.status.heat_drawn = false;
        let Some(pose) = pose.filter(|_| self.wrist_alpha > 0.0) else {
            return;
        };
        let width = wrist.width_m.max(0.01);
        let bar = (wrist.health && fraction.is_some()).then(|| width / wrist.bar_aspect.max(1.0));
        let heat = wrist
            .heat
            .then(|| atlas::part_texture(Part::Heat, |part| part.cell))
            .flatten()
            .map(|cell| width * cell.rect.size.y / cell.rect.size.x.max(1.0));
        let heights: Vec<f32> = bar.into_iter().chain(heat).collect();
        let mut quads = stacked_corners(&pose, width, &heights, WRIST_GAP_M)
            .into_iter()
            .map(|corners| corners.map(|c| c.extend(1.0)));
        if bar.is_some() {
            self.bar_corners = quads.next();
        }
        if heat.is_some() {
            self.heat_corners = quads.next();
            self.status.heat_drawn = true;
        }
        if let (Some(gpu), Some(fraction), Some(trail)) = (self.gpu.as_mut(), fraction, trail) {
            let low = fraction <= wrist.low_health;
            gpu.bar
                .update(context, BarKey::new(BAR_TEXELS.0, fraction, trail, low));
        }
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if let Some(corners) = self.cross_corners {
            atlas::part_texture(Part::Cross, |part| {
                gpu.quad.draw_faded(
                    context,
                    device,
                    target,
                    part.target.color_srv(),
                    &corners,
                    self.cross_alpha,
                )
            });
        }
        if let Some(corners) = self.ammo_corners {
            atlas::part_texture(Part::Ammo, |part| {
                gpu.quad
                    .draw(context, device, target, part.target.color_srv(), &corners)
            });
        }
        if let Some(corners) = self.tether_corners {
            atlas::part_texture(Part::Tether, |part| {
                gpu.quad
                    .draw(context, device, target, part.target.color_srv(), &corners)
            });
        }
        if let Some(corners) = self.heat_corners {
            atlas::part_texture(Part::Heat, |part| {
                gpu.quad.draw_faded(
                    context,
                    device,
                    target,
                    part.target.color_srv(),
                    &corners,
                    self.wrist_alpha,
                )
            });
        }
        if let Some(corners) = self.bar_corners {
            gpu.quad.draw_faded(
                context,
                device,
                target,
                gpu.bar.target().color_srv(),
                &corners,
                self.wrist_alpha,
            );
        }
    }
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            quad: HudQuad::new(device)?,
            bar: Bar::new(device)?,
        })
    }
}

/// Log a resource failure once per session; the next frame retries regardless.
fn log_once(error: &anyhow::Error) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("hud anchored readouts: {error:#}");
    }
}
