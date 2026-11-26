//! CGraphicsEngine-area detours, mirroring `jc3gi::graphics_engine`. Each leaf owns its detours in
//! its own [`HookLibrary`]; [`hook_library`] nests them into one parent library. The leaves are
//! grouped by pipeline stage -- [`scene`] (world geometry and culling), [`screen`] (screen-space
//! and fullscreen passes), and [`post`] (the post-effects chain) -- with the engine-core seams at
//! the root; each group's modules are re-exported here so consumers address them by the stable
//! `graphics_engine::<leaf>` path regardless of grouping (leaves nothing outside the group
//! references are not re-exported).

use re_utilities::hook_library::HookLibrary;

// The eye and preview captures the post-draw and the post chain fill, which the VR blit, the
// mirror, the recorder, the screenshots, and the Previews tab read.
pub mod capture;
// Exposure and post-effect config structs, accessed by `tone_mapping` and `post_effects` via
// `Config::lock_query`.
pub mod config;
// The replay of `GeneralMkIII`'s declaration creation that builds the hardware-instanced layout twins.
mod declaration_replay;
// `graphics_engine` stays public for hooks::game's BLOCK_FLIP reference. The inner name mirrors
// jc3gi (CGraphicsEngine in its own module), hence the module_inception allow.
#[allow(clippy::module_inception)]
pub mod graphics_engine;
// The registrations the post-draw, the UI redirect, and the flip run for the layers above.
pub mod post_draw;
mod render_pass;
// `shader` is public so the debug UI can read its patched-shader count.
pub mod shader;
// Single-pass stereo render-thread detours (the cb13 mirror; on by default while a VR session runs).
mod single_pass;

pub mod post;
mod scene;
mod screen;

pub use post::{post_effects, tone_mapping};
pub use render_pass::shadow_fit_gap_m;
pub use scene::{palette_edit, player_draw, render_block, terrain};
pub use screen::{clustered_lighting, reconstruction, ss_decal, ssao};
pub(crate) use single_pass::upload_global_constants;

/// Bundle every CGraphicsEngine-area detour into one hook library, mirroring how the game groups
/// these classes.
pub fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(graphics_engine::hook_library())
        .with_hook_library(declaration_replay::hook_library())
        .with_hook_library(scene::hook_library())
        .with_hook_library(screen::hook_library())
        .with_hook_library(post::hook_library())
        .with_hook_library(render_pass::hook_library())
        .with_hook_library(shader::hook_library())
        .with_hook_library(single_pass::hook_library())
}
