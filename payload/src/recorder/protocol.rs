//! The capture host's control protocol, version 1, as the payload speaks it.
//!
//! The capture host (`tools/capture/relay.py`, started by `scripts/capture_host.sh`) records the
//! game's mirror window; the payload only tells it when a take starts and stops and where its
//! files go. Each message is one line of UTF-8 JSON, at most [`LINE_LIMIT`] bytes. The payload
//! opens with a [`start_line`] naming the take, its folder as the host names it, and the game
//! window as the payload sees it; the host answers with [`HostMessage::Started`] once it records,
//! or [`HostMessage::Error`] and a close. The payload ends the take with a [`stop_line`] (or by
//! closing the connection), and the host answers [`HostMessage::Stopped`] once the video file is
//! finalized. A message of an unknown type is ignored. The relay's module documentation is the
//! other half of this description.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The protocol name the start line carries.
pub(crate) const PROTOCOL: &str = "jc3vr-capture";
/// The protocol version the start line carries.
pub(crate) const VERSION: u32 = 1;
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

/// A message from the host.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HostMessage {
    /// The host is recording.
    Started {
        /// The video file, as the host names it.
        file: String,
        /// The wall-clock time of the video's first frame, when the recorder reported it.
        video_start_unix_ms: Option<f64>,
        region: Region,
        /// The output the region is on, when the host knows it.
        output: Option<String>,
    },
    /// The host could not start, or stopped recording on its own; it closes the connection.
    Error { message: String },
    /// The host finalized the video after a stop line.
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

/// The start line for take `take`, recorded into `folder` (the take's folder as the host names it,
/// `None` when the payload cannot name it).
pub(crate) fn start_line(take: &str, folder: Option<&str>, window: &WindowHint) -> Vec<u8> {
    line(&Start {
        protocol: PROTOCOL,
        version: VERSION,
        kind: "start",
        take,
        folder,
        window,
    })
}

/// The stop line.
pub(crate) fn stop_line() -> Vec<u8> {
    line(&Stop { kind: "stop" })
}

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
