//! Phase one: the file-level reference map of `payload/src` and the cut-out layer crates' sources,
//! checked against the allowlist.
//!
//! A file in `payload/src` belongs to the crate the module table assigns it, and so does a `crate::`
//! path it names. A file under `crates/jc3vr-<layer>/src` already lives in its crate, so it belongs
//! to that layer whatever its module path, and its `crate::` paths stay inside it; only the
//! `jc3vr_<layer>::` paths it names can point upward.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs, io,
    path::Path,
};

use crate::{
    allowlist::{Allowlist, Edge},
    layer::Layer,
    refs::{Reference, Root, references},
    table::ModuleTable,
};

/// The data files, relative to the repository root, named in failure messages.
const TABLE_PATH: &str = "crates/layering-guard/modules.txt";
const ALLOWLIST_PATH: &str = "crates/layering-guard/allowlist.txt";

/// A source file read from the tree.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// The path relative to the repository root, with `/` separators.
    pub path: String,
    pub text: String,
}

/// The `.rs` files under one source root.
#[derive(Debug)]
pub struct SourceTree {
    /// The root relative to the repository root, such as `payload/src`.
    pub root: String,
    /// The layer every file under the root belongs to, for a cut-out layer crate; `None` for
    /// `payload/src`, whose files the module table places.
    pub layer: Option<Layer>,
    pub files: Vec<SourceFile>,
}

/// Every upward edge in a tree, with the references behind it, and everything the table could not
/// place.
#[derive(Debug, Default)]
pub struct PhaseOneReport {
    pub edges: BTreeMap<Edge, Vec<Hit>>,
    pub unmapped_files: Vec<(String, String)>,
    pub unmapped_references: Vec<(String, usize, String)>,
}

/// One reference behind an upward edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    pub line: usize,
    pub path: String,
    pub from: Layer,
}

/// Reads every `.rs` file under `repo/root`, in a stable order, for files the module table places.
pub fn scan_tree(repo: &Path, root: &str) -> io::Result<SourceTree> {
    read_tree(repo, root, None)
}

/// Reads the sources of every layer crate under `repo/crates` (`crates/jc3vr-<layer>/src`), in a
/// stable order. A `jc3vr-*` directory that names no layer is an error, so a misnamed crate cannot
/// escape the scan.
pub fn scan_layer_crates(repo: &Path) -> io::Result<Vec<SourceTree>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(repo.join("crates"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("jc3vr-") && entry.file_type()?.is_dir() {
            names.push(name);
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let layer = Layer::from_package(&name).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("crates/{name} is not a layer crate (expected jc3vr-<layer>)"),
                )
            })?;
            read_tree(repo, &format!("crates/{name}/src"), Some(layer))
        })
        .collect()
}

/// Maps every file and reference in `tree` and collects the upward edges.
pub fn scan_files(table: &ModuleTable, tree: &SourceTree) -> PhaseOneReport {
    scan_trees(table, std::slice::from_ref(tree))
}

/// Maps every file and reference in `trees` and collects the upward edges into one report.
pub fn scan_trees(table: &ModuleTable, trees: &[SourceTree]) -> PhaseOneReport {
    let mut report = PhaseOneReport::default();
    for tree in trees {
        scan_into(&mut report, table, tree);
    }
    for hits in report.edges.values_mut() {
        hits.sort();
        hits.dedup();
    }
    report
}

fn read_tree(repo: &Path, root: &str, layer: Option<Layer>) -> io::Result<SourceTree> {
    let mut files = Vec::new();
    collect(&repo.join(root), root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(SourceTree {
        root: root.to_owned(),
        layer,
        files,
    })
}

fn scan_into(report: &mut PhaseOneReport, table: &ModuleTable, tree: &SourceTree) {
    for file in &tree.files {
        let module = module_path(&tree.root, &file.path);
        let from = match tree.layer {
            Some(layer) => layer,
            None => match table.resolve(&module) {
                Some(layer) => layer,
                None => {
                    report
                        .unmapped_files
                        .push((file.path.clone(), module.join("::")));
                    continue;
                }
            },
        };
        for reference in references(&file.text) {
            let Some(to) = resolve(table, tree.layer, &reference) else {
                report.unmapped_references.push((
                    file.path.clone(),
                    reference.line,
                    reference.display(),
                ));
                continue;
            };
            if to == from || from.may_depend_on(to) {
                continue;
            }
            report
                .edges
                .entry(Edge {
                    file: file.path.clone(),
                    to,
                })
                .or_default()
                .push(Hit {
                    line: reference.line,
                    path: reference.display(),
                    from,
                });
        }
    }
}

impl PhaseOneReport {
    /// Compares the edges with the allowlist, returning a readable failure message on a mismatch.
    pub fn check(&self, allowlist: &Allowlist) -> Result<(), String> {
        let found: BTreeSet<&Edge> = self.edges.keys().collect();
        let mut new: Vec<&Edge> = found
            .iter()
            .copied()
            .filter(|e| !allowlist.contains(e))
            .collect();
        let mut stale: Vec<&Edge> = allowlist
            .edges
            .iter()
            .filter(|e| !found.contains(e))
            .collect();
        new.sort_by_key(|edge| edge.line());
        stale.sort_by_key(|edge| edge.line());
        if new.is_empty()
            && stale.is_empty()
            && self.unmapped_files.is_empty()
            && self.unmapped_references.is_empty()
        {
            return Ok(());
        }

        let mut out = String::from("layering guard: the payload's crate layering changed.\n");
        if !self.unmapped_files.is_empty() {
            let _ = writeln!(
                out,
                "\nThese files' modules have no entry in {TABLE_PATH}. Add a `<module path> <layer>` \
                 line for each (docs/architecture.md, \"Crate layering\", says which crate owns what):"
            );
            for (file, module) in &self.unmapped_files {
                let _ = writeln!(out, "    {file} (module `{module}`)");
            }
        }
        if !self.unmapped_references.is_empty() {
            let _ = writeln!(
                out,
                "\nThese references name a path with no entry in {TABLE_PATH}. Add a line for its \
                 module or item:"
            );
            for (file, line, path) in &self.unmapped_references {
                let _ = writeln!(out, "    {file}:{line}: {path}");
            }
        }
        if !new.is_empty() {
            let _ = writeln!(
                out,
                "\nNew upward references. A crate may only use the crates below it, so invert these \
                 (docs/architecture.md, \"Crate layering\"):"
            );
            for edge in &new {
                let hits = &self.edges[*edge];
                let _ = writeln!(out, "\n  {} ({}) -> {}", edge.file, hits[0].from, edge.to);
                for hit in hits {
                    let _ = writeln!(out, "    line {}: {} is in {}", hit.line, hit.path, edge.to);
                }
            }
            let _ = writeln!(
                out,
                "\n  Only if an edge is a known back-edge whose inversion is planned, allowlist it by \
                 adding these lines to {ALLOWLIST_PATH} (keep it sorted):"
            );
            for edge in &new {
                let _ = writeln!(out, "    {}", edge.line());
            }
        }
        if !stale.is_empty() {
            let _ = writeln!(
                out,
                "\nThese allowlisted edges no longer exist, so the allowlist can shrink. Delete these \
                 lines from {ALLOWLIST_PATH}:"
            );
            for edge in &stale {
                let _ = writeln!(out, "    {}", edge.line());
            }
        }
        Err(out)
    }
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<SourceFile>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{rel}/{name}");
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if name.ends_with(".rs") {
            let text = fs::read_to_string(entry.path())?;
            out.push(SourceFile { path, text });
        }
    }
    Ok(())
}

/// The module path of a file: `a/b/mod.rs` and `a/b.rs` are both `a::b`, and the crate root
/// `lib.rs` is `lib`.
fn module_path(root: &str, file: &str) -> Vec<String> {
    let rel = file
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(file);
    let rel = rel.strip_suffix(".rs").unwrap_or(rel);
    let mut segments: Vec<String> = rel.split('/').map(str::to_owned).collect();
    if segments.len() > 1 && segments.last().is_some_and(|last| last == "mod") {
        segments.pop();
    }
    segments
}

/// The layer a reference points into: a `crate::` path in a layer crate stays in that crate, one in
/// `payload/src` goes through the table, and a `jc3vr_<layer>::` path names its crate directly.
fn resolve(
    table: &ModuleTable,
    crate_layer: Option<Layer>,
    reference: &Reference,
) -> Option<Layer> {
    match &reference.root {
        Root::Local => crate_layer.or_else(|| table.resolve(&reference.segments)),
        Root::Extern(name) => Layer::from_package(name),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const TABLE: &str = "
        lib payload
        hooks core
        hooks::game payload
        vr render
        controllers gameplay
        controllers::frame input
        hud hud
        director tooling
    ";

    fn tree(files: &[(&str, &str)]) -> SourceTree {
        SourceTree {
            root: "payload/src".into(),
            layer: None,
            files: files
                .iter()
                .map(|(path, text)| SourceFile {
                    path: format!("payload/src/{path}"),
                    text: (*text).to_owned(),
                })
                .collect(),
        }
    }

    fn scan(files: &[(&str, &str)]) -> PhaseOneReport {
        scan_files(&ModuleTable::parse(TABLE).unwrap(), &tree(files))
    }

    fn edge(file: &str, to: Layer) -> Edge {
        Edge {
            file: format!("payload/src/{file}"),
            to,
        }
    }

    #[test]
    fn module_paths() {
        assert_eq!(
            module_path("payload/src", "payload/src/vr/frame.rs"),
            ["vr", "frame"]
        );
        assert_eq!(
            module_path("payload/src", "payload/src/vr/input/mod.rs"),
            ["vr", "input"]
        );
        assert_eq!(
            module_path("payload/src", "payload/src/vr/input.rs"),
            ["vr", "input"]
        );
        assert_eq!(module_path("payload/src", "payload/src/lib.rs"), ["lib"]);
        assert_eq!(module_path("payload/src", "payload/src/mod.rs"), ["mod"]);
    }

    #[test]
    fn downward_and_internal_references_are_fine() {
        let report = scan(&[
            (
                "vr/frame.rs",
                "use crate::{hooks::patcher, controllers::frame::ControllerFrame};",
            ),
            ("controllers/mod.rs", "use crate::{vr, controllers::frame};"),
            ("lib.rs", "use crate::{hud, director, controllers};"),
        ]);
        assert!(report.edges.is_empty(), "{:?}", report.edges);
        assert!(report.unmapped_files.is_empty() && report.unmapped_references.is_empty());
    }

    #[test]
    fn upward_references_become_file_edges() {
        let report = scan(&[
            (
                "vr/frame.rs",
                "use crate::controllers::roomscale;\nfn f() { crate::hud::draw(); }",
            ),
            ("controllers/frame.rs", "use crate::vr::input::SemanticSet;"),
            (
                "hooks/mod.rs",
                "use crate::{controllers::hook_library, hooks::game::reset};",
            ),
        ]);
        let edges: Vec<_> = report.edges.keys().cloned().collect();
        assert_eq!(
            edges,
            [
                edge("controllers/frame.rs", Layer::Render),
                edge("hooks/mod.rs", Layer::Gameplay),
                edge("hooks/mod.rs", Layer::Payload),
                edge("vr/frame.rs", Layer::Gameplay),
                edge("vr/frame.rs", Layer::Hud),
            ]
        );
        assert_eq!(
            report.edges[&edge("vr/frame.rs", Layer::Hud)],
            [Hit {
                line: 2,
                path: "crate::hud::draw".into(),
                from: Layer::Render
            }]
        );
    }

    #[test]
    fn sibling_layers_are_upward() {
        let report = scan(&[("hud/mod.rs", "use crate::director::keymap;")]);
        assert_eq!(
            report.edges.keys().cloned().collect::<Vec<_>>(),
            [edge("hud/mod.rs", Layer::Tooling)]
        );
    }

    #[test]
    fn extern_layer_crates_resolve_directly() {
        let report = scan(&[(
            "vr/mod.rs",
            "use jc3vr_core::lifecycle;\nuse jc3vr_gameplay::bindings;",
        )]);
        assert_eq!(
            report.edges.keys().cloned().collect::<Vec<_>>(),
            [edge("vr/mod.rs", Layer::Gameplay)]
        );
    }

    fn crate_tree(layer: Layer, files: &[(&str, &str)]) -> SourceTree {
        let root = format!("crates/{}/src", layer.package());
        SourceTree {
            files: files
                .iter()
                .map(|(path, text)| SourceFile {
                    path: format!("{root}/{path}"),
                    text: (*text).to_owned(),
                })
                .collect(),
            root,
            layer: Some(layer),
        }
    }

    #[test]
    fn layer_crate_files_belong_to_their_crate() {
        let table = ModuleTable::parse(TABLE).unwrap();
        let core = crate_tree(
            Layer::Core,
            &[
                // `lib` and `director` map to payload and tooling in the table, and `crate::hud`
                // to the HUD, but inside the core crate they are the core crate's own.
                (
                    "lib.rs",
                    "pub mod director;
use crate::hud::Thing;",
                ),
                ("director/mod.rs", "use crate::director::clean_view;"),
                ("hooks/mod.rs", "use jc3vr_render::vr;"),
            ],
        );
        let render = crate_tree(Layer::Render, &[("vr/mod.rs", "use jc3vr_core::hash;")]);
        let report = scan_trees(&table, &[core, render]);
        assert!(report.unmapped_files.is_empty() && report.unmapped_references.is_empty());
        assert_eq!(
            report.edges.keys().cloned().collect::<Vec<_>>(),
            [Edge {
                file: "crates/jc3vr-core/src/hooks/mod.rs".into(),
                to: Layer::Render,
            }]
        );
    }

    #[test]
    fn unmapped_files_and_references_are_reported() {
        let report = scan(&[
            ("widgets/mod.rs", "use crate::vr;"),
            ("vr/mod.rs", "use crate::gizmo::Thing;"),
        ]);
        assert_eq!(
            report.unmapped_files,
            [(
                "payload/src/widgets/mod.rs".to_owned(),
                "widgets".to_owned()
            )]
        );
        assert_eq!(
            report.unmapped_references,
            [(
                "payload/src/vr/mod.rs".to_owned(),
                1,
                "crate::gizmo::Thing".to_owned()
            )]
        );
        let message = report.check(&Allowlist::default()).unwrap_err();
        assert!(
            message.contains("payload/src/widgets/mod.rs (module `widgets`)"),
            "{message}"
        );
        assert!(
            message.contains("payload/src/vr/mod.rs:1: crate::gizmo::Thing"),
            "{message}"
        );
    }

    #[test]
    fn allowlisted_edges_pass() {
        let report = scan(&[("vr/frame.rs", "use crate::controllers::roomscale;")]);
        let allowlist = Allowlist::parse("payload/src/vr/frame.rs -> gameplay\n").unwrap();
        assert_eq!(report.check(&allowlist), Ok(()));
    }

    #[test]
    fn new_edges_fail_with_the_lines_to_add() {
        let report = scan(&[(
            "vr/frame.rs",
            "use crate::controllers::roomscale;\nuse crate::hud;",
        )]);
        let allowlist = Allowlist::parse("payload/src/vr/frame.rs -> gameplay\n").unwrap();
        let message = report.check(&allowlist).unwrap_err();
        assert!(
            message.contains("payload/src/vr/frame.rs (render) -> hud"),
            "{message}"
        );
        assert!(
            message.contains("line 2: crate::hud is in hud"),
            "{message}"
        );
        assert!(
            message.contains("\n    payload/src/vr/frame.rs -> hud\n"),
            "{message}"
        );
        assert!(!message.contains("-> gameplay"), "{message}");
    }

    #[test]
    fn stale_entries_fail_with_the_lines_to_delete() {
        let report = scan(&[("vr/frame.rs", "use crate::hooks::patcher;")]);
        let allowlist =
            Allowlist::parse("payload/src/gone.rs -> ui\npayload/src/vr/frame.rs -> gameplay\n")
                .unwrap();
        let message = report.check(&allowlist).unwrap_err();
        assert!(message.contains("Delete these lines"), "{message}");
        assert!(
            message.contains(
                "\n    payload/src/gone.rs -> ui\n    payload/src/vr/frame.rs -> gameplay\n"
            ),
            "{message}"
        );
        assert!(!message.contains("New upward references"), "{message}");
    }
}
