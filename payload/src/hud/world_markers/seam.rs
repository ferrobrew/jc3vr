//! The game side of the world markers: the classification override inside the HUD's
//! point-of-interest update, and the snapshot of the visible markers at the capture seam.
//!
//! Both run on the thread that runs `CUIManager::PreRender` (the HUD update, then the movie's
//! advance and capture), in that order, once per capture. Neither locks the VR runtime state: the
//! headset's field of view comes from the per-frame view record ([`crate::vr::capture_views`]).

use std::{cell::Cell, collections::HashMap};

use glam::{Vec2, Vec3};
use jc3gi::{
    types::{math::Matrix4, shared_ptr::SharedPtr},
    ui::{
        hud_ui::{HUDState, HUDUI, POIRecord},
        poi::{POI, ResetSharedPtrPOI},
        scaleform::{DisplayInfo, MovieImpl, Value},
        ui_manager::{ScreenPos, UIManager},
    },
};
use parking_lot::Mutex;
use windows::Win32::System::Threading::{EnterCriticalSection, LeaveCriticalSection};

use crate::hud::{
    HudMode,
    config::WorldMarkersConfig,
    world_markers::{
        CaptureLayout, MAX_MARKERS, MarkerRecord,
        layout::{FovTangents, clamp_into},
        publish_layout,
    },
};

/// Mark the start of `CHUDUI::UpdatePOIs` on this thread: the projections that follow are the
/// points of interest's, and the previous update's classifications are dropped.
pub(crate) fn begin_update_pois() {
    IN_UPDATE_POIS.with(|flag| flag.set(true));
    *CLASSIFIED.lock() = Some(HashMap::new());
}

/// Mark the end of `CHUDUI::UpdatePOIs` on this thread.
pub(crate) fn end_update_pois() {
    IN_UPDATE_POIS.with(|flag| flag.set(false));
}

/// The inputs of one point-of-interest projection, after the original `Get2DInfo` ran.
pub(crate) struct Projection<'a> {
    pub manager: &'a UIManager,
    pub world: Vec3,
    /// The game's own camera transform for the projection (not the panel's).
    pub camera: &'a Matrix4,
    pub margin: f32,
    pub out_x: &'a mut f32,
    pub out_y: &'a mut f32,
    pub out_pos: &'a mut ScreenPos,
}

/// Classify a point of interest's projection for the world markers. Called from the `Get2DInfo`
/// detour after the original; a no-op outside `UpdatePOIs` or with the prototype off.
///
/// With [`classify_with_headset_fov`](WorldMarkersConfig::classify_with_headset_fov), a point the
/// panel projection put off screen (an edge arrow) but that is inside the headset's field of view
/// is reclassified on screen and its stage position clamped into the safe area, so the HUD draws
/// its on-screen icon there for the crop. The final classification is recorded by world point for
/// the capture seam.
pub(crate) fn classify(projection: Projection<'_>, cfg: &WorldMarkersConfig) {
    if !cfg.enabled || !IN_UPDATE_POIS.with(Cell::get) {
        return;
    }
    if cfg.classify_with_headset_fov
        && *projection.out_pos != ScreenPos::SCREEN_POS_ONSCREEN
        && in_headset_fov(projection.camera, projection.world, cfg)
    {
        let safe = &projection.manager.m_SafeArea;
        let inset = projection.margin;
        let clamped = clamp_into(
            Vec2::new(*projection.out_x, *projection.out_y),
            Vec2::new(safe.x1 + inset, safe.y1 + inset),
            Vec2::new(safe.x2 - inset, safe.y2 - inset),
        );
        *projection.out_x = clamped.x;
        *projection.out_y = clamped.y;
        *projection.out_pos = ScreenPos::SCREEN_POS_ONSCREEN;
    }
    CLASSIFIED
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(world_key(projection.world), *projection.out_pos);
}

/// Snapshot the visible world-placed markers at the capture seam, before the capture.
///
/// # Safety
/// Must be called from the `MovieImpl::Capture` detour for the main movie.
pub(crate) unsafe fn on_capture(cfg: &WorldMarkersConfig) {
    let collected = if cfg.enabled && crate::hud::current_mode() == HudMode::Hud {
        // SAFETY: forwarded from the caller.
        unsafe { collect() }
    } else {
        None
    };
    *PENDING.lock() = collected;
}

/// Publish the snapshot taken before the capture, keyed by the snapshot the capture produced.
pub(crate) fn after_capture(movie: &MovieImpl) {
    let Some(mut layout) = PENDING.lock().take() else {
        publish_layout(None);
        return;
    };
    layout.snapshot = movie.RenderContext.SnapshotFrameIds[0];
    publish_layout(Some(layout));
}

thread_local! {
    /// Whether this thread is inside `CHUDUI::UpdatePOIs`.
    static IN_UPDATE_POIS: Cell<bool> = const { Cell::new(false) };
}

/// The latest `UpdatePOIs` pass's final screen classifications, by world point.
static CLASSIFIED: Mutex<Option<HashMap<[u32; 3], ScreenPos>>> = Mutex::new(None);

/// The snapshot taken before the capture, published after it.
static PENDING: Mutex<Option<CaptureLayout>> = Mutex::new(None);

/// A world point's exact bit pattern: `UpdatePOIs` projects a copy of the POI's transform
/// translation, so the capture seam's read of the same translation matches it exactly.
fn world_key(world: Vec3) -> [u32; 3] {
    world.to_array().map(f32::to_bits)
}

/// Whether `world` is inside the headset's field of view (both eyes' together) from the game
/// camera, falling back to the configured symmetric field of view without a headset frame.
fn in_headset_fov(camera: &Matrix4, world: Vec3, cfg: &WorldMarkersConfig) -> bool {
    let inset = cfg.fov_inset_deg.max(0.0).to_radians();
    let fov = crate::vr::capture_views()
        .map(|views| {
            let [a, b] = views.eyes.map(|eye| {
                FovTangents::from_angles(eye.fov[0], eye.fov[1], eye.fov[2], eye.fov[3], inset)
            });
            a.union(b)
        })
        .unwrap_or_else(|| FovTangents::symmetric(cfg.fallback_half_fov_deg.to_radians(), inset));
    let m = &camera.data;
    let row = |i: usize| Vec3::new(m[i * 4], m[i * 4 + 1], m[i * 4 + 2]);
    fov.contains(row(3), row(0), row(1), row(2), world)
}

/// Walk the HUD's point-of-interest records under their lock and collect the visible markers the
/// last `UpdatePOIs` classified on screen.
///
/// # Safety
/// As [`on_capture`].
unsafe fn collect() -> Option<CaptureLayout> {
    // SAFETY: the HUD and UI manager are live singletons in gameplay.
    let (hud, manager) = unsafe { (HUDUI::get()?, UIManager::get()?) };
    if matches!(
        hud.m_State,
        HUDState::HUD_CINEMATIC | HUDState::HUD_HIDE_ALL | HUDState::HUD_HIDE_ALL_NON_OVERLAY
    ) {
        return None;
    }
    let classified = CLASSIFIED.lock().take().unwrap_or_default();
    let mut layout = CaptureLayout {
        snapshot: 0,
        stage: Vec2::new(manager.m_CachedStageWidth, manager.m_CachedStageHeight),
        labelled_id: hud.m_ShowingLabelPOIId,
        markers: Vec::new(),
        skipped: 0,
    };
    if hud.m_POIListLock.is_null() || hud.m_POIListHead.is_null() {
        return Some(layout);
    }
    // SAFETY: the list lock is the HUD's own critical section; every node is live while it is
    // held. The marker clips are the HUD's managed values, read on the capture thread.
    unsafe {
        EnterCriticalSection(hud.m_POIListLock);
        let head = hud.m_POIListHead;
        let mut node = (*head).m_Next;
        while !node.is_null() && node != head {
            let record = &mut (*node).m_Record;
            node = (*node).m_Next;
            if !record.m_Visible {
                continue;
            }
            let Some(marker) = read_marker(record, &classified) else {
                continue;
            };
            if layout.markers.len() < MAX_MARKERS {
                layout.markers.push(marker);
            } else {
                layout.skipped += 1;
            }
        }
        LeaveCriticalSection(hud.m_POIListLock);
    }
    Some(layout)
}

/// Read one visible record's marker, if its POI is alive and was classified on screen.
///
/// # Safety
/// The caller holds the list lock, on the capture thread.
unsafe fn read_marker(
    record: &mut POIRecord,
    classified: &HashMap<[u32; 3], ScreenPos>,
) -> Option<MarkerRecord> {
    if record.m_POI.pn.is_null()
        || record.m_Element.value_type() == 0
        || record.m_Element.pObjectInterface.is_null()
    {
        return None;
    }
    let mut strong = SharedPtr::<POI> {
        px: std::ptr::null_mut(),
        pn: std::ptr::null_mut(),
    };
    // SAFETY: `lock` takes a strong reference only while the POI is alive; it is released below.
    let poi = unsafe {
        record.m_POI.lock(&mut strong);
        strong.px.as_ref()
    };
    let marker = poi.and_then(|poi| {
        if poi.m_IsInvisible {
            return None;
        }
        let t = &poi.m_Transform.data;
        let world = Vec3::new(t[12], t[13], t[14]);
        if classified.get(&world_key(world)) != Some(&ScreenPos::SCREEN_POS_ONSCREEN) {
            return None;
        }
        // SAFETY: the element is the record's live managed clip value.
        let info = unsafe { display_info(&mut record.m_Element)? };
        info.Visible.then(|| MarkerRecord {
            id: poi.m_ID,
            world,
            origin: Vec2::new(info.X as f32, info.Y as f32),
            offset: Vec2::from(poi.m_Attach2DOffset.data),
        })
    });
    if !strong.pn.is_null() {
        // SAFETY: releases the reference `lock` took.
        unsafe { ResetSharedPtrPOI(&mut strong) };
    }
    marker
}

/// Read a clip's display info through its managed value.
///
/// # Safety
/// `value` must be a live managed display-object value, read on the capture thread.
unsafe fn display_info(value: &mut Value) -> Option<DisplayInfo> {
    // SAFETY: forwarded from the caller.
    unsafe {
        let interface = value.pObjectInterface.as_mut()?;
        let mut info = DisplayInfo::default();
        interface
            .GetDisplayInfo(value.mValue as *mut std::ffi::c_void, &mut info)
            .then_some(info)
    }
}
