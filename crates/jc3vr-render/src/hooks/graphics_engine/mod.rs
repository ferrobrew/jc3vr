//! CGraphicsEngine-area detours, mirroring `jc3gi::graphics_engine`. Each leaf owns its detours in
//! its own [`HookLibrary`]; [`hook_library`] nests them into one parent library. The leaves are
//! grouped by pipeline stage -- [`scene`] (world geometry and culling), [`screen`] (screen-space
//! and fullscreen passes), and [`post`] (the post-effects chain) -- with the engine-core seams at
//! the root; each group's modules are re-exported here so consumers address them by the stable
//! `graphics_engine::<leaf>` path regardless of grouping (leaves nothing outside the group
//! references are not re-exported).

use re_utilities::hook_library::HookLibrary;

// Exposure and post-effect config structs, read by `tone_mapping` and `post_effects`.
pub mod config;
// The replay of `GeneralMkIII`'s declaration creation that builds the hardware-instanced layout twins.
mod declaration_replay;
// The `CGraphicsEngine` detours, mirroring `jc3gi::graphics_engine::graphics_engine`; public for the
// payload's flip block and hook-thread log.
pub mod engine;
mod render_pass;
// `shader` is public for the payload's reconcile, reload, and eject calls and the debug UI's
// patched-shader count.
pub mod shader;
// Single-pass stereo render-thread detours (the per-pass cb13 mirror and the baked-constant
// render-block re-issues).
mod single_pass;

pub mod post;
mod scene;
mod screen;

pub use post::{post_effects, tone_mapping};
pub use render_pass::shadow_fit_gap_m;
pub use scene::{render_block, terrain};
pub use screen::{clustered_lighting, reconstruction, ss_decal, ssao};
pub(crate) use single_pass::upload_global_constants;

/// Bundle every CGraphicsEngine-area detour into one hook library, mirroring how the game groups
/// these classes.
pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(engine::hook_library())
        .with_hook_library(declaration_replay::hook_library())
        .with_hook_library(scene::hook_library())
        .with_hook_library(screen::hook_library())
        .with_hook_library(post::hook_library())
        .with_hook_library(render_pass::hook_library())
        .with_hook_library(shader::hook_library())
        .with_hook_library(single_pass::hook_library())
}
