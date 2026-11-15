//! The injector's capture host: it stays running after the injection and encodes the eye views the
//! payload streams for each take.
//!
//! The host listens on loopback (port 47172 by default, the payload's default) and serves one take
//! at a time ([`take`]): the payload connects when a recording starts, sends the stream's format
//! ([`protocol`]), and streams `yuv4mpegpipe` frames that an ffmpeg beside the injector encodes into
//! `video.mkv` in the take's folder, with a hardware H.264 encoder where one works ([`encoder`]).
//! The host exits when the game does.
//!
//! When the port is taken, another capture host already serves the takes (on Linux, the relay
//! `scripts/capture_host.sh` started before the injection, which feeds the same stream to the host's
//! own ffmpeg), and the injector leaves the recording to it.

mod encoder;
mod protocol;
mod take;

use std::{
    io::ErrorKind,
    net::{Ipv4Addr, TcpListener},
    time::Duration,
};

use anyhow::Context as _;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT},
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
                WaitForSingleObject,
            },
        },
    },
    core::{s, w},
};

use crate::capture::encoder::EncoderCache;

/// The port the payload connects to by default.
pub(crate) const DEFAULT_PORT: u16 = 47172;

/// Serve takes on `port` until the game process `pid` exits. Returns at once, without error, when
/// another capture host holds the port.
pub(crate) fn serve(port: u16, pid: u32) -> anyhow::Result<()> {
    let game = GameProcess::open(pid)?;
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            tracing::info!(
                "capture host: port {port} is taken, so another capture host serves the takes \
                 (scripts/capture_host.sh on Linux); the injector will not record"
            );
            return Ok(());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("capture host: could not listen on {port}"));
        }
    };
    listener
        .set_nonblocking(true)
        .context("capture host: could not make the listener non-blocking")?;
    tracing::info!(
        "capture host: listening on 127.0.0.1:{port}; leave the injector running to record takes"
    );
    if !ffmpeg_sidecar::command::ffmpeg_is_installed() {
        tracing::warn!(
            "capture host: no ffmpeg beside the injector or on PATH; run scripts/fetch_ffmpeg.sh \
             (or scripts/fetch_ffmpeg.ps1 on Windows) before recording"
        );
    }
    if let Some(version) = wine_version() {
        tracing::warn!(
            "capture host: running under Wine {version}, where ffmpeg.exe reaches no hardware \
             encoder and encodes in software next to the game; for the GPU's encoder, start \
             scripts/capture_host.sh on the host before injecting"
        );
    }
    let encoders = EncoderCache::default();
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                tracing::info!("capture host: connection from {peer}");
                // An accepted socket inherits the listener's non-blocking mode on Windows.
                if let Err(error) = stream.set_nonblocking(false) {
                    tracing::error!("capture host: could not set up the connection: {error}");
                    continue;
                }
                take::run(stream, &encoders);
                tracing::info!("capture host: listening on 127.0.0.1:{port}");
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if !game.alive() {
                    tracing::info!("capture host: the game has exited");
                    return Ok(());
                }
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(error) => {
                tracing::warn!("capture host: accept failed: {error}");
                std::thread::sleep(ACCEPT_POLL);
            }
        }
    }
}

/// How often the idle host checks for a connection and for the game's exit.
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// A handle on the game process that can only tell whether it still runs.
struct GameProcess(HANDLE);

impl GameProcess {
    fn open(pid: u32) -> anyhow::Result<Self> {
        // SAFETY: a plain handle request; the handle is closed on drop.
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .with_context(|| format!("capture host: could not open the game process {pid}"))?;
        Ok(Self(handle))
    }

    fn alive(&self) -> bool {
        // SAFETY: a live handle opened with SYNCHRONIZE access.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
}

impl Drop for GameProcess {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Wine's version, when the injector runs under Wine.
fn wine_version() -> Option<String> {
    // SAFETY: ntdll is always loaded; `wine_get_version` takes nothing and returns a static C
    // string.
    unsafe {
        let ntdll = GetModuleHandleW(w!("ntdll.dll")).ok()?;
        let get_version = GetProcAddress(ntdll, s!("wine_get_version"))?;
        let get_version: extern "C" fn() -> *const std::ffi::c_char =
            std::mem::transmute(get_version);
        let version = get_version();
        (!version.is_null()).then(|| {
            std::ffi::CStr::from_ptr(version)
                .to_string_lossy()
                .into_owned()
        })
    }
}
