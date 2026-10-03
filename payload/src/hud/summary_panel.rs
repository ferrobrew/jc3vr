//! The controls summary's floating panel: the third egui-to-texture panel, after the gameplay HUD
//! and the debug panel ([`crate::hud::egui_panel`]), anchored to the off-hand wrist rather than the
//! head ([`crate::controllers::summary`]).
//!
//! It has its own egui context and renderer, so that it never shares input, output, fonts, or
//! textures with the debug UI, and it takes no input. On eye 0 the glance and the fade are
//! advanced, the summary is painted into the panel's own render target while it is visible, and the
//! world corners are cached; both eyes then draw the same corners through the shared quad pass
//! ([`crate::hud::quad`]), as the other panels do. Nothing is drawn, and the panel's resources are
//! not created, until it is first shown.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use egui::{
    Color32, FontFamily, FontId, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
    text::{LayoutJob, TextFormat},
};
use glam::Vec4;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{
        frame::{self, Button, InteractionProfile, Pose},
        hands::late,
        scheme,
        summary::{
            self, SummaryConfig,
            content::{Side, Summary, metrics},
            glance::{Glance, TrackpadPin, fade, looking, wrist_pose},
            glyphs::{Glyph, glyphs},
        },
    },
    hooks,
    hud::{quad::HudQuad, target::HudTarget},
};

/// Draw the summary panel for `eye` over `target`. Called from the render-thread post-draw hook
/// after the other panels, with the engine context mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut panel = PANEL.lock();
    if eye == 0 {
        let (enabled, dominant, config) = Config::lock_query(|c| {
            (
                c.controllers.enabled && c.controllers.summary.enabled,
                c.controllers.dominant_hand,
                c.controllers.summary,
            )
        });
        let active = enabled
            && scheme::is_controllers()
            && !crate::director::cinematic::active()
            && crate::vr::render_params(0).is_some();
        panel.prepare(context, device, active, dominant.other(), &config);
    }
    panel.draw(context, device, target);
}

/// The panel's readout for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PanelStatus {
    pub alpha: f32,
    pub glance: bool,
    pub pinned: bool,
    pub mode_recent: bool,
}

pub fn status() -> PanelStatus {
    PANEL.lock().status
}

static PANEL: Mutex<Panel> = Mutex::new(Panel::new());

struct Panel {
    glance: Glance,
    trackpad_pin: TrackpadPin,
    alpha: f32,
    last_prepare: Option<Instant>,
    corners: Option<[Vec4; 4]>,
    status: PanelStatus,
    /// The GPU side, created the first time the panel shows.
    gpu: Option<Gpu>,
}

impl Panel {
    const fn new() -> Self {
        Self {
            glance: Glance::new(),
            trackpad_pin: TrackpadPin::new(),
            alpha: 0.0,
            last_prepare: None,
            corners: None,
            status: PanelStatus {
                alpha: 0.0,
                glance: false,
                pinned: false,
                mode_recent: false,
            },
            gpu: None,
        }
    }

    /// Eye 0: advance the glance and the fade, paint the summary while it is visible, and cache the
    /// panel's corners.
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        active: bool,
        off: frame::Hand,
        config: &SummaryConfig,
    ) {
        let now = Instant::now();
        let dt = self
            .last_prepare
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        self.last_prepare = Some(now);
        self.corners = None;

        let head = crate::hud::render_camera_pose().map(|(position, orientation)| Pose {
            position,
            orientation,
        });
        let grip = late::render_hands()[off.index()].map(|h| h.grip.at(hooks::camera::last_dtf()));
        let pose = grip.map(|grip| wrist_pose(grip, off, &config.placement));
        let published = summary::published();

        let looked = match (active && config.show_on_glance, head, pose) {
            (true, Some(head), Some(pose)) => {
                looking(head, &pose, &config.glance, self.glance.shown())
            }
            _ => false,
        };
        let glance = self.glance.step(looked, dt, &config.glance);
        let (touched, pressed) = if active && config.show_on_trackpad_touch {
            frame::with_latest(|f| {
                let hand = f.hand(off);
                let index = hand.profile == InteractionProfile::ValveIndex;
                let pressed = hand.buttons.contains(Button::TrackpadPressLeft)
                    || hand.buttons.contains(Button::TrackpadPressRight);
                (index && hand.inputs.trackpad_touch, index && pressed)
            })
            .unwrap_or_default()
        } else {
            (false, false)
        };
        let pinned = self.trackpad_pin.step(touched, pressed, dt);
        let mode_recent = active
            && published.as_ref().is_some_and(|p| {
                summary::mode_changed_within(
                    p,
                    now,
                    Duration::from_secs_f32(config.show_on_mode_change_s.max(0.0)),
                )
            });
        let shown = active && pose.is_some() && (glance || pinned || mode_recent);
        self.alpha = fade(self.alpha, shown, dt, config.fade_s);
        self.status = PanelStatus {
            alpha: self.alpha,
            glance,
            pinned,
            mode_recent,
        };
        let (Some(pose), Some(published)) = (pose, published) else {
            return;
        };
        if self.alpha <= 0.0 {
            return;
        }

        let (width, height) = (config.resolution.0.max(64), config.resolution.1.max(64));
        if self.gpu.is_none() {
            match Gpu::new(device) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => {
                    tracing::error!("controls summary panel: {e:#}");
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if let Err(e) = gpu.paint(
            context,
            device,
            &published.summary,
            (width, height),
            self.alpha,
        ) {
            tracing::error!("controls summary panel: {e:#}");
            return;
        }
        let panel_width = config.placement.width_m.max(0.01);
        let panel_height = panel_width * height as f32 / width as f32;
        self.corners = Some(
            pose.corners(panel_width, panel_height)
                .map(|corner| corner.extend(1.0)),
        );
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let (Some(corners), Some(gpu)) = (self.corners, self.gpu.as_ref()) else {
            return;
        };
        let Some(panel_target) = gpu.target.as_ref() else {
            return;
        };
        gpu.quad
            .draw(context, device, target, panel_target.color_srv(), &corners);
    }
}

/// The panel's egui context, renderer, glyph textures, render target, and quad pass.
struct Gpu {
    egui: egui::Context,
    renderer: egui_directx11::Renderer,
    quad: HudQuad,
    target: Option<HudTarget>,
    glyphs: Vec<(Glyph, TextureHandle)>,
    started: Instant,
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let egui = egui::Context::default();
        egui.set_fonts(fonts());
        let glyphs = Glyph::ALL
            .iter()
            .map(|&glyph| {
                let image =
                    image::load_from_memory_with_format(glyph.png(), image::ImageFormat::Png)
                        .with_context(|| format!("decoding the {glyph:?} glyph"))?
                        .to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                Ok((
                    glyph,
                    egui.load_texture(format!("{glyph:?}"), color, TextureOptions::LINEAR),
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            renderer: egui_directx11::Renderer::new(&device.m_Device)
                .context("creating the egui renderer")?,
            quad: HudQuad::new(device).context("creating the quad pass")?,
            egui,
            target: None,
            glyphs,
            started: Instant::now(),
        })
    }

    /// Paint `summary` at `alpha` into the panel target, (re)creating it at `size`.
    fn paint(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        summary: &Arc<Summary>,
        size: (u32, u32),
        alpha: f32,
    ) -> anyhow::Result<()> {
        if self.target.as_ref().map(HudTarget::size) != Some(size) {
            self.target = None;
            self.target =
                Some(HudTarget::new(device, size.0, size.1).context("creating the panel target")?);
        }
        let Some(target) = self.target.as_ref() else {
            return Ok(());
        };
        let screen = Vec2::new(size.0 as f32, size.1 as f32);
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)),
            time: Some(self.started.elapsed().as_secs_f64()),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let glyphs = &self.glyphs;
        let output = self.egui.run_ui(input, |ui| {
            let mut painter = ui.painter().clone();
            painter.set_opacity(alpha);
            paint_summary(&painter, summary, screen, glyphs);
        });
        let (output, _, _) = egui_directx11::split_output(output);
        // SAFETY: the target's view is live, and the caller holds the engine context mutex.
        unsafe { context.ClearRenderTargetView(target.color_rtv(), &[0.0; 4]) };
        self.renderer
            .render(context, target.color_rtv(), &self.egui, output)
            .context("rendering the summary")
    }
}

/// Paint the summary over a `size` panel: the heading, then each group under its controller's
/// glyph, each row's glyphs in a fixed column and its label beside them.
fn paint_summary(
    painter: &egui::Painter,
    summary: &Summary,
    size: Vec2,
    textures: &[(Glyph, TextureHandle)],
) {
    let m = metrics(summary, size.y);
    let panel = Rect::from_min_size(Pos2::ZERO, size);
    painter.rect_filled(panel, m.margin * 1.5, BACKGROUND);
    let texture = |glyph: Glyph| {
        textures
            .iter()
            .find(|(g, _)| *g == glyph)
            .map(|(_, t)| t.id())
    };
    let full_uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let image = |glyph: Glyph, min: Pos2, side: f32| {
        if let Some(id) = texture(glyph) {
            painter.image(
                id,
                Rect::from_min_size(min, Vec2::splat(side)),
                full_uv,
                Color32::WHITE,
            );
        }
    };

    let left = m.margin * 1.5;
    let inner_width = size.x - 2.0 * left;
    let mut y = m.margin;
    let heading = painter.layout(
        summary.heading.clone(),
        FontId::new(m.heading_px * 0.55, FontFamily::Name(HEADING_FAMILY.into())),
        TEXT,
        inner_width,
    );
    let heading_y = y + (m.heading_px - heading.size().y) * 0.5;
    painter.galley(Pos2::new(left, heading_y), heading, TEXT);
    y += m.heading_px;
    painter.line_segment(
        [
            Pos2::new(left, y - m.margin * 0.5),
            Pos2::new(size.x - left, y - m.margin * 0.5),
        ],
        (2.0, ACCENT),
    );

    let text_x = left + 2.0 * m.glyph_px + m.margin;
    for group in &summary.groups {
        let header = match group.side {
            Side::Hand(hand) => vec![Glyph::controller(hand)],
            Side::Either => vec![Glyph::ControllerLeft, Glyph::ControllerRight],
        };
        for (i, &glyph) in header.iter().enumerate() {
            image(
                glyph,
                Pos2::new(left + i as f32 * m.group_px, y),
                m.group_px,
            );
        }
        y += m.group_px;
        for line in &group.lines {
            let mut job = LayoutJob::default();
            job.wrap.max_width = (size.x - text_x - left).max(1.0);
            let font = FontId::proportional(m.font_px);
            if let Some(prefix) = &line.prefix {
                job.append(
                    prefix,
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color: DIM,
                        ..Default::default()
                    },
                );
            }
            job.append(
                &line.text,
                if line.prefix.is_some() {
                    m.font_px * 0.3
                } else {
                    0.0
                },
                TextFormat {
                    font_id: font,
                    color: TEXT,
                    ..Default::default()
                },
            );
            let galley = painter.layout_job(job);
            let row = m.row_px.max(galley.size().y + m.margin * 0.5);
            let glyph_y = y + (m.row_px - m.glyph_px) * 0.5;
            for (i, &glyph) in glyphs(line.control).iter().enumerate() {
                image(
                    glyph,
                    Pos2::new(left + i as f32 * m.glyph_px, glyph_y),
                    m.glyph_px,
                );
            }
            let text_y = y + (m.row_px - galley.size().y).max(0.0) * 0.5;
            painter.galley(Pos2::new(text_x, text_y), galley, TEXT);
            y += row;
        }
    }
}

/// The fonts: Libre Franklin (SIL OFL, `payload/assets/fonts/libre-franklin`), a revival of the
/// Franklin Gothic the game's own UI is set in, before egui's bundled fonts, which cover the
/// Cyrillic it lacks.
fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/libre-franklin/LibreFranklin-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        SEMIBOLD.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/libre-franklin/LibreFranklin-SemiBold.ttf"
        ))),
    );
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, REGULAR.to_owned());
    let mut heading = vec![SEMIBOLD.to_owned()];
    heading.extend(proportional.iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(HEADING_FAMILY.into()), heading);
    fonts
}

const REGULAR: &str = "libre-franklin";
const SEMIBOLD: &str = "libre-franklin-semibold";
const HEADING_FAMILY: &str = "summary-heading";

/// The game's HUD palette: white text, the unavailable-text grey for prefixes, and a dark
/// translucent backing.
const TEXT: Color32 = Color32::WHITE;
const DIM: Color32 = Color32::from_rgb(160, 160, 160);
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(10, 12, 15, 215);
const ACCENT: Color32 = Color32::from_rgb(200, 40, 30);
