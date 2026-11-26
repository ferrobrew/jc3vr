//! Keep native below-vehicle attachment from snapping a current physical handhold onto a roof, and
//! tell the subsystems that hold Havok bodies when the world destroys some.
//!
//! The subsystems register what they need at installation: the refusals of the automatic entry
//! into a vehicle's stunt area below the player ([`register_auto_attach_blocker`]), asked in
//! [`AutoAttachBlocker`] order, and the observer of a body destruction
//! ([`register_bodies_destroying`]), called before the world destroys them.

use std::sync::OnceLock;

use detours_macro::detour;
use jc3gi::{character::character::Character, physics::hknpWorld};
use re_utilities::hook_library::HookLibrary;

/// The refusals of the automatic entry into a vehicle's stunt area below the player, in the order
/// they are asked.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AutoAttachBlocker {
    /// A physical handhold the player climbs by.
    PhysicalClimb,
    /// The vehicle rail holding the player.
    VehicleRail,
}

impl AutoAttachBlocker {
    /// Every blocker, in the order they are asked.
    pub const ALL: [AutoAttachBlocker; Self::COUNT] =
        [AutoAttachBlocker::PhysicalClimb, AutoAttachBlocker::VehicleRail];

    const COUNT: usize = 2;

    const fn index(self) -> usize {
        match self {
            AutoAttachBlocker::PhysicalClimb => 0,
            AutoAttachBlocker::VehicleRail => 1,
        }
    }
}

/// A refusal of the automatic attachment ([`register_auto_attach_blocker`]): whether the attachment
/// of the character is refused.
pub type BlocksAutoAttach = unsafe fn(*const Character) -> bool;

/// Register `blocker`'s refusal: called with the character the engine is about to attach, it
/// returns whether the attachment is refused. One per blocker: a second registration is refused
/// and logged.
///
/// The refusal is called from the detour on the attachment, while the native caller keeps the
/// character and its proxy member alive; it may read only members of that character.
pub fn register_auto_attach_blocker(blocker: AutoAttachBlocker, blocks: BlocksAutoAttach) {
    if BLOCKERS[blocker.index()].set(blocks).is_err() {
        tracing::error!(
            ?blocker,
            "character: an auto-attach blocker is already registered; keeping the first"
        );
    }
}

/// Register the observer of a body destruction: called with the world and the bodies' ids before
/// the world destroys them. One observer: a second registration is refused and logged.
///
/// The observer runs on the thread destroying the bodies, inside the world's destruction call;
/// `ids` points at `count` ids, or is null or `count` is not positive.
pub fn register_bodies_destroying(observer: unsafe fn(*mut hknpWorld, *const u32, i32)) {
    if BODIES_DESTROYING.set(observer).is_err() {
        tracing::error!(
            "character: a body destruction observer is already registered; keeping the first"
        );
    }
}

/// Every blocker still unregistered, in the order they are asked.
pub fn missing_auto_attach_blockers() -> impl Iterator<Item = AutoAttachBlocker> {
    AutoAttachBlocker::ALL
        .into_iter()
        .filter(|blocker| BLOCKERS[blocker.index()].get().is_none())
}

/// Whether the observer of a body destruction is registered.
pub fn bodies_destroying_registered() -> bool {
    BODIES_DESTROYING.get().is_some()
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&AUTO_ATTACH_BELOW_BINDER)
        .with_static_binder(&DESTROY_BODIES_BINDER)
}

static BLOCKERS: [OnceLock<BlocksAutoAttach>; AutoAttachBlocker::COUNT] =
    [const { OnceLock::new() }; AutoAttachBlocker::COUNT];

static BODIES_DESTROYING: OnceLock<unsafe fn(*mut hknpWorld, *const u32, i32)> = OnceLock::new();

#[detour(address = jc3gi::character::character::Character::AutoAttachToStuntAreaOnVehicleBelow_ADDRESS)]
fn auto_attach_below(character: *mut Character) -> bool {
    // The engine's ground-body support also advertises a vehicle below a physical grip. Only
    // this automatic entry is suppressed; ordinary grapple hang and explicit interactions remain native.
    let blocked = BLOCKERS
        .iter()
        .filter_map(OnceLock::get)
        .any(|blocks| unsafe { blocks(character) });
    if blocked {
        return false;
    }
    AUTO_ATTACH_BELOW.get().unwrap().call(character)
}

#[detour(address = jc3gi::physics::hknpWorld::destroyBodies_ADDRESS)]
fn destroy_bodies(world: *mut hknpWorld, ids: *const u32, count: i32, activation_mode: i32) {
    if let Some(observer) = BODIES_DESTROYING.get() {
        unsafe { observer(world, ids, count) };
    }
    DESTROY_BODIES
        .get()
        .unwrap()
        .call(world, ids, count, activation_mode);
}
