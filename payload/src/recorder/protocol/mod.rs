//! The capture host's control protocol, versions 1 and 2, as the payload speaks it.
//!
//! Each control message is one line of UTF-8 JSON, at most [`LINE_LIMIT`] bytes. The payload opens
//! with a start line naming the take, its folder, and the game window; the host answers with
//! [`HostMessage::Started`] once it records, or [`HostMessage::Error`] and a close. A message of an
//! unknown type is ignored.
//!
//! The version decides who makes the pixels.
//!
//! - Version 1 ([`start_line`]): the host records the game's mirror window on screen (the relay,
//!   `tools/capture/relay.py`, with gpu-screen-recorder). The payload ends the take with a
//!   [`stop_line`] (or by closing the connection), and the host answers [`HostMessage::Stopped`]
//!   once the video file is finalized.
//! - Version 2 ([`stream_start_line`]): the payload streams the eye views itself. The start line
//!   carries the stream's format ([`VideoFormat`]) and the take's folder as the payload names it,
//!   for a host running in the game's own Windows environment (the injector). Once the host has
//!   answered [`HostMessage::Started`], the payload writes a `yuv4mpegpipe` stream on the same
//!   connection, and it ends the take by closing its writing half: the end of the stream is the
//!   stop. The host answers [`HostMessage::Stopped`] once the file is finalized. While the take
//!   runs the host may send [`HostMessage::Audio`] when its audio capture changes.
//!
//! The relay's and the injector's module documentation are the other half of this description.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::recorder::layout::{EyeRect, StreamLayout};

/// The protocol name the start line carries.
pub(crate) const PROTOCOL: &str = "jc3vr-capture";
/// The version of a take the host records from the mirror window.
pub(crate) const MIRROR_VERSION: u32 = 1;
/// The version of a take the payload streams itself.
pub(crate) const STREAM_VERSION: u32 = 2;
/// The longest control line either side accepts.
pub(crate) const LINE_LIMIT: usize = 64 * 1024;

/// The game window as the payload describes it, which the host uses to find the window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct WindowHint {
    pub(crate) title: Option<String>,
    pub(crate) client_size: Option<[u32; 2]>,
}

/// The region the host records, in the compositor's logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The stream a version 2 take sends: its size, rate, and where each eye is in the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VideoFormat {
    pub(crate) layout: StreamLayout,
    pub(crate) frame_rate: u32,
}

/// A message from the host.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HostMessage {
    /// The host is recording.
    Started {
        /// The video file, as the host names it.
        file: String,
        /// The wall-clock time of the video's first frame, when the recorder reported it.
        #[serde(default)]
        video_start_unix_ms: Option<f64>,
        /// The screen region recorded, for a mirror take.
        #[serde(default)]
        region: Option<Region>,
        /// The output the region is on, when the host knows it.
        #[serde(default)]
        output: Option<String>,
        /// The video encoder the host chose, for a streamed take.
        #[serde(default)]
        encoder: Option<String>,
        /// What the host records the audio from, or why it records none.
        #[serde(default)]
        audio: Option<String>,
    },
    /// The host's audio capture changed during the take.
    Audio { state: String },
    /// The host could not start, or stopped recording on its own; it closes the connection.
    Error { message: String },
    /// The host finalized the video after the take ended.
    Stopped { exit_code: Option<i32> },
    /// A message type this version does not know.
    #[serde(other)]
    Unknown,
}

/// A control line that could not be read.
#[derive(Debug)]
pub(crate) enum ProtocolError {
    /// The line is longer than [`LINE_LIMIT`].
    TooLong,
    /// The line is not a message of this protocol.
    Malformed(serde_json::Error),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "capture protocol: a line from the host is longer than {LINE_LIMIT} bytes"
            ),
            Self::Malformed(error) => {
                write!(
                    f,
                    "capture protocol: a malformed line from the host: {error}"
                )
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// The version 1 start line for take `take`, recorded into `folder` (the take's folder as the host
/// names it, `None` when the payload cannot name it).
pub(crate) fn start_line(take: &str, folder: Option<&str>, window: &WindowHint) -> Vec<u8> {
    line(&Start {
        protocol: PROTOCOL,
        version: MIRROR_VERSION,
        kind: "start",
        take,
        folder,
        window,
    })
}

/// The version 2 start line for take `take`, streaming `video`. `folder` is the take's folder as a
/// host outside the game's Windows environment names it, when known, and `payload_folder` as the
/// payload names it.
pub(crate) fn stream_start_line(
    take: &str,
    folder: Option<&str>,
    payload_folder: &str,
    window: &WindowHint,
    video: &VideoFormat,
) -> Vec<u8> {
    line(&StreamStart {
        protocol: PROTOCOL,
        version: STREAM_VERSION,
        kind: "start",
        take,
        folder,
        payload_folder,
        window,
        video: VideoJson {
            format: "yuv4mpegpipe",
            width: video.layout.width,
            height: video.layout.height,
            frame_rate: video.frame_rate,
            pixel_format: "yuv420p",
            color_space: "bt709",
            color_range: "limited",
            layout: video.layout.arrangement.label(),
            eyes: &video.layout.eyes,
        },
    })
}

/// The version 1 stop line.
pub(crate) fn stop_line() -> Vec<u8> {
    line(&Stop { kind: "stop" })
}

/// The `yuv4mpegpipe` stream header for `video`: the size, the rate, and progressive 4:2:0 in
/// limited range with centred chroma.
pub(crate) fn stream_header(video: &VideoFormat) -> String {
    format!(
        "YUV4MPEG2 W{} H{} F{}:1 Ip A1:1 C420jpeg XYSCSS=420JPEG XCOLORRANGE=LIMITED\n",
        video.layout.width, video.layout.height, video.frame_rate
    )
}

/// The tag that starts each frame of the stream, before its planar bytes.
pub(crate) const FRAME_TAG: &[u8] = b"FRAME\n";

/// Parse one line from the host, without its newline.
pub(crate) fn parse_host_line(line: &[u8]) -> Result<HostMessage, ProtocolError> {
    serde_json::from_slice(line).map_err(ProtocolError::Malformed)
}

/// Splits the host's byte stream into lines.
#[derive(Default)]
pub(crate) struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    /// Add bytes read from the host, returning the lines they complete.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError> {
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=end).collect();
            line.pop();
            lines.push(line);
        }
        if self.pending.len() > LINE_LIMIT {
            return Err(ProtocolError::TooLong);
        }
        Ok(lines)
    }
}

#[derive(Serialize)]
struct Start<'a> {
    protocol: &'a str,
    version: u32,
    #[serde(rename = "type")]
    kind: &'a str,
    take: &'a str,
    folder: Option<&'a str>,
    window: &'a WindowHint,
}

#[derive(Serialize)]
struct StreamStart<'a> {
    protocol: &'a str,
    version: u32,
    #[serde(rename = "type")]
    kind: &'a str,
    take: &'a str,
    folder: Option<&'a str>,
    payload_folder: &'a str,
    window: &'a WindowHint,
    video: VideoJson<'a>,
}

#[derive(Serialize)]
struct VideoJson<'a> {
    format: &'a str,
    width: u32,
    height: u32,
    frame_rate: u32,
    pixel_format: &'a str,
    color_space: &'a str,
    color_range: &'a str,
    layout: &'a str,
    eyes: &'a [EyeRect; 2],
}

#[derive(Serialize)]
struct Stop<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

fn line(message: &impl Serialize) -> Vec<u8> {
    let mut bytes =
        serde_json::to_vec(message).expect("capture protocol: a control message serializes");
    bytes.push(b'\n');
    bytes
}

#[cfg(test)]
mod tests;
