//! The local player's draw inputs the character render blocks read, published by the character
//! hook on the game thread and loaded by the draws on the render worker threads: the `CRBIInfo`
//! pointers of the player's model instances (the ownership test for the head hide and the body's
//! palette edit), the skeleton indices of the bones the head hide collapses, and the point they
//! collapse to.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use jc3gi::character::character::AnimatedModel;

/// Publish the `CRBIInfo` pointers of the local player's model instances, slot-for-slot (zero for
/// empty slots), from the character hook. Every render-block draw receives its owning model
/// instance's embedded `CRBIInfo` as the `info` argument, so pointer identity is an *exact*
/// ownership test — unlike the earlier position-radius check, which failed at vehicle/wingsuit
/// speeds and false-matched nearby characters. The slots are kept distinct because they are
/// different models with different skeletons: the parachute occupies one of them, and collapsing
/// the body skeleton's bone indices on the chute's skeleton crumpled the canopy.
pub fn publish_player_rbi_infos(infos: &[usize; PLAYER_MODEL_SLOTS]) {
    for (slot, &info) in PLAYER_RBI_INFOS.iter().zip(infos) {
        slot.store(info, Ordering::Relaxed);
    }
}

/// The number of model-instance slots on `CAnimatedModel`, as a `usize` for array sizing.
pub const PLAYER_MODEL_SLOTS: usize = AnimatedModel::MODEL_INSTANCE_SLOTS as usize;

/// The largest skeleton bone index `publish_collapse_bones` accepts. Skeleton indices beyond an
/// `i16`'s range are implausible (the engine's bone index space is i16-sized), so they are dropped
/// rather than risking a whole-body collapse.
const MAX_BONE_INDEX: u32 = 0x7FFF;

/// Publish the NECK bone's model-space position, from the character hook: the collapse target
/// point. Collapsed vertices contract here (inside the collar) rather than to each bone's own
/// position — vertices blended between a collapsed and a kept bone form a stretch cone to the
/// collapse point, and anchoring that point at the neck keeps the cone tucked into the collar
/// instead of dangling off the chin (the beard flap). The point cannot be sampled from the
/// palette itself: where the translation lives in each matrix depends on the block's palette
/// layout, and reading the wrong slots yields a pseudo-point rather than a position (observed
/// in-game as collapsed clusters parked ~0.6 m under the head, with blended vertices streaking
/// down toward them). Non-finite positions are dropped, leaving the previous target in place.
pub fn publish_collapse_target(position: glam::Vec3) {
    if !position.is_finite() {
        return;
    }
    for (slot, value) in COLLAPSE_TARGET.iter().zip(position.to_array()) {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
    COLLAPSE_TARGET_VALID.store(true, Ordering::Relaxed);
}

/// Publish the skeleton indices of the bones to collapse (HEAD plus the facial set), from the
/// character hook (the game thread owns the skeleton; the draws run on the render thread and only
/// load these). Index 0 (the root — also what a failed name lookup can resolve to) and
/// implausibly large values are dropped rather than risking a whole-body collapse.
pub fn publish_collapse_bones(indices: &[u32]) {
    let mut len = 0;
    for &index in indices {
        if index == 0 || index > MAX_BONE_INDEX || len >= COLLAPSE_BONES.len() {
            continue;
        }
        COLLAPSE_BONES[len].store(index, Ordering::Relaxed);
        len += 1;
    }
    COLLAPSE_BONE_LEN.store(len, Ordering::Relaxed);
}

/// The published `CRBIInfo` pointer of the local player's body model, or zero when none is.
#[inline]
pub fn player_body_info() -> usize {
    PLAYER_RBI_INFOS[AnimatedModel::BODY_MODEL_SLOT as usize].load(Ordering::Relaxed)
}

/// The published skeleton indices of the bones to collapse.
#[inline]
pub fn collapse_bones() -> &'static [AtomicU32] {
    &COLLAPSE_BONES[..COLLAPSE_BONE_LEN.load(Ordering::Relaxed)]
}

/// Whether the collapse target has been published at least once.
#[inline]
pub fn collapse_target_published() -> bool {
    COLLAPSE_TARGET_VALID.load(Ordering::Relaxed)
}

/// The published collapse target: the NECK bone's model-space position (see
/// [`publish_collapse_target`]).
pub fn collapse_target() -> glam::Vec3 {
    let [x, y, z] = &COLLAPSE_TARGET;
    glam::Vec3::new(
        f32::from_bits(x.load(Ordering::Relaxed)),
        f32::from_bits(y.load(Ordering::Relaxed)),
        f32::from_bits(z.load(Ordering::Relaxed)),
    )
}

/// The skeleton indices of the bones to collapse; the first [`COLLAPSE_BONE_LEN`] entries are
/// valid.
static COLLAPSE_BONES: [AtomicU32; 16] = [const { AtomicU32::new(0) }; 16];
static COLLAPSE_BONE_LEN: AtomicUsize = AtomicUsize::new(0);

/// The NECK bone's model-space position as `f32` bits (see [`publish_collapse_target`]), plus a
/// flag for "published at least once".
static COLLAPSE_TARGET: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static COLLAPSE_TARGET_VALID: AtomicBool = AtomicBool::new(false);

/// The `CRBIInfo` pointers of the local player's model instances, slot-for-slot (zero = empty).
static PLAYER_RBI_INFOS: [AtomicUsize; PLAYER_MODEL_SLOTS] =
    [const { AtomicUsize::new(0) }; PLAYER_MODEL_SLOTS];
