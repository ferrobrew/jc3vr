//! The game side of the atlas: at the capture seam, lay out the cells, move the weapon, heat, and
//! tether readouts and the aim part's clips into theirs, and after the capture move them back and
//! publish the layout keyed by the snapshot the capture produced.
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
                AtlasLayout, Cell, LayoutRequest, PartRequest, Placement, carried_placement,
                compose, extents_from_edges, layout, local_placement,
            },
            publish,
        },
        config::HudConfig,
        scaleform::{self, AtlasClip, CLIP_HANDLES, ClipHandle},
        state::applied_redirect,
        world_markers::{
            self,
            layout::{CropExtents, StageFrame},
        },
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
                            _ => {}
                        }
                    }
                    None => failures += 1,
                }
            }
            if let Some(cell) = layout.aim {
                // SAFETY: as above.
                let carried = unsafe { carry_aim(atlas, &cell) };
                failures += carried.failures;
                if !carried.moves.is_empty() {
                    moved.aim = Some(cell);
                }
                moves.extend(carried.moves);
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
                if let Target::Clip(clip) = target
                    && let Some(index) = drift_index(*clip)
                {
                    LAST_RESTORED.lock()[index] = Some(saved.position);
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
        aim: pending.moved.aim,
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
    aim: Option<Cell>,
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
/// overrode. The cross and the aim part's clips are left out: the HUD places them every update.
static LAST_RESTORED: Mutex<[Option<Vec2>; 3]> = Mutex::new([None, None, None]);

/// The readout's slot in [`LAST_RESTORED`], for the clips whose placement only the atlas changes.
fn drift_index(clip: AtlasClip) -> Option<usize> {
    match clip {
        AtlasClip::Weapons => Some(0),
        AtlasClip::Heat => Some(1),
        AtlasClip::Tether => Some(2),
        _ => None,
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
        gutter: GUTTER_PX,
        ammo: (atlas.ammo.enabled && controllers).then_some(AMMO),
        heat: (atlas.wrist.enabled && controllers).then_some(HEAT),
        cross: (atlas.cross.enabled && controllers)
            .then(|| extents_from_edges(hud.m_SelectionWheelBB.data, CROSS_MARGIN_PX))
            .flatten()
            .map(|extents| PartRequest {
                extents,
                max_scale: CROSS_MAX_SCALE,
            }),
        tether: (atlas.tether.enabled && controllers).then_some(TETHER),
        aim: Some(AIM),
        label: markers.enabled.then_some(world_markers::LABEL_CROP),
        marker: markers.enabled.then_some(world_markers::CROP),
        max_markers: world_markers::MAX_MARKERS,
    };
    Some((layout(&request)?, frame))
}

/// The empty margin around every cell, in stage pixels, wider than the glows and shadows the moved
/// parts draw past their bounds.
const GUTTER_PX: f32 = 6.0;

/// The weapon readout's extents around its clip's origin, in the clip's own pixels, and the largest
/// scale it is captured at; it is scaled down further to fit a band.
const AMMO: PartRequest = PartRequest {
    extents: CropExtents {
        left: 25.0,
        right: 285.0,
        up: 90.0,
        down: 175.0,
    },
    max_scale: 1.0,
};

/// The heat readout's extents and largest scale, as [`AMMO`].
const HEAT: PartRequest = PartRequest {
    extents: CropExtents {
        left: 140.0,
        right: 170.0,
        up: 110.0,
        down: 30.0,
    },
    max_scale: 1.0,
};

/// Room added around the selection cross's own extent (which the game reports), in its own pixels,
/// for its glows and its arms' highlight animations.
const CROSS_MARGIN_PX: f32 = 12.0;

/// The largest scale the selection cross is captured at; it is scaled down further to fit a band.
const CROSS_MAX_SCALE: f32 = 1.0;

/// The tether readout's extents and largest scale, as [`AMMO`].
const TETHER: PartRequest = PartRequest {
    extents: CropExtents {
        left: 60.0,
        right: 60.0,
        up: 25.0,
        down: 30.0,
    },
    max_scale: 1.0,
};

/// The aim part's extents around the aim point, in the center container's own pixels, and the
/// largest scale it is captured at, as [`AMMO`]. They hold every weapon reticle shape (the vehicle
/// cannon's and the mech's sit above the center, the grenade launcher's below it), the hit marker
/// at either of the heights the movie gives it, and the grapple reticle's halves spinning about the
/// aim point.
const AIM: PartRequest = PartRequest {
    extents: CropExtents {
        left: 60.0,
        right: 60.0,
        up: 125.0,
        down: 60.0,
    },
    max_scale: 1.0,
};

/// How near the grapple reticle's attached clip must sit to its base clip to count as at the base,
/// in the clips' parent's pixels.
const AT_BASE_PX: f32 = 0.5;

/// The aim part's moves for one capture.
#[derive(Default)]
struct CarriedAim {
    moves: Vec<(Target, Saved)>,
    failures: u64,
}

/// Carry the aim part's clips into `cell`: the weapon reticle group and the hit marker keep their
/// offsets from the center container's origin, the screen center the camera aims through, and the
/// grapple reticle's base and ghost land with their own origins, the projected grapple aim point,
/// on the cell's origin. The attached clip goes with them only while the movie holds it at the
/// base; while a wire is attached it marks the attach point and stays where the movie put it.
///
/// # Safety
/// The capture seam; `atlas` is the registry's atlas handles, in [`ATLAS_CLIPS`] order.
///
/// [`ATLAS_CLIPS`]: crate::hud::scaleform::ATLAS_CLIPS
unsafe fn carry_aim(atlas: &mut [ClipHandle], cell: &Cell) -> CarriedAim {
    let mut carried = CarriedAim::default();
    // SAFETY: forwarded from the caller.
    let (Some(center), Some(reticles), Some(grapple)) = (unsafe {
        (
            placement(&mut atlas[AtlasClip::Center as usize]),
            placement(&mut atlas[AtlasClip::Reticles as usize]),
            placement(&mut atlas[AtlasClip::GrappleReticle as usize]),
        )
    }) else {
        carried.failures += 1;
        return carried;
    };
    let reticles = compose(center, reticles);
    let grapple = compose(reticles, grapple);
    // SAFETY: forwarded from the caller.
    let (base, attached) = unsafe {
        (
            placement(&mut atlas[AtlasClip::GrappleBase as usize]),
            placement(&mut atlas[AtlasClip::GrappleAttached as usize]),
        )
    };
    let attached_at_base = base
        .zip(attached)
        .is_some_and(|(base, attached)| base.position.distance(attached.position) <= AT_BASE_PX);
    let mut clips = vec![
        (AtlasClip::WeaponReticle, reticles, Some(center.position)),
        (AtlasClip::HitMarker, center, Some(center.position)),
        (AtlasClip::GrappleBase, grapple, None),
        (AtlasClip::GrappleGhost, grapple, None),
    ];
    if attached_at_base {
        clips.push((AtlasClip::GrappleAttached, grapple, None));
    }
    for (clip, parent, anchor) in clips {
        let Some(value) = atlas[clip as usize].value.as_deref_mut() else {
            carried.failures += 1;
            continue;
        };
        // SAFETY: forwarded from the caller; the value is the registry's live clip handle.
        match unsafe { carry_into(value, parent, anchor, cell) } {
            Some(saved) => carried.moves.push((Target::Clip(clip), saved)),
            None => carried.failures += 1,
        }
    }
    carried
}

/// Carry a clip in a parent placed at `parent` into `cell`, the stage point `anchor` (the clip's
/// own origin when `None`) landing on the cell's origin, returning its own placement to restore, or
/// `None` when it could not be read or written.
///
/// # Safety
/// `value` must be a live managed display-object value, on the capture seam.
unsafe fn carry_into(
    value: &mut Value,
    parent: Placement,
    anchor: Option<Vec2>,
    cell: &Cell,
) -> Option<Saved> {
    // SAFETY: forwarded from the caller.
    let info = unsafe { read_info(value)? };
    let saved = Saved {
        position: Vec2::new(info.X as f32, info.Y as f32),
        scale: Vec2::new(info.XScale as f32, info.YScale as f32),
    };
    let own = Placement {
        position: saved.position,
        scale: saved.scale / 100.0,
    };
    let anchor = anchor.unwrap_or_else(|| compose(parent, own).position);
    let local = carried_placement(parent, own, anchor, cell)?;
    // SAFETY: forwarded from the caller.
    unsafe { write_placement(value, local) }.then_some(saved)
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
        && let Some(index) = drift_index(clip)
        && let Some(last) = LAST_RESTORED.lock()[index]
        && last != saved.position
    {
        STATUS.lock().drift += 1;
    }
    let local = local_placement(parent, cell.origin, cell.scale)?;
    // SAFETY: forwarded from the caller.
    unsafe { write_placement(value, local) }.then_some(saved)
}

/// Write a clip's position and scale in its parent through its managed value.
///
/// # Safety
/// `value` must be a live managed display-object value, on the capture seam.
unsafe fn write_placement(value: &mut Value, local: Placement) -> bool {
    let mut write = DisplayInfo::default();
    write.X = f64::from(local.position.x);
    write.Y = f64::from(local.position.y);
    write.XScale = f64::from(local.scale.x) * 100.0;
    write.YScale = f64::from(local.scale.y) * 100.0;
    write.VarsSet = PLACEMENT_VARS as u16;
    // SAFETY: forwarded from the caller.
    unsafe { write_info(value, &write) }
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
