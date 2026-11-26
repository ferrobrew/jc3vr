//! Runs the layering guard over this repository. See the crate docs and docs/architecture.md,
//! "Crate layering", for what each phase checks.

use std::{
    fs,
    path::{Path, PathBuf},
};

use layering_guard::{
    Allowlist, ModuleTable, check_manifests, check_render_hooks, load_layer_manifests,
    load_render_hooks, scan_layer_crates, scan_tree, scan_trees,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("layering guard: could not read {}: {error}", path.display())
    })
}

#[test]
fn payload_references_respect_the_layering() {
    let table = ModuleTable::parse(&read("modules.txt")).unwrap_or_else(|error| panic!("{error}"));
    let allowlist =
        Allowlist::parse(&read("allowlist.txt")).unwrap_or_else(|error| panic!("{error}"));
    let tree = scan_tree(&repo(), "payload/src")
        .unwrap_or_else(|error| panic!("layering guard: could not read payload/src: {error}"));
    assert!(
        !tree.files.is_empty(),
        "layering guard: payload/src holds no Rust files"
    );
    let crates = scan_layer_crates(&repo()).unwrap_or_else(|error| {
        panic!("layering guard: could not read the layer crates' sources: {error}")
    });
    let trees: Vec<_> = std::iter::once(tree).chain(crates).collect();
    if let Err(message) = scan_trees(&table, &trees).check(&allowlist) {
        panic!("{message}");
    }
}

#[test]
fn layer_crate_manifests_respect_the_layering() {
    let manifests = load_layer_manifests(&repo())
        .unwrap_or_else(|error| panic!("layering guard: could not read a manifest: {error}"));
    let hooks = load_render_hooks(&repo()).unwrap_or_else(|error| {
        panic!("layering guard: could not read the render crate's hooks: {error}")
    });
    let violations: Vec<String> = check_manifests(&manifests)
        .into_iter()
        .chain(check_render_hooks(&hooks))
        .collect();
    assert!(
        violations.is_empty(),
        "layering guard: the layer crates break the layering:\n    {}",
        violations.join("\n    ")
    );
}
