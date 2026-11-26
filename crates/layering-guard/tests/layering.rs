//! Runs the layering guard over this repository. See the crate docs and docs/architecture.md,
//! "Crate layering", for what each check enforces.

use std::path::{Path, PathBuf};

use layering_guard::{
    check_hooks, check_manifests, check_workspace, load_hooks, load_layer_manifests, load_workspace,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn assert_clean(what: &str, violations: Vec<String>) {
    assert!(
        violations.is_empty(),
        "layering guard: {what}:\n    {}",
        violations.join("\n    ")
    );
}

#[test]
fn layer_crate_dependencies_point_down() {
    let manifests = load_layer_manifests(&repo())
        .unwrap_or_else(|error| panic!("layering guard: could not read a manifest: {error}"));
    assert_clean(
        "the layer crates' manifests break the layering",
        check_manifests(&manifests),
    );
}

#[test]
fn only_the_payload_is_a_cdylib() {
    let (root, members) = load_workspace(&repo())
        .unwrap_or_else(|error| panic!("layering guard: could not read the workspace: {error}"));
    assert_clean(
        "the workspace breaks the layering",
        check_workspace(&root, &members),
    );
}

#[test]
fn hooks_reach_upward_only_through_registrations() {
    let hooks = load_hooks(&repo()).unwrap_or_else(|error| {
        panic!("layering guard: could not read the view and render crates' hooks: {error}")
    });
    assert!(
        !hooks.is_empty(),
        "layering guard: the view and render crates' hooks hold no Rust files"
    );
    assert_clean(
        "the view and render crates' hooks name a crate above them",
        check_hooks(&hooks),
    );
}
