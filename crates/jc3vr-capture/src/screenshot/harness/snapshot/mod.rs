//! GPU snapshots for the capture harness's fast comparison: a `snapshot` request keeps the next
//! eligible frame's final image and geometry planes as GPU textures, compares them on the GPU with
//! snapshots kept earlier in the schedule (`snapshot_diff_cs`), and acknowledges with a summary per
//! eye and plane: the differing pixel count, the mean difference, the bounding box, and a heat
//! map. Nothing larger than the summary is read back unless the dump policy asks for the compared
//! pixels, which are then cropped to the differing region and written compressed off the render
//! thread. See `docs/mod/testing/fast-comparison.md`.
//!
//! The snapshot reuses the screenshot harness's readiness: it is taken on a frame that renders in
//! gameplay with both of the mode's eyes fresh, after the visual configuration is ready, and under
//! a motion configuration at the requested phase after a full cycle.

mod diff;
mod dump;
mod request;
mod store;
mod summary;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use jc3gi::graphics_engine::{graphics_engine::GraphicsEngine, texture::Texture};
use jc3vr_render::{
    capture_arming::{Arming, arm},
    screenshot::CaptureSource,
};
use jc3vr_view::stereo::{dispatch, dispatch::RenderMode, engine_context::EngineContext};
use parking_lot::Mutex;
pub use request::{Plane, SnapshotError, SnapshotRequest, SnapshotTable};
use serde::Serialize;
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Resource},
    core::Interface as _,
};

use crate::{
    screenshot::harness::{
        RequestId,
        snapshot::{
            diff::{GRID, Job, Pipeline},
            dump::{DumpImage, DumpRecord, SavedRecord},
            request::DumpPolicy,
            store::PlaneTexture,
            summary::{Kind, Totals, totals},
        },
        stages::Stage,
    },
    testing::visual::VisualMetadata,
};

/// Admit a snapshot request. Game thread. A request that cannot start is acknowledged as failed.
pub fn request(request: SnapshotRequest) {
    arm(Arming::Capture);
    let Some(folder) = captures_folder() else {
        tracing::error!("snapshot: no configuration directory");
        return;
    };
    if folder
        .join(format!("{}.json", request.id.as_str()))
        .exists()
    {
        tracing::warn!(
            id = request.id.as_str(),
            "snapshot: the id already has a result"
        );
        return;
    }
    let mut state = STATE.lock();
    if let Some(pending) = state.pending.as_ref() {
        let error = format!("snapshot: {} is still pending", pending.request.id.as_str());
        publish_failure(&folder, &request.id, &request.label, error);
        return;
    }
    if request.phase.is_some() && crate::testing::visual::motion_period().is_none() {
        publish_failure(
            &folder,
            &request.id,
            &request.label,
            "snapshot: a motion phase requires an active visual motion".to_owned(),
        );
        return;
    }
    state.pending = Some(Pending {
        after: crate::screenshot::harness::frame_identity(),
        started: Instant::now(),
        frames_seen: 0,
        folder,
        request,
    });
}

/// Release every kept snapshot's textures, and acknowledge with how many there were. Game thread.
pub fn release(id: RequestId) {
    let Some(folder) = captures_folder() else {
        tracing::error!("snapshot: no configuration directory");
        return;
    };
    let released = std::mem::take(&mut STATE.lock().kept).len();
    let result = ReleaseResult {
        id: id.as_str(),
        state: "released",
        released,
    };
    if let Err(error) = write_json(&folder.join(format!("{}.json", id.as_str())), &result) {
        tracing::error!("snapshot: {error:#}");
    }
}

/// Whether a snapshot waits for this frame's eyes: the eye captures must be kept fresh for it.
pub(crate) fn wants_eyes() -> bool {
    let phase = {
        let state = STATE.lock();
        match state.pending.as_ref() {
            None => return false,
            Some(pending) => pending.request.phase,
        }
    };
    phase.is_none_or(crate::testing::visual::capture_phase_due)
}

/// Keep `stage`'s texture for a pending snapshot that wants its plane, on a frame it may be taken
/// on. Render thread, from the stage hooks.
pub(crate) fn stage_hook(stage: Stage, texture: impl FnOnce() -> anyhow::Result<*mut Texture>) {
    let Some(plane) = plane_of(stage) else {
        return;
    };
    plane_hook(plane, |_| {
        let texture = texture()?;
        // SAFETY: supplied by a live render hook.
        let texture = unsafe { texture.as_ref() }.context("the stage texture is unavailable")?;
        Ok(texture.m_Texture.clone())
    });
}

/// Keep the texture the ambient-occlusion block's composite wrote, the render target it leaves
/// bound, for a pending snapshot that wants the [`Plane::Ssao`] plane. Render thread, right after the
/// block's draw.
pub(crate) fn ambient_occlusion_hook() {
    plane_hook(Plane::Ssao, |ctx| {
        let mut targets = [None];
        // SAFETY: the context is locked by `plane_hook`; the call fills one slot.
        unsafe { ctx.OMGetRenderTargets(Some(&mut targets), None) };
        let [Some(view)] = targets else {
            anyhow::bail!("the ambient-occlusion block left no render target bound");
        };
        // SAFETY: a live view.
        Ok(unsafe { view.GetResource() }?)
    });
    plane_hook(Plane::SsaoVolumes, |ctx| {
        let mut views = [None, None];
        // SAFETY: the context is locked by `plane_hook`; the call fills two slots.
        unsafe { ctx.PSGetShaderResources(0, Some(&mut views)) };
        let [_, Some(view)] = views else {
            anyhow::bail!("the ambient-occlusion composite left no volume texture bound");
        };
        // SAFETY: a live view.
        Ok(unsafe { view.GetResource() }?)
    });
}

/// Mark the start (`true`) or the end of the ambient-occlusion block's draw, so the draws inside it
/// are counted for the [`Plane::SsaoStep0`] family. Render thread.
pub(crate) fn ambient_occlusion_block(inside: bool) {
    AMBIENT_OCCLUSION_STEP.store(if inside { 0 } else { NOT_IN_BLOCK }, Ordering::Relaxed);
}

/// Keep the render target a draw inside the ambient-occlusion block left, as the next plane of the
/// [`Plane::SsaoStep0`] family, for a pending snapshot that wants it. Render thread, right after an
/// engine draw.
pub(crate) fn ambient_occlusion_draw_hook() {
    let step = AMBIENT_OCCLUSION_STEP.load(Ordering::Relaxed);
    if step == NOT_IN_BLOCK {
        return;
    }
    AMBIENT_OCCLUSION_STEP.store(step + 1, Ordering::Relaxed);
    let Some(plane) = Plane::ssao_step(step) else {
        return;
    };
    plane_hook(plane, |ctx| {
        let mut targets = [None];
        // SAFETY: the context is locked by `plane_hook`; the call fills one slot.
        unsafe { ctx.OMGetRenderTargets(Some(&mut targets), None) };
        let [Some(view)] = targets else {
            anyhow::bail!("the ambient-occlusion draw left no render target bound");
        };
        // SAFETY: a live view.
        Ok(unsafe { view.GetResource() }?)
    });
}

/// The next draw's index inside the ambient-occlusion block, or [`NOT_IN_BLOCK`] outside it.
static AMBIENT_OCCLUSION_STEP: AtomicU32 = AtomicU32::new(NOT_IN_BLOCK);
const NOT_IN_BLOCK: u32 = u32::MAX;

/// Keep `resource`, an eye-sized texture of `eye`'s alone, as that eye's `plane` for a pending
/// snapshot that wants it, whatever the mode. Render thread, with the engine's immediate context
/// locked or not (its critical section is reentrant).
pub(crate) fn eye_plane_hook(plane: Plane, eye: usize, resource: &ID3D11Resource) {
    plane_source_hook(plane, Some(eye), |_| Ok(resource.clone()));
}

/// Keep the resource `source` names for a pending snapshot that wants `plane`, on a frame it may be
/// taken on. Render thread; `source` runs with the engine's immediate context locked.
fn plane_hook(
    plane: Plane,
    source: impl FnOnce(&ID3D11DeviceContext) -> anyhow::Result<ID3D11Resource>,
) {
    plane_source_hook(plane, None, source);
}

/// [`plane_hook`] for `explicit_eye`'s texture alone when it is set, or for the eye being drawn
/// (both eyes side by side under single-pass) when it is not.
fn plane_source_hook(
    plane: Plane,
    explicit_eye: Option<usize>,
    source: impl FnOnce(&ID3D11DeviceContext) -> anyhow::Result<ID3D11Resource>,
) {
    if !wants_eyes() || !crate::testing::visual::ready() {
        return;
    }
    let Some(frame) = crate::screenshot::harness::frame_identity() else {
        return;
    };
    let eye = explicit_eye.unwrap_or_else(jc3vr_view::stereo::draw_index);
    let Some(context) = EngineContext::get() else {
        return;
    };
    // The engine's context lock is taken before the state's, as the final-image hook does.
    context.with_lock(|ctx| {
        let mut state = STATE.lock();
        let State { pending, stash, .. } = &mut *state;
        let Some(pending) = pending.as_ref() else {
            return;
        };
        if !pending.request.planes.contains(&plane) {
            return;
        }
        if stash.frame != Some(frame) {
            stash.frame = Some(frame);
            stash
                .spare
                .extend(stash.planes.drain(..).map(|entry| entry.texture));
            stash.errors.clear();
        }
        if stash
            .planes
            .iter()
            .any(|entry| entry.plane == plane && entry.eye == eye)
        {
            return;
        }
        let reuse = stash.spare.pop();
        let copied = (|| {
            // SAFETY: the stage hooks run on the render thread with the engine's live device.
            let engine = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
            let device =
                unsafe { engine.m_Device.as_ref() }.context("graphics device unavailable")?;
            let resource = source(ctx)?;
            // SAFETY: the context is locked and the resource is live.
            unsafe { PlaneTexture::copy(&device.m_Device, ctx, &resource, reuse) }
        })();
        match copied {
            Ok(texture) => stash.planes.push(StashEntry {
                plane,
                eye,
                one_eye: explicit_eye.is_some(),
                texture,
            }),
            Err(error) => stash.errors.push((plane, format!("{error:#}"))),
        }
    });
}

/// Take a pending snapshot from this frame, if it is eligible, run its comparisons, and hand the
/// acknowledgement (and any dumps) to a writer thread. Render thread, inside the engine's context
/// critical section, on the frame's last near dispatch.
pub(crate) fn capture(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &CaptureSource<'_>,
) {
    let Some(frame) = crate::screenshot::harness::frame_identity() else {
        return;
    };
    let mode = dispatch::mode();
    let selected = mode.selected_eye();
    let mut state = STATE.lock();
    let Some(pending) = state.pending.as_mut() else {
        return;
    };
    pending.frames_seen += 1;
    let visual = match pending.request.phase {
        Some(phase) => {
            if !crate::testing::visual::capture_phase_due(phase) {
                return;
            }
            let visual = crate::testing::visual::capture_metadata(frame);
            if let Err(error) =
                crate::screenshot::harness::motion_capture_gate(&visual, phase, frame)
            {
                let pending = state.pending.take().expect("checked above");
                publish_failure(
                    &pending.folder,
                    &pending.request.id,
                    &pending.request.label,
                    error,
                );
                return;
            }
            visual
        }
        None => {
            let visual = crate::testing::visual::metadata();
            if !visual.ready || visual.mode != Some(mode) {
                return;
            }
            visual
        }
    };
    // At a still camera, a snapshot that names a jitter index waits for a frame at it, so every
    // configuration's snapshot takes the same sub-pixel offset whatever its warm-up's length.
    let jitter_phase = jc3vr_view::vr::render_params(0).and_then(|params| params.jitter);
    if let (Some(wanted), Some(jitter)) = (pending.request.jitter_phase, jitter_phase)
        && pending.request.phase.is_none()
        && jitter.phase != wanted % jitter.phase_count.max(1)
    {
        return;
    }
    if !jc3vr_view::vr::is_running_published()
        || !jc3vr_core::hooks::in_gameplay()
        || !jc3vr_view::stereo::active()
        || pending.after == Some(frame)
        || !crate::screenshot::harness::eyes_fresh(frame, selected)
    {
        return;
    }
    let pending = state.pending.take().expect("checked above");
    let started = Instant::now();
    let outcome = take(&mut state, device, context, source, frame, mode, &pending);
    let snapshot = match outcome {
        Ok(snapshot) => snapshot,
        Err(error) => {
            publish_failure(
                &pending.folder,
                &pending.request.id,
                &pending.request.label,
                format!("snapshot: {error:#}"),
            );
            return;
        }
    };
    let compared = compare(&mut state, device, context, &pending.request, &snapshot);
    let saves = if pending.request.save {
        save_planes(device, context, &snapshot)
    } else {
        Vec::new()
    };
    let gpu_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut result = SnapshotResult {
        id: pending.request.id.as_str().to_owned(),
        state: "completed",
        error: None,
        label: pending.request.label.clone(),
        mode: Some(mode),
        frame: Some(frame),
        eye_width: snapshot.eye_size[0],
        eye_height: snapshot.eye_size[1],
        planes: snapshot
            .planes
            .iter()
            .map(|plane| PlaneRecord {
                plane: plane.plane,
                eye: plane.eye,
                width: plane.texture.width,
                height: plane.texture.height,
                format: plane.texture.format.storage.0,
                kind: plane.texture.format.kind,
            })
            .collect(),
        plane_errors: snapshot.errors.clone(),
        comparisons: Vec::new(),
        saved: Vec::new(),
        fsr: jc3vr_render::fsr::dispatch_records(frame),
        kept: pending.request.keep,
        visual: Some(visual),
        timing: Timing {
            frames_waited: pending.frames_seen,
            wait_ms: pending.started.elapsed().as_secs_f64() * 1000.0 - gpu_ms,
            gpu_ms,
        },
    };
    let mut dumps = Vec::new();
    match compared {
        Ok(comparisons) => {
            for comparison in comparisons {
                let record = comparison.record;
                if let Some(images) = comparison.dump {
                    dumps.push((record.dump_stem(), images));
                }
                result.comparisons.push(record);
            }
        }
        Err(error) => {
            result.state = "failed";
            result.error = Some(format!("snapshot: {error:#}"));
        }
    }
    if !result.plane_errors.is_empty() && result.error.is_none() {
        result.state = "failed";
        result.error = Some("snapshot: a requested plane was not captured".to_owned());
    }
    if pending.request.keep {
        state.kept.retain(|kept| kept.label != snapshot.label);
        state.kept.push(snapshot);
    }
    let path = pending
        .folder
        .join(format!("{}.json", pending.request.id.as_str()));
    let dump_folder = pending
        .folder
        .join(format!("{}.dump", pending.request.id.as_str()));
    state.writers.retain(|writer| !writer.is_finished());
    let spawned = std::thread::Builder::new()
        .name("jc3vr-snapshot-writer".to_owned())
        .spawn(move || {
            if !saves.is_empty() {
                result.saved = dump::write_saved(&dump_folder, saves);
            }
            if !dumps.is_empty() {
                let records = dump::write(&dump_folder, dumps);
                for comparison in &mut result.comparisons {
                    comparison.dump = records
                        .iter()
                        .find(|record| record.stem == comparison.dump_stem())
                        .cloned();
                }
            }
            if let Err(error) = write_json(&path, &result) {
                tracing::error!("snapshot: {error:#}");
            }
        });
    match spawned {
        Ok(handle) => state.writers.push(handle),
        Err(error) => tracing::error!("snapshot: could not start the writer: {error}"),
    }
}

/// Fail a snapshot that waited too long for an eligible frame. Game thread.
pub(crate) fn tick() {
    let mut state = STATE.lock();
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| pending.started.elapsed() > TIMEOUT)
    {
        let pending = state.pending.take().expect("checked above");
        publish_failure(
            &pending.folder,
            &pending.request.id,
            &pending.request.label,
            format!(
                "snapshot: no eligible frame within {}s ({} frames seen)",
                TIMEOUT.as_secs(),
                pending.frames_seen
            ),
        );
    }
}

/// Release the snapshots and wait for the writers, for eject. Returns whether every writer
/// finished.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    let writers = {
        let mut state = STATE.lock();
        if let Some(pending) = state.pending.take() {
            publish_failure(
                &pending.folder,
                &pending.request.id,
                &pending.request.label,
                "snapshot: the payload is shutting down".to_owned(),
            );
        }
        state.kept.clear();
        state.stash = Stash::default();
        state.pipeline = None;
        std::mem::take(&mut state.writers)
    };
    writers.into_iter().all(|writer| writer.join().is_ok())
}

const TIMEOUT: Duration = Duration::from_secs(30);

/// How far a dump's crop reaches past the differing pixels' bounding box.
const DUMP_MARGIN: u32 = 16;

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    kept: Vec::new(),
    stash: Stash {
        frame: None,
        planes: Vec::new(),
        spare: Vec::new(),
        errors: Vec::new(),
    },
    pipeline: None,
    writers: Vec::new(),
});

struct State {
    pending: Option<Pending>,
    kept: Vec<Snapshot>,
    stash: Stash,
    pipeline: Option<Pipeline>,
    writers: Vec<JoinHandle<()>>,
}

// SAFETY: the Direct3D objects in the state are free-threaded for reference counting, and are only
// used for rendering on the render thread with the engine's immediate context locked.
unsafe impl Send for State {}

struct Pending {
    request: SnapshotRequest,
    folder: PathBuf,
    after: Option<i64>,
    started: Instant,
    frames_seen: u32,
}

/// The geometry planes copied so far on the frame a snapshot may be taken on.
#[derive(Default)]
struct Stash {
    frame: Option<i64>,
    planes: Vec<StashEntry>,
    /// Copies from an earlier frame, reused when the next frame's source matches.
    spare: Vec<PlaneTexture>,
    errors: Vec<(Plane, String)>,
}

struct StashEntry {
    plane: Plane,
    /// The eye the stage hook recorded: the eye being drawn (eye 0 for the single-pass walk).
    eye: usize,
    /// Whether the texture holds `eye` alone in every mode, rather than both eyes side by side
    /// under single-pass.
    one_eye: bool,
    texture: PlaneTexture,
}

struct Snapshot {
    label: String,
    eye_size: [u32; 2],
    planes: Vec<SnapshotPlane>,
    errors: Vec<PlaneError>,
}

struct SnapshotPlane {
    plane: Plane,
    eye: usize,
    origin: [u32; 2],
    /// The compared region's size: the eye's, or a reduced-resolution plane's own share of it.
    size: [u32; 2],
    texture: Arc<PlaneTexture>,
}

#[derive(Debug, Clone, Serialize)]
struct PlaneError {
    plane: Plane,
    error: String,
}

fn plane_of(stage: Stage) -> Option<Plane> {
    match stage {
        Stage::GeometryDepth => Some(Plane::Depth),
        Stage::GBuffer0 => Some(Plane::Albedo),
        Stage::GBuffer1 => Some(Plane::Normals),
        Stage::Velocity => Some(Plane::Velocity),
        Stage::PostResolve => Some(Plane::PostResolve),
        Stage::PreAtmosphere => Some(Plane::PreAtmosphere),
        Stage::PostAtmosphere => Some(Plane::PostAtmosphere),
        Stage::PostSun => Some(Plane::PostSun),
        Stage::MainColor => Some(Plane::MainColor),
        Stage::PreAa => Some(Plane::PreAa),
        Stage::PostAa => Some(Plane::PostAa),
        _ => None,
    }
}

/// Copy the final image, and gather it with this frame's stashed planes into a snapshot.
fn take(
    state: &mut State,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &CaptureSource<'_>,
    frame: i64,
    mode: RenderMode,
    pending: &Pending,
) -> anyhow::Result<Snapshot> {
    let selected = mode.selected_eye();
    let mut finals: Vec<(Option<usize>, PlaneTexture)> = Vec::new();
    {
        // SAFETY: the render thread inside the engine's context critical section; the sources are
        // the frame's finished eye images.
        unsafe {
            match source {
                CaptureSource::Full(resource) => {
                    finals.push((None, PlaneTexture::copy(device, context, resource, None)?));
                }
                CaptureSource::Eyes(eyes) => {
                    for (eye, texture) in eyes.iter().enumerate() {
                        let resource: ID3D11Resource = texture.cast()?;
                        finals.push((
                            Some(eye),
                            PlaneTexture::copy(device, context, &resource, None)?,
                        ));
                    }
                }
            }
        }
    }
    let eye_size = match (source, finals.first()) {
        (CaptureSource::Full(_), Some((_, texture))) if selected.is_none() => {
            [texture.width / 2, texture.height]
        }
        (_, Some((_, texture))) => [texture.width, texture.height],
        (_, None) => anyhow::bail!("the final image was not captured"),
    };
    let mut snapshot = Snapshot {
        label: pending.request.label.clone(),
        eye_size,
        planes: Vec::new(),
        errors: Vec::new(),
    };
    for (eye, texture) in finals {
        add_plane(&mut snapshot, Plane::Final, eye.or(selected), texture);
    }
    let stash = &mut state.stash;
    let stashed = if stash.frame == Some(frame) {
        stash.frame = None;
        std::mem::take(&mut stash.planes)
    } else {
        Vec::new()
    };
    for (plane, error) in stash.errors.drain(..) {
        snapshot.errors.push(PlaneError { plane, error });
    }
    for entry in stashed {
        // An isolated eye records the eye it draws; the single-pass walk records eye 0 for a
        // double-wide plane that holds both.
        let eye = if mode == RenderMode::SinglePass && !entry.one_eye {
            None
        } else {
            Some(entry.eye)
        };
        add_plane(&mut snapshot, entry.plane, eye, entry.texture);
    }
    for plane in &pending.request.planes {
        if !snapshot.planes.iter().any(|entry| entry.plane == *plane)
            && !snapshot.errors.iter().any(|error| error.plane == *plane)
        {
            snapshot.errors.push(PlaneError {
                plane: *plane,
                error: "the plane was not captured on the snapshot's frame".to_owned(),
            });
        }
    }
    Ok(snapshot)
}

/// Add `texture` to `snapshot` as `eye`'s plane, or as both eyes' when `eye` is `None` and the
/// texture holds them side by side.
fn add_plane(snapshot: &mut Snapshot, plane: Plane, eye: Option<usize>, texture: PlaneTexture) {
    let [mut width, mut height] = snapshot.eye_size;
    let texture = Arc::new(texture);
    if plane.may_be_reduced() {
        // A reduced-resolution plane is compared at its own size: the whole texture for one eye, or
        // each half of a double-wide one.
        width = if eye.is_some() {
            texture.width
        } else {
            texture.width / 2
        };
        height = texture.height;
    }
    if texture.height < height {
        snapshot.errors.push(PlaneError {
            plane,
            error: format!(
                "the {}x{} plane is smaller than the {width}x{height} eye",
                texture.width, texture.height
            ),
        });
        return;
    }
    let eyes: Vec<(usize, u32)> = match eye {
        Some(eye) if texture.width >= width => vec![(eye, 0)],
        None if texture.width >= 2 * width => vec![(0, 0), (1, width)],
        _ => {
            snapshot.errors.push(PlaneError {
                plane,
                error: format!(
                    "the {}-pixel-wide plane does not hold the {width}-pixel eye{}",
                    texture.width,
                    if eye.is_none() { "s" } else { "" }
                ),
            });
            return;
        }
    };
    for (eye, x) in eyes {
        snapshot.planes.push(SnapshotPlane {
            plane,
            eye,
            origin: [x, 0],
            size: [width, height],
            texture: Arc::clone(&texture),
        });
    }
}

struct Compared {
    record: ComparisonRecord,
    dump: Option<[DumpImage; 2]>,
}

/// Compare `snapshot` with each kept snapshot the request names, plane by plane and eye by eye.
fn compare(
    state: &mut State,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    request: &SnapshotRequest,
    snapshot: &Snapshot,
) -> anyhow::Result<Vec<Compared>> {
    let mut pairs = Vec::new();
    for label in &request.compare {
        let kept = state
            .kept
            .iter()
            .find(|kept| &kept.label == label)
            .with_context(|| format!("no kept snapshot is labelled {label}"))?;
        if kept.eye_size != snapshot.eye_size {
            anyhow::bail!(
                "{label}'s {:?} eyes differ from this snapshot's {:?}",
                kept.eye_size,
                snapshot.eye_size
            );
        }
        for test in &snapshot.planes {
            if let Some(reference) = kept.planes.iter().find(|entry| {
                entry.plane == test.plane && entry.eye == test.eye && entry.size == test.size
            }) {
                pairs.push((label.as_str(), reference, test));
            }
        }
    }
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    if state.pipeline.is_none() {
        state.pipeline = Some(Pipeline::new(device)?);
    }
    let pipeline = state.pipeline.as_ref().expect("created above");
    let jobs: Vec<Job<'_>> = pairs
        .iter()
        .map(|(_, reference, test)| {
            let final_image = test.plane == Plane::Final;
            Job {
                a: &reference.texture,
                b: &test.texture,
                origin_a: reference.origin,
                origin_b: test.origin,
                size: test.size,
                threshold: if final_image {
                    request.pixel_threshold
                } else if reference.texture.format.kind == Kind::Depth {
                    request.depth_ulps as f32
                } else {
                    0.0
                },
                channels: if final_image { 3 } else { 4 },
            }
        })
        .collect();
    // SAFETY: the render thread inside the engine's context critical section; the planes are live.
    let cells = unsafe { pipeline.run(context, &jobs)? };
    let mut compared = Vec::with_capacity(pairs.len());
    for ((label, reference, test), (cells, job)) in pairs.iter().zip(cells.iter().zip(&jobs)) {
        let size = test.size;
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let totals = totals(cells, GRID, pixels);
        let failed = request.limits.exceeded(&totals);
        let wants_dump = match request.dump {
            DumpPolicy::Never => false,
            DumpPolicy::OnFailure => failed,
            DumpPolicy::Always => true,
        };
        let dump = if wants_dump {
            let region = match (request.dump_crop, totals.bbox) {
                (true, Some(bbox)) => summary::crop(bbox, DUMP_MARGIN, size[0], size[1]),
                _ => (0, 0, size[0], size[1]),
            };
            let png = test.plane == Plane::Final;
            // SAFETY: as above.
            unsafe {
                match (
                    DumpImage::read(
                        device,
                        context,
                        &reference.texture,
                        reference.origin,
                        region,
                        png,
                    ),
                    DumpImage::read(device, context, &test.texture, test.origin, region, png),
                ) {
                    (Ok(a), Ok(b)) => Some([a, b]),
                    (Err(error), _) | (_, Err(error)) => {
                        tracing::warn!("snapshot: dump readback failed: {error:#}");
                        None
                    }
                }
            }
        } else {
            None
        };
        compared.push(Compared {
            record: ComparisonRecord {
                against: (*label).to_owned(),
                plane: test.plane,
                eye: test.eye,
                kind: reference.texture.format.kind,
                threshold: job.threshold,
                failed,
                totals,
                dump: None,
            },
            dump,
        });
    }
    Ok(compared)
}

/// Read every plane of `snapshot` back whole, eye by eye, to be written by the writer thread. A
/// plane that cannot be read back is logged and left out.
fn save_planes(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    snapshot: &Snapshot,
) -> Vec<(String, DumpImage)> {
    snapshot
        .planes
        .iter()
        .filter_map(|plane| {
            let stem = format!(
                "{}.{}.eye{}",
                snapshot.label,
                plane_name(plane.plane),
                plane.eye
            );
            let region = (0, 0, plane.size[0], plane.size[1]);
            // SAFETY: the render thread inside the engine's context critical section; the plane is
            // live.
            let read = unsafe {
                DumpImage::read(
                    device,
                    context,
                    &plane.texture,
                    plane.origin,
                    region,
                    plane.plane == Plane::Final,
                )
            };
            match read {
                Ok(image) => Some((stem, image)),
                Err(error) => {
                    tracing::warn!("snapshot: saving {stem} failed: {error:#}");
                    None
                }
            }
        })
        .collect()
}

/// The plane's wire name.
fn plane_name(plane: Plane) -> String {
    serde_json::to_value(plane)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn captures_folder() -> Option<PathBuf> {
    jc3vr_core::config::engine::paths::dir().map(|dir| dir.join("captures"))
}

#[derive(Serialize)]
struct SnapshotResult {
    id: String,
    state: &'static str,
    error: Option<String>,
    label: String,
    mode: Option<RenderMode>,
    frame: Option<i64>,
    eye_width: u32,
    eye_height: u32,
    planes: Vec<PlaneRecord>,
    plane_errors: Vec<PlaneError>,
    comparisons: Vec<ComparisonRecord>,
    /// The planes written whole, when the request asked for them.
    saved: Vec<SavedRecord>,
    /// What FSR resolved each eye with on the snapshot's frame, for the eyes it resolved.
    fsr: Vec<jc3vr_render::fsr::DispatchRecord>,
    kept: bool,
    visual: Option<VisualMetadata>,
    timing: Timing,
}

#[derive(Serialize)]
struct PlaneRecord {
    plane: Plane,
    eye: usize,
    width: u32,
    height: u32,
    format: i32,
    kind: Kind,
}

#[derive(Serialize)]
struct ComparisonRecord {
    against: String,
    plane: Plane,
    eye: usize,
    kind: Kind,
    threshold: f32,
    /// Whether the differences exceed the request's failure limits.
    failed: bool,
    #[serde(flatten)]
    totals: Totals,
    dump: Option<DumpRecord>,
}

impl ComparisonRecord {
    fn dump_stem(&self) -> String {
        format!(
            "{}.{}.eye{}",
            self.against,
            plane_name(self.plane),
            self.eye
        )
    }
}

#[derive(Serialize)]
struct Timing {
    /// The final-image hooks seen while the snapshot waited, including the one it was taken on.
    frames_waited: u32,
    /// The wall-clock time from admission to the frame it was taken on.
    wait_ms: f64,
    /// The time the copies, comparisons, and readbacks took on the render thread.
    gpu_ms: f64,
}

#[derive(Serialize)]
struct ReleaseResult<'a> {
    id: &'a str,
    state: &'static str,
    released: usize,
}

fn publish_failure(folder: &Path, id: &RequestId, label: &str, error: String) {
    tracing::warn!(id = id.as_str(), "{error}");
    let result = SnapshotResult {
        id: id.as_str().to_owned(),
        state: "failed",
        error: Some(error),
        label: label.to_owned(),
        mode: None,
        frame: None,
        eye_width: 0,
        eye_height: 0,
        planes: Vec::new(),
        plane_errors: Vec::new(),
        comparisons: Vec::new(),
        saved: Vec::new(),
        fsr: Vec::new(),
        kept: false,
        visual: None,
        timing: Timing {
            frames_waited: 0,
            wait_ms: 0.0,
            gpu_ms: 0.0,
        },
    };
    if let Err(error) = write_json(&folder.join(format!("{}.json", id.as_str())), &result) {
        tracing::error!("snapshot: {error:#}");
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(value).context("serializing the result")?;
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("publishing {}", path.display()))
}
