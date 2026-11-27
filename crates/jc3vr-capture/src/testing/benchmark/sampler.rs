//! A sampling profile of the draw task, for a benchmark run that asks for it.
//!
//! The draw task is one engine fragment that walks the render passes and records every draw of a
//! dispatch, and the frame-timing slot says only how long it took. This samples where that time
//! goes: a thread of its own wakes about once a millisecond, suspends whichever thread is running
//! the draw task at that moment, copies its instruction pointer and the top of its stack, and
//! resumes it. Each sample also carries what the draw path had published at that instant: the
//! render pass being drawn, the render-block type whose run it is in, and which eye of a per-eye
//! re-issue it is drawing. The harness attributes the instruction pointer to a module (the game,
//! the Direct3D runtime, the payload, Wine) and the return addresses to the game function that
//! called out (`tools/rendering/benchmark/samples.py`).
//!
//! Suspending a thread under Wine costs a few round trips to the server, so a sampled run is slower
//! than an unsampled one and its frame times are not comparable with the others'. While the target
//! is suspended the sampler only reads its context and copies its stack into a buffer allocated
//! beforehand: it takes no lock and allocates nothing a suspended thread could be holding. The
//! return addresses are filtered after the target is resumed.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use jc3vr_render::probes::state::{current_pass_id, per_eye_reissue_eye};
use parking_lot::Mutex;
use serde::Serialize;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Diagnostics::{
            Debug::{
                CONTEXT, CONTEXT_FLAGS, GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind,
                UNW_FLAG_NHANDLER,
            },
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE,
            },
        },
        Threading::{
            GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT,
            THREAD_SUSPEND_RESUME,
        },
    },
};

/// Mark the current thread as running the draw task for as long as the guard lives. Called at the
/// top of the draw task's detour; one relaxed load while no sampler runs.
#[must_use = "the thread is marked only while the guard is held"]
pub(crate) fn enter_draw_task() -> DrawTaskMark {
    if !ACTIVE.load(Ordering::Relaxed) {
        return DrawTaskMark(false);
    }
    // SAFETY: on x86-64 Windows `gs` holds the thread's TEB, whose second and third quadwords are
    // the base (highest address) and the limit (lowest committed address) of the thread's stack.
    let (stack_base, stack_limit): (u64, u64) = unsafe {
        let base: u64;
        let limit: u64;
        std::arch::asm!(
            "mov {}, gs:[0x08]",
            "mov {}, gs:[0x10]",
            out(reg) base,
            out(reg) limit,
            options(nostack, readonly, preserves_flags),
        );
        (base, limit)
    };
    *DRAW_THREAD.lock() = Some(DrawThread {
        // SAFETY: no preconditions.
        id: unsafe { GetCurrentThreadId() },
        stack_base,
        stack_limit,
    });
    DrawTaskMark(true)
}

/// The draw task's mark on its thread; see [`enter_draw_task`].
pub(crate) struct DrawTaskMark(bool);

impl Drop for DrawTaskMark {
    fn drop(&mut self) {
        if self.0 {
            *DRAW_THREAD.lock() = None;
            DRAW_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Publish the render-block type whose run the draw thread has entered, from the engine's type
/// switch.
#[cfg(feature = "profiler")]
pub(crate) fn note_block_type(block_type: *const RenderBlockTypeBase) {
    CURRENT_BLOCK_TYPE.store(block_type as usize, Ordering::Relaxed);
}

/// A running sampler; [`Sampler::finish`] stops it and returns what it collected.
pub(crate) struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Collected>>,
}

impl Sampler {
    /// Start sampling the draw task.
    pub(crate) fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        ACTIVE.store(true, Ordering::Relaxed);
        let thread = std::thread::Builder::new()
            .name("jc3vr-draw-sampler".to_owned())
            .spawn({
                let stop = stop.clone();
                move || sample_until(&stop)
            })
            .map_err(|error| tracing::warn!(%error, "benchmark: the draw sampler did not start"))
            .ok();
        Self { stop, thread }
    }

    /// Stop sampling and gather the samples with the module and render-block type tables they
    /// refer to.
    pub(crate) fn finish(mut self) -> Option<DrawTaskSamples> {
        self.stop.store(true, Ordering::Relaxed);
        ACTIVE.store(false, Ordering::Relaxed);
        let collected = self.thread.take()?.join().ok()?;
        Some(collected.into_report())
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        ACTIVE.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The samples of one run, as the acknowledgement carries them.
#[derive(Serialize)]
pub(crate) struct DrawTaskSamples {
    /// The interval the sampler aimed for, and the mean time a target spent suspended per sample.
    pub(crate) period_us: u64,
    pub(crate) mean_suspend_us: f64,
    /// Wake-ups that found no thread running the draw task.
    pub(crate) idle: u64,
    /// Samples whose target left the draw task while it was being sampled, which are dropped.
    pub(crate) torn: u64,
    pub(crate) modules: Vec<ModuleRange>,
    /// The render-block types the samples name, by the address the `block_type` column holds.
    pub(crate) block_types: Vec<(u64, String)>,
    pub(crate) columns: &'static [&'static str],
    pub(crate) rows: Vec<[u64; SAMPLE_COLUMNS.len()]>,
}

#[derive(Serialize, Clone)]
pub(crate) struct ModuleRange {
    pub(crate) name: String,
    pub(crate) base: u64,
    pub(crate) size: u64,
}

/// The columns of a sample row: the pass id (255 outside a pass), the per-eye re-issue marker (0
/// outside one, else the eye plus one), the render-block type's address, the instruction pointer,
/// and up to [`MAX_RETURNS`] return addresses into a loaded module, innermost first (0 where
/// absent).
pub(crate) const SAMPLE_COLUMNS: [&str; 4 + MAX_RETURNS] = [
    "pass",
    "reissue",
    "block_type",
    "rip",
    "ret0",
    "ret1",
    "ret2",
    "ret3",
    "ret4",
    "ret5",
    "ret6",
    "ret7",
    "ret8",
    "ret9",
    "ret10",
    "ret11",
    "ret12",
    "ret13",
    "ret14",
    "ret15",
];

/// Enough return addresses to reach past the Direct3D runtime's and the payload's own frames to
/// the game function that called them.
const MAX_RETURNS: usize = 16;
/// The most samples a run keeps: two minutes at the sampling rate.
const MAX_SAMPLES: usize = 150_000;
/// How much of the target's stack a sample copies (64 KiB), enough for the frames of the draw path
/// from the draw task's own entry down.
const STACK_COPY_WORDS: usize = 8192;
const PERIOD: Duration = Duration::from_millis(1);
/// `CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER`.
const CONTEXT_AMD64_CONTROL_INTEGER: u32 = 0x0010_0003;

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The thread running the draw task and its stack's bounds, kept together so the sampler never
/// pairs one thread with another's stack. Locked once per dispatch while sampling, and by the sampler
/// only while it is not suspending anything.
static DRAW_THREAD: Mutex<Option<DrawThread>> = Mutex::new(None);
/// Advanced each time a draw task ends, so a sample whose target left the task meanwhile is dropped.
static DRAW_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct DrawThread {
    id: u32,
    stack_base: u64,
    stack_limit: u64,
}
static CURRENT_BLOCK_TYPE: AtomicUsize = AtomicUsize::new(0);

/// The sampler's buffers, allocated before sampling so nothing is allocated while a target is
/// suspended.
#[repr(C, align(16))]
struct Scratch {
    context: CONTEXT,
    stack: [u64; STACK_COPY_WORDS],
}

struct Collected {
    modules: Vec<ModuleRange>,
    rows: Vec<[u64; SAMPLE_COLUMNS.len()]>,
    idle: u64,
    torn: u64,
    suspend_ns: u64,
}

impl Collected {
    fn into_report(self) -> DrawTaskSamples {
        let mut types: Vec<u64> = self.rows.iter().map(|row| row[2]).collect();
        types.sort_unstable();
        types.dedup();
        let block_types = types
            .into_iter()
            .filter(|&address| address != 0)
            .map(|address| {
                // SAFETY: the engine's render-block types are registered once at startup and live
                // for the process; the address was published by its own type switch.
                let name = unsafe {
                    (address as *const RenderBlockTypeBase)
                        .as_ref()
                        .and_then(|ty| ty.get_type_name_str())
                };
                (address, name.unwrap_or("<unnamed>").to_owned())
            })
            .collect();
        let sampled = self.rows.len() as u64;
        DrawTaskSamples {
            period_us: PERIOD.as_micros() as u64,
            mean_suspend_us: if sampled == 0 {
                0.0
            } else {
                self.suspend_ns as f64 / sampled as f64 / 1000.0
            },
            idle: self.idle,
            torn: self.torn,
            modules: self.modules,
            block_types,
            columns: &SAMPLE_COLUMNS,
            rows: self.rows,
        }
    }
}

fn sample_until(stop: &AtomicBool) -> Collected {
    let modules = loaded_modules();
    let mut collected = Collected {
        modules,
        rows: Vec::with_capacity(MAX_SAMPLES),
        idle: 0,
        torn: 0,
        suspend_ns: 0,
    };
    // SAFETY: `CONTEXT` and the stack array are plain data, valid when zeroed.
    let mut scratch: Box<Scratch> = Box::new(unsafe { std::mem::zeroed() });
    let mut handles = ThreadHandles::default();
    while !stop.load(Ordering::Relaxed) && collected.rows.len() < MAX_SAMPLES {
        std::thread::sleep(PERIOD);
        let sequence = DRAW_SEQUENCE.load(Ordering::Acquire);
        let Some(target) = *DRAW_THREAD.lock() else {
            collected.idle += 1;
            continue;
        };
        let Some(handle) = handles.get(target.id) else {
            continue;
        };
        let started = Instant::now();
        // SAFETY: `handle` is a thread handle opened for suspension and context reads, and the stack
        // bounds are that thread's own; the copy reads `[rsp, stack_base)` of the suspended thread
        // only when `rsp` lies within its committed stack.
        let Some(sample) = (unsafe { sample_thread(handle, target, &mut scratch) }) else {
            continue;
        };
        collected.suspend_ns += started.elapsed().as_nanos() as u64;
        if DRAW_SEQUENCE.load(Ordering::Acquire) != sequence
            || DRAW_THREAD.lock().is_none_or(|now| now.id != target.id)
        {
            collected.torn += 1;
            continue;
        }
        let mut row = [0u64; SAMPLE_COLUMNS.len()];
        row[0] = sample.pass;
        row[1] = sample.reissue;
        row[2] = sample.block_type;
        row[3] = sample.rip;
        let returns = unwind(
            &scratch.context,
            &scratch.stack[..sample.stack_words],
            &collected.modules,
        );
        row[4..4 + returns.len()].copy_from_slice(&returns);
        collected.rows.push(row);
    }
    collected
}

struct RawSample {
    pass: u64,
    reissue: u64,
    block_type: u64,
    rip: u64,
    stack_words: usize,
}

/// Suspend `handle`'s thread, read its context, the draw path's published state, and the top of its
/// stack, and resume it. Nothing between the suspension and the resumption takes a lock or
/// allocates.
unsafe fn sample_thread(
    handle: HANDLE,
    target: DrawThread,
    scratch: &mut Scratch,
) -> Option<RawSample> {
    unsafe {
        if SuspendThread(handle) == u32::MAX {
            return None;
        }
        scratch.context.ContextFlags = CONTEXT_FLAGS(CONTEXT_AMD64_CONTROL_INTEGER);
        let sample = if GetThreadContext(handle, &mut scratch.context).is_ok() {
            let rsp = scratch.context.Rsp;
            let on_stack = (target.stack_limit..target.stack_base).contains(&rsp);
            let stack_words = if on_stack && rsp.is_multiple_of(8) {
                (((target.stack_base - rsp) / 8) as usize).min(STACK_COPY_WORDS)
            } else {
                0
            };
            std::ptr::copy_nonoverlapping(
                rsp as *const u64,
                scratch.stack.as_mut_ptr(),
                stack_words,
            );
            Some(RawSample {
                pass: u64::from(current_pass_id()),
                reissue: per_eye_reissue_eye().map_or(0, |eye| eye as u64 + 1),
                block_type: CURRENT_BLOCK_TYPE.load(Ordering::Relaxed) as u64,
                rip: scratch.context.Rip,
                stack_words,
            })
        } else {
            None
        };
        ResumeThread(handle);
        sample
    }
}

/// The return addresses of the sampled thread's call chain, innermost first, by unwinding its
/// context through the modules' unwind tables over the stack copy, after the thread is resumed.
///
/// The copy holds the real stack from the sampled `rsp` up, so every register that pointed into
/// that range is moved to the same offset in the copy before each step; the return addresses read
/// from it are the real ones. A frame without unwind data (a detour trampoline, a leaf) is taken to
/// keep its return address at `rsp`. Where the chain cannot be followed further (it leaves the copy,
/// or lands outside every module), the rest is filled by [`return_addresses`]' scan from there.
fn unwind(context: &CONTEXT, stack: &[u64], modules: &[ModuleRange]) -> Vec<u64> {
    let mut found = Vec::with_capacity(MAX_RETURNS);
    let real_base = context.Rsp;
    let copy_base = stack.as_ptr() as u64;
    let length = stack.len() as u64 * 8;
    let to_copy = |value: u64| {
        if (real_base..real_base + length).contains(&value) {
            value - real_base + copy_base
        } else {
            value
        }
    };
    let in_module = |address: u64| {
        modules
            .iter()
            .any(|m| (m.base..m.base + m.size).contains(&address))
    };
    let mut frame = *context;
    frame.Rsp = copy_base;
    for register in [
        &mut frame.Rbp,
        &mut frame.Rbx,
        &mut frame.Rsi,
        &mut frame.Rdi,
        &mut frame.R12,
        &mut frame.R13,
        &mut frame.R14,
        &mut frame.R15,
    ] {
        *register = to_copy(*register);
    }
    while found.len() < MAX_RETURNS && length >= 8 {
        let pc = frame.Rip;
        if !in_module(pc) {
            break;
        }
        let mut image_base = 0u64;
        // SAFETY: a lookup in the process's own loaded modules' function tables; the target thread
        // is running again, so the loader lock this may take is not held by a suspended thread.
        let entry = unsafe { RtlLookupFunctionEntry(pc, &mut image_base, None) };
        if entry.is_null() {
            if !(copy_base..copy_base + length - 7).contains(&frame.Rsp) {
                break;
            }
            // SAFETY: `Rsp` lies within the stack copy, eight bytes below its end.
            frame.Rip = unsafe { *(frame.Rsp as *const u64) };
            frame.Rsp += 8;
        } else {
            let mut handler_data = std::ptr::null_mut();
            let mut establisher = 0u64;
            // SAFETY: the context's stack pointer and frame registers point into the stack copy,
            // which holds the sampled frames; unwinding one frame reads only within it.
            unsafe {
                RtlVirtualUnwind(
                    UNW_FLAG_NHANDLER,
                    image_base,
                    pc,
                    entry,
                    &mut frame,
                    &mut handler_data,
                    &mut establisher,
                    None,
                );
            }
            frame.Rbp = to_copy(frame.Rbp);
        }
        if !(copy_base..=copy_base + length).contains(&frame.Rsp) || frame.Rip == 0 {
            break;
        }
        found.push(frame.Rip);
    }
    if found.len() < 2 {
        found.clear();
        found.extend(return_addresses(stack, modules));
    }
    found.truncate(MAX_RETURNS);
    found
}

/// The stack words that are return addresses into a loaded module's image, innermost first: each
/// must lie in a module and follow a call instruction. Stale frames above the live chain can pass
/// both tests, so the deeper entries are probable rather than certain.
fn return_addresses(stack: &[u64], modules: &[ModuleRange]) -> Vec<u64> {
    let mut found = Vec::with_capacity(MAX_RETURNS);
    for &value in stack {
        if found.len() == MAX_RETURNS {
            break;
        }
        if value < 8
            || !modules
                .iter()
                .any(|m| (m.base + 8..m.base + m.size).contains(&value))
        {
            continue;
        }
        // SAFETY: `value` lies at least eight bytes into a loaded module's image, which stays mapped
        // for the process (the module list is the process's own); a module's headers and sections
        // are readable.
        if unsafe { follows_call(value) } {
            found.push(value);
        }
    }
    found
}

/// Whether the bytes before `address` end in one of the encodings of `call`: a relative call, or an
/// indirect call through a register or memory operand, with or without a REX prefix.
unsafe fn follows_call(address: u64) -> bool {
    let byte = |back: u64| unsafe { std::ptr::read_volatile((address - back) as *const u8) };
    if byte(5) == 0xE8 {
        return true;
    }
    // `FF /2`: the ModRM byte's reg field is 2. The operand lengths that end at `address`: a
    // register or `[reg]` (2 bytes), `[reg+disp8]` or a SIB (3), a SIB with disp8 (4), `[rip+disp32]`
    // or `[reg+disp32]` (6), and a SIB with disp32 (7).
    for length in [2u64, 3, 4, 6, 7] {
        if byte(length) == 0xFF && (byte(length - 1) >> 3) & 7 == 2 {
            return true;
        }
    }
    false
}

/// Every module loaded in the process, from a module snapshot.
fn loaded_modules() -> Vec<ModuleRange> {
    let mut modules = Vec::new();
    // SAFETY: a module snapshot of the current process; the entry is sized as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, 0) else {
            return modules;
        };
        let mut entry = MODULEENTRY32W {
            dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Module32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let length = entry
                .szModule
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szModule.len());
            modules.push(ModuleRange {
                name: String::from_utf16_lossy(&entry.szModule[..length]),
                base: entry.modBaseAddr as u64,
                size: u64::from(entry.modBaseSize),
            });
            more = Module32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    modules
}

/// The sampler's open thread handles, by thread id, closed when it finishes.
#[derive(Default)]
struct ThreadHandles(Vec<(u32, HANDLE)>);

impl ThreadHandles {
    fn get(&mut self, tid: u32) -> Option<HANDLE> {
        if let Some(&(_, handle)) = self.0.iter().find(|(id, _)| *id == tid) {
            return Some(handle);
        }
        // SAFETY: opening a thread of this process by id.
        let handle =
            unsafe { OpenThread(THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME, false, tid) }.ok()?;
        self.0.push((tid, handle));
        Some(handle)
    }
}

impl Drop for ThreadHandles {
    fn drop(&mut self) {
        for &(_, handle) in &self.0 {
            // SAFETY: each handle was opened by `get` and is closed once.
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}
