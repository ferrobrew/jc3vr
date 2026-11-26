//! Guards the payload's crate layering.
//!
//! The payload is split into layer crates under `crates/` and the payload crate on top
//! (`docs/architecture.md`, "Crate layering"). Each crate may depend only on the crates below it,
//! and crates side by side may not depend on each other. Cargo forbids cycles, but an acyclic edge
//! can still point the wrong way, so the guard checks, as ordinary tests:
//!
//! - that every layer has its crate, named for its layer, and that every dependency of each, in
//!   every section (normal, dev, build, and per-target), points down the order, read by package
//!   name and by path, so the render crate and the body, weapons, physical, vehicles, grapple, and
//!   gameplay crates, the weapons and physical crates, the vehicles and grapple crates, and the HUD
//!   and tooling crates, have no edge between them, only the tooling crate and the crates above it
//!   depend on the capture crate, and no layer crate depends on the payload;
//! - that every layer crate is a workspace member, and that only the payload is a `cdylib` among
//!   the workspace's members;
//! - that the view and render crates' `hooks` modules never name the body, weapons, physical,
//!   vehicles, grapple, gameplay, or HUD crates, which reach them only through registrations.

mod layer;
mod lex;
mod manifest;
mod refs;
mod source;

pub use crate::{
    layer::Layer,
    manifest::{
        HOOK_CRATES, HOOKS_FORBIDDEN, ManifestSource, check_hooks, check_manifests,
        check_workspace, load_hooks, load_layer_manifests, load_workspace,
    },
    source::{SourceFile, read_sources},
};
