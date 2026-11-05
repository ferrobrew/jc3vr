//! The body-anchored HUD readouts, under the controller scheme: an ammunition counter above each
//! hand holding a gun, the tether readout above the off-hand's forearm, the health bar and the heat and grenade
//! readouts on the inside of the off-hand's wrist, and the weapon selection cross where the hand steering the
//! weapon wheel was when it opened.
//!
//! The readouts above the hand and the forearm stand upright and face the eye. They are drawn
//! without a depth test, like every HUD quad, so they are placed clear of the hand and the gun
//! rather than tested against them: a readout near the hand that the scene's depth could hide
//! would vanish into the gun or a wall the gun is pushed against.
//!
//! The tether, heat, and grenade readouts are the game's own clips, moved into atlas cells for the
//! capture ([`crate::hud::atlas`]) and drawn here from their cells' copies, so they keep the game's
//! values, icons, and fades. The health bar is the mod's own ([`health`]), from the player
//! character's health, sampled at the capture seam. So are the ammunition counters
//! ([`hand_ammo`]), sampled by the weapon handling's input tick: each hand holds a weapon of its
//! own, and the game's weapon readout shows the wielded one alone, so every gun gets the same
//! counter rather than one gun the game's readout and the other the mod's. The game's weapon
//! readout keeps its atlas cell, which takes it off the panel, and is not drawn.
//!
//! On eye 0 the anchors are placed from the render-time hand poses and the head
//! ([`place`]); both eyes then draw the same corners through the shared quad pass, after the
//! floating panel, since the readouts are nearer than it.

pub(crate) mod hand_ammo;
mod health;
pub(crate) mod place;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use anyhow::Context as _;
use glam::{Vec2, Vec3, Vec4};
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
        frame::{Hand, Pose},
        hands::late,
        scheme,
        summary::{
            config::GlanceConfig,
            glance::{Glance, fade, looking},
        },
        weapons::wheel::{self, CROSS_LINGER},
    },
    hooks,
    hud::{
        HudMode,
        anchored::{
            hand_ammo::{COUNT_TEXELS, CountKey},
            health::{BAR_TEXELS, BarKey, HealthTrail},
            place::{
                Attention, ammo_anchor, facing_corners, fade_out, follow, forearm_anchor,
                inner_wrist_pose, pull_toward, sighting, stacked_corners, upright_corners,
                wants_attention,
            },
        },
        atlas::{self, Part},
        config::{AtlasConfig, HudConfig},
        quad::HudQuad,
        target::KeyedTexture,
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
        let (cfg, dominant) = Config::lock_query(|c| (c.hud, c.controllers.acting_hand()));
        let active = cfg.atlas.enabled
            && cfg.redirect
            && cfg.quad
            && crate::hud::current_mode() == HudMode::Hud
            && scheme::is_controllers()
            && !crate::director::clean_view::hides_everything()
            && crate::vr::render_params(0).is_some();
        state.prepare(context, device, active, dominant, &cfg.atlas);
    }
    state.draw(context, device, target);
}

/// The readouts' state, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct AnchoredStatus {
    /// The hands with an ammunition counter, by [`Hand::index`].
    pub counts: [bool; 2],
    /// The health fraction and the trail, when sampled.
    pub health: Option<(f32, f32)>,
    pub heat_level: i32,
    pub wrist_alpha: f32,
    pub glance: bool,
    pub attention: bool,
    /// Whether the heat readout's cell was drawn.
    pub heat_drawn: bool,
    /// Whether the grenade readout's cell was drawn.
    pub grenades_drawn: bool,
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

/// How long the weapon readout takes to fade out as the gun is raised to the eye, and back in,
/// seconds.
const SIGHT_FADE_S: f32 = 0.15;

/// The gap between the health bar and the heat readout on the wrist, metres.
const WRIST_GAP_M: f32 = 0.006;

/// How far above the gun hand's grip an ammunition counter's lower edge sits, metres, straight up
/// in the world whatever the hand's tilt, so the quad clears the hand and the gun in it.
const AMMO_CLEARANCE_M: f32 = 0.10;

/// How far an ammunition counter is pulled from above the hand toward the head, metres.
const AMMO_PULL_M: f32 = 0.02;

/// The halflife of an ammunition counter's follow of its hand, seconds.
const AMMO_FOLLOW_HALFLIFE_S: f32 = 0.05;

/// The width of an ammunition counter, metres; its height follows the texture's aspect.
const COUNT_WIDTH_M: f32 = 0.08;

/// How far back along the forearm from the wrist the tether readout sits, metres.
const TETHER_BACK_M: f32 = 0.12;

/// How far above the forearm the tether readout's lower edge sits, metres, straight up in the
/// world.
const TETHER_CLEARANCE_M: f32 = 0.06;

/// The tether readout's width, metres; its height follows the cell's aspect.
const TETHER_WIDTH_M: f32 = 0.08;

/// The halflife of the tether readout's follow of the forearm, seconds.
const TETHER_FOLLOW_HALFLIFE_S: f32 = 0.05;

/// The selection cross's width, metres; its height follows the cell's aspect.
const CROSS_WIDTH_M: f32 = 0.18;

/// How far the selection cross is pulled from the hand toward the head, metres, so the hand does
/// not cover it.
const CROSS_PULL_M: f32 = 0.08;

/// The wrist panel's centre in the grip frame, metres: +X out of the palm, +Z back along the
/// forearm (mirrored on X between the hands, so +X is the palm side of either). It sits toward the
/// elbow from the C4 detonator button on the same side of the wrist.
const WRIST_OFFSET_M: Vec3 = Vec3::new(0.04, 0.0, 0.16);

/// The wrist panel's width, metres.
const WRIST_WIDTH_M: f32 = 0.08;

/// The health bar's width over its height.
const WRIST_BAR_ASPECT: f32 = 10.0;

/// The grenade readout's height on the wrist, metres; its width follows the cell's aspect. The
/// readout is a narrow column of icons, so it is sized by its height rather than the wrist's width.
const WRIST_GRENADES_HEIGHT_M: f32 = 0.04;

/// The glance at the inside of the wrist that shows the wrist readout.
const WRIST_GLANCE: GlanceConfig = GlanceConfig::new();

/// The wrist readout's fade in and out, seconds.
const WRIST_FADE_S: f32 = 0.15;

/// How long the wrist readout shows on its own after damage, seconds.
const WRIST_AFTER_DAMAGE_S: f32 = 4.0;

/// The health fraction at and below which the wrist readout shows on its own.
const WRIST_LOW_HEALTH: f32 = 0.35;

struct State {
    last_prepare: Option<Instant>,
    /// Each hand's ammunition counter, by [`Hand::index`].
    counts: [HandCount; 2],
    glance: Glance,
    wrist_alpha: f32,
    trail: HealthTrail,
    last_health: Option<i16>,
    last_damage: Option<Instant>,
    bar_corners: Option<[Vec4; 4]>,
    heat_corners: Option<[Vec4; 4]>,
    grenade_corners: Option<[Vec4; 4]>,
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

/// One hand's ammunition counter: where it is, its corners this frame, and its opacity.
#[derive(Copy, Clone)]
struct HandCount {
    position: Option<Vec3>,
    corners: Option<[Vec4; 4]>,
    alpha: f32,
}

impl HandCount {
    const fn new() -> Self {
        Self {
            position: None,
            corners: None,
            alpha: 1.0,
        }
    }
}

/// The quad pass, the health bar's texture, and each hand's counter texture, created on the first
/// active frame.
struct Gpu {
    quad: HudQuad,
    bar: KeyedTexture<BarKey>,
    counts: [KeyedTexture<CountKey>; 2],
}

impl State {
    const fn new() -> Self {
        Self {
            last_prepare: None,
            counts: [HandCount::new(); 2],
            glance: Glance::new(),
            wrist_alpha: 0.0,
            trail: HealthTrail::new(),
            last_health: None,
            last_damage: None,
            bar_corners: None,
            heat_corners: None,
            grenade_corners: None,
            cross_offset: None,
            cross_open: false,
            cross_corners: None,
            cross_alpha: 0.0,
            tether_position: None,
            tether_corners: None,
            status: AnchoredStatus {
                counts: [false; 2],
                health: None,
                heat_level: 0,
                wrist_alpha: 0.0,
                glance: false,
                attention: false,
                heat_drawn: false,
                grenades_drawn: false,
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
        for count in &mut self.counts {
            count.corners = None;
        }
        self.bar_corners = None;
        self.heat_corners = None;
        self.grenade_corners = None;
        self.cross_corners = None;
        self.tether_corners = None;
        let sample = *SAMPLE.lock();
        if !active {
            self.counts = [HandCount::new(); 2];
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
        let hands = late::drawn_hands(dtf);
        let grip = |hand: Hand| hands[hand.index()].map(|h| h.grip);

        // A counter above each hand holding a gun.
        let sampled = hand_ammo::sampled().filter(|_| cfg.ammo.enabled);
        self.status.counts = [false; 2];
        for hand in Hand::BOTH {
            let count = &mut self.counts[hand.index()];
            let ammo = sampled.and_then(|s| s.hands[hand.index()]);
            let (Some(ammo), Some(head), Some(grip)) = (ammo, head, grip(hand)) else {
                count.position = None;
                count.alpha = 1.0;
                continue;
            };
            let width = COUNT_WIDTH_M;
            let height = width * COUNT_TEXELS.1 as f32 / COUNT_TEXELS.0 as f32;
            let target = ammo_anchor(
                grip.position,
                head.position,
                AMMO_CLEARANCE_M,
                height,
                AMMO_PULL_M,
            );
            let position = follow(count.position, target, dt, AMMO_FOLLOW_HALFLIFE_S, SNAP_M);
            count.position = Some(position);
            count.corners = upright_corners(
                position,
                head.position,
                head.orientation * Vec3::Y,
                width,
                height,
            )
            .map(|corners| corners.map(|c| c.extend(1.0)));
            let sighted = hands[hand.index()].map(|h| h.aim).is_some_and(|aim| {
                sighting(
                    grip.position,
                    aim.orientation * Vec3::NEG_Z,
                    head.position,
                    head.orientation * Vec3::NEG_Z,
                )
            });
            count.alpha = fade(count.alpha, !sighted, dt, SIGHT_FADE_S);
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.counts[hand.index()].update(context, CountKey::new(&ammo), |key| {
                    hand_ammo::paint(COUNT_TEXELS.0, COUNT_TEXELS.1, key)
                });
            }
            self.status.counts[hand.index()] = count.corners.is_some();
        }

        // The tether readout, above the off-hand's forearm, where the grapple fires from.
        self.status.tether_drawn = false;
        let off = dominant.other();
        if cfg.tether.enabled
            && let Some(head) = head
            && let Some(wrist) = hands[off.index()].map(|h| h.wrist)
            && let Some(cell) = atlas::part_texture(Part::Tether, |part| part.cell)
        {
            let width = TETHER_WIDTH_M;
            let height = width * cell.rect.size.y / cell.rect.size.x.max(1.0);
            let target = forearm_anchor(wrist, TETHER_BACK_M, TETHER_CLEARANCE_M, height);
            let position = follow(
                self.tether_position,
                target,
                dt,
                TETHER_FOLLOW_HALFLIFE_S,
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
                        pull_toward(grip.position, head.position, CROSS_PULL_M) - head.position
                    });
                }
                self.cross_open = anchor.open;
                self.cross_alpha = if anchor.open {
                    1.0
                } else {
                    fade_out(anchor.closed_s, CROSS_LINGER.as_secs_f32())
                };
                if let Some(offset) = self.cross_offset {
                    let width = CROSS_WIDTH_M;
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

        // The wrist: the health bar and the heat and grenade readouts.
        let pose = grip(off).map(|grip| inner_wrist_pose(grip, off, WRIST_OFFSET_M));
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
            WRIST_LOW_HEALTH,
            WRIST_AFTER_DAMAGE_S,
        );
        let looked = match (head, pose) {
            (Some(head), Some(pose)) => looking(head, &pose, &WRIST_GLANCE, self.glance.shown()),
            _ => false,
        };
        let glance = self.glance.step(looked, dt, &WRIST_GLANCE);
        // The wrist shows on a glance, and also on its own while the player's state wants
        // attention.
        let shown = cfg.wrist.enabled && pose.is_some() && (glance || attention);
        self.wrist_alpha = fade(self.wrist_alpha, shown, dt, WRIST_FADE_S);
        let trail = fraction.map(|f| self.trail.step(f, dt));
        self.status.health = fraction.zip(trail);
        self.status.heat_level = sample.map_or(0, |s| s.heat_level);
        self.status.wrist_alpha = self.wrist_alpha;
        self.status.glance = glance;
        self.status.attention = attention;
        self.status.heat_drawn = false;
        self.status.grenades_drawn = false;
        let Some(pose) = pose.filter(|_| self.wrist_alpha > 0.0) else {
            return;
        };
        let width = WRIST_WIDTH_M;
        let bar = fraction
            .is_some()
            .then_some(Vec2::new(width, width / WRIST_BAR_ASPECT));
        let heat = atlas::part_texture(Part::Heat, |part| part.cell)
            .map(|cell| Vec2::new(width, width * cell.rect.size.y / cell.rect.size.x.max(1.0)));
        let grenades = atlas::part_texture(Part::Grenades, |part| part.cell).map(|cell| {
            let height = WRIST_GRENADES_HEIGHT_M;
            Vec2::new(
                height * cell.rect.size.x / cell.rect.size.y.max(1.0),
                height,
            )
        });
        let sizes: Vec<Vec2> = bar.into_iter().chain(heat).chain(grenades).collect();
        let mut quads = stacked_corners(&pose, &sizes, WRIST_GAP_M)
            .into_iter()
            .map(|corners| corners.map(|c| c.extend(1.0)));
        if bar.is_some() {
            self.bar_corners = quads.next();
        }
        if heat.is_some() {
            self.heat_corners = quads.next();
            self.status.heat_drawn = true;
        }
        if grenades.is_some() {
            self.grenade_corners = quads.next();
            self.status.grenades_drawn = true;
        }
        if let (Some(gpu), Some(fraction), Some(trail)) = (self.gpu.as_mut(), fraction, trail) {
            let low = fraction <= WRIST_LOW_HEALTH;
            gpu.bar.update(
                context,
                BarKey::new(BAR_TEXELS.0, fraction, trail, low),
                |key| health::paint(BAR_TEXELS.0, BAR_TEXELS.1, key),
            );
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
        for (count, texture) in self.counts.iter().zip(&gpu.counts) {
            if let Some(corners) = count.corners.filter(|_| count.alpha > 0.0) {
                gpu.quad.draw_faded(
                    context,
                    device,
                    target,
                    texture.target().color_srv(),
                    &corners,
                    count.alpha,
                );
            }
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
        if let Some(corners) = self.grenade_corners {
            atlas::part_texture(Part::Grenades, |part| {
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
        let count = || {
            KeyedTexture::new(device, COUNT_TEXELS)
                .context("creating an ammunition counter texture")
        };
        Ok(Self {
            quad: HudQuad::new(device)?,
            bar: KeyedTexture::new(device, BAR_TEXELS)
                .context("creating the health bar texture")?,
            counts: [count()?, count()?],
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
