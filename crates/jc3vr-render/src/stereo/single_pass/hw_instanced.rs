//! Hardware-instanced draws once for both eyes: the step-rate-doubled twins of the buildings' input
//! layouts, and how they are obtained after a late injection.
//!
//! A hardware-instanced draw takes its per-instance data from vertex-buffer elements that advance
//! once every `InstanceDataStepRate` instances. Doubling the instance count of such a draw would read
//! past the batch's data, but doubling every per-instance element's step rate as well keeps instances
//! `2k` and `2k + 1` on element `k`, and the patched vertex shader takes its eye from the instance
//! parity. So one draw serves both eyes once the layout bound for it is swapped for a twin whose
//! per-instance step rates are doubled.
//!
//! D3D11 cannot read a layout's elements back, and the engine builds them on the stack inside
//! [`RenderBlockTypeGeneralMkIII::Create`] at registration, long before the payload is injected. The
//! twins are therefore made by running that `Create` again, on a scratch copy of the live type
//! object, with the type's other side effects suppressed for that one call by the detours in
//! `crate::hooks::graphics_engine::declaration_replay`: the texture, shader-holder, sampler, and
//! constant-buffer acquisitions return without acquiring anything (the copy already holds the live
//! object's shader holders, which is all the declaration code reads), the per-vertex declarations
//! are skipped, and each instanced declaration is created by the engine's own
//! [`CreateVertexDeclarationInstanced`](jc3gi::graphics_engine::graphics_engine::CreateVertexDeclarationInstanced)
//! from its element array with every step rate doubled. `Create` stores each result in the field the
//! live object keeps the original in, so pairing the scratch copy's declaration fields with the live
//! object's maps every live layout to its twin, with no element table in the mod.
//!
//! The replay runs on the draw thread at the start of a dispatch, once per injection and again if
//! the live type's declarations ever change, and the twins are destroyed through the engine on
//! eject ([`release`]). A draw whose bound layout has no twin keeps `instanced_per_eye`'s re-issue.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    fmt,
    mem::MaybeUninit,
    sync::atomic::{AtomicUsize, Ordering},
};

use jc3gi::graphics_engine::{
    graphics_engine::{
        DestroyVertexDeclaration, HDevice_t, HVertexDeclaration_t, SInstancedVertexElement,
    },
    render_block::RenderBlockTypeGeneralMkIII,
    render_engine::RenderEngine,
};
use parking_lot::Mutex;
use windows::{Win32::Graphics::Direct3D11::ID3D11InputLayout, core::Interface};

use crate::draw_path::bump;

/// Whether this thread is replaying the type's `Create` on the scratch copy, which the replay
/// detours consult to suppress everything but the instanced declarations.
pub(crate) fn replaying() -> bool {
    REPLAYING.with(Cell::get)
}

/// Create one instanced declaration during the replay: `create` receives the element array with
/// every step rate doubled and calls the engine's creation with it. The handle it returns is the
/// twin, recorded for pairing. A null or empty array, or a step rate that would overflow, creates
/// nothing and returns null, which leaves that slot unpaired.
///
/// # Safety
/// `elements` must be null or point to `count` live elements.
pub(crate) unsafe fn replay_instanced(
    elements: *const SInstancedVertexElement,
    count: i32,
    create: impl FnOnce(*const SInstancedVertexElement) -> *mut HVertexDeclaration_t,
) -> *mut HVertexDeclaration_t {
    let Ok(len) = usize::try_from(count) else {
        return std::ptr::null_mut();
    };
    if elements.is_null() || len == 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller guarantees `count` live elements behind a non-null pointer.
    let source = unsafe { std::slice::from_raw_parts(elements, len) };
    let Some(doubled) = doubled_step_rates(source) else {
        return std::ptr::null_mut();
    };
    let twin = create(doubled.as_ptr());
    if !twin.is_null() {
        REPLAY_CREATED.with(|created| created.borrow_mut().push(twin as usize));
    }
    twin
}

/// Build the twins if they are missing or stale. Runs on the draw thread at the start of a
/// dispatch, where no draw of this thread is in flight.
pub(super) fn maintain() {
    if !crate::stereo::single_pass::active() {
        return;
    }
    // SAFETY: the singleton slot is static engine storage, set once at registration.
    let Some(live) = (unsafe { RenderBlockTypeGeneralMkIII::get() }) else {
        return;
    };
    let live_slots = declaration_slots(live);
    if live_slots.iter().all(|&slot| slot == 0) {
        return;
    }
    let previous = {
        let mut state = STATE.lock();
        match &*state {
            State::Built(twins) if twins.live_slots == live_slots => return,
            State::Failed { live_slots: seen } if *seen == live_slots => return,
            _ => std::mem::replace(&mut *state, State::Unbuilt),
        }
    };
    if let State::Built(stale) = previous {
        tracing::info!(
            target: "single_pass",
            "hardware-instanced twins: the GeneralMkIII declarations changed; rebuilding"
        );
        destroy(stale.device, &stale.handles);
    }
    // SAFETY: `live` is the registered type object, and this is the draw thread between draws.
    let next = match unsafe { build(live, live_slots) } {
        Ok(twins) => {
            tracing::info!(
                target: "single_pass",
                "hardware-instanced twins: {} GeneralMkIII layouts twinned",
                twins.by_layout.len()
            );
            State::Built(twins)
        }
        Err(error) => {
            tracing::warn!(target: "single_pass", "{error}");
            State::Failed { live_slots }
        }
    };
    *STATE.lock() = next;
}

/// The twin of `layout`, if it has one, with its own reference.
pub(super) fn twin_for(layout: &ID3D11InputLayout) -> Option<ID3D11InputLayout> {
    let key = layout.as_raw() as usize;
    let state = STATE.lock();
    let State::Built(twins) = &*state else {
        return None;
    };
    let handle = *twins.by_layout.get(&key)?;
    // SAFETY: the handle is a twin this module created and destroys only after removing it from the
    // state under this lock; cloning takes a reference that outlives that destruction.
    Some(unsafe { (*(handle as *const HVertexDeclaration_t)).m_Layout.clone() })
}

/// Note a hardware-instanced draw whose layout has no twin, logging each such layout once.
pub(super) fn note_untwinned(layout: &ID3D11InputLayout) {
    UNTWINNED_DRAWS.fetch_add(1, Ordering::Relaxed);
    if UNTWINNED_LAYOUTS.lock().insert(layout.as_raw() as usize) {
        tracing::info!(
            target: "single_pass",
            "hardware-instanced draw with an untwinned input layout {:p}; it stays on the per-eye \
             re-issue",
            layout.as_raw()
        );
    }
}

/// Count one draw issued once for both eyes through a twin.
pub(super) fn note_drawn_once() {
    bump(&DRAWN_ONCE);
}

/// The draws issued once through a twin and those left on the re-issue for want of one since the
/// last call, for the per-range log.
pub(super) fn take_counts() -> (usize, usize) {
    (
        DRAWN_ONCE.swap(0, Ordering::Relaxed),
        UNTWINNED_DRAWS.swap(0, Ordering::Relaxed),
    )
}

/// A one-line status for the debug UI.
pub fn status() -> String {
    match &*STATE.lock() {
        State::Unbuilt => "twins not built yet (built at the first single-pass dispatch)".into(),
        State::Built(twins) => format!("{} layouts twinned", twins.by_layout.len()),
        State::Failed { .. } => "twin build failed; see the log".into(),
    }
}

/// Destroy the twins. Runs on eject once the draw detours are disabled.
pub(super) fn release() {
    let previous = std::mem::replace(&mut *STATE.lock(), State::Unbuilt);
    if let State::Built(twins) = previous {
        destroy(twins.device, &twins.handles);
    }
    UNTWINNED_LAYOUTS.lock().clear();
}

thread_local! {
    static REPLAYING: Cell<bool> = const { Cell::new(false) };
    static REPLAY_CREATED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

static STATE: Mutex<State> = Mutex::new(State::Unbuilt);
static UNTWINNED_LAYOUTS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());
static DRAWN_ONCE: AtomicUsize = AtomicUsize::new(0);
static UNTWINNED_DRAWS: AtomicUsize = AtomicUsize::new(0);

/// The number of declaration fields of [`RenderBlockTypeGeneralMkIII`].
const SLOT_COUNT: usize = 22;

type Slots = [usize; SLOT_COUNT];

enum State {
    Unbuilt,
    Built(Twins),
    /// The build failed for these live declarations; it is not retried until they change.
    Failed {
        live_slots: Slots,
    },
}

struct Twins {
    /// The engine device the twins were created on, as an address.
    device: usize,
    /// The live type's declaration fields the twins were built against.
    live_slots: Slots,
    /// Every twin handle, each destroyed once.
    handles: Vec<usize>,
    /// Each live layout's twin handle, keyed by the live `ID3D11InputLayout` address.
    by_layout: BTreeMap<usize, usize>,
}

/// The type's declaration fields as addresses, in field order.
fn declaration_slots(ty: &RenderBlockTypeGeneralMkIII) -> Slots {
    let mut slots = [0; SLOT_COUNT];
    let fields = ty
        .m_VertexDeclaration
        .iter()
        .chain(ty.m_VertexDeclarationHWInstancingVelocity.iter().flatten())
        .chain(ty.m_VertexDeclarationDepth.iter())
        .chain(ty.m_VertexDeclarationDepthAlphaTest.iter());
    for (slot, field) in slots.iter_mut().zip(fields) {
        *slot = *field as usize;
    }
    slots
}

/// The elements with every step rate doubled, or `None` when one would overflow.
fn doubled_step_rates(
    elements: &[SInstancedVertexElement],
) -> Option<Vec<SInstancedVertexElement>> {
    elements
        .iter()
        .map(|element| {
            Some(SInstancedVertexElement {
                m_StepRate: element.m_StepRate.checked_mul(2)?,
                ..*element
            })
        })
        .collect()
}

/// Why the twins could not be built.
#[derive(Debug, PartialEq, Eq)]
enum BuildError {
    /// The render engine or its graphics device is not reachable.
    NoDevice,
    /// The replay created no instanced declaration at all.
    NothingCreated,
    /// The pairing of the scratch copy's fields with the live object's failed.
    Pairing(PairError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hardware-instanced twins: ")?;
        match self {
            Self::NoDevice => write!(f, "the render engine's graphics device is not reachable"),
            Self::NothingCreated => {
                write!(
                    f,
                    "replaying GeneralMkIII's Create created no instanced declaration"
                )
            }
            Self::Pairing(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Replay the type's `Create` on a scratch copy and pair the twins it creates with the live layouts.
/// On failure every twin created is destroyed again.
///
/// # Safety
/// `live` must be the registered type object, and the caller must be the draw thread between draws.
unsafe fn build(
    live: &RenderBlockTypeGeneralMkIII,
    live_slots: Slots,
) -> Result<Twins, BuildError> {
    // SAFETY: the singleton slot is static engine storage.
    let engine = unsafe { RenderEngine::get() }.ok_or(BuildError::NoDevice)?;
    let device = engine.m_ResourceContext.m_GraphicsDevice;
    if device.is_null() {
        return Err(BuildError::NoDevice);
    }
    let mut scratch = Box::new(ScratchType(MaybeUninit::uninit()));
    // SAFETY: a bitwise copy into storage of the same type; the copy is never dropped or destroyed
    // through the engine, so the shader holders and other references it duplicates are not released
    // twice.
    unsafe {
        std::ptr::copy_nonoverlapping(std::ptr::from_ref(live), scratch.0.as_mut_ptr(), 1);
    }
    REPLAY_CREATED.with(|created| created.borrow_mut().clear());
    REPLAYING.with(|flag| flag.set(true));
    // SAFETY: the scratch copy is a complete type object; the replay detours keep `Create` from
    // acquiring anything but the instanced declarations while the flag is raised on this thread.
    unsafe { (*scratch.0.as_mut_ptr()).Create(&raw const engine.m_ResourceContext) };
    REPLAYING.with(|flag| flag.set(false));
    let created = REPLAY_CREATED.with(|created| std::mem::take(&mut *created.borrow_mut()));
    // SAFETY: `Create` initialised the copy's declaration fields (and the copy held the live values
    // before that).
    let scratch_slots = declaration_slots(unsafe { scratch.0.assume_init_ref() });
    if created.is_empty() {
        return Err(BuildError::NothingCreated);
    }
    let pairs = match pair_twins(&live_slots, &scratch_slots, &created) {
        Ok(pairs) => pairs,
        Err(error) => {
            destroy(device as usize, &created);
            return Err(BuildError::Pairing(error));
        }
    };
    let by_layout = pairs
        .iter()
        .map(|&(live_handle, twin)| {
            // SAFETY: a live declaration handle the type object holds for the process's lifetime.
            let layout = unsafe {
                (*(live_handle as *const HVertexDeclaration_t))
                    .m_Layout
                    .as_raw()
            };
            (layout as usize, twin)
        })
        .collect();
    Ok(Twins {
        device: device as usize,
        live_slots,
        handles: created,
        by_layout,
    })
}

/// Storage for the scratch copy, aligned past the type's own alignment so the engine's code sees an
/// object placed as the allocator places the live one.
#[repr(C, align(16))]
struct ScratchType(MaybeUninit<RenderBlockTypeGeneralMkIII>);

/// Why the scratch copy's declaration fields could not be paired with the live object's.
#[derive(Debug, PartialEq, Eq)]
enum PairError {
    /// A field holds a twin in the copy but nothing in the live object.
    NoLiveDeclaration { slot: usize },
    /// One twin sits in fields whose live declarations differ.
    Inconsistent { slot: usize },
    /// A twin the replay created is in no declaration field.
    Unplaced { count: usize },
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLiveDeclaration { slot } => write!(
                f,
                "declaration field {slot} holds a twin but the live type holds no declaration there"
            ),
            Self::Inconsistent { slot } => write!(
                f,
                "declaration field {slot} repeats a twin whose live declaration differs from the \
                 first field's"
            ),
            Self::Unplaced { count } => {
                write!(f, "{count} created twins are in no declaration field")
            }
        }
    }
}

/// Pair the copy's twins with the live declarations in the same fields: one `(live, twin)` pair per
/// twin. A field the copy holds anything else in is skipped (the per-vertex declarations the replay
/// leaves null, and the aliases `Create` copies from them).
fn pair_twins(
    live: &Slots,
    scratch: &Slots,
    created: &[usize],
) -> Result<Vec<(usize, usize)>, PairError> {
    let created: BTreeSet<usize> = created.iter().copied().collect();
    let mut by_twin: BTreeMap<usize, usize> = BTreeMap::new();
    for (slot, (&live, &twin)) in live.iter().zip(scratch).enumerate() {
        if !created.contains(&twin) {
            continue;
        }
        if live == 0 {
            return Err(PairError::NoLiveDeclaration { slot });
        }
        if *by_twin.entry(twin).or_insert(live) != live {
            return Err(PairError::Inconsistent { slot });
        }
    }
    let unplaced = created.len() - by_twin.len();
    if unplaced > 0 {
        return Err(PairError::Unplaced { count: unplaced });
    }
    Ok(by_twin
        .into_iter()
        .map(|(twin, live)| (live, twin))
        .collect())
}

/// Destroy twin handles through the engine.
fn destroy(device: usize, handles: &[usize]) {
    for &handle in handles {
        // SAFETY: each handle is a declaration this module created on `device` and destroys once.
        unsafe {
            DestroyVertexDeclaration(
                device as *mut HDevice_t,
                handle as *mut HVertexDeclaration_t,
            )
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(step_rate: i32) -> SInstancedVertexElement {
        SInstancedVertexElement {
            m_Stream: 1,
            m_Size: 4,
            m_Type: 5,
            m_Usage: 3,
            m_UsageIndex: 2,
            m_StepRate: step_rate,
        }
    }

    #[test]
    fn doubling_changes_only_the_step_rate() {
        let doubled = doubled_step_rates(&[element(1), element(0), element(3)]).unwrap();
        let rates: Vec<i32> = doubled.iter().map(|e| e.m_StepRate).collect();
        assert_eq!(rates, [2, 0, 6]);
        for e in &doubled {
            assert_eq!(
                (e.m_Stream, e.m_Size, e.m_Type, e.m_Usage, e.m_UsageIndex),
                (1, 4, 5, 3, 2)
            );
        }
    }

    #[test]
    fn doubling_declines_an_overflowing_step_rate() {
        assert!(doubled_step_rates(&[element(1), element(i32::MAX)]).is_none());
    }

    fn slots(values: &[(usize, usize)]) -> Slots {
        let mut slots = [0; SLOT_COUNT];
        for &(index, value) in values {
            slots[index] = value;
        }
        slots
    }

    #[test]
    fn twins_pair_with_the_live_declaration_in_the_same_field() {
        let live = slots(&[(0, 0x100), (3, 0x110), (4, 0x120), (6, 0x130), (9, 0x100)]);
        // Field 0 is per-vertex (null in the copy), field 9 an alias of it.
        let scratch = slots(&[(3, 0x900), (4, 0x910), (6, 0x920)]);
        let pairs = pair_twins(&live, &scratch, &[0x900, 0x910, 0x920]).unwrap();
        assert_eq!(pairs, [(0x110, 0x900), (0x120, 0x910), (0x130, 0x920)]);
    }

    #[test]
    fn an_aliased_twin_pairs_once() {
        let live = slots(&[(4, 0x120), (5, 0x120)]);
        let scratch = slots(&[(4, 0x900), (5, 0x900)]);
        assert_eq!(
            pair_twins(&live, &scratch, &[0x900]).unwrap(),
            [(0x120, 0x900)]
        );
    }

    #[test]
    fn copied_live_values_are_not_twins() {
        // A field the replay left untouched still holds the live value from the copy.
        let live = slots(&[(1, 0x100), (4, 0x120)]);
        let scratch = slots(&[(1, 0x100), (4, 0x900)]);
        assert_eq!(
            pair_twins(&live, &scratch, &[0x900]).unwrap(),
            [(0x120, 0x900)]
        );
    }

    #[test]
    fn pairing_rejects_a_twin_without_a_live_declaration() {
        let live = slots(&[]);
        let scratch = slots(&[(7, 0x900)]);
        assert_eq!(
            pair_twins(&live, &scratch, &[0x900]),
            Err(PairError::NoLiveDeclaration { slot: 7 })
        );
    }

    #[test]
    fn pairing_rejects_an_inconsistent_alias() {
        let live = slots(&[(4, 0x120), (5, 0x130)]);
        let scratch = slots(&[(4, 0x900), (5, 0x900)]);
        assert_eq!(
            pair_twins(&live, &scratch, &[0x900]),
            Err(PairError::Inconsistent { slot: 5 })
        );
    }

    #[test]
    fn pairing_rejects_an_unplaced_twin() {
        let live = slots(&[(4, 0x120)]);
        let scratch = slots(&[(4, 0x900)]);
        assert_eq!(
            pair_twins(&live, &scratch, &[0x900, 0x910]),
            Err(PairError::Unplaced { count: 1 })
        );
    }
}
