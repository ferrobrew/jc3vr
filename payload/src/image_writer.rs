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
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
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

use crate::session::{self, CaptureFile, CaptureFolder};

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
    if sender.send(Job { target, image }).is_err() {
        tracing::error!("image writer: the writer thread has exited; the image is dropped");
    }
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
}

/// The writer thread: one image at a time until every sender is gone.
fn run(receiver: Receiver<Job>) {
    for job in receiver {
        match write(job) {
            Ok(path) => tracing::debug!("image writer: wrote {}", path.display()),
            Err(e) => tracing::error!("image writer: {e:#}"),
        }
    }
}

fn write(Job { target, mut image }: Job) -> anyhow::Result<PathBuf> {
    to_rgba8_opaque(&mut image.pixels, image.order);

    let (path, sidecar) = match &target {
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

    let file =
        std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(
            &image.pixels,
            image.width,
            image.height,
            ExtendedColorType::Rgba8,
        )
        .with_context(|| format!("encoding {}", path.display()))?;

    if let Some(sidecar) = sidecar {
        let json_path = path.with_extension("json");
        let json = serde_json::to_string_pretty(sidecar)
            .with_context(|| format!("serializing {}", json_path.display()))?;
        std::fs::write(&json_path, json)
            .with_context(|| format!("writing {}", json_path.display()))?;
    }
    Ok(path)
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
    use pretty_assertions::assert_eq;

    use super::*;

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
