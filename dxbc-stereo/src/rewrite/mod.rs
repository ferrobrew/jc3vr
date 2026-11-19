//! The single-pass stereo rewrite layer.
//!
//! Rewrites DXBC shader bytecode so one instance-doubled draw renders both eyes. Four functional
//! areas each target a different shader family: `remap` retargets an opaque model VS's per-eye `cb0`
//! rows to a mod-owned `cb13`, `reproject` post-multiplies a baked-WVP VS's clip position by a
//! per-eye `M_eye`, and `terrain` handles the tessellation VS and DS ends of the terrain path.
//! `pinned` is the reprojection for a vertex or domain shader that cannot learn its eye and is drawn
//! once per eye instead, reading whichever eye the payload pinned `cb13` to. The shared DXBC
//! scaffolding lives in `common`.
//!
//! `ssdecal` is a pixel-shader rewrite: it separates the screen-space decal permutations'
//! depth-texture UV from the viewport-normalized UV their reconstruction basis needs, so the two can
//! address differently-sized spaces. `overlay` maps the final composite's screen-space overlays over
//! one eye of a double-wide target, and `planar` does the same for the WaveWorks water's planar
//! reflection lookup. `ambient` runs the ambient-occlusion generation pass over each eye's half of a
//! double-wide target, and `seam` keeps the post chain's screen-mapped filters and the composite's
//! bloom lookups inside each eye's half, along with the screen-space reflection blur's taps.
//! `reflection` traces the screen-space reflections over each eye's half of a double-wide target.
//! `camera` gives every pixel shader that reads the camera position the position of the eye whose
//! half of a double-wide target the pixel lies in.

mod ambient;
pub use ambient::{
    AMBIENT_OCCLUSION_CB_REGISTER, AMBIENT_OCCLUSION_ROWS_PER_EYE, localize_ambient_occlusion,
    localize_ambient_occlusion_filter,
};
mod camera;
pub use camera::{
    EYE_CAMERA_BOUNDARY_ROW, EYE_CAMERA_CB_REGISTER, EYE_CAMERA_CB_ROWS, EYE_CAMERA_ROWS_PER_EYE,
    EYE_CAMERA_SOURCE_ROWS, localize_eye_camera,
};
mod common;
mod lake;
pub use lake::{
    bound_lake_candidate_depth, capture_lake_planar_uv, isolate_lake_reflection,
    mirror_lake_color_overflow, mirror_lake_reflection, offset_lake_reflection,
    reproject_lake_reflection, scale_lake_refraction, trace_lake_displacement,
    trace_lake_gradients, trace_lake_inner_edge, trace_lake_lighting, trace_lake_normals,
    trace_lake_response, trace_lake_samples, trace_lake_waves,
};
mod overlay;
pub use overlay::localize_screen_overlays;
mod pinned;
pub use pinned::reproject_pinned_eye;
mod planar;
pub use planar::localize_planar_reflection;
mod resolve;
pub use resolve::{
    impostor_atlas_scale, local_aa_neighbor, local_bark_dither, local_dof_edge,
    local_fog_reconstruction, physical_impostor_fade, trace_deferred_samples, trace_ssr_ray,
};
mod reflection;
pub use reflection::{
    SCREEN_SPACE_REFLECTION_CB_REGISTER, SCREEN_SPACE_REFLECTION_ROWS_PER_EYE,
    ScreenSpaceReflectionRow, localize_screen_space_reflection,
};
mod remap;
mod reproject;
mod seam;
pub use seam::{localize_composite_bloom, localize_post_filter, localize_reflection_blur};
mod ssdecal;
mod terrain;

pub use common::{MEYE_ROW_BASE, STEREO_CB_REGISTER, STEREO_CB_ROWS, STEREO_REPROJ_CB_ROWS};
pub use remap::patch_vertex_shader;
pub use reproject::reproject_vertex_shader;
pub use ssdecal::{SSDECAL_EYE_BIAS_REGISTER, bias_ssdecal_depth_uv};
pub use terrain::{
    forward_eye_hull_shader, inject_eye_forward_vertex_shader, reproject_domain_shader,
};
