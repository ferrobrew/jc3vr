//! Guards the payload's planned crate layering while the payload is split into crates.
//!
//! The split assigns every payload module to one of eight crates (`docs/architecture.md`, "Crate
//! layering"). Each crate may depend only on the crates below it. Until a module is cut out into
//! its crate, nothing but this guard stops a new reference from pointing upward, so the guard runs
//! in two phases, both as ordinary tests:
//!
//! - Phase one scans `payload/src`, maps every file and every `crate::` reference to its target
//!   crate through the module table (`modules.txt`), and fails on any upward edge that is not in
//!   the allowlist (`allowlist.txt`). It also fails on an allowlisted edge that no longer exists,
//!   so the allowlist only shrinks.
//! - Phase two reads each `crates/jc3vr-*/Cargo.toml` and the payload's manifest, and checks the
//!   dependencies against the same layer order, that only the payload is a `cdylib`, and that the
//!   render crate's `hooks` module never names the gameplay or HUD crates. It checks whichever of
//!   those crates exist, so it passes vacuously before the first cut.

mod allowlist;
mod layer;
mod lex;
mod manifest;
mod refs;
mod scan;
mod table;

pub use crate::{
    allowlist::{Allowlist, AllowlistError, Edge},
    layer::Layer,
    manifest::{
        ManifestSource, RENDER_HOOKS_FORBIDDEN, check_manifests, check_render_hooks,
        load_layer_manifests, load_render_hooks,
    },
    scan::{Hit, PhaseOneReport, SourceFile, SourceTree, scan_files, scan_tree},
    table::{ModuleTable, TableError},
};
