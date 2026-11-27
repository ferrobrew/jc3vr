//! The water's second eye under the double-wide collapse: the pixel-stage state the engine binds
//! once for a water surface's first draw, captured while the first eye's draws run and put back for
//! the second eye's, whose draws find it unbound.
//!
//! [`begin_water_surface`] opens a replay for one water surface and [`WaterReplayGuard::begin_eye`]
//! switches between capturing (eye 0) and restoring (eye 1). The draw detours reach it through
//! [`packed_water_context`], once per draw, on the draw thread: the state is thread-local, and the
//! guard is not `Send`.
//!
//! # Safety
//!
//! Every `unsafe fn` here takes the engine's immediate context, which the caller keeps live for the
//! duration of the call by being the draw detour the engine called with it, on the draw thread. The
//! interfaces the getters hand back are owned references, released when dropped.

use std::{cell::RefCell, ffi::c_void, marker::PhantomData, rc::Rc};

use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, ID3D11Buffer, ID3D11DeviceContext, ID3D11DeviceContext1,
        ID3D11PixelShader, ID3D11SamplerState, ID3D11ShaderResourceView,
    },
    core::Interface,
};

pub(crate) struct WaterReplayGuard {
    active: bool,
    _not_send: PhantomData<Rc<()>>,
}

pub(crate) fn begin_water_surface() -> WaterReplayGuard {
    let active = STATE.with_borrow_mut(State::begin_frame);
    WaterReplayGuard {
        active,
        _not_send: PhantomData,
    }
}

impl WaterReplayGuard {
    pub(crate) fn begin_eye(&self, eye: usize) {
        if !self.active {
            return;
        }
        STATE.with_borrow_mut(|state| state.begin_eye(eye));
    }
}

impl Drop for WaterReplayGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        STATE.with_borrow_mut(State::end_frame);
    }
}

/// The immediate context behind `context` when a water draw runs under the double-wide collapse,
/// after capturing or restoring the draw's state for the current eye; `None` outside a water
/// surface or the collapse, or without a depth target bound.
///
/// # Safety
///
/// `context` is the live immediate-context pointer the detoured draw call received, on the draw
/// thread.
pub(crate) unsafe fn packed_water_context(context: &*mut c_void) -> Option<&ID3D11DeviceContext> {
    // SAFETY: the function's contract (see the module docs and `# Safety`) makes `context` a live
    // immediate context, and every call below is on it.
    let phase = STATE.with_borrow(|state| state.phase);
    if phase == Phase::Idle {
        return None;
    }
    let ctx = unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(context) }?;
    let mut dsv = None;
    unsafe { ctx.OMGetRenderTargets(None, Some(&mut dsv)) };
    dsv.as_ref()?;
    match phase {
        Phase::Capture => unsafe { capture_draw(ctx) },
        Phase::Restore => unsafe { restore_draw(ctx) },
        Phase::Idle => {}
    }
    jc3vr_view::stereo::layout::double_wide_active().then_some(ctx)
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

const CONSTANT_SLOT: u32 = 2;
const PROGRAM_SAMPLER_SLOTS: [u32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
/// The pixel resource slots the second eye's run can find unbound: the shadow maps, and the three
/// sky-lighting textures the water shaders light the surface from.
const RESOURCE_SLOTS: [u32; 7] = [13, 14, 22, 23, 24, 44, 45];
/// The pixel constant slot of the frame's lighting constants, which the sky lighting reads.
const LIGHTING_CONSTANT_SLOT: u32 = 3;
const SAMPLER_SLOTS: [u32; 2] = [13, 15];

#[derive(Default)]
struct State {
    active: bool,
    phase: Phase,
    shadows_restored: bool,
    snapshot: Snapshot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Capture,
    Restore,
}

impl State {
    fn begin_frame(&mut self) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.phase = Phase::Idle;
        self.shadows_restored = false;
        self.snapshot = Snapshot::default();
        true
    }

    fn end_frame(&mut self) {
        self.active = false;
        self.phase = Phase::Idle;
        self.shadows_restored = false;
        self.snapshot = Snapshot::default();
    }

    fn begin_eye(&mut self, eye: usize) {
        self.phase = match eye {
            0 => Phase::Capture,
            1 => Phase::Restore,
            _ => Phase::Idle,
        };
    }

    fn shadow_restore_pending(&self) -> bool {
        self.phase == Phase::Restore && !self.shadows_restored && self.snapshot.shadows.is_some()
    }

    fn mark_shadows_restored(&mut self) {
        self.shadows_restored = true;
    }
}

#[derive(Default)]
struct Snapshot {
    shadows: Option<Shadows>,
    programs: Vec<(usize, ProgramState)>,
}

struct ProgramState {
    _shader: ID3D11PixelShader,
    buffer: Option<CapturedBuffer>,
    samplers: [Option<ID3D11SamplerState>; PROGRAM_SAMPLER_SLOTS.len()],
}

impl ProgramState {
    fn new(shader: ID3D11PixelShader) -> Self {
        Self {
            _shader: shader,
            buffer: None,
            samplers: std::array::from_fn(|_| None),
        }
    }
}

struct Shadows {
    resources: [Option<ID3D11ShaderResourceView>; RESOURCE_SLOTS.len()],
    samplers: [Option<ID3D11SamplerState>; SAMPLER_SLOTS.len()],
    lighting: Option<ID3D11Buffer>,
}

impl Default for Shadows {
    fn default() -> Self {
        Self {
            resources: std::array::from_fn(|_| None),
            samplers: std::array::from_fn(|_| None),
            lighting: None,
        }
    }
}

struct CapturedBuffer {
    buffer: ID3D11Buffer,
    range: ConstantRange,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ConstantRange {
    first_constant: u32,
    num_constants: u32,
    range_known: bool,
}

unsafe fn capture_draw(ctx: &ID3D11DeviceContext) {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    if STATE.with_borrow(|state| state.snapshot.shadows.is_none()) {
        let shadows = unsafe { capture_shadows(ctx) };
        STATE.with_borrow_mut(|state| state.snapshot.shadows = Some(shadows));
    }
    let Some(shader) = (unsafe { bound_program(ctx) }) else {
        return;
    };
    let buffer = unsafe { captured_buffer(ctx) };
    let samplers: [Option<ID3D11SamplerState>; PROGRAM_SAMPLER_SLOTS.len()] =
        std::array::from_fn(|index| unsafe { captured_sampler(ctx, PROGRAM_SAMPLER_SLOTS[index]) });
    if buffer.is_none() && samplers.iter().all(Option::is_none) {
        return;
    }
    let key = shader.as_raw() as usize;
    STATE.with_borrow_mut(|state| {
        if program_entry(&state.snapshot.programs, key).is_none() {
            insert_program(
                &mut state.snapshot.programs,
                key,
                ProgramState::new(shader.clone()),
            );
        }
        if let Some(entry) = program_entry_mut(&mut state.snapshot.programs, key) {
            update_if_bound(&mut entry.buffer, buffer);
            for (slot, sampler) in entry.samplers.iter_mut().zip(samplers) {
                update_if_bound(slot, sampler);
            }
        }
    });
}

unsafe fn restore_draw(ctx: &ID3D11DeviceContext) {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    if STATE.with_borrow(State::shadow_restore_pending) {
        STATE.with_borrow(|state| {
            if let Some(shadows) = &state.snapshot.shadows {
                unsafe { restore_shadows(ctx, shadows) };
            }
        });
        STATE.with_borrow_mut(State::mark_shadows_restored);
    }
    let Some(shader) = (unsafe { bound_program(ctx) }) else {
        return;
    };
    let key = shader.as_raw() as usize;
    if unsafe { captured_buffer(ctx) }.is_none() {
        STATE.with_borrow(|state| {
            if let Some(entry) = program_entry(&state.snapshot.programs, key)
                && let Some(saved) = entry.buffer.as_ref()
            {
                unsafe { bind_buffer(ctx, saved) };
            }
        });
    }
    for (index, slot) in PROGRAM_SAMPLER_SLOTS.into_iter().enumerate() {
        if unsafe { captured_sampler(ctx, slot) }.is_some() {
            continue;
        }
        STATE.with_borrow(|state| {
            if let Some(entry) = program_entry(&state.snapshot.programs, key)
                && let Some(saved) = entry.samplers[index].as_ref()
            {
                unsafe { ctx.PSSetSamplers(slot, Some(&[Some(saved.clone())])) };
            }
        });
    }
}

unsafe fn capture_shadows(ctx: &ID3D11DeviceContext) -> Shadows {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    Shadows {
        resources: std::array::from_fn(|index| unsafe {
            captured_resource(ctx, RESOURCE_SLOTS[index])
        }),
        samplers: std::array::from_fn(|index| unsafe {
            captured_sampler(ctx, SAMPLER_SLOTS[index])
        }),
        lighting: unsafe { lighting_constants(ctx) },
    }
}

unsafe fn restore_shadows(ctx: &ID3D11DeviceContext, shadows: &Shadows) {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    for (index, slot) in RESOURCE_SLOTS.into_iter().enumerate() {
        let current = unsafe { captured_resource(ctx, slot) };
        let saved = shadows.resources[index].as_ref();
        if fills_missing(current.is_some(), saved.is_some()) {
            unsafe { ctx.PSSetShaderResources(slot, Some(&[saved.cloned()])) };
        }
    }
    for (index, slot) in SAMPLER_SLOTS.into_iter().enumerate() {
        let current = unsafe { captured_sampler(ctx, slot) };
        let saved = shadows.samplers[index].as_ref();
        if fills_missing(current.is_some(), saved.is_some()) {
            unsafe { ctx.PSSetSamplers(slot, Some(&[saved.cloned()])) };
        }
    }
    let current = unsafe { lighting_constants(ctx) };
    if fills_missing(current.is_some(), shadows.lighting.is_some()) {
        unsafe {
            ctx.PSSetConstantBuffers(
                LIGHTING_CONSTANT_SLOT,
                Some(std::slice::from_ref(&shadows.lighting)),
            )
        };
    }
}

unsafe fn lighting_constants(ctx: &ID3D11DeviceContext) -> Option<ID3D11Buffer> {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    let mut buffers = [None];
    unsafe { ctx.PSGetConstantBuffers(LIGHTING_CONSTANT_SLOT, Some(&mut buffers)) };
    buffers[0].take()
}

unsafe fn bound_program(ctx: &ID3D11DeviceContext) -> Option<ID3D11PixelShader> {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    let mut shader = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    shader
}

unsafe fn captured_buffer(ctx: &ID3D11DeviceContext) -> Option<CapturedBuffer> {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    if let Ok(ctx1) = ctx.cast::<ID3D11DeviceContext1>() {
        let mut buffers = [None];
        let mut first_constant = 0u32;
        let mut num_constants = 0u32;
        unsafe {
            ctx1.PSGetConstantBuffers1(
                CONSTANT_SLOT,
                1,
                Some(buffers.as_mut_ptr()),
                Some(&mut first_constant),
                Some(&mut num_constants),
            )
        };
        let buffer = buffers[0].take()?;
        return Some(CapturedBuffer {
            buffer,
            range: ranged(first_constant, num_constants),
        });
    }
    let mut buffers = [None];
    unsafe { ctx.PSGetConstantBuffers(CONSTANT_SLOT, Some(&mut buffers)) };
    let buffer = buffers[0].take()?;
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    Some(CapturedBuffer {
        buffer,
        range: whole_buffer_range(desc.ByteWidth)?,
    })
}

unsafe fn captured_resource(
    ctx: &ID3D11DeviceContext,
    slot: u32,
) -> Option<ID3D11ShaderResourceView> {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    let mut view = None;
    unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut view))) };
    view
}

unsafe fn captured_sampler(ctx: &ID3D11DeviceContext, slot: u32) -> Option<ID3D11SamplerState> {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    let mut sampler = None;
    unsafe { ctx.PSGetSamplers(slot, Some(std::slice::from_mut(&mut sampler))) };
    sampler
}

unsafe fn bind_buffer(ctx: &ID3D11DeviceContext, saved: &CapturedBuffer) {
    // SAFETY: `ctx` is the live immediate context the draw detour was called with (see the module
    // docs), which every call below needs.
    let buffers = [Some(saved.buffer.clone())];
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { saved.buffer.GetDesc(&mut desc) };
    if saved.range.range_known
        && !binds_whole_buffer(saved.range, desc.ByteWidth)
        && let Ok(ctx1) = ctx.cast::<ID3D11DeviceContext1>()
    {
        unsafe {
            ctx1.PSSetConstantBuffers1(
                CONSTANT_SLOT,
                1,
                Some(buffers.as_ptr()),
                Some(&saved.range.first_constant as *const u32),
                Some(&saved.range.num_constants as *const u32),
            )
        };
        return;
    }
    unsafe { ctx.PSSetConstantBuffers(CONSTANT_SLOT, Some(&buffers)) };
}

fn insert_program<T>(entries: &mut Vec<(usize, T)>, program: usize, value: T) {
    if !entries.iter().any(|(existing, _)| *existing == program) {
        entries.push((program, value));
    }
}

fn program_entry<T>(entries: &[(usize, T)], program: usize) -> Option<&T> {
    entries
        .iter()
        .find(|(existing, _)| *existing == program)
        .map(|(_, value)| value)
}

fn program_entry_mut<T>(entries: &mut [(usize, T)], program: usize) -> Option<&mut T> {
    entries
        .iter_mut()
        .find(|(existing, _)| *existing == program)
        .map(|(_, value)| value)
}

fn update_if_bound<T>(slot: &mut Option<T>, current: Option<T>) {
    if current.is_some() {
        *slot = current;
    }
}

fn fills_missing(current_bound: bool, saved_bound: bool) -> bool {
    !current_bound && saved_bound
}

fn ranged(first_constant: u32, num_constants: u32) -> ConstantRange {
    ConstantRange {
        first_constant,
        num_constants,
        range_known: true,
    }
}

fn binds_whole_buffer(range: ConstantRange, byte_width: u32) -> bool {
    !range.range_known || (range.first_constant == 0 && range.num_constants == byte_width / 16)
}

fn whole_buffer_range(byte_width: u32) -> Option<ConstantRange> {
    let num_constants = byte_width / 16;
    (num_constants != 0).then_some(ConstantRange {
        first_constant: 0,
        num_constants,
        range_known: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_persists_for_the_whole_eye() {
        let guard = begin_water_surface();
        guard.begin_eye(0);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Capture);
        guard.begin_eye(1);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Restore);
        guard.begin_eye(2);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Idle);
    }

    #[test]
    fn nested_guard_is_inert() {
        let outer = begin_water_surface();
        let inner = begin_water_surface();
        outer.begin_eye(0);
        inner.begin_eye(1);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Capture);
        drop(inner);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Capture);
        drop(outer);
        assert_eq!(STATE.with_borrow(|state| state.phase), Phase::Idle);
    }

    #[test]
    fn program_entries_keep_two_shaders_apart() {
        let mut entries = Vec::new();
        insert_program(&mut entries, 0x1000, 0xAAAAu32);
        insert_program(&mut entries, 0x2000, 0xBBBBu32);
        insert_program(&mut entries, 0x1000, 0xCCCCu32);
        assert_eq!(entries.len(), 2);
        assert_eq!(program_entry(&entries, 0x1000), Some(&0xAAAA));
        assert_eq!(program_entry(&entries, 0x2000), Some(&0xBBBB));
        assert_eq!(program_entry(&entries, 0x3000), None);
        *program_entry_mut(&mut entries, 0x2000).expect("the second program") = 0xDDDD;
        assert_eq!(program_entry(&entries, 0x1000), Some(&0xAAAA));
        assert_eq!(program_entry(&entries, 0x2000), Some(&0xDDDD));
    }

    #[test]
    fn update_if_bound_ignores_a_null_capture() {
        let mut slot = None;
        update_if_bound(&mut slot, None::<u32>);
        assert_eq!(slot, None);
        update_if_bound(&mut slot, Some(1u32));
        assert_eq!(slot, Some(1));
        update_if_bound(&mut slot, None::<u32>);
        assert_eq!(slot, Some(1));
        update_if_bound(&mut slot, Some(2u32));
        assert_eq!(slot, Some(2));
    }

    #[test]
    fn shadow_restore_fires_once() {
        let guard = begin_water_surface();
        guard.begin_eye(1);
        STATE.with_borrow_mut(|state| state.snapshot.shadows = Some(Shadows::default()));
        assert!(STATE.with_borrow(State::shadow_restore_pending));
        STATE.with_borrow_mut(State::mark_shadows_restored);
        assert!(!STATE.with_borrow(State::shadow_restore_pending));
    }

    #[test]
    fn fills_missing_keeps_a_new_binding() {
        assert!(fills_missing(false, true));
        assert!(!fills_missing(true, true));
        assert!(!fills_missing(true, false));
        assert!(!fills_missing(false, false));
    }

    #[test]
    fn whole_buffer_bind_avoids_a_whole_buffer_range() {
        assert!(binds_whole_buffer(ranged(0, 4), 64));
        assert!(binds_whole_buffer(ranged(0, 16), 256));
        assert!(!binds_whole_buffer(ranged(16, 16), 256));
        assert!(binds_whole_buffer(
            whole_buffer_range(64).expect("a whole buffer range"),
            64
        ));
    }

    #[test]
    fn constant_ranges_record_the_bound_window() {
        assert_eq!(
            ranged(3, 5),
            ConstantRange {
                first_constant: 3,
                num_constants: 5,
                range_known: true,
            }
        );
        assert_eq!(whole_buffer_range(0), None);
        assert_eq!(
            whole_buffer_range(64),
            Some(ConstantRange {
                first_constant: 0,
                num_constants: 4,
                range_known: false,
            })
        );
    }
}
