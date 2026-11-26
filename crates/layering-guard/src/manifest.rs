//! Phase two: the cut crates' manifests and the render crate's hooks.
//!
//! Once a module is cut out into `crates/jc3vr-<layer>`, Cargo forbids cycles, but an acyclic edge
//! can still be wrong (HUD depending on tooling). These checks read the manifests of whichever
//! layer crates exist, so they pass vacuously until the first cut.

use std::{fs, io, path::Path};

use toml::{Table, Value};

use crate::{
    layer::Layer,
    refs::{Root, references},
    scan::{SourceFile, scan_tree},
};

/// The layers the render crate's `hooks` module must never name.
pub const RENDER_HOOKS_FORBIDDEN: [Layer; 2] = [Layer::Gameplay, Layer::Hud];

/// A layer crate's manifest.
#[derive(Clone, Debug)]
pub struct ManifestSource {
    /// The crate's directory relative to the repository root: `payload` or `crates/jc3vr-<layer>`.
    pub dir: String,
    pub text: String,
}

/// Reads the payload's manifest and every `crates/jc3vr-*/Cargo.toml`.
pub fn load_layer_manifests(repo: &Path) -> io::Result<Vec<ManifestSource>> {
    let mut dirs = vec!["payload".to_owned()];
    for entry in fs::read_dir(repo.join("crates"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("jc3vr-") && entry.file_type()?.is_dir() {
            dirs.push(format!("crates/{name}"));
        }
    }
    dirs.sort();
    dirs.into_iter()
        .map(|dir| {
            let text = fs::read_to_string(repo.join(&dir).join("Cargo.toml"))?;
            Ok(ManifestSource { dir, text })
        })
        .collect()
}

/// Checks each manifest's name, crate type, and dependencies against its layer. Returns one
/// message per violation.
pub fn check_manifests(manifests: &[ManifestSource]) -> Vec<String> {
    manifests.iter().flat_map(check_manifest).collect()
}

/// Reads the render crate's `hooks` module, or nothing before the render cut.
pub fn load_render_hooks(repo: &Path) -> io::Result<Vec<SourceFile>> {
    const ROOT: &str = "crates/jc3vr-render/src/hooks";
    if !repo.join(ROOT).is_dir() {
        return Ok(Vec::new());
    }
    Ok(scan_tree(repo, ROOT)?.files)
}

/// Checks that no file names a crate in [`RENDER_HOOKS_FORBIDDEN`].
pub fn check_render_hooks(files: &[SourceFile]) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        for reference in references(&file.text) {
            let Root::Extern(name) = &reference.root else {
                continue;
            };
            if let Some(layer) = Layer::from_package(name)
                && RENDER_HOOKS_FORBIDDEN.contains(&layer)
            {
                out.push(format!(
                    "{}:{}: {} names the {layer} crate; the render crate's hooks must reach gameplay \
                     and the HUD only through registrations they install",
                    file.path,
                    reference.line,
                    reference.display()
                ));
            }
        }
    }
    out
}

fn check_manifest(manifest: &ManifestSource) -> Vec<String> {
    let path = format!("{}/Cargo.toml", manifest.dir);
    let layer = if manifest.dir == "payload" {
        Some(Layer::Payload)
    } else {
        manifest
            .dir
            .strip_prefix("crates/jc3vr-")
            .and_then(Layer::from_name)
            .filter(|&layer| layer != Layer::Payload)
    };
    let Some(layer) = layer else {
        return vec![format!(
            "{path}: `{}` is not a layer crate; the layers are {}",
            manifest.dir,
            Layer::ALL[..7]
                .iter()
                .map(|layer| format!("crates/{}", layer.package()))
                .collect::<Vec<_>>()
                .join(", ")
        )];
    };
    let table: Table = match toml::from_str(&manifest.text) {
        Ok(table) => table,
        Err(error) => return vec![format!("{path}: could not parse the manifest: {error}")],
    };

    let mut out = Vec::new();
    let name = table
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str);
    if name != Some(layer.package().as_str()) {
        out.push(format!(
            "{path}: the package is named {}, but the {layer} crate must be named `{}`",
            name.map_or("nothing".to_owned(), |name| format!("`{name}`")),
            layer.package()
        ));
    }

    let cdylib = table
        .get("lib")
        .and_then(|lib| lib.get("crate-type"))
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("cdylib")));
    match (layer, cdylib) {
        (Layer::Payload, false) => out.push(format!(
            "{path}: the payload must stay a `cdylib`; the injector loads it as a DLL"
        )),
        (Layer::Payload, true) | (_, false) => {}
        (_, true) => out.push(format!(
            "{path}: only the payload is a `cdylib`; the {layer} crate must be a plain library"
        )),
    }

    for (section, dependencies) in dependency_tables(&table) {
        for (key, spec) in dependencies {
            let package = spec
                .get("package")
                .and_then(Value::as_str)
                .unwrap_or(key.as_str());
            let Some(dependency) = Layer::from_package(package) else {
                continue;
            };
            if !layer.may_depend_on(dependency) {
                let below: Vec<&str> = Layer::ALL
                    .into_iter()
                    .filter(|&other| layer.may_depend_on(other))
                    .map(Layer::name)
                    .collect();
                out.push(format!(
                    "{path}: the {layer} crate depends on `{package}` ({dependency}) in [{section}], \
                     but it may only depend on {}",
                    if below.is_empty() { "no layer crate".to_owned() } else { below.join(", ") }
                ));
            }
        }
    }
    out
}

/// Every dependency table in a manifest, with its section name, including the per-target ones.
fn dependency_tables(table: &Table) -> Vec<(String, &Table)> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut out = Vec::new();
    for kind in KINDS {
        if let Some(dependencies) = table.get(kind).and_then(Value::as_table) {
            out.push((kind.to_owned(), dependencies));
        }
    }
    if let Some(targets) = table.get("target").and_then(Value::as_table) {
        for (target, spec) in targets {
            for kind in KINDS {
                if let Some(dependencies) = spec.get(kind).and_then(Value::as_table) {
                    out.push((format!("target.{target}.{kind}"), dependencies));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn manifest(dir: &str, text: &str) -> ManifestSource {
        ManifestSource {
            dir: dir.to_owned(),
            text: text.to_owned(),
        }
    }

    const PAYLOAD: &str = r#"
        [package]
        name = "jc3vr_payload"
        [lib]
        crate-type = ["cdylib"]
        [dependencies]
        jc3vr-core = { path = "../crates/jc3vr-core" }
        jc3vr-ui = { path = "../crates/jc3vr-ui" }
    "#;

    #[test]
    fn todays_payload_alone_passes() {
        let text = "[package]\nname = \"jc3vr_payload\"\n[lib]\ncrate-type = [\"cdylib\"]\n[dependencies]\nglam = \"0.30\"\n";
        assert_eq!(
            check_manifests(&[manifest("payload", text)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn downward_dependencies_pass() {
        let hud = r#"
            [package]
            name = "jc3vr-hud"
            [dependencies]
            jc3vr-gameplay = { path = "../jc3vr-gameplay" }
            render = { package = "jc3vr-render", path = "../jc3vr-render" }
            [target.'cfg(windows)'.dependencies]
            jc3vr-core = { workspace = true }
        "#;
        let violations = check_manifests(&[
            manifest("payload", PAYLOAD),
            manifest("crates/jc3vr-hud", hud),
        ]);
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn sibling_and_upward_dependencies_fail() {
        let hud = r#"
            [package]
            name = "jc3vr-hud"
            [dependencies]
            jc3vr-tooling = { path = "../jc3vr-tooling" }
            [dev-dependencies]
            ui = { package = "jc3vr-ui", path = "../jc3vr-ui" }
        "#;
        let violations = check_manifests(&[manifest("crates/jc3vr-hud", hud)]);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains("depends on `jc3vr-tooling` (tooling) in [dependencies]"));
        assert!(violations[0].contains("may only depend on core, input, render, gameplay"));
        assert!(violations[1].contains("`jc3vr-ui` (ui) in [dev-dependencies]"));
    }

    #[test]
    fn core_depends_on_no_layer() {
        let core = "[package]\nname = \"jc3vr-core\"\n[target.'cfg(windows)'.dependencies]\njc3vr-input = \"0\"\n";
        let violations = check_manifests(&[manifest("crates/jc3vr-core", core)]);
        assert_eq!(violations.len(), 1);
        assert!(
            violations[0].contains("[target.cfg(windows).dependencies]"),
            "{}",
            violations[0]
        );
        assert!(
            violations[0].ends_with("may only depend on no layer crate"),
            "{}",
            violations[0]
        );
    }

    #[test]
    fn only_the_payload_is_a_cdylib() {
        let render =
            "[package]\nname = \"jc3vr-render\"\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n";
        let payload = "[package]\nname = \"jc3vr_payload\"\n";
        let violations = check_manifests(&[
            manifest("crates/jc3vr-render", render),
            manifest("payload", payload),
        ]);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains("only the payload is a `cdylib`"));
        assert!(violations[1].contains("must stay a `cdylib`"));
    }

    #[test]
    fn names_and_directories_must_match_a_layer() {
        let misnamed = "[package]\nname = \"jc3vr-hud\"\n";
        let violations = check_manifests(&[
            manifest("crates/jc3vr-tooling", misnamed),
            manifest("crates/jc3vr-widgets", misnamed),
            manifest("crates/jc3vr-payload", misnamed),
        ]);
        assert_eq!(violations.len(), 3, "{violations:#?}");
        assert!(violations[0].contains("must be named `jc3vr-tooling`"));
        assert!(violations[1].contains("is not a layer crate"));
        assert!(violations[2].contains("is not a layer crate"));
    }

    #[test]
    fn unparseable_manifests_are_reported() {
        let violations = check_manifests(&[manifest("crates/jc3vr-ui", "[package\n")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("crates/jc3vr-ui/Cargo.toml: could not parse"));
    }

    #[test]
    fn render_hooks_must_not_name_gameplay_or_hud() {
        let files = [
            SourceFile {
                path: "crates/jc3vr-render/src/hooks/camera/mod.rs".into(),
                text: "use jc3vr_core::hooks::patcher;\n// jc3vr_hud::pointer in a comment.\nuse jc3vr_gameplay::aim;".into(),
            },
            SourceFile {
                path: "crates/jc3vr-render/src/hooks/graphics_engine/mod.rs".into(),
                text: "fn f() { jc3vr_hud::egui_panel::draw_quad(); }".into(),
            },
        ];
        let violations = check_render_hooks(&files);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].starts_with("crates/jc3vr-render/src/hooks/camera/mod.rs:3: jc3vr_gameplay::aim names the gameplay crate"));
        assert!(violations[1].contains("jc3vr_hud::egui_panel::draw_quad names the hud crate"));
    }
}
