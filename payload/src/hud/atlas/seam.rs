//! The game side of the atlas: at the capture seam, lay out the cells, move the weapon, heat, and
//! tether readouts into theirs, and after the capture move them back and publish the layout keyed by the
//! snapshot the capture produced.
//!
//! Both halves run in the `MovieImpl::Capture` detour, on the thread that runs
//! `CUIManager::PreRender`, with every display-tree writer quiescent. The moves are display-info
//! writes of position and scale through the cached clip handles, the same kind of write the
//! overlay suppression makes there; nothing in the game or the movie's ActionScript sees them,
//! because they are undone before the next advance.

use glam::Vec2;
use jc3gi::ui::{
    hud_ui::{HUDState, HUDUI},
    scaleform::{DisplayInfo, MovieImpl, Value},
    ui_manager::UIManager,
};
use parking_lot::Mutex;

use crate::{
    controllers::scheme,
    hud::{
        HudMode,
        atlas::{
            AtlasCapture, STATUS,
            layout::{
                AtlasLayout, Cell, LayoutRequest, PartRequest, Placement, extents_from_edges,
                layout, local_placement,
            },
            publish,
        },
        config::{CropExtents, HudConfig},
        scaleform::{self, AtlasClip, CLIP_HANDLES, ClipHandle},
        state::applied_redirect,
        world_markers::{self, layout::StageFrame},
    },
};

/// Lay out the atlas for this capture and move the readouts into their cells. Returns the layout
/// when the atlas is active, for the world markers to move their clips into its marker cells.
///
/// # Safety
/// Must be called from the `MovieImpl::Capture` detour for the main movie, before the original.
pub(crate) unsafe fn begin_capture(cfg: &HudConfig) -> Option<AtlasLayout> {
    let mut pending = PENDING.lock();
    *pending = None;
    let (layout, frame) = active_layout(cfg)?;
    let mut moves = Vec::new();
    let mut moved = MovedParts::default();
    let mut failures = 0;
    {
        let mut handles = CLIP_HANDLES.lock();
        let atlas = handles
            .as_mut()
            .filter(|_| scaleform::discovered_in_hud())
            .map(|h| &mut h.atlas)
            .filter(|atlas| atlas.len() == scaleform::ATLAS_CLIPS.len());
        if let Some(atlas) = atlas {
            // SAFETY: the capture seam, per the function contract.
            let parent = unsafe { placement(&mut atlas[AtlasClip::BottomRight as usize]) };
            for (clip, cell) in [
                (AtlasClip::Weapons, layout.ammo),
                (AtlasClip::Heat, layout.heat),
                (AtlasClip::Tether, layout.tether),
            ] {
                let (Some(parent), Some(cell)) = (parent, cell) else {
                    continue;
                };
                let Some(value) = atlas[clip as usize].value.as_deref_mut() else {
                    failures += 1;
                    continue;
                };
                // SAFETY: as above.
                match unsafe { move_into(value, Target::Clip(clip), parent, &cell) } {
                    Some(saved) => {
                        moves.push((Target::Clip(clip), saved));
                        match clip {
                            AtlasClip::Weapons => moved.ammo = Some(cell),
                            AtlasClip::Heat => moved.heat = Some(cell),
                            AtlasClip::Tether => moved.tether = Some(cell),
                            AtlasClip::BottomRight => {}
                        }
                    }
                    None => failures += 1,
                }
            }
        }
    }
    // The selection cross moves only while the wheel shows it at a hand; otherwise it stays where
    // the wheel or the game placed it on the panel.
    if let Some(cell) = layout.cross
        && crate::controllers::weapons::wheel::cross_anchor().is_some()
        // SAFETY: the HUD is a live singleton in gameplay, read on the capture thread.
        && let Some(hud) = unsafe { HUDUI::get() }
        && hud.m_SelectionWheelUi.value_type() != 0
    {
        // SAFETY: the cross is the HUD's own managed clip value, on the capture seam; it is a child
        // of the HUD clip, which sits at the stage's origin unscaled.
        match unsafe { move_into(&mut hud.m_SelectionWheelUi, Target::Cross, STAGE, &cell) } {
            Some(saved) => {
                moves.push((Target::Cross, saved));
                moved.cross = Some(cell);
            }
            None => failures += 1,
        }
    }
    {
        let mut status = STATUS.lock();
        status.moves += moves.len() as u64;
        status.move_failures += failures;
    }
    *pending = Some(Pending {
        layout: layout.clone(),
        frame,
        moved,
        moves,
    });
    Some(layout)
}

/// Move the readouts back and publish the capture's layout, keyed by the snapshot it produced.
///
/// # Safety
/// Must be called from the `MovieImpl::Capture` detour for the main movie, after the original.
pub(crate) unsafe fn after_capture(movie: &MovieImpl) {
    let Some(pending) = PENDING.lock().take() else {
        publish(None);
        return;
    };
    let mut restored = 0u64;
    let mut failed = 0u64;
    {
        let mut handles = CLIP_HANDLES.lock();
        for (target, saved) in &pending.moves {
            let value = match target {
                Target::Clip(clip) => handles
                    .as_mut()
                    .and_then(|h| h.atlas.get_mut(*clip as usize))
                    .and_then(|handle| handle.value.as_deref_mut()),
                // SAFETY: the HUD is a live singleton in gameplay, read on the capture thread.
                Target::Cross => unsafe { HUDUI::get() }.map(|hud| &mut hud.m_SelectionWheelUi),
            };
            // SAFETY: the capture seam, per the function contract; the value is the one moved.
            if let Some(value) = value
                && unsafe { write_info(value, &saved.write()) }
            {
                restored += 1;
                if let Target::Clip(clip) = target {
                    LAST_RESTORED.lock()[clip_index(*clip)] = Some(saved.position);
                }
            } else {
                failed += 1;
            }
        }
    }
    {
        let mut status = STATUS.lock();
        status.restores += restored;
        status.restore_failures += failed;
    }
    publish(Some(AtlasCapture {
        snapshot: movie.RenderContext.SnapshotFrameIds[0],
        frame: pending.frame,
        ammo: pending.moved.ammo,
        heat: pending.moved.heat,
        cross: pending.moved.cross,
        tether: pending.moved.tether,
        layout: pending.layout,
    }));
}

/// The parts moved for one capture.
#[derive(Debug, Default, Copy, Clone)]
struct MovedParts {
    ammo: Option<Cell>,
    heat: Option<Cell>,
    cross: Option<Cell>,
    tether: Option<Cell>,
}

/// A clip the atlas moves.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Target {
    /// A clip of the handle registry.
    Clip(AtlasClip),
    /// The weapon selection cross, through the HUD's own value for it.
    Cross,
}

/// One capture's layout and moves, between the two halves of the seam.
struct Pending {
    layout: AtlasLayout,
    frame: StageFrame,
    moved: MovedParts,
    moves: Vec<(Target, Saved)>,
}

/// A moved clip's own placement, written back after the capture.
#[derive(Debug, Copy, Clone)]
struct Saved {
    position: Vec2,
    /// In percent, as read.
    scale: Vec2,
}

impl Saved {
    /// The display-info write that restores the placement, with the exact values read.
    fn write(&self) -> DisplayInfo {
        let mut info = DisplayInfo::default();
        info.X = f64::from(self.position.x);
        info.Y = f64::from(self.position.y);
        info.XScale = f64::from(self.scale.x);
        info.YScale = f64::from(self.scale.y);
        info.VarsSet = PLACEMENT_VARS as u16;
        info
    }
}

/// The display-info fields a move writes: position and scale.
const PLACEMENT_VARS: u32 =
    DisplayInfo::V_X | DisplayInfo::V_Y | DisplayInfo::V_XSCALE | DisplayInfo::V_YSCALE;

/// The placement of a clip whose parent is the stage itself.
const STAGE: Placement = Placement {
    position: Vec2::ZERO,
    scale: Vec2::ONE,
};

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// The position each moved readout was last restored to, to notice a restore something else
/// overrode. The cross is left out: the HUD places it every update.
static LAST_RESTORED: Mutex<[Option<Vec2>; 3]> = Mutex::new([None, None, None]);

fn clip_index(clip: AtlasClip) -> usize {
    match clip {
        AtlasClip::Heat => 1,
        AtlasClip::Tether => 2,
        AtlasClip::Weapons | AtlasClip::BottomRight => 0,
    }
}

/// The atlas layout for this capture, or `None` when the atlas does not act: it is off, the HUD is
/// not the gameplay HUD or is hidden, or the applied redirect does not lay the HUD out inside the
/// stage yet (the cells would land on the edge containers).
fn active_layout(cfg: &HudConfig) -> Option<(AtlasLayout, StageFrame)> {
    if !(cfg.atlas.enabled && cfg.redirect && cfg.quad) {
        return None;
    }
    if crate::hud::current_mode() != HudMode::Hud {
        return None;
    }
    let applied = applied_redirect().filter(|a| a.stage_safe_area)?;
    // SAFETY: the HUD and UI manager are live singletons in gameplay, read on the capture thread.
    let (hud, manager) = unsafe { (HUDUI::get()?, UIManager::get()?) };
    if matches!(
        hud.m_State,
        HUDState::HUD_CINEMATIC | HUDState::HUD_HIDE_ALL | HUDState::HUD_HIDE_ALL_NON_OVERLAY
    ) {
        return None;
    }
    let stage = Vec2::new(manager.m_CachedStageWidth, manager.m_CachedStageHeight);
    let (width, height) = applied.texture;
    let frame = StageFrame::new(stage, width as f32 / height.max(1) as f32)?;
    let controllers = scheme::is_controllers();
    let atlas = &cfg.atlas;
    let markers = &cfg.world_markers;
    let request = LayoutRequest {
        stage,
        frame,
        gutter: atlas.gutter_px,
        ammo: (atlas.ammo.enabled && controllers).then_some(PartRequest {
            extents: atlas.ammo.extents,
            max_scale: atlas.ammo.max_scale,
        }),
        heat: (atlas.wrist.enabled && atlas.wrist.heat && controllers).then_some(PartRequest {
            extents: atlas.wrist.heat_extents,
            max_scale: atlas.wrist.heat_max_scale,
        }),
        cross: (atlas.cross.enabled && controllers)
            .then(|| extents_from_edges(hud.m_SelectionWheelBB.data, atlas.cross.margin_px))
            .flatten()
            .map(|extents| PartRequest {
                extents,
                max_scale: atlas.cross.max_scale,
            }),
        tether: (atlas.tether.enabled && controllers).then_some(PartRequest {
            extents: atlas.tether.extents,
            max_scale: atlas.tether.max_scale,
        }),
        label: (markers.enabled && atlas.markers).then_some(CropExtents {
            left: markers.crop.left + markers.label_extra_left.max(0.0),
            ..markers.crop
        }),
        marker: (markers.enabled && atlas.markers).then_some(markers.crop),
        max_markers: world_markers::MAX_MARKERS,
    };
    Some((layout(&request)?, frame))
}

/// A clip's placement in stage coordinates, from its display info. The safe-area containers are
/// children of the HUD clip, which sits at the stage's origin unscaled.
///
/// # Safety
/// The capture seam.
unsafe fn placement(handle: &mut ClipHandle) -> Option<Placement> {
    // SAFETY: forwarded from the caller.
    let info = unsafe { scaleform::display_info(handle)? };
    Some(Placement {
        position: Vec2::new(info.X as f32, info.Y as f32),
        scale: Vec2::new(info.XScale as f32, info.YScale as f32) / 100.0,
    })
}

/// Move a clip so its origin lands on its cell's origin at its cell's scale, returning its own
/// placement to restore, or `None` when it could not be read or written.
///
/// # Safety
/// `value` must be a live managed display-object value, on the capture seam.
unsafe fn move_into(
    value: &mut Value,
    target: Target,
    parent: Placement,
    cell: &Cell,
) -> Option<Saved> {
    // SAFETY: forwarded from the caller.
    let info = unsafe { read_info(value)? };
    let saved = Saved {
        position: Vec2::new(info.X as f32, info.Y as f32),
        scale: Vec2::new(info.XScale as f32, info.YScale as f32),
    };
    if let Target::Clip(clip) = target
        && let Some(last) = LAST_RESTORED.lock()[clip_index(clip)]
        && last != saved.position
    {
        STATUS.lock().drift += 1;
    }
    let local = local_placement(parent, cell.origin, cell.scale)?;
    let mut write = DisplayInfo::default();
    write.X = f64::from(local.position.x);
    write.Y = f64::from(local.position.y);
    write.XScale = f64::from(local.scale.x) * 100.0;
    write.YScale = f64::from(local.scale.y) * 100.0;
    write.VarsSet = PLACEMENT_VARS as u16;
    // SAFETY: forwarded from the caller.
    unsafe { write_info(value, &write) }.then_some(saved)
}

/// Read a clip's display info through its managed value.
///
/// # Safety
/// `value` must be a live managed display-object value, on the capture seam.
unsafe fn read_info(value: &mut Value) -> Option<DisplayInfo> {
    // SAFETY: forwarded from the caller.
    unsafe {
        let interface = value.pObjectInterface.as_mut()?;
        let mut info = DisplayInfo::default();
        interface
            .GetDisplayInfo(value.mValue as *mut std::ffi::c_void, &mut info)
            .then_some(info)
    }
}

/// Write the display-info fields `info.VarsSet` selects through a clip's managed value.
///
/// # Safety
/// `value` must be a live managed display-object value, on the capture seam.
unsafe fn write_info(value: &mut Value, info: &DisplayInfo) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe {
        value.pObjectInterface.as_mut().is_some_and(|interface| {
            interface.SetDisplayInfo(value.mValue as *mut std::ffi::c_void, info)
        })
    }
}
