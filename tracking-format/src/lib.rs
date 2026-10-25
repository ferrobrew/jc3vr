//! The tracking file: a recording of what the OpenXR runtime gave the mod each frame, in the form
//! the mod published it, so that a replay can publish it again.
//!
//! This crate is the file's data model and codec and nothing else, so that it builds on any host:
//! the payload records and replays through it, converting between this model and its own types,
//! and host tools write recordings with it.
//!
//! # Layout
//!
//! All integers and floats are little-endian. A file is:
//!
//! 1. The magic, the eight bytes `JC3VRTRK`.
//! 2. The format version, a `u16` ([`VERSION`]).
//! 3. The header's length, a `u32`, then the [`Header`].
//! 4. Records until the end of the file, each a tag byte, a `u32` payload length, and the payload.
//!    [`TAG_FRAME`] carries a [`Frame`]; [`TAG_INPUT`] carries independently versioned input history.
//!    Version 2 keeps the version 1 frame/header layouts. Version 1 reads return no input history
//!    and cannot reproduce within-sync order, source interpretation, or logical input timing.
//!    Version 3 stores complete serialized frame/input batches inside [`TAG_CHUNK`] records.
//!    Each chunk independently selects raw or LZ4 block storage and verifies metadata and stored
//!    bytes with CRC-32. The expanded records retain their original bytes and order.
//!
//! Versions 1 and 2 skip unknown tags. Version 3 rejects unknown required tags; only unknown tags
//! with [`OPTIONAL_TAG_BIT`] set are skippable. Chunks cannot contain chunks or required unknown
//! records. A cut final outer record leaves all earlier records readable and marks
//! [`Recording::truncated`]. A complete malformed chunk, unread inner bytes, a bad checksum, or a
//! resource-budget violation is an error. [`decode_with_limits`] bounds aggregate resource use.
//!
//! The poses are in the cockpit frame, as the mod publishes them (the eye views and the controller
//! spaces re-based against the recenter baseline): metres, +X right, +Y up, -Z forward. A replay
//! therefore needs no baseline of its own. A recenter during the recording is recorded as an event
//! on the frame it took effect in.

mod chunk;
mod codec;
pub mod input;
mod input_codec;
pub mod model;
mod read;
mod wire;

use glam::{Quat, Vec3};
pub use openxr_sys::SessionState;

pub use crate::{
    chunk::{MAX_CHUNK_BYTES, MAX_CHUNK_RECORDS, OPTIONAL_TAG_BIT, TAG_CHUNK, encode_chunk},
    read::{DecodeLimits, decode_with_limits},
    wire::{DecodeError, DecodeErrorKind},
};
use crate::{
    model::{ActionSetKind, Hand, HandFrame, Pose, RecenterCause, RecenterKind, SetState, Stage},
    wire::Encoder,
};

/// The file extension of a tracking recording.
pub const EXTENSION: &str = "jc3trk";

/// The newest supported format version. Legacy preamble functions retain versions 1 and 2.
pub const VERSION: u16 = 3;

/// A whole recording, as read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub header: Header,
    pub frames: Vec<Frame>,
    /// Ordered input records, absent in legacy snapshots. No order is inferred from frame order.
    pub input: Vec<input::CapturedInput>,
    /// The decoded container version; version 1 cannot reproduce ordered input decisions.
    pub version: u16,
    /// Whether the last record was cut short and left out.
    pub truncated: bool,
    /// How many records of a kind this build does not know were skipped.
    pub skipped_records: u32,
}

/// What a recording knows about itself and where it started.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The build that recorded it.
    pub build: String,
    /// The local wall-clock stamp of the recording's start.
    pub recorded_at: String,
    /// The version of the semantic action schema the recorded semantic states are indexed by.
    pub action_schema: u32,
    pub start: StartMarker,
    /// The dominant hand configured when the recording started.
    pub dominant_hand: Hand,
    /// The VR world scale configured when the recording started.
    pub world_scale: f32,
    /// The director's game-clock rate when the recording started.
    pub time_scale: f32,
}

/// Where the player was when the recording started.
#[derive(Debug, Clone, PartialEq)]
pub struct StartMarker {
    /// The player character's world matrix, row-major, translation in elements 12 to 14.
    pub transform: [f32; 16],
    /// The director bookmark the player stood at, if any.
    pub bookmark: Option<String>,
}

impl StartMarker {
    /// The marker's world position.
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.transform[12], self.transform[13], self.transform[14])
    }
}

/// One rendered frame's tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Nanoseconds since the recording started, on the host's monotonic clock.
    pub time_ns: u64,
    /// The runtime's predicted display time the poses were located at.
    pub display_time_ns: i64,
    pub session: SessionSample,
    /// The eye views, when the runtime located them this frame.
    pub head: Option<HeadSample>,
    pub stage: Stage,
    /// The published controller frame, when the controllers were sampled.
    pub controllers: Option<ControllerSample>,
    /// The published semantic frame, when the controllers were sampled.
    pub semantic: Option<SemanticSample>,
    /// The recenters that re-based the cockpit before this frame was located.
    pub recenters: Vec<RecenterEvent>,
}

/// The session's state for one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SessionSample {
    /// The session state the runtime last reported.
    pub state: Option<SessionState>,
    /// Whether the runtime asked for the frame to be rendered.
    pub should_render: bool,
    /// Whether the view pose was valid.
    pub head_tracked: bool,
}

/// Both eyes' located views in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HeadSample {
    /// Indexed left, right.
    pub eyes: [EyeSample; 2],
}

/// One eye's located view.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EyeSample {
    /// The eye pose in the cockpit frame.
    pub pose: Pose,
    /// The field of view as `left, right, up, down` angles in radians.
    pub fov: [f32; 4],
}

/// The published controller frame, less the fields a publish stamps.
#[derive(Debug, Clone, PartialEq)]
pub struct ControllerSample {
    pub focused: bool,
    pub action_set: ActionSetKind,
    /// Indexed by [`Hand::index`].
    pub hands: [HandFrame; 2],
}

/// The published semantic frame, less its publish stamp.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSample {
    pub action_set: ActionSetKind,
    pub global: SetState,
    pub mode: SetState,
}

/// A recenter of the cockpit baseline.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RecenterEvent {
    pub kind: RecenterKind,
    pub cause: RecenterCause,
    /// The turn of the cockpit frame, new frame from old.
    pub turn: Quat,
}

/// The tag of a [`Frame`] record.
pub const TAG_FRAME: u8 = 1;

/// A snapshot-only preamble. Existing writers remain v1 until they also capture input history.
pub fn encode_preamble(header: &Header) -> Vec<u8> {
    preamble(header, 1)
}

/// A version 2 preamble for writers that include the independent input-record sequence.
pub fn encode_input_preamble(header: &Header) -> Vec<u8> {
    preamble(header, 2)
}

/// A version 3 preamble. Every required outer record is an independently recoverable chunk.
pub fn encode_chunked_preamble(header: &Header) -> Vec<u8> {
    preamble(header, VERSION)
}

fn preamble(header: &Header, version: u16) -> Vec<u8> {
    let mut payload = Encoder::default();
    codec::header(&mut payload, header);
    let mut out = Encoder::default();
    out.bytes.extend_from_slice(&MAGIC);
    out.u16(version);
    out.u32(payload.bytes.len() as u32);
    out.bytes.extend_from_slice(&payload.bytes);
    out.bytes
}

/// One frame's record.
pub fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut payload = Encoder::default();
    codec::frame(&mut payload, frame);
    let mut out = Encoder::default();
    out.u8(TAG_FRAME);
    out.u32(payload.bytes.len() as u32);
    out.bytes.extend_from_slice(&payload.bytes);
    out.bytes
}

/// The independent input-history record tag. The frame payload remains byte-compatible with v1.
pub const TAG_INPUT: u8 = 2;

/// Encode one bounded input record. Invalid producers do not silently truncate paths or routes.
pub fn encode_input(captured: &input::CapturedInput) -> Result<Vec<u8>, DecodeError> {
    if let input::InputRecord::Group(group) = &captured.record
        && !group.is_bounded()
    {
        return Err(DecodeError {
            offset: 0,
            kind: DecodeErrorKind::InvalidValue {
                field: "input group bounds",
                value: group.samples.len() as u64,
            },
        });
    }
    if let input::InputRecord::Policy(policy) = &captured.record
        && !policy.is_valid()
    {
        return Err(DecodeError {
            offset: 0,
            kind: DecodeErrorKind::InvalidValue {
                field: "input policy decision",
                value: u64::from(policy.version),
            },
        });
    }
    let mut payload = Encoder::default();
    input_codec::encode(&mut payload, captured);
    let mut out = Encoder::default();
    out.u8(TAG_INPUT);
    out.u32(payload.bytes.len() as u32);
    out.bytes.extend_from_slice(&payload.bytes);
    Ok(out.bytes)
}

/// Read a whole file within [`DecodeLimits::default`].
pub fn decode(bytes: &[u8]) -> Result<Recording, DecodeError> {
    decode_with_limits(bytes, DecodeLimits::default())
}

/// The bytes a file starts with.
const MAGIC: [u8; 8] = *b"JC3VRTRK";

/// The bytes of a record's tag and length.
const RECORD_FRAMING: usize = 5;
