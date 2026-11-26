//! The grappling hook and its wire's two ends, read one way for every caller: the comfort filter,
//! the physical grapple and its lock, the snatch, the hand anchor, the mode snapshot, the shooting
//! permission, and the diagnostics.
//!
//! A character's hook hangs off its inventory, and a wire has two ends, the hook's (which flies to
//! and holds the target) and the device's (on the gauntlet). The engine side is in
//! `docs/engine/gameplay/grapple-pipeline.md`.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::{
        grappling_hook::{GrapplingHook, GrapplingHookState, GrapplingHookWire, WireState},
        wire_end::{WireEnd, WireEndState},
    },
};
use parking_lot::Mutex;

use crate::controllers::physics;

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct BodyHit {
    pub body: u32,
    pub owner: NonZeroUsize,
    pub local_point: Vec3,
}

impl BodyHit {
    pub(crate) unsafe fn at(body: u32, owner: NonZeroUsize, world: Vec3) -> Option<Self> {
        let state = unsafe { physics::body_state(body, owner) }.ok()?;
        let local_point = state.pose.local(world);
        local_point.is_finite().then_some(Self {
            body,
            owner,
            local_point,
        })
    }

    pub(crate) unsafe fn point(self) -> Option<Vec3> {
        let state = unsafe { physics::body_state(self.body, self.owner) }.ok()?;
        let point = state.pose.point(self.local_point);
        point.is_finite().then_some(point)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CustomOwner {
    Seat,
    Snatch,
}

pub(crate) unsafe fn claim_custom(owner: CustomOwner) -> bool {
    let Some(hook) = (unsafe { local_hook() }) else {
        return false;
    };
    let mut claim = CUSTOM.lock();
    if claim.is_some_and(|claim| {
        claim.wire.is_some_and(|wire| {
            hook.m_ActiveWire.px as usize != wire.get()
                && hook.m_State == GrapplingHookState::GHS_INACTIVE
                && !hook.m_ActiveWire.exists()
                && !hook.m_AttachmentWire.exists()
                && !firing(hook)
        })
    }) {
        *claim = None;
    }
    if claim.is_some()
        || hook.m_State != GrapplingHookState::GHS_INACTIVE
        || hook.m_ActiveWire.exists()
        || hook.m_AttachmentWire.exists()
        || firing(hook)
    {
        return false;
    }
    *claim = Some(CustomClaim { owner, wire: None });
    true
}

pub(crate) fn own_custom(owner: CustomOwner, wire: NonZeroUsize) {
    let mut claim = CUSTOM.lock();
    if let Some(claim) = claim.as_mut().filter(|claim| claim.owner == owner) {
        claim.wire = Some(wire);
    }
}

pub(crate) fn owns_custom(owner: CustomOwner, wire: NonZeroUsize) -> bool {
    CUSTOM
        .lock()
        .is_some_and(|claim| claim.owner == owner && claim.wire == Some(wire))
}

pub(crate) fn release_custom(owner: CustomOwner) {
    let mut claim = CUSTOM.lock();
    if claim.is_some_and(|claim| claim.owner == owner) {
        *claim = None;
    }
}

#[derive(Copy, Clone)]
struct CustomClaim {
    owner: CustomOwner,
    wire: Option<NonZeroUsize>,
}

static CUSTOM: Mutex<Option<CustomClaim>> = Mutex::new(None);

/// The local player's character and grappling hook.
///
/// # Safety
///
/// The game thread, or a thread that reads the hook's plain fields where the game does not free it.
pub(crate) unsafe fn local() -> Option<(&'static mut Character, &'static mut GrapplingHook)> {
    let character = unsafe { Character::GetLocalPlayerCharacter().as_mut() }?;
    let hook = unsafe { character.m_Inventory.m_GrapplingHook.px.as_mut() }?;
    Some((character, hook))
}

/// The local player's grappling hook.
///
/// # Safety
///
/// As for [`local`].
pub(crate) unsafe fn local_hook() -> Option<&'static mut GrapplingHook> {
    unsafe { local() }.map(|(_, hook)| hook)
}

/// `character`'s grappling hook.
///
/// # Safety
///
/// `character` must be live, and its hook not freed while the borrow lasts.
pub unsafe fn of(character: &Character) -> Option<&GrapplingHook> {
    unsafe { character.m_Inventory.m_GrapplingHook.px.as_ref() }
}

/// `character`'s grappling hook, to act on.
///
/// # Safety
///
/// As for [`of`].
pub(crate) unsafe fn of_mut(character: &mut Character) -> Option<&mut GrapplingHook> {
    unsafe { character.m_Inventory.m_GrapplingHook.px.as_mut() }
}

/// What every reader of the hook's phase reads of it: plain fields, with no call into the game, so
/// that it may be read on the render thread too.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HookRead {
    pub state: GrapplingHookState,
    /// Whether the hook has an active wire.
    pub active_wire: bool,
    /// Whether a fire is committed and waits on the fire animation's release ([`firing`]).
    pub firing: bool,
}

impl HookRead {
    pub(crate) fn of(hook: &GrapplingHook) -> Self {
        Self {
            state: hook.m_State,
            active_wire: hook.m_ActiveWire.exists(),
            firing: firing(hook),
        }
    }
}

/// Whether a fire of `hook`, the grapple's or a tether's, is committed and waits on the fire
/// animation's release.
pub(crate) fn firing(hook: &GrapplingHook) -> bool {
    hook.m_WaitingForGrappleFire || hook.m_WaitingForTetherFire
}

/// `wire`'s hook end, which flies to and holds the target.
pub fn hook_end(wire: &GrapplingHookWire) -> &WireEnd {
    &wire.m_WireEnds[HOOK_END]
}

/// `wire`'s hook end, to act on.
pub(crate) fn hook_end_mut(wire: &mut GrapplingHookWire) -> &mut WireEnd {
    &mut wire.m_WireEnds[HOOK_END]
}

/// `wire`'s device end, on the gauntlet.
pub fn device_end(wire: &GrapplingHookWire) -> &WireEnd {
    &wire.m_WireEnds[DEVICE_END]
}

pub(crate) fn device_end_mut(wire: &mut GrapplingHookWire) -> &mut WireEnd {
    &mut wire.m_WireEnds[DEVICE_END]
}

/// `wire`'s hook end, while the wire is valid and that end is attached.
pub(crate) fn attached_hook_end(wire: &GrapplingHookWire) -> Option<&WireEnd> {
    let end = hook_end(wire);
    (wire.m_WireState != WireState::GHWS_INVALID
        && end.m_WireEndState == WireEndState::GWES_ATTACHED)
        .then_some(end)
}

/// The index of a wire's hook end, and of its device end.
const HOOK_END: usize = 0;
const DEVICE_END: usize = 1;
