//! Guards the payload's crate layering.
//!
//! The payload is split into seven layer crates under `crates/` and the payload crate on top
//! (`docs/architecture.md`, "Crate layering"). Each crate may depend only on the crates below it.
//! Cargo forbids cycles, but an acyclic edge can still point the wrong way, so the guard checks, as
//! ordinary tests:
//!
//! - that every layer has its crate, named for its layer, and that every dependency of each, in
//!   every section (normal, dev, build, and per-target), points down the order, read by package
//!   name and by path, so the HUD and tooling crates have no edge between them and no layer crate
//!   depends on the payload;
//! - that every layer crate is a workspace member, and that only the payload is a `cdylib` among
//!   the workspace's members;
//! - that the render crate's `hooks` module never names the gameplay or HUD crates, which reach it
//!   only through registrations.

mod layer;
mod lex;
mod manifest;
mod refs;
mod source;

pub use crate::{
    layer::Layer,
    manifest::{
        ManifestSource, RENDER_HOOKS_FORBIDDEN, check_manifests, check_render_hooks,
        check_workspace, load_layer_manifests, load_render_hooks, load_workspace,
    },
    source::{SourceFile, read_sources},
};
