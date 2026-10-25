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
//!
//! The current usable hints occupy fixed physical left/right columns. Layout measures wrapped
//! rows at a readable size and paginates overflow without shrinking. A visible-time clock cycles
//! pages with a page indicator; no controller input is assigned to this panel.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use egui::{
    Color32, FontFamily, FontId, Pos2, Rect, TexturesDelta, Vec2,
    text::{LayoutJob, TextFormat},
};
use glam::Vec4;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

use crate::{
    config::Config,
    controllers::{
        bindings::tables::describe::{Control, StickDir},
        frame::{self, Button, InteractionProfile, Pose},
        hands::late,
        scheme,
        summary::{
            self, Published, SummaryConfig,
            content::Metrics,
            current::{CellKind, CurrentSummary, Hint},
            glance::{Glance, TrackpadPin, fade, looking, wrist_pose},
            grid::{Bounds, Grid, layout},
            page_clock::{PageClock, PageTime},
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
                c.controllers.acting_hand(),
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
    /// The panel texture's size, pixels, and its row height, as last laid out.
    pub layout: Option<PanelLayout>,
}

/// The panel's size for the current summary.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PanelLayout {
    /// The texture's size, pixels.
    pub size: (u32, u32),
    /// The configured readable row height, independent of the number of hints.
    pub row_px: f32,
    /// The visible page and total, both one-based.
    pub page: usize,
    pub pages: usize,
}

pub fn status() -> PanelStatus {
    PANEL.lock().status
}

static PANEL: Mutex<Panel> = Mutex::new(Panel::new());

struct Panel {
    glance: Glance,
    trackpad_pin: TrackpadPin,
    pages: PageClock,
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
            pages: PageClock::new(),
            alpha: 0.0,
            last_prepare: None,
            corners: None,
            status: PanelStatus {
                alpha: 0.0,
                glance: false,
                pinned: false,
                mode_recent: false,
                layout: None,
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
        let grip = late::drawn_hands(hooks::camera::last_dtf())[off.index()].map(|h| h.grip);
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
        if !shown || published.is_none() {
            self.pages.pause();
        }
        self.alpha = fade(self.alpha, shown, dt, config.fade_s);
        self.status = PanelStatus {
            alpha: self.alpha,
            glance,
            pinned,
            mode_recent,
            layout: self.status.layout,
        };
        let (Some(pose), Some(published)) = (pose, published) else {
            return;
        };
        if self.alpha <= 0.0 {
            return;
        }
        summary::note_shown();

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
        let key = LayoutKey {
            revision: published.wrist_revision,
            width: config.width_px,
            row_px_bits: config.row_px.to_bits(),
            max_height: config.max_height_px,
        };
        gpu.measure(&published.current, key, config.row_px);
        let count = gpu
            .measured
            .as_ref()
            .map_or(1, |(_, grid)| grid.pages.len());
        let page = self
            .pages
            .step(published.wrist_revision, count, shown, shown && glance, dt);
        let layout = match gpu.paint(context, device, &published, self.alpha, page) {
            Ok(layout) => layout,
            Err(e) => {
                tracing::error!("controls summary panel: {e:#}");
                return;
            }
        };
        self.status.layout = Some(layout);
        let (width, height) = layout.size;
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

/// The panel's egui context, renderer, render target, and quad pass.
struct Gpu {
    egui: egui::Context,
    renderer: egui_directx11::Renderer,
    quad: HudQuad,
    target: Option<HudTarget>,
    started: Instant,
    /// The layout of the summary last measured, and what it was measured for.
    measured: Option<(LayoutKey, Grid)>,
    /// The texture updates of a measuring pass, which the next painted frame uploads.
    pending_textures: Option<TexturesDelta>,
}

/// What a measured layout depends on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct LayoutKey {
    revision: u64,
    width: u32,
    row_px_bits: u32,
    max_height: u32,
}

impl Gpu {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let egui = egui::Context::default();
        egui.set_fonts(fonts());
        Ok(Self {
            renderer: egui_directx11::Renderer::new(&device.m_Device)
                .context("creating the egui renderer")?,
            quad: HudQuad::new(device).context("creating the quad pass")?,
            egui,
            target: None,
            started: Instant::now(),
            measured: None,
            pending_textures: None,
        })
    }

    /// Paint the published summary at `alpha` into the panel target, (re)creating it at the
    /// summary's measured size, which is returned.
    fn paint(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        published: &Published,
        alpha: f32,
        page: PageTime,
    ) -> anyhow::Result<PanelLayout> {
        let Some((_, grid)) = self.measured.as_ref() else {
            anyhow::bail!("summary panel: layout is unavailable");
        };
        let layout = PanelLayout {
            size: grid.size,
            row_px: grid.metrics.row_px,
            page: page.index + 1,
            pages: page.count,
        };
        let size = layout.size;
        if self.target.as_ref().map(HudTarget::size) != Some(size) {
            self.target = None;
            self.target =
                Some(HudTarget::new(device, size.0, size.1).context("creating the panel target")?);
        }
        let Some(target) = self.target.as_ref() else {
            return Ok(layout);
        };
        let screen = Vec2::new(size.0 as f32, size.1 as f32);
        let summary = &published.current;
        let output = self.egui.run_ui(self.input(screen), |ui| {
            let mut painter = ui.painter().clone();
            painter.set_opacity(alpha);
            paint_summary(&painter, summary, grid, page);
        });
        let (mut output, _, _) = egui_directx11::split_output(output);
        // The measuring pass's texture updates (the font atlas, laid out there first) must reach
        // the renderer, or the text samples a texture that was never uploaded.
        if let Some(mut pending) = self.pending_textures.take() {
            pending.append(output.textures_delta);
            output.textures_delta = pending;
        }
        // SAFETY: the target's view is live, and the caller holds the engine context mutex.
        unsafe { context.ClearRenderTargetView(target.color_rtv(), &[0.0; 4]) };
        self.renderer
            .render(context, target.color_rtv(), &self.egui, output)
            .context("rendering the summary")?;
        Ok(layout)
    }

    /// Measure the same fixed cells that the paint pass uses, without a GPU draw.
    fn measure(&mut self, summary: &CurrentSummary, key: LayoutKey, row_px: f32) {
        if self
            .measured
            .as_ref()
            .is_some_and(|(measured, _)| *measured == key)
        {
            return;
        }
        let mut grid = None;
        let output = self.egui.run_ui(
            self.input(Vec2::new(key.width as f32, key.max_height as f32)),
            |ui| {
                grid = Some(measure_summary(
                    ui.painter(),
                    summary,
                    key.width,
                    key.max_height,
                    row_px,
                ));
            },
        );
        let (output, _, _) = egui_directx11::split_output(output);
        match self.pending_textures.as_mut() {
            Some(pending) => pending.append(output.textures_delta),
            None => self.pending_textures = Some(output.textures_delta),
        }
        self.measured = grid.map(|grid| (key, grid));
    }

    fn input(&self, screen: Vec2) -> egui::RawInput {
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
        input
    }
}

/// The production measurement path, also used by renderer-free tests.
fn measure_summary(
    painter: &egui::Painter,
    summary: &CurrentSummary,
    width: u32,
    height: u32,
    row_px: f32,
) -> Grid {
    layout(summary, width, height, row_px, &|hint, width, m| {
        let galley = painter.layout_job(hint_job(hint, width, m));
        (galley.size().y + m.margin * 0.5).max(m.row_px)
    })
}

/// Paint one measured page. Cell geometry does not depend on handedness.
fn paint_summary(painter: &egui::Painter, summary: &CurrentSummary, grid: &Grid, page: PageTime) {
    let m = grid.metrics;
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(grid.size.0 as f32, grid.size.1 as f32),
        ),
        m.margin,
        BACKGROUND,
    );
    paint_text(
        painter,
        &summary.heading,
        grid.heading,
        m.heading_px * 0.55,
        TEXT,
    );
    let Some(selected) = grid.pages.get(page.index) else {
        return;
    };
    for cell in &selected.cells {
        let title = match cell.kind {
            CellKind::Left => "Left hand",
            CellKind::Right => "Right hand",
            CellKind::Available => "Available here",
            CellKind::ViewMenu => "View and menu",
        };
        let header = Bounds {
            height: m.group_px,
            ..cell.bounds
        };
        paint_text(painter, title, header, m.font_px, ACCENT);
        for &(index, bounds) in &cell.rows {
            let hint = &summary.cells[cell.kind.index()][index];
            let galley = painter.layout_job(hint_job(hint, bounds.width, m));
            painter.galley(Pos2::new(bounds.x, bounds.y), galley, TEXT);
        }
    }
    let footer = if summary.cells.iter().all(Vec::is_empty) {
        "No usable controls here".to_owned()
    } else if page.count > 1 {
        format!(
            "Page {}/{} - next in {:.0}s | Input/bindings: full reference",
            page.index + 1,
            page.count,
            page.remaining_s.ceil()
        )
    } else {
        "Page 1/1 | Input/bindings: full reference".to_owned()
    };
    paint_text(painter, &footer, grid.footer, m.font_px, DIM);
}

fn hint_job(hint: &Hint, width: f32, m: Metrics) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(1.0);
    let font = FontId::proportional(m.font_px);
    let sources = hint
        .sources
        .iter()
        .map(|source| {
            source
                .name
                .clone()
                .unwrap_or_else(|| control_name(source.control).to_owned())
        })
        .collect::<Vec<_>>()
        .join(" / ");
    job.append(
        &sources,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color: SOURCE,
            ..Default::default()
        },
    );
    if let Some(prefix) = &hint.prefix {
        job.append(
            &format!(" {prefix}"),
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: DIM,
                ..Default::default()
            },
        );
    }
    job.append(
        &format!(" {}", hint.text),
        0.0,
        TextFormat {
            font_id: font,
            color: TEXT,
            ..Default::default()
        },
    );
    job
}

fn control_name(control: Control) -> &'static str {
    match control {
        Control::Trigger(_) => "Trigger",
        Control::Grip(_) | Control::EitherGrip => "Grip",
        Control::A(_) | Control::EitherA => "Lower button",
        Control::B(_) | Control::EitherB => "Upper button",
        Control::Stick(_, StickDir::Any) | Control::EitherStick(StickDir::Any) => "Stick",
        Control::Stick(_, StickDir::Vertical) | Control::EitherStick(StickDir::Vertical) => {
            "Stick up/down"
        }
        Control::Stick(_, StickDir::Horizontal) | Control::EitherStick(StickDir::Horizontal) => {
            "Stick left/right"
        }
        Control::Stick(_, StickDir::Down) | Control::EitherStick(StickDir::Down) => "Stick down",
        Control::StickClick(_) => "Stick click",
        Control::Trackpad(_) | Control::EitherTrackpad => "Trackpad",
        Control::Motion(_) | Control::EitherMotion => "Motion",
        Control::BothStickClicks => "Both stick clicks",
        Control::System => "System",
        Control::LeftStickClickAndRightB => "Left stick click + right upper button",
        Control::Named(..) => "",
        Control::Unbound => "Unbound",
    }
}

fn paint_text(painter: &egui::Painter, text: &str, bounds: Bounds, size: f32, color: Color32) {
    let galley = painter.layout(
        text.to_owned(),
        FontId::proportional(size),
        color,
        bounds.width,
    );
    painter.galley(Pos2::new(bounds.x, bounds.y), galley, color);
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
/// The source control's compact text label.
const SOURCE: Color32 = Color32::from_rgb(235, 200, 120);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{frame::Hand, summary::current::Source};

    #[test]
    fn compact_source_names_preserve_stick_directions() {
        for hand in Hand::BOTH {
            assert_eq!(control_name(Control::Stick(hand, StickDir::Any)), "Stick");
            assert_eq!(
                control_name(Control::Stick(hand, StickDir::Vertical)),
                "Stick up/down"
            );
            assert_eq!(
                control_name(Control::Stick(hand, StickDir::Horizontal)),
                "Stick left/right"
            );
            assert_eq!(
                control_name(Control::Stick(hand, StickDir::Down)),
                "Stick down"
            );
        }
        assert_eq!(
            control_name(Control::EitherStick(StickDir::Down)),
            "Stick down"
        );
    }

    #[test]
    fn production_wrapped_rows_keep_font_size_and_measured_page_bounds() {
        let context = egui::Context::default();
        context.set_fonts(fonts());
        let mut summary = CurrentSummary {
            heading: "On foot".to_owned(),
            ..Default::default()
        };
        summary.cells[0] = (0..5)
            .map(|_| Hint {
                sources: vec![Source {
                    control: Control::A(Hand::Left),
                    name: Some("Localized physical controller source".to_owned()),
                }],
                prefix: Some("Hold".to_owned()),
                text:
                    "A long localized action description that wraps across several readable lines"
                        .repeat(3),
            })
            .collect();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let grid = measure_summary(ui.painter(), &summary, 480, 400, 32.0);
            assert_eq!(grid.metrics.row_px, 32.0);
            assert!(grid.pages.len() > 1);
            let mut seen = 0;
            for (index, page) in grid.pages.iter().enumerate() {
                for cell in &page.cells {
                    for (row, bounds) in &cell.rows {
                        let galley = ui.painter().layout_job(hint_job(
                            &summary.cells[0][*row],
                            bounds.width,
                            grid.metrics,
                        ));
                        assert!(bounds.height >= galley.size().y);
                        assert!(bounds.y + bounds.height <= grid.footer.y);
                        seen += 1;
                    }
                }
                paint_summary(
                    ui.painter(),
                    &summary,
                    &grid,
                    PageTime {
                        index,
                        count: grid.pages.len(),
                        remaining_s: 8.0,
                    },
                );
            }
            assert_eq!(seen, 5);
        });
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn production_measure_and_paint_keep_columns_and_expose_every_page() {
        for count in [0usize, 1, 11] {
            let context = egui::Context::default();
            context.set_fonts(fonts());
            let mut summary = CurrentSummary {
                heading: "On foot".to_owned(),
                ..Default::default()
            };
            for hand in Hand::BOTH {
                summary.cells[hand.index()] = (0..count)
                    .map(|index| Hint {
                        sources: vec![Source {
                            control: Control::Trigger(hand),
                            name: None,
                        }],
                        prefix: None,
                        text: format!("Action {index}"),
                    })
                    .collect();
            }
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                let grid = measure_summary(ui.painter(), &summary, 480, 1280, 32.0);
                assert_eq!(grid.metrics.row_px, 32.0);
                assert_eq!(grid.pages.len(), count.max(1).div_ceil(4));
                let mut seen = [0; 2];
                for (page_index, page) in grid.pages.iter().enumerate() {
                    if let [left, right] = page.cells.as_slice() {
                        assert!(left.bounds.x < right.bounds.x);
                        assert_eq!(left.bounds.y, right.bounds.y);
                        assert_eq!(left.bounds.width, right.bounds.width);
                    }
                    for cell in &page.cells {
                        seen[cell.kind.index()] += cell.rows.len();
                        for (_, row) in &cell.rows {
                            assert!(row.y + row.height <= grid.footer.y);
                        }
                    }
                    paint_summary(
                        ui.painter(),
                        &summary,
                        &grid,
                        PageTime {
                            index: page_index,
                            count: grid.pages.len(),
                            remaining_s: 8.0,
                        },
                    );
                }
                assert_eq!(seen, [count, count]);
            });
            assert!(!output.shapes.is_empty());
        }
    }
}
