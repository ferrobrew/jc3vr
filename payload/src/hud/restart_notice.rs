//! The restart notice: a head-locked panel telling the player that the game must restart for the
//! controller input to match the settings ([`crate::vr::input::restart`]). It shows for as long as
//! that holds: until the game restarts, or, for a changed dominant hand, until the hand is changed
//! back. The desktop mirror shows it through the eyes' images.
//!
//! Like the controls summary's panel ([`crate::hud::summary_panel`]), it has its own egui context,
//! renderer, and render target, takes no input, and draws through the shared quad pass on both
//! eyes from corners computed on eye 0. The text is painted once per reason, not every frame, and
//! nothing is created until the notice first shows.

use anyhow::Context as _;
use egui::{Color32, FontId, Pos2, Rect, Vec2, text::LayoutJob};
use glam::{Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    controllers::summary::glance::PanelPose,
    hud::{quad::HudQuad, target::HudTarget},
    vr::input::restart::{self, RestartReason},
};

/// Draw the notice for `eye` over `target`, while one is due and a VR frame renders. Called from
/// the render-thread post-draw hook with the engine context mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut notice = NOTICE.lock();
    if eye == 0 {
        notice.prepare(context, device);
    }
    notice.draw(context, device, target);
}

static NOTICE: Mutex<Notice> = Mutex::new(Notice {
    corners: None,
    gpu: None,
    failed: false,
});

struct Notice {
    corners: Option<[Vec4; 4]>,
    gpu: Option<Gpu>,
    /// Whether creating or painting the panel failed, so it is not retried every frame.
    failed: bool,
}

impl Notice {
    /// Eye 0: decide whether the notice shows, paint it if its reason changed, and place it ahead
    /// of the head.
    fn prepare(&mut self, context: &ID3D11DeviceContext, device: &Device) {
        self.corners = None;
        if self.failed || crate::vr::render_params(0).is_none() {
            return;
        }
        let Some(reason) = restart::current() else {
            return;
        };
        let Some((head, orientation)) = crate::hud::render_camera_pose() else {
            return;
        };
        if self.gpu.is_none() {
            match Gpu::new(device) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => {
                    tracing::error!("restart notice: {e:#}");
                    self.failed = true;
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if gpu.painted != Some(reason)
            && let Err(e) = gpu.paint(context, reason)
        {
            tracing::error!("restart notice: {e:#}");
            self.failed = true;
            return;
        }
        let pose = PanelPose {
            center: head + orientation * OFFSET_M,
            rotation: orientation,
        };
        let height = WIDTH_M * HEIGHT_PX as f32 / WIDTH_PX as f32;
        self.corners = Some(pose.corners(WIDTH_M, height).map(|c| c.extend(1.0)));
    }

    fn draw(&self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let (Some(corners), Some(gpu)) = (self.corners, self.gpu.as_ref()) else {
            return;
        };
        gpu.quad
            .draw(context, device, target, gpu.target.color_srv(), &corners);
    }
}

/// The panel's egui context, renderer, render target, and quad pass.
struct Gpu {
    egui: egui::Context,
    renderer: egui_directx11::Renderer,
    quad: HudQuad,
    target: HudTarget,
    /// The reason the target holds the text of.
    painted: Option<RestartReason>,
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            egui: egui::Context::default(),
            renderer: egui_directx11::Renderer::new(&device.m_Device)
                .context("creating the egui renderer")?,
            quad: HudQuad::new(device).context("creating the quad pass")?,
            target: HudTarget::new(device, WIDTH_PX, HEIGHT_PX)
                .context("creating the notice target")?,
            painted: None,
        })
    }

    fn paint(
        &mut self,
        context: &ID3D11DeviceContext,
        reason: RestartReason,
    ) -> anyhow::Result<()> {
        let size = Vec2::new(WIDTH_PX as f32, HEIGHT_PX as f32);
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let headline = reason.headline();
        let detail = reason.detail();
        let output = self.egui.run_ui(input, |ui| {
            let painter = ui.painter();
            let panel = Rect::from_min_size(Pos2::ZERO, size);
            painter.rect_filled(panel, MARGIN_PX, BACKGROUND);
            painter.rect_filled(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(MARGIN_PX * 0.5, size.y)),
                0.0,
                ACCENT,
            );
            let width = size.x - 2.0 * MARGIN_PX;
            let mut job = LayoutJob::default();
            job.wrap.max_width = width;
            job.append(
                headline,
                0.0,
                egui::TextFormat::simple(FontId::proportional(HEADLINE_PX), TEXT),
            );
            job.append(
                &format!("\n{detail}"),
                0.0,
                egui::TextFormat::simple(FontId::proportional(DETAIL_PX), DIM),
            );
            let galley = painter.layout_job(job);
            let y = ((size.y - galley.size().y) * 0.5).max(MARGIN_PX * 0.5);
            painter.galley(Pos2::new(MARGIN_PX, y), galley, TEXT);
        });
        let (output, _, _) = egui_directx11::split_output(output);
        // SAFETY: the target's view is live, and the caller holds the engine context mutex.
        unsafe { context.ClearRenderTargetView(self.target.color_rtv(), &[0.0; 4]) };
        self.renderer
            .render(context, self.target.color_rtv(), &self.egui, output)
            .context("rendering the notice")?;
        self.painted = Some(reason);
        Ok(())
    }
}

/// The texture's size, pixels.
const WIDTH_PX: u32 = 1024;
const HEIGHT_PX: u32 = 256;
/// The panel's width, metres.
const WIDTH_M: f32 = 0.8;
/// Where the panel's centre sits in the head's frame, metres: ahead, and low enough to leave the
/// centre of the view clear.
const OFFSET_M: Vec3 = Vec3::new(0.0, -0.35, -1.4);
const MARGIN_PX: f32 = 28.0;
const HEADLINE_PX: f32 = 44.0;
const DETAIL_PX: f32 = 30.0;

/// The summary panel's palette: white text, grey detail, the accent red, and a dark translucent
/// backing.
const TEXT: Color32 = Color32::WHITE;
const DIM: Color32 = Color32::from_rgb(200, 200, 200);
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(10, 12, 15, 225);
const ACCENT: Color32 = Color32::from_rgb(200, 40, 30);
