//! The capture-only shader rewrites the harness's experiments request.
//!
//! The lake traces and planar lookups and the resolve, reflection-ray, impostor, bark, fog,
//! depth-of-field, and anti-aliasing trials rewrite a pristine game shader before the trial binds
//! it. Those rewrites sit behind `dxbc-stereo`'s `capture-rewrites` feature so the shipped payload
//! does not carry them. The harness builds the payload with the feature (through the payload's and
//! this crate's `capture-rewrites`). A build without it refuses a request whose options need a
//! rewrite when the request arrives ([`AVAILABLE`]), and each rewrite below also fails with an error
//! that names the missing feature, so an experiment reports the build rather than a wrong image.

/// Whether this build carries the capture-only rewrites.
pub(crate) const AVAILABLE: bool = cfg!(feature = "capture-rewrites");

macro_rules! capture_rewrites {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {$(
        #[cfg(feature = "capture-rewrites")]
        pub(crate) fn $name(blob: &[u8] $(, $arg: $ty)*) -> anyhow::Result<Vec<u8>> {
            Ok(dxbc_stereo::$name(blob $(, $arg)*)?)
        }

        #[cfg(not(feature = "capture-rewrites"))]
        pub(crate) fn $name(_blob: &[u8] $(, $arg: $ty)*) -> anyhow::Result<Vec<u8>> {
            $(let _ = $arg;)*
            anyhow::bail!(
                "capture rewrites: {} needs a payload built with the capture-rewrites feature",
                stringify!($name)
            )
        }
    )*};
}

capture_rewrites! {
    trace_deferred_samples(shadow: bool);
    trace_ssr_ray(
        march_eye: Option<bool>,
        pack_veto: bool,
        physical_grid: bool,
        history: bool,
        center_history: bool
    );
    local_bark_dither(origin: f32, color: bool);
    physical_impostor_fade();
    impostor_atlas_scale(right_origin: f32, left: bool);
    local_fog_reconstruction();
    local_dof_edge();
    local_aa_neighbor();
    offset_lake_reflection(right: bool);
    reproject_lake_reflection(right: bool);
    mirror_lake_reflection(right: bool);
    isolate_lake_reflection(right: bool);
    capture_lake_planar_uv();
    trace_lake_samples();
    trace_lake_lighting();
    trace_lake_response();
    trace_lake_inner_edge();
    trace_lake_displacement();
    bound_lake_candidate_depth();
    mirror_lake_color_overflow();
    trace_lake_normals();
    trace_lake_waves();
    trace_lake_gradients();
}
