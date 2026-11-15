//! A take's audio: WASAPI loopback of the default output device, fed to ffmpeg as raw samples.
//!
//! Loopback records what the default output plays, which is the game's sound and anything else
//! playing. It opens on a thread of its own, since its COM objects belong to the thread that made
//! them, and reports the device and its mix format before the take starts ffmpeg, which reads the
//! samples as a second input from a loopback TCP connection the thread serves
//! ([`AudioInput::ffmpeg_args`]). The samples are kept on the wall clock ([`timeline`]), so the
//! audio stays in step with the video however the device delivers them.
//!
//! Wine implements loopback for its PulseAudio driver from Wine 9.12, where it records the default
//! sink's monitor; under an older Wine, or another audio driver, the device does not open. Either
//! way the take carries on without audio and says why. A device lost during the take (unplugged,
//! or the default output switched away) is reported and the rest of the take is silent.
//! `JC3VR_CAPTURE_AUDIO=none` records no audio.

mod timeline;

use std::{
    io::Write,
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};

use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR,
            AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient,
            IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVE_FORMAT_PCM,
            WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
        },
        System::{
            Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize, STGM_READ,
                StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        },
    },
    core::GUID,
};

use crate::capture::audio::timeline::Timeline;

/// The environment variable that turns the audio off with `none`.
pub(crate) const AUDIO_VARIABLE: &str = "JC3VR_CAPTURE_AUDIO";

/// A take's running loopback capture.
pub(crate) struct AudioInput {
    description: String,
    ffmpeg_args: Vec<String>,
    states: Receiver<String>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioInput {
    /// Open the default output for loopback. The capture starts once ffmpeg connects. Returns why
    /// there is no audio when it cannot.
    pub(crate) fn start() -> Result<Self, String> {
        if std::env::var(AUDIO_VARIABLE).is_ok_and(|value| value.trim() == "none") {
            return Err(format!("{AUDIO_VARIABLE} is none"));
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("audio: could not listen for ffmpeg: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("audio: could not read the listener's port: {error}"))?
            .port();
        let (opened_sender, opened) = mpsc::sync_channel(1);
        let (state_sender, states) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("capture-audio".to_owned())
            .spawn(move || loopback_thread(listener, opened_sender, state_sender, thread_stop))
            .map_err(|error| format!("audio: could not start the capture thread: {error}"))?;
        match opened.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(device)) => Ok(Self {
                description: format!(
                    "loopback of {}, {} Hz, {} channels",
                    device.name, device.format.rate, device.format.channels
                ),
                ffmpeg_args: device.format.ffmpeg_input(port),
                states,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(reason)) => {
                let _ = thread.join();
                Err(reason)
            }
            Err(_) => {
                // The thread is left to notice the flag; a device that hangs on open cannot be
                // waited for.
                stop.store(true, Ordering::Release);
                Err(format!(
                    "audio: the default output did not open within {} s",
                    OPEN_TIMEOUT.as_secs()
                ))
            }
        }
    }

    /// What the audio records from, for the take's `started` answer.
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    /// ffmpeg's arguments for the audio input.
    pub(crate) fn ffmpeg_args(&self) -> &[String] {
        &self.ffmpeg_args
    }

    /// The latest change in the capture's state since the last call, if any.
    pub(crate) fn changed_state(&self) -> Option<String> {
        self.states.try_iter().last()
    }

    /// End the capture, closing ffmpeg's audio input.
    pub(crate) fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("capture host: the audio thread panicked");
        }
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// How long the device has to open.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// The shared-mode buffer: room for the capture thread to fall a fifth of a second behind.
const BUFFER_100NS: i64 = 2_000_000;
/// How often the capture thread drains the device.
const CAPTURE_POLL: Duration = Duration::from_millis(10);
/// How long a write to ffmpeg may block before the audio counts as stalled.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// `WAVE_FORMAT_IEEE_FLOAT` and `WAVE_FORMAT_EXTENSIBLE`, the Windows SDK's format tags for float
/// samples and for a format whose sample type is in its sub-format GUID.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT` and `KSDATAFORMAT_SUBTYPE_PCM`, the sub-format GUIDs.
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

/// A sample type ffmpeg reads raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleFormat {
    F32,
    S16,
    S24,
    S32,
}

impl SampleFormat {
    fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::F32 => "f32le",
            Self::S16 => "s16le",
            Self::S24 => "s24le",
            Self::S32 => "s32le",
        }
    }
}

/// The device's mix format.
#[derive(Clone, Copy, Debug)]
struct Format {
    rate: u32,
    channels: u16,
    sample: SampleFormat,
    /// Bytes per frame, all channels.
    block_align: usize,
}

impl Format {
    fn ffmpeg_input(&self, port: u16) -> Vec<String> {
        vec![
            "-thread_queue_size".into(),
            "1024".into(),
            "-f".into(),
            self.sample.ffmpeg_name().into(),
            "-ar".into(),
            self.rate.to_string(),
            "-ac".into(),
            self.channels.to_string(),
            "-i".into(),
            format!("tcp://127.0.0.1:{port}"),
        ]
    }
}

/// The sample type for a format tag, sample size, and (for an extensible format) sub-format.
fn sample_format(tag: u16, bits: u16, sub_format: Option<GUID>) -> Result<SampleFormat, String> {
    let float = match (tag, sub_format) {
        (WAVE_FORMAT_IEEE_FLOAT, _) => true,
        (WAVE_FORMAT_EXTENSIBLE, Some(sub)) if sub == SUBTYPE_IEEE_FLOAT => true,
        (WAVE_FORMAT_EXTENSIBLE, Some(sub)) if sub == SUBTYPE_PCM => false,
        (tag, _) if u32::from(tag) == WAVE_FORMAT_PCM => false,
        (tag, sub) => {
            return Err(format!(
                "audio: the mix format (tag {tag:#x}, sub-format {sub:?}) is not PCM or float"
            ));
        }
    };
    match (float, bits) {
        (true, 32) => Ok(SampleFormat::F32),
        (false, 16) => Ok(SampleFormat::S16),
        (false, 24) => Ok(SampleFormat::S24),
        (false, 32) => Ok(SampleFormat::S32),
        _ => Err(format!(
            "audio: {bits}-bit {} samples are not supported",
            if float { "float" } else { "integer" }
        )),
    }
}

/// What the thread opened.
struct Opened {
    name: String,
    format: Format,
}

fn loopback_thread(
    listener: TcpListener,
    opened: SyncSender<Result<Opened, String>>,
    states: Sender<String>,
    stop: Arc<AtomicBool>,
) {
    // SAFETY: COM is initialized for this thread alone and uninitialized when it ends, after every
    // COM object it made is released.
    if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        let _ = opened.send(Err(format!("audio: could not initialize COM: {error}")));
        return;
    }
    {
        // SAFETY: COM is initialized on this thread.
        let loopback = match unsafe { Loopback::open() } {
            Ok(loopback) => loopback,
            Err(reason) => {
                let _ = opened.send(Err(reason));
                // SAFETY: balances the initialization above.
                unsafe { CoUninitialize() };
                return;
            }
        };
        let _ = opened.send(Ok(Opened {
            name: loopback.name.clone(),
            format: loopback.format,
        }));
        if let Some(stream) = accept(&listener, &stop) {
            loopback.run(stream, &states, &stop);
        }
    }
    // SAFETY: balances the initialization above; the loopback's COM objects are released.
    unsafe { CoUninitialize() };
}

/// Wait for ffmpeg to connect, until the take stops.
fn accept(listener: &TcpListener, stop: &AtomicBool) -> Option<TcpStream> {
    if let Err(error) = listener.set_nonblocking(true) {
        tracing::error!("capture host: audio: {error}");
        return None;
    }
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let ready = stream
                    .set_nonblocking(false)
                    .and_then(|()| stream.set_write_timeout(Some(WRITE_TIMEOUT)));
                return match ready {
                    Ok(()) => Some(stream),
                    Err(error) => {
                        tracing::error!("capture host: audio: {error}");
                        None
                    }
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                tracing::error!("capture host: audio: ffmpeg could not connect: {error}");
                return None;
            }
        }
    }
    None
}

/// The default output's loopback capture, opened but not started.
struct Loopback {
    name: String,
    format: Format,
    client: IAudioClient,
    capture: IAudioCaptureClient,
}

impl Loopback {
    /// # Safety
    /// COM must be initialized on the calling thread.
    unsafe fn open() -> Result<Self, String> {
        let fail = |step: &str, error: windows::core::Error| {
            format!("audio: could not {step} for loopback: {error}")
        };
        // SAFETY: COM calls on an initialized thread, with the mix format freed after use.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| fail("enumerate the audio devices", e))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| fail("find the default output", e))?;
            let name = friendly_name(&device).unwrap_or_else(|| "the default output".to_owned());
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| fail("activate the default output", e))?;
            let mix = client
                .GetMixFormat()
                .map_err(|e| fail("read the default output's mix format", e))?;
            let format = read_format(mix);
            let initialized = format.as_ref().ok().map(|_| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    BUFFER_100NS,
                    0,
                    mix,
                    None,
                )
            });
            CoTaskMemFree(Some(mix.cast()));
            let format = format?;
            if let Some(Err(error)) = initialized {
                return Err(format!(
                    "audio: {name} does not offer loopback (Wine has it for PulseAudio from \
                     9.12): {error}"
                ));
            }
            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| fail("get the capture client", e))?;
            tracing::info!(
                "capture host: audio: loopback of {name}, {} Hz, {} channels, {:?}",
                format.rate,
                format.channels,
                format.sample
            );
            Ok(Self {
                name,
                format,
                client,
                capture,
            })
        }
    }

    /// Capture into `stream` until the take stops or ffmpeg stops reading.
    fn run(&self, mut stream: TcpStream, states: &Sender<String>, stop: &AtomicBool) {
        let rate = self.format.rate;
        let mut timeline = Timeline::new(rate);
        // SAFETY: an initialized client on this thread.
        let mut lost = match unsafe { self.client.Start() } {
            Ok(()) => false,
            Err(error) => {
                let state = format!("silent: the loopback did not start: {error}");
                tracing::warn!("capture host: audio: {state}");
                let _ = states.send(state);
                true
            }
        };
        let origin = clock_100ns();
        while !stop.load(Ordering::Acquire) {
            if !lost && let Err(failure) = self.drain(&mut stream, &mut timeline, origin) {
                match failure {
                    Failure::Write(error) => {
                        tracing::warn!("capture host: audio: ffmpeg stopped taking it: {error}");
                        break;
                    }
                    Failure::Device(error) => {
                        let state = format!("silent: the output device was lost: {error}");
                        tracing::warn!("capture host: audio: {state}");
                        let _ = states.send(state);
                        lost = true;
                    }
                }
            }
            let now = frames_since(origin, clock_100ns(), rate);
            let silence = timeline.fill_to(now);
            if let Err(error) = write_silence(&mut stream, silence, self.format.block_align) {
                tracing::warn!("capture host: audio: ffmpeg stopped taking it: {error}");
                break;
            }
            std::thread::sleep(CAPTURE_POLL);
        }
        // SAFETY: an initialized client on this thread.
        let _ = unsafe { self.client.Stop() };
    }

    /// Write every packet the device holds.
    fn drain(
        &self,
        stream: &mut TcpStream,
        timeline: &mut Timeline,
        origin: i64,
    ) -> Result<(), Failure> {
        let block = self.format.block_align;
        // SAFETY: the capture client's buffer is read between GetBuffer and ReleaseBuffer, for the
        // frame count GetBuffer reports.
        unsafe {
            while self.capture.GetNextPacketSize().map_err(Failure::Device)? > 0 {
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                let mut position = 0u64;
                self.capture
                    .GetBuffer(
                        &mut data,
                        &mut frames,
                        &mut flags,
                        None,
                        Some(&mut position),
                    )
                    .map_err(Failure::Device)?;
                let timestamped =
                    flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0 && position != 0;
                let start =
                    timestamped.then(|| frames_since(origin, position as i64, self.format.rate));
                let placement = timeline.place(start, u64::from(frames));
                let mut result = write_silence(stream, placement.silence, block);
                let skip = placement.skip as usize * block;
                let len = frames as usize * block;
                if result.is_ok() && skip < len {
                    result = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        write_silence(stream, (len - skip) as u64 / block as u64, block)
                    } else {
                        stream.write_all(&std::slice::from_raw_parts(data, len)[skip..])
                    };
                }
                self.capture
                    .ReleaseBuffer(frames)
                    .map_err(Failure::Device)?;
                result.map_err(Failure::Write)?;
            }
        }
        Ok(())
    }
}

enum Failure {
    Write(std::io::Error),
    Device(windows::core::Error),
}

/// Read the mix format WASAPI allocated.
///
/// # Safety
/// `format` must point at a valid `WAVEFORMATEX`, followed by the rest of a
/// `WAVEFORMATEXTENSIBLE` when its tag says so.
unsafe fn read_format(format: *const WAVEFORMATEX) -> Result<Format, String> {
    // SAFETY: per the contract; the structures are packed, so they are read unaligned.
    let wave = unsafe { std::ptr::read_unaligned(format) };
    let sub_format = (wave.wFormatTag == WAVE_FORMAT_EXTENSIBLE).then(|| {
        // SAFETY: an extensible format's tag guarantees the longer structure.
        let extensible = unsafe { std::ptr::read_unaligned(format.cast::<WAVEFORMATEXTENSIBLE>()) };
        extensible.SubFormat
    });
    let sample = sample_format(wave.wFormatTag, wave.wBitsPerSample, sub_format)?;
    Ok(Format {
        rate: wave.nSamplesPerSec,
        channels: wave.nChannels,
        sample,
        block_align: usize::from(wave.nBlockAlign),
    })
}

/// The device's name as Windows shows it.
///
/// # Safety
/// COM must be initialized on the calling thread.
unsafe fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: COM calls on an initialized thread; the property value and its string are freed.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let text = PropVariantToStringAlloc(&value).ok();
        let _ = PropVariantClear(&mut value);
        let text = text?;
        let name = text.to_string().ok();
        CoTaskMemFree(Some(text.0.cast()));
        name
    }
}

/// The performance counter in 100-nanosecond units, the unit WASAPI timestamps packets in.
fn clock_100ns() -> i64 {
    let mut count = 0i64;
    let mut frequency = 0i64;
    // SAFETY: plain out-parameter reads; both succeed on every supported Windows.
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    if frequency <= 0 {
        return 0;
    }
    (i128::from(count) * 10_000_000 / i128::from(frequency)) as i64
}

/// Frames at `rate` from `origin` to `time`, both in 100-nanosecond units; zero before `origin`.
fn frames_since(origin: i64, time: i64, rate: u32) -> u64 {
    let elapsed = time.saturating_sub(origin).max(0);
    (i128::from(elapsed) * i128::from(rate) / 10_000_000) as u64
}

/// Write `frames` silent frames of `block` bytes.
fn write_silence(stream: &mut impl Write, frames: u64, block: usize) -> std::io::Result<()> {
    const ZEROS: [u8; 16 * 1024] = [0; 16 * 1024];
    let mut remaining = frames as usize * block;
    while remaining > 0 {
        let chunk = remaining.min(ZEROS.len());
        stream.write_all(&ZEROS[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn mix_formats_map_to_ffmpegs_raw_sample_types() {
        let extensible = WAVE_FORMAT_EXTENSIBLE;
        assert_eq!(
            sample_format(extensible, 32, Some(SUBTYPE_IEEE_FLOAT)),
            Ok(SampleFormat::F32)
        );
        assert_eq!(
            sample_format(extensible, 24, Some(SUBTYPE_PCM)),
            Ok(SampleFormat::S24)
        );
        assert_eq!(sample_format(3, 32, None), Ok(SampleFormat::F32));
        assert_eq!(sample_format(1, 16, None), Ok(SampleFormat::S16));
        assert!(sample_format(1, 8, None).is_err());
        assert!(sample_format(3, 64, None).is_err());
        assert!(sample_format(0x55, 16, None).is_err());
    }

    #[test]
    fn ffmpeg_reads_the_samples_from_the_loopback_port() {
        let format = Format {
            rate: 48_000,
            channels: 2,
            sample: SampleFormat::F32,
            block_align: 8,
        };
        assert_eq!(
            format.ffmpeg_input(50_000),
            [
                "-thread_queue_size",
                "1024",
                "-f",
                "f32le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-i",
                "tcp://127.0.0.1:50000"
            ]
        );
    }

    #[test]
    fn frame_counts_follow_the_hundred_nanosecond_clock() {
        assert_eq!(frames_since(1_000, 1_000 + 10_000_000, 48_000), 48_000);
        assert_eq!(frames_since(5_000, 1_000, 48_000), 0);
        let mut out = Vec::new();
        write_silence(&mut out, 5_000, 8).unwrap();
        assert_eq!(out.len(), 40_000);
        assert!(out.iter().all(|&b| b == 0));
    }
}
