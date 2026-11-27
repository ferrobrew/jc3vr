//! The volumetric fog generation pass, run once per eye under the single-pass collapse.
//!
//! In its generate pass, [`RenderBlockFogVolume::Draw`] draws one viewport-covering triangle into the
//! block type's own half-resolution fog target. Each pixel samples depth at its texture UV,
//! reconstructs a world position through a UV-to-world matrix the block builds from
//! `Inverse(RenderContext::m_ViewProjectionF)`, and ray-marches the sun shadow cascade and cloud
//! shadow map from the camera towards it. Under the collapse that target is double-wide and the render
//! context holds the centred camera, so one centred basis reconstructs both halves: the left half gets
//! a frustum compressed into half the width and the right half one unrelated to it. Like the other
//! reconstruction passes, the error turns with the camera. It is worst towards the sun, where the
//! in-scattering the march accumulates is strongest.
//!
//! The block is not a `PerspectiveFovInverse` consumer, so the substituted inverse the other splits
//! rely on never reaches it. Instead each masked run hands the render context that eye's
//! view-projection for the length of the call, with the full-target-NDC to eye-NDC remap folded in, so
//! the inverse the block takes and uploads reconstructs that eye's frustum over that eye's half. The
//! depth sample is untouched and keeps reading the double-wide UV. The camera position the pixel
//! shader marches from is the fragment `GlobalConstants` row the shared per-eye staging already moves
//! to the eye while a run is masked. The render context's render-camera position, whose height feeds
//! the cloud-shadow blend, is moved to the eye too, and everything is put back after the call.
//!
//! The mask is armed at block entry ([`reconstruction::SplitPolicy::at_entry`]) because the block
//! binds its own reduced-resolution render setup before it draws, and only an at-entry mask follows
//! that bind. The pass issues no dispatches and keeps no temporal state: it blends additively into a
//! target cleared ahead of it, so two disjoint masked runs write exactly what one whole-target run
//! would. The upsample pass that follows reconstructs nothing (it only compares depths at matching
//! UVs) and is left alone.

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        render_block::{RBIInfo, RenderBlockFogVolume},
        render_engine::RenderPassId,
    },
    types::math::{Matrix4, Vector3},
};
use jc3vr_view::stereo::collapse_eye;
use re_utilities::hook_library::HookLibrary;

use crate::{
    hooks::graphics_engine::{
        reconstruction,
        reconstruction::{DispatchPhase, SplitOutcome, SplitPolicy, half_target_view_projection},
    },
    stereo::single_pass::full_eye_view_projection,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&FOG_VOLUME_DRAW_BINDER)
}

#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockFogVolume::Draw_ADDRESS)]
fn fog_volume_draw(
    this: *const RenderBlockFogVolume,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let call = || FOG_VOLUME_DRAW.get().unwrap().call(this, rc, info);
    // SAFETY: `rc` is the live render context for this dispatch; the engine's draw dispatch guarantees
    // it is valid for the duration of `Draw`.
    let Some(context) = (unsafe { rc.as_ref() }) else {
        return call();
    };
    if context.m_ActiveRenderPass != RenderPassId::RP_FOG_VOLUME_GENERATE as i32 {
        return call();
    }
    let ctx = Some(context.m_Context);
    let outcome = reconstruction::split_fullscreen_pass_policy(
        SplitPolicy::at_entry(DispatchPhase::Both),
        true,
        ctx,
        |eye| {
            // Only a masked run draws one eye's half; the degenerate unmasked second run covers the
            // whole target and keeps the engine's own basis.
            // SAFETY: as above; nothing else touches the render context during this call.
            let saved = (reconstruction::masked_eye() == Some(eye))
                .then(|| unsafe { rc.as_mut() }.and_then(|rc| stage_eye_basis(rc, eye)))
                .flatten();
            call();
            if let Some(saved) = saved {
                // SAFETY: as above.
                if let Some(rc) = unsafe { rc.as_mut() } {
                    saved.restore(rc);
                }
            }
        },
    );
    if outcome == SplitOutcome::NotTaken {
        call();
    }
}

/// The render-context fields [`stage_eye_basis`] replaced, as the engine wrote them.
struct SavedBasis {
    view_projection_f: Matrix4,
    render_camera_position: Vector3,
}

impl SavedBasis {
    fn restore(self, rc: &mut RenderContext) {
        rc.m_ViewProjectionF = self.view_projection_f;
        rc.m_RenderCameraPosition = self.render_camera_position;
    }
}

/// Give `rc` `eye`'s view-projection, mapped onto that eye's half of the bound target, and that eye's
/// render-camera position. `None`, with nothing changed, when the eye's pose or projection is not
/// available.
fn stage_eye_basis(rc: &mut RenderContext, eye: usize) -> Option<SavedBasis> {
    let view_projection = glam::Mat4::from(full_eye_view_projection(eye)?);
    let offset = collapse_eye(eye)?.offset();
    let staged = half_target_view_projection(eye, view_projection);
    if !staged.is_finite() || !offset.is_finite() {
        return None;
    }
    let saved = SavedBasis {
        view_projection_f: rc.m_ViewProjectionF,
        render_camera_position: rc.m_RenderCameraPosition,
    };
    rc.m_ViewProjectionF = Matrix4::from(staged);
    rc.m_RenderCameraPosition = Vector3::from(glam::Vec3::from(rc.m_RenderCameraPosition) + offset);
    if !ENGAGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "per-eye fog volume engaged: fog generation now reconstructs each half of its target \
             with that eye's view-projection",
        );
    }
    Some(saved)
}

/// One-shot latch for the engagement log line above.
static ENGAGED: AtomicBool = AtomicBool::new(false);
