//! The one image writer: every PNG the mod writes (the F12 screenshot and the render trace's per-eye
//! frames) is converted, named, encoded, and written here, on one background thread.
//!
//! Callers read their pixels back on whichever thread owns the immediate context and hand over an
//! owned [`Image`] with an [`ImageTarget`] saying what it is. The writer thread does everything that
//! does not need the context: the channel swizzle and the forced-opaque alpha, the path resolution
//! in the session folder, the PNG encode, the file write, and any JSON sidecar. Inline, a
//! double-wide capture is tens of millions of pixels of scalar conversion plus a synchronous
//! deflate: seconds of frozen engine on every image.
//!
//! An image's file name is `<stem>.png` when it holds the whole frame and `<stem>_eye<N>.png` when
//! it holds one eye; a sidecar sits beside it as `<stem>.json`. The stem is the capture's stamp for
//! a screenshot and the frame index (`frameNNN`) for a trace frame, so a trace's images align 1:1
//! with its per-frame events. The folders are the session's (see [`crate::session`]).
//!
//! The queue between the submitters and the writer is bounded, so a trace that produces images
//! faster than they encode blocks its submitter until one drains rather than holding an unbounded
//! number of frames in memory: at worst it degrades to the writing-inline cost it replaces.
//!
//! A writer thread outliving the payload is the hazard this buys: a thread still executing when
//! [`crate::module::exit`] unmaps the image parks forever holding whatever it took. [`shutdown`]
//! closes admission, lets the queue drain, and waits for the thread, the same way
//! [`crate::vr::tail::shutdown`] and the profiler's capture writer do.

use std::{
    io::Write as _,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    thread::JoinHandle,
    time::Duration,
};

use anyhow::Context as _;
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use parking_lot::Mutex;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

use crate::{
    screenshot::harness::{
        stages::{Stage, StageArtifact, StageImage, StageRecord},
        types::CaptureMetadata,
    },
    session::{self, CaptureFile, CaptureFolder},
};

/// The byte order of a read-back image's four channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOrder {
    Rgba,
    Bgra,
}

impl ChannelOrder {
    /// The channel order of an 8-bit UNORM colour format, or `None` for any format the writer does
    /// not convert.
    pub fn of(format: DXGI_FORMAT) -> Option<Self> {
        match format {
            DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => Some(Self::Rgba),
            DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => Some(Self::Bgra),
            _ => None,
        }
    }
}

/// One read-back image: tightly packed rows of four bytes per pixel, in the source's own channel
/// order and with its own alpha, both normalized on the writer thread.
pub struct Image {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    order: ChannelOrder,
}

impl Image {
    /// Copies `height` rows of `width` pixels out of a mapping whose rows are `row_pitch` bytes
    /// apart, dropping the pitch padding. `None` when `bytes` is too short for that shape.
    pub fn from_rows(
        bytes: &[u8],
        row_pitch: usize,
        width: u32,
        height: u32,
        order: ChannelOrder,
    ) -> Option<Self> {
        let row_bytes = width as usize * 4;
        if row_pitch < row_bytes {
            return None;
        }
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            pixels.extend_from_slice(bytes.get(y * row_pitch..y * row_pitch + row_bytes)?);
        }
        Some(Self {
            pixels,
            width,
            height,
            order,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// What an image is, which decides where it is written and what it is called.
pub enum ImageTarget {
    /// An F12 screenshot holding the whole frame (both eyes side by side in stereo), written as
    /// `<stamp>.png` in the session's screenshot folder with `sidecar` beside it.
    Screenshot {
        stamp: String,
        sidecar: serde_json::Value,
    },
    /// One eye of one frame of a render trace, written as `frameNNN_eye<N>.png` in that trace's
    /// folder.
    TraceFrame {
        trace_stamp: String,
        frame: u32,
        eye: usize,
    },
    HarnessScreenshot {
        path: PathBuf,
        sidecar: Box<CaptureMetadata>,
        stages: Vec<StageImage>,
    },
}

pub(crate) struct WriteOutcome {
    pub result: Result<PathBuf, String>,
    pub stages: Vec<StageRecord>,
    pub resolve_inputs: Option<crate::screenshot::harness::resolve_probe::Capture>,
}

impl From<Result<PathBuf, String>> for WriteOutcome {
    fn from(result: Result<PathBuf, String>) -> Self {
        Self {
            result,
            stages: Vec::new(),
            resolve_inputs: None,
        }
    }
}

/// Queue `image` for the writer thread, spawning it on first use. Blocks while the queue is full.
/// Errors (shutdown has begun, the thread cannot be spawned) are logged and the image dropped: an
/// image is diagnostics and must never take its caller down with it.
pub fn submit(target: ImageTarget, image: Image) {
    let sender = {
        let mut writer = WRITER.lock();
        if writer.closed || crate::is_shutting_down() {
            return;
        }
        match writer.sender() {
            Ok(sender) => sender,
            Err(e) => {
                tracing::error!("image writer: could not spawn the writer thread: {e}");
                return;
            }
        }
    };
    // Sent outside the lock so a full queue blocks only this submitter, not `shutdown`.
    if sender
        .send(Job {
            target,
            image,
            completion: None,
        })
        .is_err()
    {
        tracing::error!("image writer: the writer thread has exited; the image is dropped");
    }
}

pub(crate) fn try_submit(
    target: ImageTarget,
    image: Image,
) -> anyhow::Result<Receiver<WriteOutcome>> {
    let mut writer = WRITER.lock();
    anyhow::ensure!(
        !crate::is_shutting_down(),
        "image writer: admission is closed"
    );
    writer.try_submit(target, image)
}

/// Close admission, let the queued images drain, and wait for the writer thread, up to a bounded
/// budget. Returns whether the thread is confirmed stopped (or never ran). A `false` return means the
/// caller must not unload -- see [`crate::vr::tail::shutdown`]'s doc comment for the full argument.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    let handle = {
        let mut writer = WRITER.lock();
        writer.closed = true;
        writer.sender = None;
        writer.handle.take()
    };
    let Some(handle) = handle else {
        return true;
    };
    for poll in 0..SHUTDOWN_POLLS {
        if handle.is_finished() {
            let _ = handle.join();
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
        if poll > 0 && poll % 100 == 0 {
            tracing::debug!("image writer: still draining, {} s elapsed", poll / 100);
        }
    }
    tracing::error!(
        "image writer: the writer did not finish within {} s; leaving the payload mapped rather \
         than unmapping under a live thread",
        SHUTDOWN_POLLS / 100,
    );
    false
}

/// How many images may wait for the writer before [`submit`] blocks. A trace's two eyes per frame
/// plus a screenshot fit without blocking while the writer keeps up.
const QUEUE_DEPTH: usize = 8;

/// How long [`shutdown`] waits, in 10 ms polls: 10 s, matching the profiler's capture writer. A full
/// queue of double-wide captures is a lot of pixels to deflate, and waiting too long costs only a
/// slower eject, while unloading too early is the unrecoverable wedge this exists to avoid.
const SHUTDOWN_POLLS: u32 = 1000;

static WRITER: Mutex<Writer> = Mutex::new(Writer {
    sender: None,
    handle: None,
    closed: false,
});

struct Writer {
    sender: Option<SyncSender<Job>>,
    handle: Option<JoinHandle<()>>,
    /// Set by [`shutdown`]; admission never reopens.
    closed: bool,
}

impl Writer {
    fn try_submit(
        &mut self,
        target: ImageTarget,
        image: Image,
    ) -> anyhow::Result<Receiver<WriteOutcome>> {
        anyhow::ensure!(!self.closed, "image writer: admission is closed");
        let sender = self
            .sender()
            .context("image writer: could not spawn the writer thread")?;
        let (completion, receiver) = mpsc::channel();
        sender
            .try_send(Job {
                target,
                image,
                completion: Some(completion),
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => anyhow::anyhow!("image writer: the queue is full"),
                TrySendError::Disconnected(_) => {
                    anyhow::anyhow!("image writer: the writer thread has disconnected")
                }
            })?;
        Ok(receiver)
    }

    /// The live queue's sender, spawning the writer thread when there is none.
    fn sender(&mut self) -> std::io::Result<SyncSender<Job>> {
        if let Some(sender) = &self.sender {
            return Ok(sender.clone());
        }
        let (sender, receiver) = mpsc::sync_channel(QUEUE_DEPTH);
        let handle = std::thread::Builder::new()
            .name("jc3vr-image-writer".to_owned())
            .spawn(move || run(receiver))?;
        self.sender = Some(sender.clone());
        self.handle = Some(handle);
        Ok(sender)
    }
}

struct Job {
    target: ImageTarget,
    image: Image,
    completion: Option<Sender<WriteOutcome>>,
}

/// The writer thread: one image at a time until every sender is gone.
fn run(receiver: Receiver<Job>) {
    for job in receiver {
        let outcome = write_job(job.target, job.image);
        match &outcome.result {
            Ok(path) => tracing::debug!("image writer: wrote {}", path.display()),
            Err(e) => tracing::error!("image writer: {e:#}"),
        }
        if let Some(completion) = job.completion {
            let _ = completion.send(outcome);
        }
    }
}

fn write_job(mut target: ImageTarget, image: Image) -> WriteOutcome {
    let mut records = Vec::new();
    let mut resolve_inputs = None;
    if let ImageTarget::HarnessScreenshot {
        path,
        sidecar,
        stages,
    } = &mut target
    {
        let id = sidecar.id.clone();
        if let Some(capture) = &mut sidecar.capture {
            for stage in std::mem::take(stages) {
                let record = &mut capture.stages[stage.record];
                let name = stage_name(record.stage);
                let raw = matches!(&stage.artifact, StageArtifact::Raw(_));
                let extension = match (&stage.artifact, record.stage) {
                    (StageArtifact::Png(_), _) => "png",
                    (StageArtifact::Raw(_), stage) if stage.is_hdr() => "r11g11b10",
                    (StageArtifact::Raw(_), _) => "raw",
                };
                let stage_path =
                    path.with_file_name(format!("{}.{name}.eye{}.{extension}", id, record.eye));
                let result = match stage.artifact {
                    StageArtifact::Png(image) => write_png(&stage_path, image),
                    StageArtifact::Raw(bytes) => std::fs::write(&stage_path, bytes)
                        .with_context(|| format!("writing {}", stage_path.display())),
                };
                match result {
                    Ok(()) if raw => record.raw = Some(stage_path),
                    Ok(()) => record.png = Some(stage_path),
                    Err(error) => record.error = Some(format!("image writer: {error:#}")),
                }
            }
            if let Some(resolve) = capture.resolve_inputs.as_mut() {
                if let Some(folder) = path.parent() {
                    resolve.write_artifacts(folder, &id);
                }
                resolve_inputs = Some(resolve.clone());
            }
            records = capture.stages.clone();
        }
    }
    WriteOutcome {
        result: write(target, image).map_err(|error| format!("image writer: {error:#}")),
        stages: records,
        resolve_inputs,
    }
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::GeometryDepth => "geometry_depth",
        Stage::GBuffer0 => "gbuffer0",
        Stage::GBuffer1 => "gbuffer1",
        Stage::GBuffer2 => "gbuffer2",
        Stage::GBuffer3 => "gbuffer3",
        Stage::MainColor => "maincolor",
        Stage::PreAtmosphere => "pre_atmosphere",
        Stage::PostAtmosphere => "post_atmosphere",
        Stage::PostResolve => "post_resolve",
        Stage::SunDepthClipTreatment => "sun_depth_clip_treatment",
        Stage::PreSunColor => "pre_sun_color",
        Stage::PreSunDepth => "pre_sun_depth",
        Stage::PostSun => "post_sun",
        Stage::WaterBefore => "water_before",
        Stage::WaterAfterLeft => "water_after_left",
        Stage::WaterAfterRight => "water_after_right",
        Stage::WaterDepthBefore => "water_depth_before",
        Stage::WaterDepthAfterLeft => "water_depth_after_left",
        Stage::WaterDepthAfterRight => "water_depth_after_right",
        Stage::WaterReflectionTreatment => "water_reflection_treatment",
        Stage::WaterInputReflection => "water_input_t1",
        Stage::WaterInputStaticReflection => "water_input_t2",
        Stage::WaterInputReflectionAlpha => "water_input_t12",
        Stage::WaterInputRefraction => "water_input_t3",
        Stage::PreAa => "pre_aa",
        Stage::PostAa => "post_aa",
    }
}

fn write(target: ImageTarget, image: Image) -> anyhow::Result<PathBuf> {
    let (path, sidecar) = match &target {
        ImageTarget::HarnessScreenshot { path, .. } => (path.clone(), None),
        ImageTarget::Screenshot { stamp, sidecar } => (
            session::capture_file(CaptureFile::Screenshot, stamp, None)?,
            Some(sidecar),
        ),
        ImageTarget::TraceFrame {
            trace_stamp,
            frame,
            eye,
        } => (
            session::capture_folder(CaptureFolder::Trace, trace_stamp)?
                .join(trace_frame_name(*frame, *eye)),
            None,
        ),
    };

    write_png(&path, image)?;
    if let ImageTarget::HarnessScreenshot { sidecar, .. } = &target {
        write_sidecar(&path, sidecar)?;
    } else if let Some(sidecar) = sidecar {
        write_sidecar(&path, sidecar)?;
    }
    Ok(path)
}

fn write_png(path: &std::path::Path, mut image: Image) -> anyhow::Result<()> {
    to_rgba8_opaque(&mut image.pixels, image.order);
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    PngEncoder::new(&mut writer)
        .write_image(
            &image.pixels,
            image.width,
            image.height,
            ExtendedColorType::Rgba8,
        )
        .with_context(|| format!("encoding {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("flushing {}", path.display()))
}

fn write_sidecar(path: &std::path::Path, sidecar: &impl serde::Serialize) -> anyhow::Result<()> {
    let json_path = path.with_extension("json");
    let file = std::fs::File::create(&json_path)
        .with_context(|| format!("creating {}", json_path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, sidecar)
        .with_context(|| format!("writing {}", json_path.display()))?;
    writer
        .flush()
        .with_context(|| format!("flushing {}", json_path.display()))
}

/// A trace frame's image name: the frame index as the stem, marked as one eye.
fn trace_frame_name(frame: u32, eye: usize) -> String {
    format!("frame{frame:03}_eye{eye}.png")
}

/// Normalizes read-back rows in place: swaps R and B for the BGRA formats, and forces alpha opaque.
///
/// The alpha is discarded rather than passed through because nothing downstream of the captured
/// targets reads it -- the VR blit and the desktop mirror both draw into an opaque target with
/// blending off -- so the engine is free to leave it at whatever the last pass wrote, and it does:
/// the eye is cleared fully transparent behind a static-background full-screen UI. A PNG *is*
/// composited by its alpha, so passing that through turns a good capture into a viewer's blank page.
fn to_rgba8_opaque(pixels: &mut [u8], order: ChannelOrder) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if order == ChannelOrder::Bgra {
            pixel.swap(0, 2);
        }
        pixel[3] = 0xFF;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn harness_writes_png_and_adjacent_sidecar() {
        let directory = TestDirectory::new();
        let path = directory.0.join("capture.capture.png");
        let result_path = directory.0.join("capture.json");
        std::fs::write(&result_path, b"protocol result").unwrap();
        let sidecar = test_metadata();
        let result = write(
            ImageTarget::HarnessScreenshot {
                path: path.clone(),
                sidecar: Box::new(sidecar.clone()),
                stages: Vec::new(),
            },
            test_image(),
        )
        .unwrap();
        assert_eq!(result, path);
        let image = image::open(&path).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.as_raw(), &[30, 20, 10, 255]);
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(metadata, serde_json::to_value(sidecar).unwrap());
        assert_eq!(std::fs::read(result_path).unwrap(), b"protocol result");
    }

    #[test]
    fn stage_artifacts_finish_before_completion_and_sidecar_and_report_failures() {
        for fail in [false, true] {
            let directory = TestDirectory::new();
            let path = directory.0.join("writer-test.capture.png");
            let raw_path = directory.0.join("writer-test.maincolor.eye0.r11g11b10");
            let pre_path = directory.0.join("writer-test.pre_aa.eye0.png");
            let post_path = directory.0.join("writer-test.post_aa.eye1.png");
            if fail {
                std::fs::create_dir(&pre_path).unwrap();
            }
            let mut metadata = test_metadata();
            metadata.capture = Some(test_capture(&path));
            let raw = vec![0, 255, 128, 1];
            let stages = vec![
                StageImage {
                    record: 0,
                    artifact: StageArtifact::Raw(raw.clone()),
                },
                StageImage {
                    record: 1,
                    artifact: StageArtifact::Png(test_image()),
                },
                StageImage {
                    record: 2,
                    artifact: StageArtifact::Png(
                        Image::from_rows(&[10, 20, 30, 7], 4, 1, 1, ChannelOrder::Rgba).unwrap(),
                    ),
                },
            ];
            let (sender, jobs) = mpsc::sync_channel(1);
            let mut writer = test_writer(Some(sender), false);
            let completion = writer
                .try_submit(
                    ImageTarget::HarnessScreenshot {
                        path: path.clone(),
                        sidecar: Box::new(metadata),
                        stages,
                    },
                    test_image(),
                )
                .unwrap();
            assert!(matches!(
                completion.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            drop(writer);
            run(jobs);
            let outcome = completion.recv().unwrap();
            assert_eq!(outcome.result, Ok(path.clone()));
            assert_eq!(std::fs::read(&raw_path).unwrap(), raw);
            assert_eq!(outcome.stages[0].raw, Some(raw_path));
            assert_eq!(outcome.stages[2].png, Some(post_path.clone()));
            assert_eq!(
                image::open(post_path).unwrap().to_rgba8().as_raw(),
                &[10, 20, 30, 255]
            );
            if fail {
                assert!(outcome.stages[1].png.is_none());
                assert!(outcome.stages[1].error.as_ref().unwrap().contains("pre_aa"));
            } else {
                assert_eq!(outcome.stages[1].png, Some(pre_path.clone()));
                assert_eq!(
                    image::open(pre_path).unwrap().to_rgba8().as_raw(),
                    &[30, 20, 10, 255]
                );
            }
            let sidecar: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap())
                    .unwrap();
            assert_eq!(
                sidecar["stages"],
                serde_json::to_value(outcome.stages).unwrap()
            );
        }
    }

    #[test]
    fn geometry_artifacts_keep_stable_names_alpha_and_acknowledged_metadata() {
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        let template = capture.stages[0].clone();
        capture.stages = [
            (Stage::GeometryDepth, "geometry_depth", 8),
            (Stage::WaterDepthBefore, "water_depth_before", 8),
            (Stage::WaterDepthAfterLeft, "water_depth_after_left", 8),
            (Stage::WaterDepthAfterRight, "water_depth_after_right", 8),
            (Stage::GBuffer0, "gbuffer0", 4),
            (Stage::GBuffer1, "gbuffer1", 4),
            (Stage::GBuffer2, "gbuffer2", 4),
            (Stage::GBuffer3, "gbuffer3", 4),
        ]
        .into_iter()
        .map(|(stage, name, pixel_bytes)| {
            assert_eq!(serde_json::to_value(stage).unwrap(), name);
            let mut record = template.clone();
            record.stage = stage;
            record.source.as_mut().unwrap().bytes_per_pixel = Some(pixel_bytes);
            record
        })
        .collect();
        let stages = capture
            .stages
            .iter()
            .enumerate()
            .map(|(record, stage)| StageImage {
                record,
                artifact: StageArtifact::Raw(
                    (0..stage.source.as_ref().unwrap().bytes_per_pixel.unwrap() as u8).collect(),
                ),
            })
            .collect();
        metadata.capture = Some(capture);
        let (sender, jobs) = mpsc::sync_channel(1);
        let mut writer = test_writer(Some(sender), false);
        let completion = writer
            .try_submit(
                ImageTarget::HarnessScreenshot {
                    path: path.clone(),
                    sidecar: Box::new(metadata),
                    stages,
                },
                test_image(),
            )
            .unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(writer);
        run(jobs);
        let outcome = completion.recv().unwrap();
        assert_eq!(outcome.result, Ok(path.clone()));
        for record in &outcome.stages {
            let name = stage_name(record.stage);
            let raw_path = directory.0.join(format!("writer-test.{name}.eye0.raw"));
            assert_eq!(record.raw.as_ref(), Some(&raw_path));
            assert!(record.png.is_none());
            assert!(record.error.is_none());
            assert!(record.means.is_none());
            let bytes = std::fs::read(raw_path).unwrap();
            assert_eq!(bytes[3], 3);
            assert_eq!(
                bytes.len(),
                record.source.as_ref().unwrap().bytes_per_pixel.unwrap()
            );
        }
        let sidecar: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(
            sidecar["stages"],
            serde_json::to_value(outcome.stages).unwrap()
        );
    }

    #[test]
    fn raw_stage_failure_is_acknowledged_in_the_sidecar() {
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let raw_path = directory.0.join("writer-test.geometry_depth.eye0.raw");
        std::fs::create_dir(&raw_path).unwrap();
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        capture.stages.truncate(1);
        capture.stages[0].stage = Stage::GeometryDepth;
        metadata.capture = Some(capture);
        let outcome = write_job(
            ImageTarget::HarnessScreenshot {
                path: path.clone(),
                sidecar: Box::new(metadata),
                stages: vec![StageImage {
                    record: 0,
                    artifact: StageArtifact::Raw(vec![0; 8]),
                }],
            },
            test_image(),
        );
        assert_eq!(outcome.result, Ok(path.clone()));
        let record = &outcome.stages[0];
        assert!(record.raw.is_none());
        assert!(
            record
                .error
                .as_ref()
                .unwrap()
                .contains("geometry_depth.eye0.raw")
        );
        let sidecar: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(
            sidecar["stages"],
            serde_json::to_value(outcome.stages).unwrap()
        );
    }

    #[test]
    fn hdr_stage_names_and_extensions_cover_every_hdr_stage() {
        let directory = TestDirectory::new();
        let path = directory.0.join("writer-test.capture.png");
        let mut metadata = test_metadata();
        let mut capture = test_capture(&path);
        let template = capture.stages[0].clone();
        let stages = [
            (Stage::MainColor, "maincolor"),
            (Stage::PreAtmosphere, "pre_atmosphere"),
            (Stage::PostAtmosphere, "post_atmosphere"),
            (Stage::PostResolve, "post_resolve"),
            (Stage::PostSun, "post_sun"),
            (Stage::WaterBefore, "water_before"),
            (Stage::WaterAfterLeft, "water_after_left"),
            (Stage::WaterAfterRight, "water_after_right"),
        ];
        capture.stages = stages
            .into_iter()
            .map(|(stage, name)| {
                assert_eq!(stage_name(stage), name);
                assert!(stage.is_hdr());
                let mut record = template.clone();
                record.stage = stage;
                record
            })
            .collect();
        metadata.capture = Some(capture);
        let images = stages
            .into_iter()
            .enumerate()
            .map(|(record, _)| StageImage {
                record,
                artifact: StageArtifact::Raw(vec![0; 4]),
            })
            .collect();
        let outcome = write_job(
            ImageTarget::HarnessScreenshot {
                path: path.clone(),
                sidecar: Box::new(metadata),
                stages: images,
            },
            test_image(),
        );
        assert_eq!(outcome.result, Ok(path));
        for (record, (_, name)) in outcome.stages.iter().zip(stages) {
            let raw_path = directory
                .0
                .join(format!("writer-test.{name}.eye0.r11g11b10"));
            assert_eq!(record.raw.as_ref(), Some(&raw_path));
            assert!(raw_path.is_file());
            assert!(record.png.is_none());
            assert!(record.error.is_none());
        }
    }

    fn test_capture(path: &std::path::Path) -> crate::screenshot::harness::types::CapturedFrame {
        crate::screenshot::harness::types::CapturedFrame {
            png: path.to_owned(),
            metadata: path.with_extension("json"),
            frame_ordinal: 1,
            predicted_display_time_ns: 42,
            source: crate::screenshot::harness::types::CaptureKind::EyeTextures,
            width: 2,
            height: 1,
            eye_width: 1,
            eye_height: 1,
            format: "test".to_owned(),
            fresh_eyes: [true; 2],
            rendered_eyes: [true; 2],
            selected_eye: None,
            mode: crate::stereo::dispatch::RenderMode::Multipass,
            visual: crate::testing::visual::metadata(),
            comparison_inputs: crate::screenshot::harness::types::ComparisonInputs {
                build_stamp: "test-build",
                profile: crate::testing::visual::DiagnosticProfile::Native,
                camera: None,
                eyes: [None, None],
                eye_width: 1,
                eye_height: 1,
                world_scale: 1.0,
            },
            effective_config: crate::config::Config::new(),
            water_reflection: None,
            cascade_scale: None,
            sun_depth_clip: None,
            resolve_inputs: None,
            single_pass: None,
            constants: Default::default(),
            stages: [(Stage::MainColor, 0), (Stage::PreAa, 0), (Stage::PostAa, 1)]
                .into_iter()
                .map(|(stage, eye)| StageRecord {
                    frame: 42,
                    stage,
                    eye,
                    source: Some(crate::screenshot::harness::stages::Source {
                        width: 1,
                        height: 1,
                        format_id: 0,
                        bytes_per_pixel: Some(4),
                        format: if stage == Stage::MainColor {
                            "DXGI_FORMAT_R11G11B10_FLOAT"
                        } else {
                            "DXGI_FORMAT_R8G8B8A8_TYPELESS"
                        }
                        .to_owned(),
                    }),
                    viewports: Vec::new(),
                    scissors: Vec::new(),
                    scissor_enabled: None,
                    means: None,
                    png: None,
                    raw: None,
                    error: None,
                    water_input: None,
                    sun_input: None,
                })
                .collect(),
        }
    }

    #[test]
    fn completion_follows_both_file_writes() {
        let directory = TestDirectory::new();
        let path = directory.0.join("completion.png");
        let (sender, jobs) = mpsc::sync_channel(1);
        let mut writer = test_writer(Some(sender), false);
        let completion = writer
            .try_submit(test_target(path.clone()), test_image())
            .unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(writer);
        run(jobs);
        assert_eq!(completion.recv().unwrap().result, Ok(path.clone()));
        assert!(image::open(&path).is_ok());
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("json")).unwrap()).unwrap();
        assert_eq!(metadata["id"], "writer-test");
        assert_eq!(metadata["state"], "captured");
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn completion_reports_png_and_sidecar_failures() {
        let directory = TestDirectory::new();
        let png_failure = directory.0.join("png_failure.png");
        let sidecar_failure = directory.0.join("sidecar_failure.png");
        std::fs::create_dir(&png_failure).unwrap();
        std::fs::create_dir(sidecar_failure.with_extension("json")).unwrap();
        for path in [&png_failure, &sidecar_failure] {
            let (sender, jobs) = mpsc::sync_channel(1);
            let mut writer = test_writer(Some(sender), false);
            let completion = writer
                .try_submit(test_target(path.clone()), test_image())
                .unwrap();
            drop(writer);
            run(jobs);
            let error = completion.recv().unwrap().result.unwrap_err();
            let failed_path = if path == &png_failure {
                path.clone()
            } else {
                path.with_extension("json")
            };
            assert!(
                error.contains(&failed_path.display().to_string()),
                "{error}"
            );
        }
        assert!(image::open(&sidecar_failure).is_ok());
    }

    #[test]
    fn abandoned_completion_does_not_stop_the_writer() {
        let directory = TestDirectory::new();
        let (sender, jobs) = mpsc::sync_channel(2);
        let mut writer = test_writer(Some(sender), false);
        let abandoned_path = directory.0.join("abandoned.png");
        drop(
            writer
                .try_submit(test_target(abandoned_path.clone()), test_image())
                .unwrap(),
        );
        let path = directory.0.join("next.png");
        let completion = writer
            .try_submit(test_target(path.clone()), test_image())
            .unwrap();
        drop(writer);
        run(jobs);
        assert_eq!(completion.recv().unwrap().result, Ok(path));
        assert!(abandoned_path.with_extension("json").is_file());
    }

    #[test]
    fn try_submit_reports_closed_full_and_disconnected_admission() {
        let target = || test_target(PathBuf::from("unused.png"));
        let mut closed = test_writer(None, true);
        assert!(
            closed
                .try_submit(target(), test_image())
                .unwrap_err()
                .to_string()
                .contains("closed")
        );
        assert!(closed.sender.is_none());

        let (sender, jobs) = mpsc::sync_channel(1);
        let mut writer = test_writer(Some(sender), false);
        let admitted = writer.try_submit(target(), test_image()).unwrap();
        assert!(
            writer
                .try_submit(target(), test_image())
                .unwrap_err()
                .to_string()
                .contains("full")
        );
        drop(jobs);
        assert!(
            writer
                .try_submit(target(), test_image())
                .unwrap_err()
                .to_string()
                .contains("disconnected")
        );
        assert!(matches!(
            admitted.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    fn test_image() -> Image {
        Image::from_rows(&[10, 20, 30, 0], 4, 1, 1, ChannelOrder::Bgra).unwrap()
    }

    fn test_target(path: PathBuf) -> ImageTarget {
        ImageTarget::HarnessScreenshot {
            path,
            sidecar: Box::new(test_metadata()),
            stages: Vec::new(),
        }
    }

    fn test_metadata() -> CaptureMetadata {
        CaptureMetadata {
            id: "writer-test".to_owned(),
            state: crate::screenshot::harness::types::CaptureState::Captured,
            readiness: crate::screenshot::harness::types::Readiness {
                gameplay_ready: true,
                xr_session_running: true,
                xr_frame_rendering: true,
                build_stamp: "test-build",
            },
            capture: None,
            error: None,
        }
    }

    fn test_writer(sender: Option<SyncSender<Job>>, closed: bool) -> Writer {
        Writer {
            sender,
            handle: None,
            closed,
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "jc3vr-image-writer-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed),
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("creating {}: {error}", path.display()),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn from_rows_drops_the_pitch_padding() {
        // Two rows of one pixel each, with four bytes of padding per row.
        let bytes = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        let image = Image::from_rows(&bytes, 8, 1, 2, ChannelOrder::Rgba).unwrap();
        assert_eq!(image.pixels, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!((image.width(), image.height()), (1, 2));
    }

    #[test]
    fn from_rows_rejects_a_short_mapping_and_a_narrow_pitch() {
        // The second row of a 1x2 image at pitch 8 ends at byte 12.
        assert!(Image::from_rows(&[0u8; 11], 8, 1, 2, ChannelOrder::Rgba).is_none());
        assert!(Image::from_rows(&[0u8; 12], 8, 1, 2, ChannelOrder::Rgba).is_some());
        assert!(Image::from_rows(&[0u8; 12], 2, 1, 2, ChannelOrder::Rgba).is_none());
    }

    #[test]
    fn conversion_swizzles_bgra_and_forces_alpha_opaque() {
        let mut bgra = [10, 20, 30, 0, 40, 50, 60, 7];
        to_rgba8_opaque(&mut bgra, ChannelOrder::Bgra);
        assert_eq!(bgra, [30, 20, 10, 255, 60, 50, 40, 255]);

        let mut rgba = [10, 20, 30, 0];
        to_rgba8_opaque(&mut rgba, ChannelOrder::Rgba);
        assert_eq!(rgba, [10, 20, 30, 255]);
    }

    /// The analyzer globs `frame*_eye<N>.png` and sorts by name, so the index must be zero-padded.
    #[test]
    fn trace_frame_names_sort_by_frame_and_mark_the_eye() {
        assert_eq!(trace_frame_name(7, 1), "frame007_eye1.png");
        assert!(trace_frame_name(9, 0) < trace_frame_name(10, 0));
    }
}
