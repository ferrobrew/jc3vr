//! The render side of the atlas: on eye 0, choose the capture for the displayed snapshot and size
//! each part's texture; on every eye, copy each moved part's cell out of the HUD texture into its
//! texture and clear the bands, so the panel shows the stage only.
//!
//! The UI renders the same snapshot into the HUD texture once per eye draw, and the texture is
//! cleared after each eye's panel, so the copies and the clearing are redone per eye. Everything
//! here runs on the render thread with the engine context mutex held.

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::{graphics_engine::device::Device, ui::ui_manager::UIManager};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_BOX, ID3D11DeviceContext};

use crate::hud::{
    atlas::{
        FrameStatus, STATUS, capture_for,
        layout::{Cell, band_texels, texel_rect},
    },
    target::HudTarget,
    world_markers::layout::TexelRect,
};

/// A part the atlas draws on a quad of its own.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Part {
    Ammo,
    Heat,
    Cross,
}

impl Part {
    const ALL: [Part; PARTS] = [Part::Ammo, Part::Heat, Part::Cross];

    const fn index(self) -> usize {
        match self {
            Part::Ammo => 0,
            Part::Heat => 1,
            Part::Cross => 2,
        }
    }
}

/// How many parts there are.
const PARTS: usize = 3;

/// Choose the frame's capture (eye 0). `active` is false while the HUD texture is not the gameplay
/// HUD's (not redirected, or full-screen UI), which drops the frame.
pub(crate) fn prepare_frame(device: &Device, hud: Option<&HudTarget>, active: bool) {
    let mut renderer = RENDERER.lock();
    renderer.frame = None;
    let Some(hud) = hud.filter(|_| active) else {
        STATUS.lock().frame = None;
        return;
    };
    // SAFETY: the UI manager and its movie are live singletons; the snapshot ids are plain integers
    // the render side reads.
    let displayed = unsafe {
        UIManager::get()
            .and_then(|m| m.m_Movie.as_ref())
            .map_or(0, |movie| movie.RenderContext.SnapshotFrameIds[2])
    };
    let Some((capture, matched)) = capture_for(displayed) else {
        STATUS.lock().frame = None;
        return;
    };
    let texture = hud.size();
    let Some(bands) = band_texels(&capture.frame, texture, capture.layout.stage) else {
        STATUS.lock().frame = None;
        return;
    };
    let mut parts = [None; PARTS];
    for (part, cell) in Part::ALL
        .iter()
        .zip([capture.ammo, capture.heat, capture.cross])
    {
        let Some(cell) = cell else {
            continue;
        };
        let Some(rect) = texel_rect(&capture.frame, texture, &cell.rect) else {
            continue;
        };
        if let Err(e) = renderer.ensure_slot(device, *part, (rect.width(), rect.height())) {
            log_once(&e);
            continue;
        }
        parts[part.index()] = Some((rect, cell));
    }
    if let Err(e) =
        renderer.ensure_blank(device, texture.0, bands[0].height().max(bands[1].height()))
    {
        log_once(&e);
        return;
    }
    renderer.frame = Some(Frame { bands, parts });
    STATUS.lock().frame = Some(FrameStatus {
        snapshot: capture.snapshot,
        displayed_snapshot: displayed,
        matched,
        layout: capture.layout.clone(),
        ammo: capture.ammo,
        heat: capture.heat,
        cross: capture.cross,
        texture,
    });
}

/// Drop the frame's capture (eye 0, when the panel is not drawn).
pub(crate) fn clear_frame() {
    RENDERER.lock().frame = None;
    STATUS.lock().frame = None;
}

/// Copy this eye's moved parts out of the HUD texture and clear the bands. Call on every eye after
/// the UI rendered into the HUD texture, after the world markers took their crops, and before the
/// panel draws.
pub(crate) fn prepare_eye(context: &ID3D11DeviceContext, hud: Option<&HudTarget>) {
    let renderer = RENDERER.lock();
    let (Some(frame), Some(hud)) = (renderer.frame.as_ref(), hud) else {
        return;
    };
    // SAFETY: every texture is ours (the HUD target, the part slots, the blank), live, and of the
    // same format; the context is the engine's immediate context under its mutex.
    unsafe {
        if renderer.blank_dirty.swap(false, Ordering::Relaxed)
            && let Some(blank) = renderer.blank.as_ref()
        {
            context.ClearRenderTargetView(blank.color_rtv(), &[0.0; 4]);
        }
        for part in Part::ALL {
            let (Some((rect, _)), Some(slot)) = (
                frame.parts[part.index()],
                renderer.slots[part.index()].as_ref(),
            ) else {
                continue;
            };
            context.CopySubresourceRegion(
                slot.color_texture(),
                0,
                0,
                0,
                0,
                hud.color_texture(),
                0,
                Some(&texel_box(rect)),
            );
        }
        let Some(blank) = renderer.blank.as_ref() else {
            return;
        };
        for band in frame.bands {
            context.CopySubresourceRegion(
                hud.color_texture(),
                0,
                band.x0,
                band.y0,
                0,
                blank.color_texture(),
                0,
                Some(&D3D11_BOX {
                    left: 0,
                    top: 0,
                    front: 0,
                    right: band.width(),
                    bottom: band.height(),
                    back: 1,
                }),
            );
        }
    }
}

/// A moved part's texture for this frame: its cell's copy and the cell it came from.
pub(crate) struct PartTexture<'a> {
    pub target: &'a HudTarget,
    pub cell: Cell,
}

/// Run `f` with a moved part's texture, or return `None` when the displayed capture did not move
/// the part.
pub(crate) fn part_texture<R>(part: Part, f: impl FnOnce(PartTexture<'_>) -> R) -> Option<R> {
    let renderer = RENDERER.lock();
    let (_, cell) = renderer.frame.as_ref()?.parts[part.index()]?;
    let target = renderer.slots[part.index()].as_ref()?;
    Some(f(PartTexture { target, cell }))
}

/// The render-side state.
static RENDERER: Mutex<Renderer> = Mutex::new(Renderer {
    slots: [None, None, None],
    blank: None,
    blank_dirty: AtomicBool::new(false),
    frame: None,
});

struct Renderer {
    /// Each part's texture, sized to its cell's texels.
    slots: [Option<HudTarget>; PARTS],
    /// A transparent-black texture as wide as the HUD texture and as tall as a band, copied over
    /// the bands.
    blank: Option<HudTarget>,
    /// Whether the blank texture still needs its clear (cleared by the first eye that copies it).
    blank_dirty: AtomicBool,
    frame: Option<Frame>,
}

/// The frame's texel rectangles in the HUD texture.
struct Frame {
    bands: [TexelRect; 2],
    /// Each moved part's texels and cell, indexed by [`Part::index`].
    parts: [Option<(TexelRect, Cell)>; PARTS],
}

impl Renderer {
    fn ensure_slot(&mut self, device: &Device, part: Part, size: (u32, u32)) -> anyhow::Result<()> {
        let slot = &mut self.slots[part.index()];
        if slot.as_ref().map(HudTarget::size) != Some(size) {
            *slot = None;
            *slot = Some(HudTarget::new(device, size.0, size.1)?);
        }
        Ok(())
    }

    fn ensure_blank(&mut self, device: &Device, width: u32, height: u32) -> anyhow::Result<()> {
        let size = (width.max(1), height.max(1));
        if self.blank.as_ref().map(HudTarget::size) != Some(size) {
            self.blank = None;
            self.blank = Some(HudTarget::new(device, size.0, size.1)?);
            self.blank_dirty.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// The copy box for a texel rectangle.
fn texel_box(rect: TexelRect) -> D3D11_BOX {
    D3D11_BOX {
        left: rect.x0,
        top: rect.y0,
        front: 0,
        right: rect.x1,
        bottom: rect.y1,
        back: 1,
    }
}

/// Log a resource failure once per session; the next frame retries regardless.
fn log_once(error: &anyhow::Error) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("hud atlas: {error:#}");
    }
}
