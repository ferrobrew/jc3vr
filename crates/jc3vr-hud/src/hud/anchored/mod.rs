//! The body-anchored HUD readouts, under the controller scheme: an ammunition counter above each
//! gun in a hand, the tether readout above the off-hand's forearm, the health bar and the heat and
//! grenade readouts on the inside of the off-hand's wrist, and the weapon selection cross where the
//! hand steering the weapon wheel was when it opened.
//!
//! The ammunition counters follow the rendered guns, including their rotation. The forearm readout
//! stands upright and faces the eye. The readouts are drawn without a depth test, like every HUD
//! quad, so they are placed clear of the hand and the gun rather than tested against them: a readout near the hand that the scene's depth could hide
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

use std::time::Instant;

use anyhow::Context as _;
use glam::{Vec2, Vec3, Vec4};
use jc3gi::{
    character::character::Character,
    graphics_engine::{device::Device, texture::Texture},
    heat_manager::HeatManager,
};
use jc3vr_body::controllers::{hands::late, scheme};
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_input::frame::{Hand, Pose};
use jc3vr_weapons::controllers::weapons::{
    hud_feed::hand_ammo as ammo_sample, wheel, wheel::CROSS_LINGER,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    controllers::summary::glance::{Glance, fade, looking},
    hud::{
        HudMode,
        anchored::{
            hand_ammo::{COUNT_TEXELS, CountKey},
            health::{BAR_TEXELS, BarKey, HealthTrail},
            place::{
                Attention, facing_corners, fade_out, follow, forearm_anchor, gun_ammo_corners,
                inner_wrist_pose, pull_toward, sighting, stacked_corners, upright_corners,
                wants_attention,
            },
        },
        atlas,
        atlas::Part,
        config::{HUD_CONFIG, HudConfig},
        log_once::LogOnce,
        quad::HudQuad,
        target::KeyedTexture,
        tuning::HUD,
        world_quads_active,
    },
};

/// Sample the player's health and heat for the wrist (the capture seam, after the HUD's update on
/// the same thread, which reads the same state).
///
/// # Safety
/// Must be called from the `MovieImpl::Capture` detour for the main movie.
pub(crate) unsafe fn sample_status(cfg: &HudConfig) {
    let wanted = cfg.atlas.enabled && crate::hud::current_mode() == HudMode::Hud;
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
    if jc3vr_view::stereo::dispatch::first_ui_eye(eye) {
        let cfg = *HUD_CONFIG.lock();
        let dominant = CONTROLLERS_CONFIG.lock().acting_hand();
        let active = cfg.atlas.enabled
            && world_quads_active(&cfg)
            && scheme::is_controllers()
            && jc3vr_view::vr::render_params(0).is_some();
        state.prepare(context, device, active, dominant);
    }
    state.draw(context, device, target);
}

/// The readouts' state, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct AnchoredStatus {
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

pub fn status() -> AnchoredStatus {
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

/// One hand's ammunition counter: its corners this frame and its opacity.
#[derive(Copy, Clone)]
struct HandCount {
    corners: Option<[Vec4; 4]>,
    alpha: f32,
}

impl HandCount {
    const fn new() -> Self {
        Self {
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
    ) {
        let tuning = &HUD.get().anchored;
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
                    FAILURES.error(&e);
                    return;
                }
            }
        }
        let head = jc3vr_view::hud::render_camera_pose().map(|(position, orientation)| Pose {
            position,
            orientation,
        });
        let dtf = jc3vr_view::hooks::camera::last_dtf();
        let hands = late::drawn_hands(dtf);
        let grip = |hand: Hand| hands[hand.index()].map(|h| h.grip);

        let guns = late::rendered_guns();
        let sampled = ammo_sample::sampled();
        self.status.counts = [false; 2];
        for hand in Hand::BOTH {
            let count = &mut self.counts[hand.index()];
            let ammo = sampled.and_then(|s| s.hands[hand.index()]);
            let gun = guns[hand.index()].filter(|gun| ammo.is_some_and(|a| a.weapon == gun.weapon));
            let (Some(ammo), Some(gun), Some(head), Some(grip)) = (ammo, gun, head, grip(hand))
            else {
                count.alpha = 1.0;
                continue;
            };
            let width = tuning.ammo.width_m;
            let height = width * COUNT_TEXELS.1 as f32 / COUNT_TEXELS.0 as f32;
            let clearance = if ammo.model.is_some() {
                tuning.ammo.gun_clearance_m
            } else {
                tuning.ammo.clearance_m
            };
            count.corners =
                gun_ammo_corners(gun.matrix, ammo.model, Vec2::new(width, height), clearance)
                    .map(|corners| corners.map(|c| c.extend(1.0)));
            let sighted = hands[hand.index()].map(|h| h.aim).is_some_and(|aim| {
                sighting(
                    grip.position,
                    aim.orientation * Vec3::NEG_Z,
                    head.position,
                    head.orientation * Vec3::NEG_Z,
                )
            });
            count.alpha = fade(count.alpha, !sighted, dt, tuning.sight_fade_s);
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
        if let Some(head) = head
            && let Some(wrist) = hands[off.index()].map(|h| h.wrist)
            && let Some(cell) = atlas::part_texture(Part::Tether, |part| part.cell)
        {
            let width = tuning.tether.width_m;
            let height = width * cell.rect.size.y / cell.rect.size.x.max(1.0);
            let target = forearm_anchor(
                wrist,
                tuning.tether.back_m,
                tuning.tether.clearance_m,
                height,
            );
            let position = follow(
                self.tether_position,
                target,
                dt,
                tuning.tether.follow_halflife_s,
                tuning.snap_m,
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
        let anchor = wheel::cross_anchor();
        let cell = atlas::part_texture(Part::Cross, |part| part.cell);
        self.status.cross_alpha = None;
        match (anchor, head, cell) {
            (Some(anchor), Some(head), Some(cell)) => {
                if anchor.open && (!self.cross_open || self.cross_offset.is_none()) {
                    self.cross_offset = grip(anchor.hand).map(|grip| {
                        pull_toward(grip.position, head.position, tuning.cross.pull_m)
                            - head.position
                    });
                }
                self.cross_open = anchor.open;
                self.cross_alpha = if anchor.open {
                    1.0
                } else {
                    fade_out(anchor.closed_s, CROSS_LINGER.as_secs_f32())
                };
                if let Some(offset) = self.cross_offset {
                    let width = tuning.cross.width_m;
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
        let pose = grip(off).map(|grip| inner_wrist_pose(grip, off, tuning.wrist.offset_m));
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
            tuning.wrist.low_health,
            tuning.wrist.after_damage_s,
        );
        let looked = match (head, pose) {
            (Some(head), Some(pose)) => {
                looking(head, &pose, &tuning.wrist.glance, self.glance.shown())
            }
            _ => false,
        };
        let glance = self.glance.step(looked, dt, &tuning.wrist.glance);
        // The wrist shows on a glance, and also on its own while the player's state wants
        // attention.
        let shown = pose.is_some() && (glance || attention);
        self.wrist_alpha = fade(self.wrist_alpha, shown, dt, tuning.wrist.fade_s);
        let trail = fraction.map(|f| self.trail.step(f, dt, &tuning.health_trail));
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
        let width = tuning.wrist.width_m;
        let bar = fraction.is_some().then_some(Vec2::new(
            width,
            width / (BAR_TEXELS.0 as f32 / BAR_TEXELS.1 as f32),
        ));
        let heat = atlas::part_texture(Part::Heat, |part| part.cell)
            .map(|cell| Vec2::new(width, width * cell.rect.size.y / cell.rect.size.x.max(1.0)));
        let grenades = atlas::part_texture(Part::Grenades, |part| part.cell).map(|cell| {
            let height = tuning.wrist.grenades_height_m;
            Vec2::new(
                height * cell.rect.size.x / cell.rect.size.y.max(1.0),
                height,
            )
        });
        let sizes: Vec<Vec2> = bar.into_iter().chain(heat).chain(grenades).collect();
        let mut quads = stacked_corners(&pose, &sizes, tuning.wrist.gap_m)
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
            let low = fraction <= tuning.wrist.low_health;
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

static FAILURES: LogOnce = LogOnce::new("hud anchored readouts");
