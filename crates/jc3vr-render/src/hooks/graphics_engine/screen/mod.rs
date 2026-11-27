//! Screen-space and fullscreen pass detours: the per-eye depth-reconstruction basis and the passes
//! that consume it.

use re_utilities::hook_library::HookLibrary;

// The atmospheric-scattering pass's per-eye re-issue under the collapse.
pub(crate) mod atmospheric_scattering;
// The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35); public so
// the shared `Graphics::Clear` / `SetRenderSetup` detours in `crate::hooks::draw_count` and the
// `SetVertexProgramConstants` detour in `crate::stereo::single_pass` can consult the per-eye froxel
// split.
pub mod clustered_lighting;
// The volumetric fog generation pass's per-eye re-issue under the collapse.
mod fog_volume;
// The remaining fullscreen depth-reconstruction passes under the collapse: SSAO's per-eye rows, and
// the per-eye re-issue of SSR, subsurface skin, and the depth-of-field basis.
pub(crate) mod fullscreen_reconstruction;
// The per-eye off-axis clip-to-view reconstruction fix (`PerspectiveFovInverse`); public so
// the shared `SetRenderSetup` detour can re-derive an at-entry per-eye scissor mask from the target
// the bind just made current.
pub mod reconstruction;
// `ssao` is public so the payload's `hooks::game` can read the recorded `CSSAOPass` pointer for the
// between-eye history-index restore.
pub mod ssao;
// The screen-space decal blocks' per-eye depth reconstruction under the collapse; reached by
// `shader` for the paired permutation rewrite.
pub mod ss_decal;

/// Bundle the screen-space pass detours into one hook library.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(reconstruction::hook_library())
        .with_hook_library(clustered_lighting::hook_library())
        .with_hook_library(ss_decal::hook_library())
        .with_hook_library(atmospheric_scattering::hook_library())
        .with_hook_library(fog_volume::hook_library())
        .with_hook_library(fullscreen_reconstruction::hook_library())
        .with_hook_library(ssao::hook_library())
}
