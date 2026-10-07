//! The hand bodies' contact events: each contact a hand body starts, with the impulse the solver
//! applied at it and what it met, as the physics system's collision listener reports it.
//!
//! The listener (`CHavokCollisionListener`) hands a contact-impulse event
//! ([`hknpContactImpulseEvent`]) to the engine instance of each of the contact's bodies whose
//! switch for it is set ([`PfxInstance::m_IsContactImpulseEventActive`]), through the instance's
//! own event virtual ([`PfxRigidBody::OnContactImpulseEvent`]), which does nothing on the engine's
//! wrapper. So each hand body's wrapper is given a copy of the wrapper's virtual table
//! ([`PfxRigidBody::VFTABLE`]) whose event slot is [`on_contact_impulse`], and its switch and the
//! body's event flag are set ([`PfxRigidBody::SetContactImpulseEventActive`]). Every other slot,
//! the destructor included, is the engine's own, so the wrapper lives and dies as before. The copy
//! carries the slot before the table too, the compiler's run-time type information for the class,
//! so a type query on the wrapper still finds it. The copy is a static, valid for the payload's
//! lifetime, and the bodies are destroyed before the payload unloads; a wrapper forgotten without
//! being destroyed (its body went away under it) still points at the copy, so forgetting one pins
//! the payload's image for the rest of the game's run.
//!
//! The events arrive during the physics step ([`hknpWorld::stepSolve`] dispatches them on the
//! thread that steps the world), where the world must not be changed and the game's objects are
//! mid-update. The handler only records the contact in a queue of its own; the blows are dealt on
//! the game thread's next frame tick ([`crate::controllers::physical_hands::physics::blows`]).
//!
//! [`hknpWorld::stepSolve`]: jc3gi::physics::hknpWorld::stepSolve

use std::{
    ffi::c_void,
    fmt,
    ptr::NonNull,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use glam::Vec3;
use jc3gi::{
    havok::{hknpContactImpulseEvent, hknpContactJacobianUtil},
    physics::{PfxInstance, PfxRigidBody, PfxRigidBodyVftable},
};
use parking_lot::Mutex;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::controllers::{
    frame::Hand,
    physical_hands::physics::{callback, hit::contact_impulse},
};

/// A contact a hand body started.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Contact {
    pub hand: Hand,
    /// The impulse the solver applied at the contact over the step, newton seconds.
    pub impulse_ns: f32,
    /// The other body's id.
    pub other_body: u32,
    /// The other body's engine instance, as an address; zero for a body without one.
    pub other_instance: usize,
    /// The other instance's owner, as an address; zero for none.
    pub other_object: usize,
    /// Where the hand body was before the step, game space.
    pub hand_position: Vec3,
    /// The velocity the hand body was driven at for the step, world space.
    pub hand_velocity: Vec3,
}

/// Why a hand body's contacts could not be listened for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContactError {
    /// The wrapper's virtual table is not the engine's wrapper table.
    ForeignTable,
}

impl fmt::Display for ContactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactError::ForeignTable => write!(
                f,
                "hand bodies: contact events: the wrapper's virtual table is not the engine's"
            ),
        }
    }
}

impl std::error::Error for ContactError {}

/// Listen for the contacts of `hand`'s body, whose wrapper `wrapper` was just constructed.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step; `wrapper` must be a live wrapper the
/// engine's constructor made.
pub(super) unsafe fn listen(
    wrapper: NonNull<PfxRigidBody>,
    hand: Hand,
) -> Result<(), ContactError> {
    // SAFETY: the wrapper's first word is its virtual table pointer.
    let slot = wrapper.as_ptr().cast::<*const PfxRigidBodyVftable>();
    let engine_table = PfxRigidBody::VFTABLE as *const PfxRigidBodyVftable;
    if unsafe { slot.read() } != engine_table {
        return Err(ContactError::ForeignTable);
    }
    let table = TABLE.get_or_init(|| {
        // SAFETY: the engine's table is a static of exactly this many slots, preceded by the
        // compiler's run-time type information pointer.
        let mut table = unsafe { engine_table.read() };
        let type_info = unsafe { engine_table.cast::<usize>().sub(1).read() };
        table.OnContactImpulseEvent = on_contact_impulse;
        HookedTable { type_info, table }
    });
    WRAPPERS[hand.index()].store(wrapper.as_ptr() as usize, Ordering::Release);
    unsafe {
        slot.write(&table.table);
        (*wrapper.as_ptr()).SetContactImpulseEventActive(true);
    }
    Ok(())
}

/// Stop attributing contacts to `hand`'s body: before its wrapper is destroyed.
pub(super) fn forget(hand: Hand) {
    WRAPPERS[hand.index()].store(0, Ordering::Release);
    MOTION.lock()[hand.index()] = None;
}

/// Stop attributing contacts to `hand`'s body, whose wrapper is forgotten without being destroyed.
/// A wrapper that listened keeps pointing at the payload's copy of its table, and whether the
/// engine still reaches it is unknown, so the payload's image is pinned rather than unmapped under
/// it.
pub(super) fn abandon(hand: Hand) {
    if listening(hand) {
        tracing::warn!(
            hand = hand.label(),
            "hand bodies: a forgotten hand body still points at the payload's virtual table; \
             pinning the payload"
        );
        crate::module::pin();
    }
    forget(hand);
}

/// Whether `hand`'s body has contact events on: its wrapper is hooked for them.
pub(super) fn listening(hand: Hand) -> bool {
    WRAPPERS[hand.index()].load(Ordering::Acquire) != 0
}

/// Note where `hand`'s body is and the velocity it is driven at for the coming step.
pub(super) fn record_motion(hand: Hand, position: Vec3, velocity: Vec3) {
    MOTION.lock()[hand.index()] = Some((position, velocity));
}

/// Take the contacts recorded since the last call, and how many were dropped because the queue was
/// full.
pub(super) fn take() -> (Vec<Contact>, u64) {
    let contacts = std::mem::take(&mut *QUEUE.lock());
    (contacts, DROPPED.swap(0, Ordering::Relaxed))
}

/// The engine wrapper's table with the event slot replaced, laid out as the compiler lays a virtual
/// table out: the run-time type information pointer in the slot before the first virtual.
#[repr(C)]
struct HookedTable {
    type_info: usize,
    table: PfxRigidBodyVftable,
}

static TABLE: OnceLock<HookedTable> = OnceLock::new();

/// Each hand's wrapper, as an address; zero for none.
static WRAPPERS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// Each hand body's position and driven velocity for the coming step.
static MOTION: Mutex<[Option<(Vec3, Vec3)>; 2]> = Mutex::new([None; 2]);

static QUEUE: Mutex<Vec<Contact>> = Mutex::new(Vec::new());
static DROPPED: AtomicU64 = AtomicU64::new(0);
static FIRST_EVENT: AtomicBool = AtomicBool::new(true);

/// The most contacts the queue holds between two frame ticks.
const QUEUE_CAPACITY: usize = 32;

/// The hand wrappers' event virtual. Records a contact the body started; never touches the world.
unsafe extern "system" fn on_contact_impulse(
    this: *mut PfxRigidBody,
    _input: *const c_void,
    event: *const hknpContactImpulseEvent,
    other: *mut PfxInstance,
    is_body_a: bool,
) {
    // The payload aborts on a panic, so the handler must not panic.
    // SAFETY: the listener passes the live event and the other body's instance, or null.
    unsafe { record(this, event, other, is_body_a) }
}

unsafe fn record(
    this: *mut PfxRigidBody,
    event: *const hknpContactImpulseEvent,
    other: *mut PfxInstance,
    is_body_a: bool,
) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.m_status != hknpContactImpulseEvent::STATUS_STARTED
        && event.m_status != hknpContactImpulseEvent::STATUS_IMMEDIATE
    {
        return;
    }
    let this = this as usize;
    let Some(hand) = Hand::BOTH
        .into_iter()
        .find(|hand| this != 0 && WRAPPERS[hand.index()].load(Ordering::Acquire) == this)
    else {
        return;
    };
    let points = if event.m_contactJacobian.is_null() {
        0
    } else {
        unsafe {
            hknpContactJacobianUtil::getNumContactPoints(
                u32::from(event.m_contactSolverType),
                event.m_contactJacobian,
            )
        }
    };
    let impulse_ns = contact_impulse(event.m_contactImpulses, points);
    if impulse_ns <= 0.0 {
        return;
    }
    if FIRST_EVENT.swap(false, Ordering::Relaxed) {
        // SAFETY: reads the calling thread's own id.
        let thread = unsafe { GetCurrentThreadId() };
        let game_thread = callback::registering_thread();
        if thread == game_thread {
            tracing::info!(
                thread,
                "hand bodies: the first contact event arrived on the game thread"
            );
        } else {
            tracing::warn!(
                thread,
                game_thread,
                "hand bodies: the first contact event arrived on another thread than the game's"
            );
        }
    }
    let other_body = event.m_bodyIds[usize::from(is_body_a)];
    let (other_instance, other_object) = match unsafe { other.as_ref() } {
        Some(instance) => (other as usize, instance.m_GameObject as usize),
        None => (0, 0),
    };
    let (hand_position, hand_velocity) =
        MOTION.lock()[hand.index()].unwrap_or((Vec3::ZERO, Vec3::ZERO));
    let mut queue = QUEUE.lock();
    if queue.len() >= QUEUE_CAPACITY {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    queue.push(Contact {
        hand,
        impulse_ns,
        other_body,
        other_instance,
        other_object,
        hand_position,
        hand_velocity,
    });
}
