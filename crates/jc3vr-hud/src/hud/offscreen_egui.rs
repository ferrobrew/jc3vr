//! An egui context drawn into a HUD target rather than a window: the mod's head- and hand-placed
//! panels lay out and paint their text with egui, then draw the target through the quad pass.
//!
//! The panels take no input, so a frame's input is only the screen, the time, and one pixel per
//! point, so that the texture's texels are egui's points.

use std::time::Instant;

use anyhow::Context as _;
use egui::{Pos2, Rect, Vec2};
use egui_directx11::RendererOutput;
use jc3gi::graphics_engine::device::Device;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::hud::target::HudTarget;

/// An egui context and the renderer that draws its output into a [`HudTarget`].
pub(crate) struct OffscreenEgui {
    egui: egui::Context,
    renderer: egui_directx11::Renderer,
    started: Instant,
}

impl OffscreenEgui {
    pub(crate) fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            egui: egui::Context::default(),
            renderer: egui_directx11::Renderer::new(&device.m_Device)
                .context("creating the egui renderer")?,
            started: Instant::now(),
        })
    }

    /// The egui context, for its fonts.
    pub(crate) fn context(&self) -> &egui::Context {
        &self.egui
    }

    /// Run one frame of `ui` over a screen of `size` points. The output's texture updates must
    /// reach [`render`](Self::render), merged into a later frame's when this one is not rendered.
    pub(crate) fn run(&self, size: Vec2, ui: impl FnMut(&mut egui::Ui)) -> RendererOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(self.started.elapsed().as_secs_f64()),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(1.0);
        let (output, _, _) = egui_directx11::split_output(self.egui.run_ui(input, ui));
        output
    }

    /// Clear `target` and draw `output` into it.
    ///
    /// The caller holds the engine context mutex.
    pub(crate) fn render(
        &mut self,
        context: &ID3D11DeviceContext,
        target: &HudTarget,
        output: RendererOutput,
    ) -> anyhow::Result<()> {
        // SAFETY: the target's view is live, and the caller holds the engine context mutex.
        unsafe { context.ClearRenderTargetView(target.color_rtv(), &[0.0; 4]) };
        self.renderer
            .render(context, target.color_rtv(), &self.egui, output)
            .context("rendering egui into a HUD target")
    }
}
