//! A capture-gated census of the draw calls one frame issues.
//!
//! For every draw call the engine makes inside a render pass on the diagnostic frame, it records the
//! pass, the render-block type that issued it, the bound vertex and pixel shaders, the device-context
//! entry point, and how the stereo draw path submitted it: promoted to an instance-doubled draw,
//! re-issued once per eye, forwarded once, or not submitted at all. Calls that agree on all of that
//! are folded into one entry with a count, so a frame's census stays a few hundred rows.
//!
//! The record names what draws in a pass range and how each family is treated under the collapse,
//! which is the starting point for chasing a defect to its drawer and for the draw-call census. It
//! observes only: nothing here changes a draw. The draw-path detours it hangs off are the stereo
//! draw path's, so a mode that does not install them records no calls.

use std::{
    collections::BTreeMap,
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use jc3gi::graphics_engine::render_engine::{RenderBlockTypeBase, RenderPassId};
use jc3vr_core::util::bump;
use parking_lot::Mutex;
use serde::Serialize;
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11PixelShader, ID3D11VertexShader},
    core::Interface,
};

use crate::{
    hooks::graphics_engine::reconstruction::split_run_eye,
    stereo::{
        draw_index,
        single_pass::{
            RawDrawKind, bound_vertex_shader_patched, current_pass_id, per_eye_reissue_eye,
            shader_identity, shader_identity::IdentityResult, vertex_identity_lookup,
        },
    },
};

/// The census of one frame, as the capture metadata carries it.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DrawCensus {
    pub(crate) frame: i64,
    pub(crate) calls: u64,
    pub(crate) submissions: u64,
    pub(crate) entries: Vec<CensusEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// One group of draw calls that agree on everything the census distinguishes.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct CensusEntry {
    /// The engine dispatch the call belongs to: one under the collapse, one per eye otherwise.
    pub(crate) dispatch: usize,
    pub(crate) pass: u8,
    pub(crate) pass_name: String,
    /// The render-block type running when the call was made, when the type switch is observed.
    pub(crate) block: Option<&'static str>,
    pub(crate) vertex_shader: ShaderName,
    /// Whether the bound vertex shader is one the single-pass rewrite patched.
    pub(crate) vertex_patched: bool,
    pub(crate) pixel_shader: ShaderName,
    pub(crate) entry: &'static str,
    /// The eye of the per-eye render-block re-issue the call was made inside, if any.
    pub(crate) reissue_eye: Option<usize>,
    /// The eye of the split fullscreen pass run the call was made inside, if any. A split pass runs
    /// its whole draw sequence once per eye, so the second run's calls are the collapse's own.
    pub(crate) split_eye: Option<usize>,
    pub(crate) treatment: Treatment,
    pub(crate) calls: u32,
    pub(crate) submissions: u32,
}

/// What the shader registries know about a bound shader.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ShaderName {
    Unbound,
    /// The program name the engine created the shader under.
    Named {
        name: String,
    },
    /// The SHA-256 of the engine's own bytecode, for a shader created without a name.
    Pristine {
        sha256: String,
    },
    /// The registry was not armed or had no record of the shader.
    Unknown {
        reason: &'static str,
    },
}

/// How the draw path submitted a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Treatment {
    /// Not submitted: a treatment consumed the call without drawing it.
    Suppressed,
    /// Submitted once, unchanged.
    Once,
    /// Promoted to one draw carrying both eyes as instances.
    InstanceDoubled,
    /// Submitted once per eye.
    PerEye,
    /// Submitted more than twice.
    Repeated,
}

impl Treatment {
    fn classify(submissions: u32, promoted: bool) -> Self {
        match (submissions, promoted) {
            (_, true) => Self::InstanceDoubled,
            (0, false) => Self::Suppressed,
            (1, false) => Self::Once,
            (2, false) => Self::PerEye,
            _ => Self::Repeated,
        }
    }
}

/// Start or stop recording at a render pass's draw, for the frame the capture chose. Recording is
/// limited to the passes the request covers by the caller asking at each pass.
pub(crate) fn begin_pass() {
    let frame = crate::screenshot::harness::draw_census_frame();
    let mut recorder = RECORDER.lock();
    recorder.close_open();
    match frame {
        Some(frame) => {
            if recorder.frame != Some(frame) {
                recorder.reset(frame);
            }
            RECORDING.store(true, Ordering::Release);
        }
        None => RECORDING.store(false, Ordering::Release),
    }
}

/// Close the pass's last call, so a submission after the pass cannot be attributed to it.
pub(crate) fn end_pass() {
    if RECORDING.swap(false, Ordering::AcqRel) {
        RECORDER.lock().close_open();
    }
}

/// Note the render-block type the pass switches to, for the calls that follow.
pub(crate) fn type_switch(next: Option<*mut RenderBlockTypeBase>) {
    if !RECORDING.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: the engine passes either null or a live render-block type whose type name is a static
    // string in the module image.
    let name = next.and_then(|p| unsafe { p.as_ref()?.get_type_name_str() });
    RECORDER.lock().block = name;
}

/// Record a draw call arriving at a device-context entry point.
///
/// # Safety
///
/// `context` must be the live immediate context the detoured entry was called on.
pub(crate) unsafe fn on_call(kind: RawDrawKind, context: *mut c_void) {
    if !RECORDING.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: per this function's contract, the pointer is the live COM interface pointer; the
    // borrowed reference ends with this call and takes no reference count.
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        return;
    };
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut ps: Option<ID3D11PixelShader> = None;
    // SAFETY: `ctx` is the live immediate context; the getters only read bound state.
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.PSGetShader(&mut ps, None, None);
    }
    let key = CallKey {
        dispatch: draw_index(),
        pass: current_pass_id(),
        block: None,
        vertex_shader: vertex_name(vs.as_ref()),
        vertex_patched: bound_vertex_shader_patched(),
        pixel_shader: pixel_name(ps.as_ref()),
        entry: kind.name(),
        reissue_eye: per_eye_reissue_eye(),
        split_eye: split_run_eye(),
    };
    let mut recorder = RECORDER.lock();
    recorder.close_open();
    let key = CallKey {
        block: recorder.block,
        ..key
    };
    recorder.open = Some(OpenCall {
        key,
        submissions: 0,
        promoted: false,
    });
    recorder.calls += 1;
}

/// Record one submission the draw path made for the open call. `promoted` marks the instance-doubled
/// promotion of a single-instance draw.
pub(crate) fn on_submission(promoted: bool) {
    bump(&SUBMISSIONS);
    if !RECORDING.load(Ordering::Acquire) {
        return;
    }
    let mut recorder = RECORDER.lock();
    recorder.submissions += 1;
    if let Some(open) = recorder.open.as_mut() {
        open.submissions += 1;
        open.promoted |= promoted;
    }
}

/// Every submission the stereo draw path has made since the payload loaded, never reset, for
/// samplers that difference two readings (the benchmark, `crate::testing::benchmark`). Counted
/// whether or not a census records, and only while the draw-path detours are installed.
pub(crate) fn submission_total() -> u64 {
    SUBMISSIONS.load(Ordering::Relaxed)
}

static SUBMISSIONS: AtomicU64 = AtomicU64::new(0);

/// Take the census recorded for `frame`, or a census carrying the reason there is none.
pub(crate) fn take(frame: i64) -> DrawCensus {
    RECORDING.store(false, Ordering::Release);
    let mut recorder = RECORDER.lock();
    recorder.close_open();
    let recorded = recorder.frame == Some(frame);
    let census = DrawCensus {
        frame,
        calls: if recorded { recorder.calls } else { 0 },
        submissions: if recorded { recorder.submissions } else { 0 },
        entries: if recorded {
            recorder
                .tallies
                .iter()
                .map(|(key, tally)| key.entry(tally))
                .collect()
        } else {
            Vec::new()
        },
        error: if !recorded {
            Some("draw census: the requested frame was not recorded".into())
        } else if recorder.overflow {
            Some("draw census: entry limit exceeded".into())
        } else if recorder.tallies.is_empty() {
            Some("draw census: no draw calls observed".into())
        } else {
            None
        },
    };
    recorder.reset_empty();
    census
}

const MAX_ENTRIES: usize = 8192;

static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDER: Mutex<Recorder> = Mutex::new(Recorder {
    frame: None,
    block: None,
    open: None,
    tallies: BTreeMap::new(),
    calls: 0,
    submissions: 0,
    overflow: false,
});

struct Recorder {
    frame: Option<i64>,
    block: Option<&'static str>,
    open: Option<OpenCall>,
    tallies: BTreeMap<TallyKey, Tally>,
    calls: u64,
    submissions: u64,
    overflow: bool,
}

impl Recorder {
    fn reset(&mut self, frame: i64) {
        self.reset_empty();
        self.frame = Some(frame);
    }

    fn reset_empty(&mut self) {
        self.frame = None;
        self.block = None;
        self.open = None;
        self.tallies.clear();
        self.calls = 0;
        self.submissions = 0;
        self.overflow = false;
    }

    fn close_open(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let key = TallyKey {
            call: open.key,
            treatment: Treatment::classify(open.submissions, open.promoted),
        };
        if !self.tallies.contains_key(&key) && self.tallies.len() >= MAX_ENTRIES {
            self.overflow = true;
            return;
        }
        let tally = self.tallies.entry(key).or_default();
        tally.calls += 1;
        tally.submissions += open.submissions;
    }
}

struct OpenCall {
    key: CallKey,
    submissions: u32,
    promoted: bool,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CallKey {
    dispatch: usize,
    pass: u8,
    block: Option<&'static str>,
    vertex_shader: ShaderName,
    vertex_patched: bool,
    pixel_shader: ShaderName,
    entry: &'static str,
    reissue_eye: Option<usize>,
    split_eye: Option<usize>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TallyKey {
    call: CallKey,
    treatment: Treatment,
}

impl TallyKey {
    fn entry(&self, tally: &Tally) -> CensusEntry {
        let call = &self.call;
        CensusEntry {
            dispatch: call.dispatch,
            pass: call.pass,
            pass_name: pass_name(call.pass),
            block: call.block,
            vertex_shader: call.vertex_shader.clone(),
            vertex_patched: call.vertex_patched,
            pixel_shader: call.pixel_shader.clone(),
            entry: call.entry,
            reissue_eye: call.reissue_eye,
            split_eye: call.split_eye,
            treatment: self.treatment,
            calls: tally.calls,
            submissions: tally.submissions,
        }
    }
}

#[derive(Default)]
struct Tally {
    calls: u32,
    submissions: u32,
}

fn vertex_name(shader: Option<&ID3D11VertexShader>) -> ShaderName {
    let Some(shader) = shader else {
        return ShaderName::Unbound;
    };
    match vertex_identity_lookup(shader.as_raw() as usize) {
        Ok(Some(identity)) => match identity.name {
            Some(name) => ShaderName::Named { name },
            None => ShaderName::Pristine {
                sha256: hex(&identity.actual_sha256),
            },
        },
        Ok(None) => ShaderName::Unknown { reason: "unmapped" },
        Err(reason) => ShaderName::Unknown { reason },
    }
}

fn pixel_name(shader: Option<&ID3D11PixelShader>) -> ShaderName {
    let Some(shader) = shader else {
        return ShaderName::Unbound;
    };
    match shader_identity::lookup(shader) {
        IdentityResult::Mapped { identity } => ShaderName::Pristine {
            sha256: identity.pristine_sha256,
        },
        IdentityResult::Unarmed => ShaderName::Unknown { reason: "unarmed" },
        IdentityResult::Unmapped => ShaderName::Unknown { reason: "unmapped" },
        IdentityResult::Incomplete => ShaderName::Unknown {
            reason: "incomplete",
        },
        IdentityResult::InstallFailed => ShaderName::Unknown {
            reason: "install_failed",
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn pass_name(pass: u8) -> String {
    if i32::from(pass) >= RenderPassId::RP_RENDERPASS_COUNT as i32 {
        return format!("unknown pass {pass}");
    }
    // SAFETY: `RenderPassId` is a contiguous `i32` enum starting at zero, and the id was checked to
    // lie below its count.
    let id = unsafe { std::mem::transmute::<i32, RenderPassId>(i32::from(pass)) };
    format!("{id:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_promotion_classifies_as_instance_doubled_whatever_the_count() {
        assert_eq!(Treatment::classify(1, true), Treatment::InstanceDoubled);
        assert_eq!(Treatment::classify(2, true), Treatment::InstanceDoubled);
    }

    #[test]
    fn the_submission_count_classifies_an_unpromoted_call() {
        assert_eq!(Treatment::classify(0, false), Treatment::Suppressed);
        assert_eq!(Treatment::classify(1, false), Treatment::Once);
        assert_eq!(Treatment::classify(2, false), Treatment::PerEye);
        assert_eq!(Treatment::classify(4, false), Treatment::Repeated);
    }

    #[test]
    fn calls_that_agree_fold_into_one_entry_split_by_treatment() {
        let mut recorder = Recorder {
            frame: Some(1),
            block: None,
            open: None,
            tallies: BTreeMap::new(),
            calls: 0,
            submissions: 0,
            overflow: false,
        };
        let key = CallKey {
            dispatch: 0,
            pass: 3,
            block: Some("Block"),
            vertex_shader: ShaderName::Unbound,
            vertex_patched: false,
            pixel_shader: ShaderName::Unbound,
            entry: "indexed",
            reissue_eye: None,
            split_eye: None,
        };
        for submissions in [2, 2, 1] {
            recorder.open = Some(OpenCall {
                key: key.clone(),
                submissions,
                promoted: false,
            });
            recorder.close_open();
        }
        let entries: Vec<_> = recorder
            .tallies
            .iter()
            .map(|(key, tally)| key.entry(tally))
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            (
                entries[0].treatment,
                entries[0].calls,
                entries[0].submissions
            ),
            (Treatment::Once, 1, 1)
        );
        assert_eq!(
            (
                entries[1].treatment,
                entries[1].calls,
                entries[1].submissions
            ),
            (Treatment::PerEye, 2, 4)
        );
    }
}
