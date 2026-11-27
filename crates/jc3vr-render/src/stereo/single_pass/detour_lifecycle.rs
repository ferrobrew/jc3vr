//! Detour lifecycle: the `DetourSlot` wrapper, the COM-vtable install/uninstall lock, the
//! `ensure_viewport_detours` installer that patches the D3D11 immediate-context and device vtables,
//! and the separately installed `CreatePixelShader` detour ([`ensure_pixel_shader_detour`]).
//!
//! The per-detour statics and detour functions live in their owning leaf modules (`viewport.rs`,
//! `draw_detours.rs`, `shader_detours.rs`, `per_eye_reissue.rs`, `shader_identity.rs`); this module
//! owns the install orchestration that publishes and enables them as a batch.

use std::{
    ffi::c_void,
    fmt,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use parking_lot::Mutex;
use retour::{Function, GenericDetour};
use windows::{Win32::Foundation::RECT, core::Interface};

use crate::stereo::single_pass::{
    active,
    draw_detours::*,
    per_eye_reissue::*,
    shader_detours::*,
    shader_identity::{
        CREATE_PIXEL_SHADER, CREATE_PIXEL_SHADER_SLOT, CreatePixelShaderFn,
        create_pixel_shader_detour,
    },
    viewport::*,
};

/// A slot for a detour that is installed once and removed on eject.
///
/// The detour is stored as an `AtomicPtr` so the hot-path read (a `get()` per draw) is a single
/// atomic load, while the install/remove path can swap the pointer under the
/// [`DETOUR_INSTALL`](static@DETOUR_INSTALL) lock without touching the detour object itself. The
/// detour is never moved after install: `GenericDetour` owns the trampoline, and the slot owns the
/// `GenericDetour` via a `Box` whose address never changes.
///
/// The slot can be emptied because Rust statics are not dropped, and a detour's trampoline lives in a
/// `VirtualAlloc` region that outlives the unmapped payload, so a detour held in a plain static would
/// strand one page per detour on every inject/eject cycle. [`take`](Self::take) hands ownership back
/// on eject.
pub(super) struct DetourSlot<T: Function>(AtomicPtr<GenericDetour<T>>);

impl<T: Function> DetourSlot<T> {
    pub(super) const fn new() -> Self {
        Self(AtomicPtr::new(std::ptr::null_mut()))
    }

    /// The installed detour, or `None` before install and after teardown.
    pub(super) fn get(&self) -> Option<&GenericDetour<T>> {
        // SAFETY: the pointer is null or a `Box` this slot owns. It is published with `Release`
        // before the detour it belongs to can be entered, and reclaimed only with every other thread
        // suspended, so a borrow taken here cannot outlive the allocation.
        unsafe { self.0.load(Ordering::Acquire).as_ref() }
    }

    /// Install `detour` into an empty slot. A second call leaves the slot alone and drops `detour`.
    pub(super) fn set(&self, detour: GenericDetour<T>) {
        let raw = Box::into_raw(Box::new(detour));
        if self
            .0
            .compare_exchange(
                std::ptr::null_mut(),
                raw,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_err()
        {
            // SAFETY: the slot was already occupied, so nothing else can have seen `raw`.
            drop(unsafe { Box::from_raw(raw) });
            // `set` is only called under `DETOUR_INSTALL`, so this should never fire; if it does, it
            // flags a real bug — a duplicate install attempted while the slot was already occupied.
            tracing::warn!("detour slot: duplicate install attempted and dropped");
        }
    }

    /// Empty the slot, returning the detour so dropping it frees the trampoline.
    pub(super) fn take(&self) -> Option<Box<GenericDetour<T>>> {
        let raw = self.0.swap(std::ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: a non-null pointer here is the `Box` this slot owned; the swap makes the take
        // exclusive.
        (!raw.is_null()).then(|| unsafe { Box::from_raw(raw) })
    }
}

unsafe extern "system" fn rs_set_scissor_rects_detour(
    context: *mut c_void,
    count: u32,
    rects: *const RECT,
) {
    let detour = RS_SET_SCISSOR_RECTS.get().expect("set before enable");
    if active() && count == 1 && !rects.is_null() {
        // Duplicating the single scissor into both slots unconditionally is correct because the
        // viewport detour keeps the scissor and viewport in lockstep:
        //
        // During a per-eye re-issue, both viewport slots are already pinned to the same eye half
        // (via `ensure_collapse_viewport` with `CollapseViewport::Eye`), so duplicating the scissor
        // into both slots matches the duplicated viewport.
        //
        // During the collapse split, both viewport slots are the two eye halves, and duplicating the
        // single scissor into both is the non-diverging fallback — each eye's scissor is the same
        // full-target rect. The scissor never needs to be split differently from the viewport because
        // the engine always sets them together.
        let rect = unsafe { *rects };
        unsafe { detour.call(context, 2, [rect, rect].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, rects) };
    }
}

/// Whether the draw-path COM detours are installed. Once a session has run single-pass they stay
/// installed until eject, and every draw then passes through them, single-pass or not.
pub fn com_detours_installed() -> bool {
    RS_SET_VIEWPORTS.get().is_some()
}

/// Install the single-pass COM-vtable detours on the immediate-context (and device) vtables, once.
/// Patching runs under a thread suspender, which narrows the window in which another thread can be
/// executing a target's prologue while it is rewritten -- it does not close it: `SuspendThread` is
/// asynchronous, and no instruction pointer is inspected, so a thread already inside the bytes being
/// overwritten stays there. Called from the active render path and from the
/// `CreateVertexProgram` hook -- the latter so the `CreateVertexShader` detour that records a patched
/// shader into the patched set exists *before* the shader is created, not lazily on the first
/// rendered frame (a shader created in between, e.g. a character shader loaded at level start, would
/// otherwise be patched at the blob level but never recorded, so `BOUND_VS_PATCHED` stays false and
/// its draw is never doubled). A session without single-pass never installs them.
pub fn ensure_viewport_detours() {
    // The whole body is serialized, not just the published-yet check: the callers are on different
    // threads (the render thread's cb13 mirror and the shader-creation thread's `CreateVertexProgram`),
    // and the publish happens only after every `GenericDetour::new` call. Two threads could otherwise
    // both pass an unpublished check and both reach `jc3vr_core::hooks::suspend::for_block`, each
    // suspending the other -- a silent, permanent hang. `uninstall_com_detours` takes the same lock so
    // an eject cannot interleave with an install.
    let _install = DETOUR_INSTALL.lock();
    if RS_SET_VIEWPORTS.get().is_some() || com_detours_failed() || jc3vr_core::is_shutting_down() {
        // Already installed, failed, or tearing down: never (re)install during eject.
        return;
    }
    // SAFETY: reads the live immediate-context and device vtables; the slots are the standard D3D11
    // layout.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let vtable = *(context.m_Context.as_raw() as *const *const usize);
        let device_vtable = *(device.m_Device.as_raw() as *const *const usize);
        let viewports_target: RsSetViewportsFn =
            std::mem::transmute(*vtable.add(RS_SET_VIEWPORTS_SLOT));
        let scissors_target: RsSetScissorRectsFn =
            std::mem::transmute(*vtable.add(RS_SET_SCISSOR_RECTS_SLOT));
        let draw_indexed_target: DrawIndexedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_SLOT));
        let draw_target: DrawFn = std::mem::transmute(*vtable.add(DRAW_SLOT));
        let draw_indexed_instanced_target: DrawIndexedInstancedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_SLOT));
        let draw_indexed_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_INDIRECT_SLOT));
        let draw_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INSTANCED_INDIRECT_SLOT));
        let vs_set_shader_target: VsSetShaderFn =
            std::mem::transmute(*vtable.add(VS_SET_SHADER_SLOT));
        let create_vertex_shader_target: CreateVertexShaderFn =
            std::mem::transmute(*device_vtable.add(CREATE_VERTEX_SHADER_SLOT));
        // Unlike the rest, this one is a static engine function (not a COM vtable slot): the leaf
        // vertex-constant stager, detoured so the baked-cb per-eye re-issue can reproject a block's own
        // constant upload.
        let set_vs_consts_target: SetVertexProgramConstantsFn =
            std::mem::transmute(jc3gi::graphics_engine::draw::SetVertexProgramConstants_ADDRESS);

        let (
            Ok(viewports_detour),
            Ok(scissors_detour),
            Ok(draw_indexed_detour_handle),
            Ok(draw_detour_handle),
            Ok(draw_indexed_instanced_detour_handle),
            Ok(draw_indexed_instanced_indirect_detour_handle),
            Ok(draw_instanced_indirect_detour_handle),
            Ok(vs_set_shader_detour_handle),
            Ok(create_vertex_shader_detour_handle),
            Ok(set_vs_consts_detour_handle),
        ) = (
            GenericDetour::new(viewports_target, rs_set_viewports_detour),
            GenericDetour::new(scissors_target, rs_set_scissor_rects_detour),
            GenericDetour::new(draw_indexed_target, draw_indexed_detour),
            GenericDetour::new(draw_target, draw_detour),
            GenericDetour::new(draw_indexed_instanced_target, draw_indexed_instanced_detour),
            GenericDetour::new(
                draw_indexed_instanced_indirect_target,
                draw_indexed_instanced_indirect_detour,
            ),
            GenericDetour::new(
                draw_instanced_indirect_target,
                draw_instanced_indirect_detour,
            ),
            GenericDetour::new(vs_set_shader_target, vs_set_shader_detour),
            GenericDetour::new(create_vertex_shader_target, create_vertex_shader_detour),
            GenericDetour::new(set_vs_consts_target, set_vertex_program_constants_detour),
        )
        else {
            COM_DETOURS_FAILED.store(true, Ordering::Release);
            tracing::error!(
                "single-pass: COM detour construction failed; single-pass rendering stays off for this session"
            );
            return;
        };

        // Publish into the statics before enabling, so a detour that fires mid-enable finds its
        // trampoline. Enabling itself runs with other threads suspended.
        RS_SET_VIEWPORTS.set(viewports_detour);
        RS_SET_SCISSOR_RECTS.set(scissors_detour);
        DRAW_INDEXED.set(draw_indexed_detour_handle);
        DRAW.set(draw_detour_handle);
        DRAW_INDEXED_INSTANCED.set(draw_indexed_instanced_detour_handle);
        DRAW_INDEXED_INSTANCED_INDIRECT.set(draw_indexed_instanced_indirect_detour_handle);
        DRAW_INSTANCED_INDIRECT.set(draw_instanced_indirect_detour_handle);
        VS_SET_SHADER.set(vs_set_shader_detour_handle);
        CREATE_VERTEX_SHADER.set(create_vertex_shader_detour_handle);
        SET_VERTEX_PROGRAM_CONSTANTS.set(set_vs_consts_detour_handle);
        let batch: [(&'static str, &dyn BatchDetour); 10] = [
            ("RSSetViewports", &RS_SET_VIEWPORTS),
            ("RSSetScissorRects", &RS_SET_SCISSOR_RECTS),
            ("DrawIndexed", &DRAW_INDEXED),
            ("Draw", &DRAW),
            ("DrawIndexedInstanced", &DRAW_INDEXED_INSTANCED),
            (
                "DrawIndexedInstancedIndirect",
                &DRAW_INDEXED_INSTANCED_INDIRECT,
            ),
            ("DrawInstancedIndirect", &DRAW_INSTANCED_INDIRECT),
            ("VSSetShader", &VS_SET_SHADER),
            ("CreateVertexShader", &CREATE_VERTEX_SHADER),
            ("SetVertexProgramConstants", &SET_VERTEX_PROGRAM_CONSTANTS),
        ];
        // The failure is carried out of the block rather than returned through it: boxing it into
        // the block's error allocates, and an allocation with the other threads suspended can wait
        // on a heap lock one of them holds.
        let mut failure = None;
        let suspension = jc3vr_core::hooks::suspend::for_block(|| {
            failure = enable_batch(&batch).err();
            Ok(())
        });
        let error = match (suspension, failure) {
            (Ok(()), None) => {
                tracing::info!(
                    "single-pass: viewport + draw + shader-tracking COM detours installed"
                );
                return;
            }
            (Err(error), _) => format!("single-pass: COM detours: {error}"),
            (Ok(()), Some(failure)) => failure.to_string(),
        };
        // A partly enabled draw path would double some draws and not others, so single-pass stays
        // off for the session and the frame renders through the multipass path. Disabled detours
        // are reclaimed here, outside the suspension; one the rollback could not disable stays in
        // its slot, a pass-through while single-pass is off, for the eject to remove.
        COM_DETOURS_FAILED.store(true, Ordering::Release);
        for (_, detour) in batch {
            detour.drop_if_disabled();
        }
        tracing::error!("{error}; single-pass rendering stays off for this session");
    }
}

/// Whether installing the draw-path COM detours failed. Single-pass rendering stays off for the rest
/// of the session once it has.
pub(super) fn com_detours_failed() -> bool {
    COM_DETOURS_FAILED.load(Ordering::Acquire)
}

pub(crate) fn vertex_identity_detour_ready() -> bool {
    CREATE_VERTEX_SHADER
        .get()
        .is_some_and(|detour| detour.is_enabled())
}

pub(crate) fn ensure_pixel_shader_detour() -> bool {
    let _install = DETOUR_INSTALL.lock();
    if let Some(detour) = CREATE_PIXEL_SHADER.get() {
        return detour.is_enabled();
    }
    if jc3vr_core::is_shutting_down() {
        return false;
    }
    let Some(device) =
        (unsafe { GraphicsEngine::get() }).and_then(|ge| unsafe { ge.m_Device.as_ref() })
    else {
        return false;
    };
    let target: CreatePixelShaderFn = unsafe {
        let vtable = *(device.m_Device.as_raw() as *const *const usize);
        std::mem::transmute(*vtable.add(CREATE_PIXEL_SHADER_SLOT))
    };
    let Ok(detour) = (unsafe { GenericDetour::new(target, create_pixel_shader_detour) }) else {
        return false;
    };
    CREATE_PIXEL_SHADER.set(detour);
    let enabled = jc3vr_core::hooks::suspend::for_block(|| {
        let detour = CREATE_PIXEL_SHADER.get().expect("just set");
        let result = unsafe { detour.enable() };
        if result.is_ok() || detour.is_enabled() {
            PIXEL_SHADER_EVER_ENABLED.store(true, Ordering::Release);
        }
        result?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .is_ok()
        && CREATE_PIXEL_SHADER
            .get()
            .is_some_and(|detour| detour.is_enabled());
    if !enabled
        && CREATE_PIXEL_SHADER
            .get()
            .is_some_and(|detour| !detour.is_enabled())
    {
        drop(CREATE_PIXEL_SHADER.take());
    }
    if PIXEL_SHADER_EVER_ENABLED.load(Ordering::Acquire) {
        // The capture harness's re-injection reads this line to refuse an eject that would tear the
        // payload down and then find it cannot unmap.
        tracing::warn!(
            "{PINNED_BY_PIXEL_SHADER_DETOUR}: an eject will be refused until the game restarts"
        );
    }
    enabled
}

/// The log line that marks the payload as pinned by the pixel-shader detour.
const PINNED_BY_PIXEL_SHADER_DETOUR: &str =
    "shader identity: the pixel-shader detour was enabled, which pins the payload";

pub(super) static PIXEL_SHADER_EVER_ENABLED: AtomicBool = AtomicBool::new(false);

static COM_DETOURS_FAILED: AtomicBool = AtomicBool::new(false);

/// Serializes COM detour installation and removal across threads.
pub(super) static DETOUR_INSTALL: Mutex<()> = Mutex::new(());

/// A detour in an install batch, enabled and rolled back as one.
trait BatchDetour {
    /// # Safety
    ///
    /// Every other thread is suspended, and the detour's slot is published.
    unsafe fn enable(&self) -> retour::Result<()>;

    /// # Safety
    ///
    /// As [`BatchDetour::enable`].
    unsafe fn disable(&self) -> retour::Result<()>;

    /// Empty the slot unless its detour is enabled. Frees the trampoline, so never under suspension.
    fn drop_if_disabled(&self);
}

impl<T: Function> BatchDetour for DetourSlot<T> {
    unsafe fn enable(&self) -> retour::Result<()> {
        unsafe { self.get().expect("published before enable").enable() }
    }

    unsafe fn disable(&self) -> retour::Result<()> {
        unsafe { self.get().expect("published before enable").disable() }
    }

    fn drop_if_disabled(&self) {
        if self.get().is_some_and(|detour| !detour.is_enabled()) {
            drop(self.take());
        }
    }
}

/// Why a batch did not enable: the detour that failed, and any detour the rollback left enabled.
#[derive(Debug)]
struct BatchFailure<const N: usize> {
    name: &'static str,
    source: retour::Error,
    stuck: [Option<&'static str>; N],
}

impl<const N: usize> fmt::Display for BatchFailure<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "single-pass: could not enable the {} detour: {}",
            self.name, self.source
        )?;
        let mut stuck = self.stuck.iter().flatten();
        if let Some(first) = stuck.next() {
            write!(f, "; the rollback left {first}")?;
            for name in stuck {
                write!(f, ", {name}")?;
            }
            write!(f, " enabled")?;
        }
        Ok(())
    }
}

/// Enable `batch` in order. On the first failure, disable the detours already enabled, newest first,
/// so the batch is all or nothing unless a disable fails too. Allocates nothing, since it runs with
/// the other threads suspended.
///
/// # Safety
///
/// As [`BatchDetour::enable`], for every detour in `batch`.
unsafe fn enable_batch<const N: usize>(
    batch: &[(&'static str, &dyn BatchDetour); N],
) -> Result<(), BatchFailure<N>> {
    for (index, (name, detour)) in batch.iter().enumerate() {
        if let Err(source) = unsafe { detour.enable() } {
            let mut stuck = [None; N];
            for (slot, (enabled, detour)) in batch[..index].iter().rev().enumerate() {
                if unsafe { detour.disable() }.is_err() {
                    stuck[slot] = Some(*enabled);
                }
            }
            return Err(BatchFailure {
                name,
                source,
                stuck,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    struct Fake<'a> {
        name: &'static str,
        fail_enable: bool,
        fail_disable: bool,
        enabled: Cell<bool>,
        log: &'a RefCell<Vec<String>>,
    }

    impl<'a> Fake<'a> {
        fn new(name: &'static str, log: &'a RefCell<Vec<String>>) -> Self {
            Self {
                name,
                fail_enable: false,
                fail_disable: false,
                enabled: Cell::new(false),
                log,
            }
        }
    }

    impl BatchDetour for Fake<'_> {
        unsafe fn enable(&self) -> retour::Result<()> {
            self.log.borrow_mut().push(format!("enable {}", self.name));
            if self.fail_enable {
                return Err(retour::Error::NotExecutable);
            }
            self.enabled.set(true);
            Ok(())
        }

        unsafe fn disable(&self) -> retour::Result<()> {
            self.log.borrow_mut().push(format!("disable {}", self.name));
            if self.fail_disable {
                return Err(retour::Error::NotInitialized);
            }
            self.enabled.set(false);
            Ok(())
        }

        fn drop_if_disabled(&self) {}
    }

    #[test]
    fn a_batch_that_enables_leaves_every_detour_enabled() {
        let log = RefCell::new(Vec::new());
        let (a, b) = (Fake::new("a", &log), Fake::new("b", &log));
        let batch: [(&'static str, &dyn BatchDetour); 2] = [("a", &a), ("b", &b)];
        assert!(unsafe { enable_batch(&batch) }.is_ok());
        assert!(a.enabled.get() && b.enabled.get());
    }

    #[test]
    fn a_failed_enable_rolls_back_the_enabled_detours_newest_first() {
        let log = RefCell::new(Vec::new());
        let (a, b, d) = (
            Fake::new("a", &log),
            Fake::new("b", &log),
            Fake::new("d", &log),
        );
        let c = Fake {
            fail_enable: true,
            ..Fake::new("c", &log)
        };
        let batch: [(&'static str, &dyn BatchDetour); 4] =
            [("a", &a), ("b", &b), ("c", &c), ("d", &d)];
        let failure = unsafe { enable_batch(&batch) }.expect_err("c fails");
        assert_eq!(failure.name, "c");
        assert_eq!(failure.stuck, [None; 4]);
        assert!(!a.enabled.get() && !b.enabled.get() && !d.enabled.get());
        assert_eq!(
            *log.borrow(),
            ["enable a", "enable b", "enable c", "disable b", "disable a"]
        );
    }

    #[test]
    fn a_detour_the_rollback_cannot_disable_is_reported() {
        let log = RefCell::new(Vec::new());
        let a = Fake {
            fail_disable: true,
            ..Fake::new("a", &log)
        };
        let b = Fake {
            fail_enable: true,
            ..Fake::new("b", &log)
        };
        let batch: [(&'static str, &dyn BatchDetour); 2] = [("a", &a), ("b", &b)];
        let failure = unsafe { enable_batch(&batch) }.expect_err("b fails");
        assert!(a.enabled.get());
        assert_eq!(
            failure.to_string(),
            "single-pass: could not enable the b detour: Address is not executable; the rollback \
             left a enabled"
        );
    }
}
