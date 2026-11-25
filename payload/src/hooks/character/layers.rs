//! The animation-layer mask behind the weapon-agnostic body
//! ([`crate::controllers::body::unarmed`]): the pose job leaves the local player's
//! masked layers out of the pose while they keep running.
//!
//! The pose job blends each layer's blender over the previous layer's output
//! ([`Blender::UpdatePose`]); a later layer's call writes nothing but its outputs, and the job skips
//! a blender without samplers, leaving the pose below it unchanged. The detour does the same for a
//! masked blender: it copies the reference pose and floats through. The layer's state machine,
//! clips, animation events, and segments are untouched, because none of them read the blended pose.
//!
//! The pose job runs on worker threads, so the masked blenders are published as plain atomics by
//! the pose-finalization pass ([`publish`]) and expire if it stops publishing ([`MASK_FRESH`]),
//! which bounds how long a freed blender's address could be honoured.

use std::{
    ptr,
    sync::{
        LazyLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::animation::pose_producer::{Blender, QsTransform};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::body::unarmed::MASKABLE_LAYERS;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&BLENDER_UPDATE_POSE_BINDER)
}

/// Publish the blenders to leave out of the pose, in [`MASKABLE_LAYERS`] order, zero for none.
pub(crate) fn publish(blenders: [usize; MASKABLE_LAYERS.len()]) {
    for (slot, blender) in MASKED.iter().zip(blenders) {
        slot.store(blender, Ordering::Relaxed);
    }
    PUBLISHED_AT_MS.store(now_ms(), Ordering::Release);
}

/// How many times each of [`MASKABLE_LAYERS`] was left out of the pose, since injection.
pub(crate) fn skipped() -> [u64; MASKABLE_LAYERS.len()] {
    std::array::from_fn(|i| SKIPPED[i].load(Ordering::Relaxed))
}

// The signature mirrors the engine's, so its arguments cannot be bundled.
#[allow(clippy::too_many_arguments)]
#[detour(address = jc3gi::animation::pose_producer::Blender::UpdatePose_ADDRESS)]
fn blender_update_pose(
    blender: *mut Blender,
    pose_out: *mut QsTransform,
    reference_pose: *mut QsTransform,
    normal_blend_pose_out: *mut QsTransform,
    num_bones: u32,
    float_slots_out: *mut f32,
    reference_floats: *mut f32,
    num_float_slots: u32,
) {
    // The first layer (the only one with a pre-additive output) is never masked, and the
    // reference must be a separate buffer for the copy to be the pass-through the job would make.
    let slot = (normal_blend_pose_out.is_null()
        && !reference_pose.is_null()
        && !ptr::eq(reference_pose, pose_out))
    .then(|| masked_slot(blender))
    .flatten();
    if let Some(slot) = slot {
        // SAFETY: the pose job passes `num_bones` transforms and `num_float_slots` floats in each
        // buffer, and the output and reference buffers are distinct allocations.
        unsafe {
            ptr::copy_nonoverlapping(reference_pose, pose_out, num_bones as usize);
            if !float_slots_out.is_null()
                && !reference_floats.is_null()
                && !ptr::eq(float_slots_out, reference_floats)
            {
                ptr::copy_nonoverlapping(
                    reference_floats,
                    float_slots_out,
                    num_float_slots as usize,
                );
            }
        }
        SKIPPED[slot].fetch_add(1, Ordering::Relaxed);
        return;
    }
    BLENDER_UPDATE_POSE.get().unwrap().call(
        blender,
        pose_out,
        reference_pose,
        normal_blend_pose_out,
        num_bones,
        float_slots_out,
        reference_floats,
        num_float_slots,
    );
}

/// How long a publication holds. The pose-finalization pass republishes every tick.
const MASK_FRESH: Duration = Duration::from_millis(250);

static MASKED: [AtomicUsize; MASKABLE_LAYERS.len()] =
    [const { AtomicUsize::new(0) }; MASKABLE_LAYERS.len()];
static SKIPPED: [AtomicU64; MASKABLE_LAYERS.len()] =
    [const { AtomicU64::new(0) }; MASKABLE_LAYERS.len()];
static PUBLISHED_AT_MS: AtomicU64 = AtomicU64::new(0);
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The slot `blender` is published in, while the publication is fresh.
fn masked_slot(blender: *mut Blender) -> Option<usize> {
    let address = blender as usize;
    if address == 0 {
        return None;
    }
    let slot = MASKED
        .iter()
        .position(|masked| masked.load(Ordering::Relaxed) == address)?;
    let age_ms = now_ms().saturating_sub(PUBLISHED_AT_MS.load(Ordering::Acquire));
    (age_ms <= MASK_FRESH.as_millis() as u64).then_some(slot)
}

fn now_ms() -> u64 {
    EPOCH.elapsed().as_millis() as u64
}
