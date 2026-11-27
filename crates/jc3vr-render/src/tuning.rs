//! The render layer's tuning, read from the shipped `tuning/render.toml` ([`RENDER`]): the scope
//! lens's source staleness, the temporal resolve's clipping and rejection, the camera-cut test FSR
//! and the temporal resolve reset on, the cull camera's size-cull field of view, and the spawn and
//! terrain detail budgets' scales.

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The render layer's tuning file.
    pub static RENDER: RenderTuning = "render.toml";
}

/// The render layer's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderTuning {
    pub(crate) scope_lens: ScopeLensTuning,
    pub taa: TaaTuning,
    pub(crate) camera_cut: CameraCutTuning,
    pub culling: CullingTuning,
    pub terrain: TerrainTuning,
}

impl Tuning for RenderTuning {
    fn validate(&self) -> Result<(), Invalid> {
        if !(1..=1000).contains(&self.scope_lens.max_copy_age_ms) {
            return Err(Invalid(format!(
                "scope_lens.max_copy_age_ms must be in [1, 1000], got {}",
                self.scope_lens.max_copy_age_ms
            )));
        }
        positive("taa.clip_gamma", self.taa.clip_gamma, 8.0)?;
        range("taa.depth_tolerance", self.taa.depth_tolerance, 0.0, 1.0)?;
        positive("camera_cut.ndc", self.camera_cut.ndc, 2.0)?;
        positive(
            "camera_cut.probe_depth_m",
            self.camera_cut.probe_depth_m,
            1000.0,
        )?;
        range(
            "culling.size_cull_fov_deg",
            self.culling.size_cull_fov_deg,
            1.0,
            179.0,
        )?;
        range(
            "culling.spawn_budget_scale",
            self.culling.spawn_budget_scale,
            1.0,
            8.0,
        )?;
        range(
            "terrain.detail_budget_scale",
            self.terrain.detail_budget_scale as f32,
            1.0,
            16.0,
        )
    }
}

/// The scope lens's source images (see [`crate::controllers::weapons::scope`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeLensTuning {
    /// How old an eye's copy may be and still be read, milliseconds: a frame or two at any usable
    /// frame rate. The other eye's copy is the previous frame's when the first eye sights in
    /// two-pass stereo.
    pub(crate) max_copy_age_ms: u64,
}

/// The temporal resolve's clipping and rejection (see [`crate::fsr::taa`]), short of the weights
/// the settings hold.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaaTuning {
    /// The half-size, in standard deviations, of the current neighbourhood's colour box the history
    /// is clipped to while the pixel moves.
    pub(crate) clip_gamma: f32,
    /// The relative slack of the depth test that rejects a disoccluded history.
    pub(crate) depth_tolerance: f32,
}

/// When FSR and the temporal resolve call a frame a camera cut and drop their history (see
/// [`crate::fsr::camera_cut`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraCutTuning {
    /// How far, in NDC, the view centre may move between frames before it is a cut.
    pub ndc: f32,
    /// The view depth of the near probe, metres.
    pub(crate) probe_depth_m: f32,
}

/// The visibility and spawn corrections for the headset's wider view (see
/// [`crate::hooks::graphics_engine::scene::culling`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CullingTuning {
    /// The field of view the scene size-cull uses in place of the injected one on the main cull
    /// camera, degrees. The engine's screen-space size cull drops an object whose angular size
    /// falls below a threshold that scales with `tan(FOV/2)`, so the injected camera's wider field
    /// drops small and distant geometry and vehicle sub-meshes that flat JC3 draws; this restores
    /// flat-equivalent density.
    pub(crate) size_cull_fov_deg: f32,
    /// The factor every spawn budget is scaled by, so the wider field of view does not despawn the
    /// vehicles and characters in front of the player.
    pub(crate) spawn_budget_scale: f32,
}

/// The terrain's detail budgets (see [`crate::hooks::graphics_engine::scene::terrain`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainTuning {
    /// The multiplier on the shipped detail-budget buffer sizes, enough for the headset's field of
    /// view.
    pub(crate) detail_budget_scale: u32,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(RENDER.get().validate(), Ok(()));
    }
}
