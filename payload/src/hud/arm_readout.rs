//! The wingsuit arm steering's readout: a head-locked panel below the view that shows what the arm
//! steering reads and writes while it acts, for tuning it in the headset
//! ([`ArmSteeringConfig::show_readout_panel`](crate::controllers::vehicles::config::ArmSteeringConfig::show_readout_panel)).
//! It shows the spread and whether the arms steer, the bank and pitch inputs, the arms' stick
//! vector, the player's stick, and their sum as written to the game.
//!
//! Like the restart notice ([`crate::hud::restart_notice`]), it has its own egui context,
//! renderer, and render target, takes no input, and draws through the shared quad pass on both
//! eyes from corners computed on eye 0. It reads the vehicle handling's published readout slot
//! ([`vehicles::arm_panel`]), which the input tick fills only while the panel is switched on and
//! the arms act, and repaints at most every [`REPAINT_INTERVAL`], since the values change at the
//! input tick's rate. Nothing is created until the panel first shows.

use std::time::{Duration, Instant};

use anyhow::Context as _;
use egui::{Color32, FontId, Pos2, Rect, Vec2, text::LayoutJob};
use glam::{Vec3, Vec4};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    controllers::{
        summary::glance::PanelPose,
        vehicles::{self, ArmReadout, arm_steering::PitchInput},
    },
    hud::{quad::HudQuad, target::HudTarget},
};

/// Draw the panel for `eye` over `target`, while the readout is published and a VR frame renders.
/// Called from the render-thread post-draw hook with the engine context mutex held.
pub(crate) fn draw(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let mut panel = PANEL.lock();
    if eye == 0 {
        panel.prepare(context, device);
    }
    panel.draw(context, device, target);
}

static PANEL: Mutex<Panel> = Mutex::new(Panel {
    corners: None,
    gpu: None,
    failed: false,
});

struct Panel {
    corners: Option<[Vec4; 4]>,
    gpu: Option<Gpu>,
    /// Whether creating or painting the panel failed, so it is not retried every frame.
    failed: bool,
}

impl Panel {
    /// Eye 0: decide whether the panel shows, repaint it when due, and place it below the view.
    fn prepare(&mut self, context: &ID3D11DeviceContext, device: &Device) {
        self.corners = None;
        if self.failed || crate::vr::render_params(0).is_none() {
            return;
        }
        let Some(readout) = vehicles::arm_panel() else {
            return;
        };
        let Some((head, orientation)) = crate::hud::render_camera_pose() else {
            return;
        };
        if self.gpu.is_none() {
            match Gpu::new(device) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => {
                    tracing::error!("arm steering readout: {e:#}");
                    self.failed = true;
                    return;
                }
            }
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let due = gpu
            .painted_at
            .is_none_or(|at| at.elapsed() >= REPAINT_INTERVAL);
        if due && let Err(e) = gpu.paint(context, &readout) {
            tracing::error!("arm steering readout: {e:#}");
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
    /// When the target was last painted.
    painted_at: Option<Instant>,
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            egui: egui::Context::default(),
            renderer: egui_directx11::Renderer::new(&device.m_Device)
                .context("creating the egui renderer")?,
            quad: HudQuad::new(device).context("creating the quad pass")?,
            target: HudTarget::new(device, WIDTH_PX, HEIGHT_PX)
                .context("creating the readout target")?,
            painted_at: None,
        })
    }

    fn paint(&mut self, context: &ID3D11DeviceContext, readout: &ArmReadout) -> anyhow::Result<()> {
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
        let lines = lines(readout);
        let engaged = readout.arms.is_some();
        let output = self.egui.run_ui(input, |ui| {
            let painter = ui.painter();
            let panel = Rect::from_min_size(Pos2::ZERO, size);
            painter.rect_filled(panel, MARGIN_PX, BACKGROUND);
            painter.rect_filled(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(MARGIN_PX * 0.5, size.y)),
                0.0,
                if engaged { ENGAGED } else { IDLE },
            );
            let mut job = LayoutJob::default();
            job.wrap.max_width = size.x - 2.0 * MARGIN_PX;
            for (index, line) in lines.iter().enumerate() {
                let text = if index == 0 {
                    line.clone()
                } else {
                    format!("\n{line}")
                };
                job.append(
                    &text,
                    0.0,
                    egui::TextFormat::simple(FontId::monospace(TEXT_PX), TEXT),
                );
            }
            let galley = painter.layout_job(job);
            let y = ((size.y - galley.size().y) * 0.5).max(MARGIN_PX * 0.5);
            painter.galley(Pos2::new(MARGIN_PX, y), galley, TEXT);
        });
        let (output, _, _) = egui_directx11::split_output(output);
        // SAFETY: the target's view is live, and the caller holds the engine context mutex.
        unsafe { context.ClearRenderTargetView(self.target.color_rtv(), &[0.0; 4]) };
        self.renderer
            .render(context, self.target.color_rtv(), &self.egui, output)
            .context("rendering the readout")?;
        self.painted_at = Some(Instant::now());
        Ok(())
    }
}

/// The readout's text, one entry per line.
fn lines(readout: &ArmReadout) -> [String; 4] {
    let pair = |v: glam::Vec2| format!("({:+.2}, {:+.2})", v.x, v.y);
    let pitch = match readout.pitch {
        PitchInput::Height(m) => format!("height {m:+.2} m"),
        PitchInput::TiltDeg(deg) => format!("tilt {deg:+.0} deg"),
        PitchInput::Sweep(m) => format!("sweep {m:+.2} m"),
    };
    [
        format!(
            "WINGSUIT ARMS  {}  spread {:.0}% (needs {:.0}%)",
            if readout.arms.is_some() {
                "STEERING"
            } else {
                "NOT SPREAD"
            },
            readout.pose.spread * 100.0,
            readout.needed_spread * 100.0,
        ),
        format!("in   bank {:+.2} m   pitch {pitch}", readout.pose.bank_m),
        format!(
            "arms {}   stick {}",
            readout.arms.map_or("--".to_owned(), pair),
            pair(readout.stick),
        ),
        format!("out  {}  (+x right, +y dive)", pair(readout.written)),
    ]
}

/// How often the panel repaints while it shows.
const REPAINT_INTERVAL: Duration = Duration::from_millis(100);

/// The texture's size, pixels.
const WIDTH_PX: u32 = 1024;
const HEIGHT_PX: u32 = 256;
/// The panel's width, metres.
const WIDTH_M: f32 = 0.7;
/// Where the panel's centre sits in the head's frame, metres: ahead and low, out of the way of the
/// flight.
const OFFSET_M: Vec3 = Vec3::new(0.0, -0.45, -1.2);
const MARGIN_PX: f32 = 28.0;
const TEXT_PX: f32 = 34.0;

const TEXT: Color32 = Color32::WHITE;
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(10, 12, 15, 210);
/// The accent bar while the arms steer, and while they do not.
const ENGAGED: Color32 = Color32::from_rgb(90, 255, 150);
const IDLE: Color32 = Color32::from_rgb(200, 200, 200);
