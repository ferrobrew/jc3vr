//! The split between malformed bytecode and a rewrite that does not apply.

use dxbc_stereo::{
    AmbientOcclusionSampling, Applicable, ClipOrigin, DxbcError, DxbcErrorKind, LocalLights,
};

mod common;
use common::shader_dir;

type Rewrite = fn(&[u8]) -> Result<Vec<u8>, DxbcError>;

/// The rewrites the payload runs at shader creation, each over one argument choice.
const SHIPPED: &[(&str, Rewrite)] = &[
    ("patch_vertex_shader", dxbc_stereo::patch_vertex_shader),
    ("patch_vertex_shader_with", |b| {
        dxbc_stereo::patch_vertex_shader_with(b, ClipOrigin::PerEye)
    }),
    (
        "reproject_vertex_shader",
        dxbc_stereo::reproject_vertex_shader,
    ),
    (
        "reproject_instanced_vertex_shader",
        dxbc_stereo::reproject_instanced_vertex_shader,
    ),
    (
        "remap_full_view_projection",
        dxbc_stereo::remap_full_view_projection,
    ),
    (
        "remap_pinned_full_view_projection",
        dxbc_stereo::remap_pinned_full_view_projection,
    ),
    ("reproject_pinned_eye", dxbc_stereo::reproject_pinned_eye),
    (
        "rebase_pinned_domain_shader",
        dxbc_stereo::rebase_pinned_domain_shader,
    ),
    (
        "inject_eye_forward_vertex_shader",
        dxbc_stereo::inject_eye_forward_vertex_shader,
    ),
    (
        "forward_eye_hull_shader",
        dxbc_stereo::forward_eye_hull_shader,
    ),
    ("rebase_domain_shader", dxbc_stereo::rebase_domain_shader),
    ("bias_ssdecal_depth_uv", dxbc_stereo::bias_ssdecal_depth_uv),
    (
        "localize_screen_overlays",
        dxbc_stereo::localize_screen_overlays,
    ),
    ("localize_planar_reflection", |b| {
        dxbc_stereo::localize_planar_reflection(b, true)
    }),
    ("scale_lake_refraction", dxbc_stereo::scale_lake_refraction),
    ("localize_ambient_occlusion", |b| {
        dxbc_stereo::localize_ambient_occlusion(b, AmbientOcclusionSampling::Exact)
    }),
    ("localize_ambient_occlusion_filter", |b| {
        dxbc_stereo::localize_ambient_occlusion_filter(b, true)
    }),
    (
        "snap_ambient_occlusion_uv",
        dxbc_stereo::snap_ambient_occlusion_uv,
    ),
    (
        "localize_ambient_occlusion_volume",
        dxbc_stereo::localize_ambient_occlusion_volume,
    ),
    (
        "localize_screen_space_reflection",
        dxbc_stereo::localize_screen_space_reflection,
    ),
    (
        "localize_reflection_blur",
        dxbc_stereo::localize_reflection_blur,
    ),
    ("localize_post_filter", dxbc_stereo::localize_post_filter),
    (
        "localize_composite_bloom",
        dxbc_stereo::localize_composite_bloom,
    ),
    ("localize_eye_camera", |b| {
        dxbc_stereo::localize_eye_camera(b, LocalLights::PerEye)
    }),
    ("localize_screen_door", dxbc_stereo::localize_screen_door),
    (
        "localize_projected_screen_door",
        dxbc_stereo::localize_projected_screen_door,
    ),
    ("localize_velocity", dxbc_stereo::localize_velocity),
];

/// The game's shaders are all well formed, so a rewrite that fails on one declines it: a malformed
/// verdict on a shader the runtime compiled would make the payload report a sound shader as broken.
#[test]
fn corpus_rewrites_only_decline_the_games_shaders() {
    let Some(dir) = shader_dir() else {
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("read the shader directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    paths.sort();
    let mut malformed = Vec::new();
    for path in &paths {
        let blob = std::fs::read(path).expect("read a shader");
        for (name, rewrite) in SHIPPED {
            if let Err(error) = rewrite(&blob)
                && error.kind() == DxbcErrorKind::Malformed
            {
                malformed.push(format!("{}: {name}: {error}", path.display()));
            }
        }
    }
    assert!(
        malformed.is_empty(),
        "malformed verdicts:\n{}",
        malformed.join("\n")
    );
}

/// A rewrite over a truncated container surfaces the defect, while one over a sound shader it does
/// not apply to comes back empty.
#[test]
fn applicable_separates_a_decline_from_a_defect() {
    let shader = include_bytes!("data/ref_vs50.dxbc");
    assert_eq!(
        dxbc_stereo::localize_velocity(shader).applicable(),
        Ok(None),
        "a vertex shader is not a velocity pass"
    );
    let truncated = &shader[..shader.len() / 2];
    let error = dxbc_stereo::localize_velocity(truncated)
        .applicable()
        .expect_err("a truncated container is malformed");
    assert_eq!(error.kind(), DxbcErrorKind::Malformed);
}
