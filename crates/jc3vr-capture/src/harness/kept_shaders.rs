//! The game shaders the harness's trials rewrite, which the render crate's shader identity census
//! keeps the bytecode of while it is armed: each one's pristine hash, the executables that count as
//! it, and a reader for its kept bytecode.

use jc3vr_render::stereo::single_pass::shader_identity::{Kept, LAKE_PS, keep, kept_bytecode};

/// Registers [`KEPT`] with the census. Once, at startup.
pub(crate) fn register() {
    keep(KEPT);
}

pub(crate) const AA_PS: &str = "a4078e6bf737300492eaf5c154ac6b232a537d022396d8c0892635063f17d765";
pub(crate) const DOF_PS: &str = "72acf8f41c94e5b9733f6c25643a3c91c21b51abd607c98b6cecaf034577a64f";
pub(crate) const FOG_PS: &str = "a10dbcd176ceeaa9b289a3c6ae0ea5935e75d0685e22fabc9aaa0b7042a6db4e";
pub(crate) const IMPOSTOR_PS: &str =
    "2c157723bbe071f6ad1c67e2279281dfcb997bc5ae10eceb5704dafbf6597b2d";
pub(crate) const BARK_COLOR_PS: &str =
    "409571499dedebe97d1dba4e7ad782e5f2a91494d0e157c849e6ef66324e3b0d";
pub(crate) const BARK_PS: &str = "7109e8678eee33b96df29f0bf195a7be083a0aafb93f41abdf91739a8bb67ad3";
pub(crate) const SSR_RAY: &str = "94981ae62a33b4bb214f62501f088a365774700d655534783f1d1aed076127e4";
pub(crate) const RESOLVE_PRISTINE: &str =
    "06f43db0887b0038147e273a3879a9b7f48c3b630955a94d695898fd7d0e52fc";
pub(crate) const RESOLVE_ACTUAL: &str =
    "6a82533e6458166ea3d04789c266ec127baf59931882955c99c4603e47f11c9b";
/// The deferred resolve's executable with each eye's camera rows selected per pixel
/// (`dxbc_stereo::localize_eye_camera` over [`RESOLVE_ACTUAL`] with the local lights unchanged), which
/// the engine creates while `single_pass.pixel_camera_per_eye` applies without
/// `single_pass.local_lights_per_eye`.
pub(crate) const RESOLVE_EYE_CAMERA: &str =
    "3d6a81d184a090a7f50b48e79c59c78a82a16d0445b54ae7547694fcee8fc834";
/// The same with the local lights measured from each eye as well, which the engine creates while both
/// switches apply.
pub(crate) const RESOLVE_EYE_LIGHTS: &str =
    "7b28420a68fdf35d8907b7c61289e16fa44ab39ae95afd78b0be08ee51dd4d4b";

pub(crate) fn aa_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(AA_PS)
}

pub(crate) fn dof_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(DOF_PS)
}

pub(crate) fn fog_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(FOG_PS)
}

pub(crate) fn impostor_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(IMPOSTOR_PS)
}

pub(crate) fn bark_bytecode(color: bool) -> Option<Vec<u8>> {
    kept_bytecode(if color { BARK_COLOR_PS } else { BARK_PS })
}

pub(crate) fn ssr_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(SSR_RAY)
}

/// The deferred resolve's executable: the engine's own with the shadow rotation patched, that with
/// each eye's camera rows, or that with each eye's camera rows and local lights.
pub(crate) fn resolve_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(RESOLVE_PRISTINE)
}

pub(crate) fn lake_bytecode() -> Option<Vec<u8>> {
    kept_bytecode(LAKE_PS)
}

/// The shaders the census keeps, each as created unrewritten except the resolve, whose shadow
/// rotation and per-eye rewrites are executables of their own.
const KEPT: &[Kept] = &[
    Kept {
        pristine: LAKE_PS,
        executables: &[(LAKE_PS, 21568)],
    },
    Kept {
        pristine: AA_PS,
        executables: &[(AA_PS, 2296)],
    },
    Kept {
        pristine: DOF_PS,
        executables: &[(DOF_PS, 2944)],
    },
    Kept {
        pristine: FOG_PS,
        executables: &[(FOG_PS, 10364)],
    },
    Kept {
        pristine: IMPOSTOR_PS,
        executables: &[(IMPOSTOR_PS, 5628)],
    },
    Kept {
        pristine: BARK_COLOR_PS,
        executables: &[(BARK_COLOR_PS, 6020)],
    },
    Kept {
        pristine: BARK_PS,
        executables: &[(BARK_PS, 1204)],
    },
    Kept {
        pristine: SSR_RAY,
        executables: &[(SSR_RAY, 8036)],
    },
    Kept {
        pristine: RESOLVE_PRISTINE,
        executables: &[
            (RESOLVE_ACTUAL, 35028),
            (RESOLVE_EYE_CAMERA, 35120),
            (RESOLVE_EYE_LIGHTS, 35392),
        ],
    },
];
