//! The terrain patch system's per-frame prep, from the centre camera in every stereo dispatch.
//!
//! The patch system's prep passes (`RP_TERRAINPATCH_CLEAR` through `RP_TERRAINPATCH_ENUMERATION`)
//! rebuild the patches' detail, histogram, triangles, and morph targets every frame on the GPU from
//! the camera position the vertex globals carry, which for those passes is the render camera's. The
//! morph targets in particular blend each patch's interior vertices between levels of detail by their
//! distance from that camera, and the patch's border stays fixed so neighbours meet. A dispatch that
//! renders one eye at a time therefore morphed each eye's terrain from that eye's own position, while
//! the single-pass collapse, which has one centred render camera, morphs both eyes from the centre.
//! The two eyes of a multipass frame disagreed with each other about where the ground's surface lies,
//! and an isolated eye disagreed with the same eye under single-pass by up to a few hundred floats of
//! depth across each patch's interior.
//!
//! Like the level-of-detail and tessellation choices the rest of the engine makes from the centred
//! active camera, the prep is a choice both eyes should share: with `stereo.terrain_prep_from_centre`
//! on, an eye-at-a-time dispatch stages the centre camera's position for those passes, and puts the
//! eye's back for the rest of the pass list.

use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        render_engine::{RenderEngine, RenderPassId},
    },
    types::math::Vector4,
};

use crate::{stereo, stereo::config::STEREO_CONFIG};

/// Stage the centre camera's position into the vertex globals for a terrain patch prep pass in an
/// eye-at-a-time stereo dispatch. Returns the eye's staged row for [`restore`] when it changed
/// anything, after which the caller uploads the globals.
pub(crate) fn stage_centre_camera(
    engine: &mut RenderEngine,
    rc: &RenderContext,
) -> Option<Vector4> {
    if !PREP_PASSES.contains(&rc.m_ActiveRenderPass)
        || !stereo::active()
        || stereo::layout::collapse_active()
        || !STEREO_CONFIG.query(|c| c.terrain_prep_from_centre)
    {
        return None;
    }
    let center = stereo::STEREO_STATE.lock().center_transform?;
    let eye = engine.m_VPGlobalConstData[CAMERA_POSITION_ROW];
    let mut staged = eye;
    staged.data[..3].copy_from_slice(&center.data[12..15]);
    if staged.data == eye.data {
        return None;
    }
    engine.m_VPGlobalConstData[CAMERA_POSITION_ROW] = staged;
    Some(eye)
}

/// Put back the row [`stage_centre_camera`] replaced, once the centre's has been uploaded.
pub(crate) fn restore(engine: &mut RenderEngine, eye: Vector4) {
    engine.m_VPGlobalConstData[CAMERA_POSITION_ROW] = eye;
}

/// The terrain patch prep passes, as the render context's active pass.
const PREP_PASSES: std::ops::RangeInclusive<i32> =
    RenderPassId::RP_TERRAINPATCH_CLEAR as i32..=RenderPassId::RP_TERRAINPATCH_ENUMERATION as i32;

/// The vertex globals row holding the camera's world position, which the prep's compute shaders read.
const CAMERA_POSITION_ROW: usize = 4;
