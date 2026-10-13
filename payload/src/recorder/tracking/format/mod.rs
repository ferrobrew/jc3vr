//! The tracking file: a recording of what the OpenXR runtime gave the mod each frame, in the form
//! the mod published it, so that a replay can publish it again.
//!
//! The file's layout, data model, and codec are the `tracking-format` crate's, which host tools
//! share. This module holds the recording in the payload's own types and converts between them and
//! the crate's ([`convert`]), so the recorder and the replay deal only in the types they publish.
//!
//! The poses are in the cockpit frame, as the mod publishes them (the eye views and the controller
//! spaces re-based against the recenter baseline), so a replay needs no baseline of its own. A
//! recenter during the recording is recorded as an event on the frame it took effect in.

mod convert;

#[cfg(test)]
mod tests;

use glam::Quat;
use openxr as xr;
pub(crate) use tracking_format::{DecodeError, EXTENSION};
// Only the tests name these.
#[cfg(test)]
use tracking_format::{DecodeErrorKind, VERSION};

use crate::{
    controllers::frame::{ActionSetKind, Hand, HandFrame, Pose},
    vr::{RecenterCause, RecenterKind, input::semantic::frame::SetState, stage::StageInfo},
};

/// A whole recording, as read back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Recording {
    pub(crate) header: Header,
    pub(crate) frames: Vec<Frame>,
    /// Whether the last record was cut short and left out.
    pub(crate) truncated: bool,
    /// How many records of a kind this build does not know were skipped.
    pub(crate) skipped_records: u32,
}

/// What a recording knows about itself and where it started.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Header {
    /// The payload build that recorded it.
    pub(crate) build: String,
    /// The local wall-clock stamp of the recording's start.
    pub(crate) recorded_at: String,
    /// The semantic action schema the recorded semantic states are indexed by
    /// ([`crate::vr::input::semantic::SCHEMA_VERSION`]).
    pub(crate) action_schema: u32,
    pub(crate) start: StartMarker,
    /// The dominant hand configured when the recording started.
    pub(crate) dominant_hand: Hand,
    /// The VR world scale configured when the recording started.
    pub(crate) world_scale: f32,
    /// The director's game-clock rate when the recording started.
    pub(crate) time_scale: f32,
}

/// Where the player was when the recording started.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StartMarker {
    /// The player character's world matrix, row-major, translation in elements 12 to 14.
    pub(crate) transform: [f32; 16],
    /// The director bookmark the player stood at, if any.
    pub(crate) bookmark: Option<String>,
}

impl StartMarker {
    /// The marker's world position.
    pub(crate) fn position(&self) -> glam::Vec3 {
        glam::Vec3::new(self.transform[12], self.transform[13], self.transform[14])
    }
}

/// One rendered frame's tracking.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frame {
    /// Nanoseconds since the recording started, on the host's monotonic clock.
    pub(crate) time_ns: u64,
    /// The runtime's predicted display time the poses were located at.
    pub(crate) display_time_ns: i64,
    pub(crate) session: SessionSample,
    /// The eye views, when the runtime located them this frame.
    pub(crate) head: Option<HeadSample>,
    pub(crate) stage: StageInfo,
    /// The published controller frame, when the controllers were sampled.
    pub(crate) controllers: Option<ControllerSample>,
    /// The published semantic frame, when the controllers were sampled.
    pub(crate) semantic: Option<SemanticSample>,
    /// The recenters that re-based the cockpit before this frame was located.
    pub(crate) recenters: Vec<RecenterEvent>,
}

/// The session's state for one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SessionSample {
    /// The session state the runtime last reported.
    pub(crate) state: Option<xr::SessionState>,
    /// Whether the runtime asked for the frame to be rendered.
    pub(crate) should_render: bool,
    /// Whether the view pose was valid.
    pub(crate) head_tracked: bool,
}

/// Both eyes' located views in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HeadSample {
    /// Indexed left, right.
    pub(crate) eyes: [EyeSample; 2],
}

/// One eye's located view.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct EyeSample {
    /// The eye pose in the cockpit frame.
    pub(crate) pose: Pose,
    /// The field of view as `left, right, up, down` angles in radians.
    pub(crate) fov: [f32; 4],
}

/// The published [`ControllerFrame`](crate::controllers::frame::ControllerFrame), less the fields
/// a publish stamps.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ControllerSample {
    pub(crate) focused: bool,
    pub(crate) action_set: ActionSetKind,
    /// Indexed by [`Hand::index`].
    pub(crate) hands: [HandFrame; 2],
}

/// The published [`SemanticFrame`](crate::vr::input::semantic::frame::SemanticFrame), less its
/// publish stamp.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SemanticSample {
    pub(crate) action_set: ActionSetKind,
    pub(crate) global: SetState,
    pub(crate) mode: SetState,
}

/// A recenter of the cockpit baseline.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct RecenterEvent {
    pub(crate) kind: RecenterKind,
    pub(crate) cause: RecenterCause,
    /// The turn of the cockpit frame, new frame from old.
    pub(crate) turn: Quat,
}

/// The bytes a file starts with: the magic, the version, and the header.
pub(crate) fn encode_preamble(header: &Header) -> Vec<u8> {
    tracking_format::encode_preamble(&convert::header_to_file(header))
}

/// One frame's record.
pub(crate) fn encode_frame(frame: &Frame) -> Vec<u8> {
    tracking_format::encode_frame(&convert::frame_to_file(frame))
}

/// Read a whole file.
pub(crate) fn decode(bytes: &[u8]) -> Result<Recording, DecodeError> {
    tracking_format::decode(bytes).map(convert::recording_from_file)
}
