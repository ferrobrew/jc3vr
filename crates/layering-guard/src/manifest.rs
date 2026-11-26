//! The layer crates' manifests, the workspace's crate types, and the view and render crates'
//! hooks.
//!
//! Cargo forbids dependency cycles, but an acyclic edge can still be wrong (HUD depending on
//! tooling, or a layer crate depending on the payload), so these checks hold each layer crate's
//! dependencies to the order in [`Layer::may_depend_on`]. Every message ends with how to fix it.

use std::{fs, io, path::Path};

use toml::{Table, Value};

use crate::{
    layer::Layer,
    refs::{Root, references},
    source::{SourceFile, read_sources},
};

/// The crates whose `hooks` modules hold the engine detours that body, physical, gameplay, and the
/// HUD reach only through registrations.
pub const HOOK_CRATES: [Layer; 2] = [Layer::View, Layer::Render];

/// The layers the view and render crates' `hooks` modules must never name.
pub const HOOKS_FORBIDDEN: [Layer; 4] = [Layer::Body, Layer::Physical, Layer::Gameplay, Layer::Hud];

/// Where the layering and the reasons for it are documented, for messages.
const LAYERING_DOC: &str = "docs/architecture.md, \"Crate layering\"";

/// A crate's manifest.
#[derive(Clone, Debug)]
pub struct ManifestSource {
    /// The crate's directory relative to the repository root, such as `payload` or
    /// `crates/jc3vr-<layer>`.
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
        .map(|dir| read_manifest(repo, dir))
        .collect()
}

/// Reads the workspace's root manifest and the manifest of every member it lists.
pub fn load_workspace(repo: &Path) -> io::Result<(String, Vec<ManifestSource>)> {
    let root = fs::read_to_string(repo.join("Cargo.toml"))?;
    let members = workspace_members(&root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let manifests = members
        .into_iter()
        .map(|dir| read_manifest(repo, dir))
        .collect::<io::Result<_>>()?;
    Ok((root, manifests))
}

/// Checks that every layer has a crate, and each crate's name, crate type, and dependencies
/// against its layer. Returns one message per violation.
pub fn check_manifests(manifests: &[ManifestSource]) -> Vec<String> {
    let mut out: Vec<String> = Layer::ALL
        .into_iter()
        .filter(|&layer| !manifests.iter().any(|m| m.dir == layer_dir(layer)))
        .map(|layer| {
            format!(
                "{}/Cargo.toml: the {layer} layer has no crate, but every layer in {LAYERING_DOC} \
                 has one; restore the crate, or change the layers in \
                 crates/layering-guard/src/layer.rs and the doc together",
                layer_dir(layer)
            )
        })
        .collect();
    out.extend(manifests.iter().flat_map(check_manifest));
    out
}

/// Checks the workspace as a whole: only the payload is a `cdylib`, and every layer crate is a
/// member. `root` is the root manifest's text and `members` its members' manifests.
pub fn check_workspace(root: &str, members: &[ManifestSource]) -> Vec<String> {
    let mut out = Vec::new();
    let listed = match workspace_members(root) {
        Ok(listed) => listed,
        Err(error) => return vec![error],
    };
    for layer in Layer::ALL {
        let dir = layer_dir(layer);
        if !listed.contains(&dir) {
            out.push(format!(
                "Cargo.toml: the {layer} crate is not a workspace member; add \"{dir}\" to \
                 [workspace] members"
            ));
        }
    }
    for member in members {
        let Ok(table) = toml::from_str::<Table>(&member.text) else {
            out.push(format!(
                "{}/Cargo.toml: could not parse the manifest; fix its TOML",
                member.dir
            ));
            continue;
        };
        if member.dir != layer_dir(Layer::Payload) && is_cdylib(&table) {
            out.push(format!(
                "{}/Cargo.toml: only the payload (`jc3vr_payload`) is a `cdylib`, because the \
                 injector loads exactly one DLL; make this crate a plain library (drop its \
                 `crate-type`) and link it into the payload",
                member.dir
            ));
        }
    }
    out
}

/// Reads the `hooks` module of every crate in [`HOOK_CRATES`].
pub fn load_hooks(repo: &Path) -> io::Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    for layer in HOOK_CRATES {
        files.extend(read_sources(
            repo,
            &format!("{}/src/hooks", layer_dir(layer)),
        )?);
    }
    Ok(files)
}

/// Checks that no file names a crate in [`HOOKS_FORBIDDEN`].
pub fn check_hooks(files: &[SourceFile]) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        for reference in references(&file.text) {
            let Root::Extern(name) = &reference.root else {
                continue;
            };
            if let Some(layer) = Layer::from_package(name)
                && HOOKS_FORBIDDEN.contains(&layer)
            {
                out.push(format!(
                    "{}:{}: {} names the {layer} crate, but the view and render crates' hooks \
                     reach body, physical, gameplay, and the HUD only through registrations; add a \
                     registration \
                     point in the crate that owns the hook and fill it from the {layer} crate's \
                     install, as the existing `register_*` modules do",
                    file.path,
                    reference.line,
                    reference.display()
                ));
            }
        }
    }
    out
}

/// The directory of a layer's crate, relative to the repository root.
fn layer_dir(layer: Layer) -> String {
    match layer {
        Layer::Payload => "payload".to_owned(),
        layer => format!("crates/{}", layer.package()),
    }
}

fn read_manifest(repo: &Path, dir: String) -> io::Result<ManifestSource> {
    let text = fs::read_to_string(repo.join(&dir).join("Cargo.toml"))?;
    Ok(ManifestSource { dir, text })
}

/// The `[workspace] members` of a root manifest. A glob is refused, so that no member can escape
/// the checks.
fn workspace_members(root: &str) -> Result<Vec<String>, String> {
    let table: Table = toml::from_str(root)
        .map_err(|error| format!("Cargo.toml: could not parse the root manifest: {error}"))?;
    let members = table
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(Value::as_array)
        .ok_or("Cargo.toml: the root manifest lists no [workspace] members")?;
    members
        .iter()
        .map(|member| match member.as_str() {
            Some(member) if member.contains(['*', '?', '[']) => Err(format!(
                "Cargo.toml: the workspace member `{member}` is a glob; list each member by path \
                 so the layering guard checks it"
            )),
            Some(member) => Ok(member.trim_end_matches('/').to_owned()),
            None => Err("Cargo.toml: a workspace member is not a string".to_owned()),
        })
        .collect()
}

fn is_cdylib(table: &Table) -> bool {
    table
        .get("lib")
        .and_then(|lib| lib.get("crate-type"))
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("cdylib")))
}

fn check_manifest(manifest: &ManifestSource) -> Vec<String> {
    let path = format!("{}/Cargo.toml", manifest.dir);
    let layer = Layer::ALL
        .into_iter()
        .find(|&layer| layer_dir(layer) == manifest.dir);
    let Some(layer) = layer else {
        return vec![format!(
            "{path}: `{}` is not a layer crate; the layer crates are {}, so rename it to its \
             layer's directory, or move it out of the `jc3vr-` namespace if it is a shared library",
            manifest.dir,
            Layer::ALL
                .iter()
                .map(|&layer| layer_dir(layer))
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
            "{path}: the package is named {}, but the {layer} crate must be named `{}`; rename \
             the package",
            name.map_or("nothing".to_owned(), |name| format!("`{name}`")),
            layer.package()
        ));
    }
    if layer == Layer::Payload && !is_cdylib(&table) {
        out.push(format!(
            "{path}: the payload must stay a `cdylib`, because the injector loads it as a DLL; \
             set `crate-type = [\"cdylib\"]` under [lib]"
        ));
    }

    for (section, dependencies) in dependency_tables(&table) {
        for (key, spec) in dependencies {
            for dependency in dependency_layers(key, spec) {
                if !layer.may_depend_on(dependency) {
                    out.push(format!(
                        "{path}: the {layer} crate depends on the {dependency} crate (`{key}` in \
                         [{section}]), {}",
                        violation_advice(layer, dependency)
                    ));
                }
            }
        }
    }
    out
}

/// Why `layer` may not depend on `dependency`, and how to remove the edge.
fn violation_advice(layer: Layer, dependency: Layer) -> String {
    if dependency == Layer::Payload {
        return "but nothing depends on the payload, which is the top of the stack; move the code \
                this crate needs into the lowest layer crate that owns it, or have the payload \
                register it from `composition::install`"
            .to_owned();
    }
    if dependency == layer {
        return "which is itself; remove the dependency".to_owned();
    }
    if matches!(
        (layer, dependency),
        (Layer::Hud, Layer::Tooling) | (Layer::Tooling, Layer::Hud)
    ) {
        return "but the HUD and tooling crates are siblings with no edge between them; move the \
                shared code down into the gameplay crate or below, or connect them from the UI \
                crate, which sees both"
            .to_owned();
    }
    if matches!(
        (layer, dependency),
        (
            Layer::Render,
            Layer::Gameplay | Layer::Physical | Layer::Body
        ) | (
            Layer::Gameplay | Layer::Physical | Layer::Body,
            Layer::Render
        )
    ) {
        let other = if layer == Layer::Render {
            dependency
        } else {
            layer
        };
        return format!(
            "but the render and {other} crates are siblings on the view crate with no edge \
             between them; move the registration type or publication cell they share down \
             into the view crate, as the overlay's inputs and the scope's lens frame are"
        );
    }
    if dependency == Layer::Capture {
        return "but only the tooling crate and the crates above it see the capture crate; reach \
                the capture tooling through a probe table this crate keeps and the capture crate \
                registers into at install, as the single-pass and pipeline probes are"
            .to_owned();
    }
    let below: Vec<&str> = Layer::ALL
        .into_iter()
        .filter(|&other| layer.may_depend_on(other))
        .map(Layer::name)
        .collect();
    format!(
        "but it may only depend on {}; move the code it needs down into one of those crates, or \
         invert the edge with a registration the {dependency} crate installs into this one (see \
         {LAYERING_DOC})",
        if below.is_empty() {
            "no layer crate".to_owned()
        } else {
            below.join(", ")
        }
    )
}

/// The layers a dependency names, by its package name and by the directory its `path` points at.
/// Both are read, so a renamed or aliased dependency cannot hide a layer crate.
fn dependency_layers(key: &str, spec: &Value) -> Vec<Layer> {
    let package = spec.get("package").and_then(Value::as_str).unwrap_or(key);
    let by_path = spec
        .get("path")
        .and_then(Value::as_str)
        .and_then(|path| Path::new(path.trim_end_matches('/')).file_name())
        .and_then(|name| name.to_str())
        .and_then(|name| match name {
            "payload" => Some(Layer::Payload),
            name => Layer::from_package(name),
        });
    let mut layers: Vec<Layer> = Layer::from_package(package)
        .into_iter()
        .chain(by_path)
        .collect();
    layers.dedup();
    layers
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

    /// A manifest for every layer, each depending on the layer directly below it (the payload on
    /// the UI crate), which passes.
    fn stack() -> Vec<ManifestSource> {
        Layer::ALL
            .into_iter()
            .map(|layer| {
                let mut text = format!("[package]\nname = \"{}\"\n", layer.package());
                if layer == Layer::Payload {
                    text.push_str("[lib]\ncrate-type = [\"cdylib\"]\n");
                }
                text.push_str("[dependencies]\nglam = \"0.30\"\n");
                if let Some(below) = Layer::ALL
                    .into_iter()
                    .rev()
                    .find(|&other| layer.may_depend_on(other))
                {
                    text.push_str(&format!(
                        "{} = {{ path = \"../{}\" }}\n",
                        below.package(),
                        below.package()
                    ));
                }
                manifest(&layer_dir(layer), &text)
            })
            .collect()
    }

    /// The stack with one layer's manifest replaced.
    fn stack_with(layer: Layer, text: &str) -> Vec<ManifestSource> {
        let mut manifests = stack();
        let dir = layer_dir(layer);
        manifests.retain(|m| m.dir != dir);
        manifests.push(manifest(&dir, text));
        manifests
    }

    #[test]
    fn the_stack_passes() {
        assert_eq!(check_manifests(&stack()), Vec::<String>::new());
    }

    #[test]
    fn downward_dependencies_pass_in_every_section() {
        let hud = r#"
            [package]
            name = "jc3vr-hud"
            [dependencies]
            jc3vr-gameplay = { path = "../jc3vr-gameplay" }
            render = { package = "jc3vr-render", path = "../jc3vr-render" }
            [dev-dependencies]
            jc3vr-core = { path = "../jc3vr-core", features = ["test-support"] }
            [build-dependencies]
            jc3vr-input = { path = "../jc3vr-input" }
            [target.'cfg(windows)'.dependencies]
            jc3vr-core = { path = "../jc3vr-core" }
        "#;
        assert_eq!(
            check_manifests(&stack_with(Layer::Hud, hud)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_missing_layer_crate_fails() {
        let mut manifests = stack();
        manifests.retain(|m| m.dir != "crates/jc3vr-tooling");
        let violations = check_manifests(&manifests);
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(
            violations[0]
                .starts_with("crates/jc3vr-tooling/Cargo.toml: the tooling layer has no crate")
        );
    }

    #[test]
    fn hud_and_tooling_have_no_edge_in_any_section() {
        let hud = r#"
            [package]
            name = "jc3vr-hud"
            [dependencies]
            jc3vr-tooling = { path = "../jc3vr-tooling" }
        "#;
        let tooling = r#"
            [package]
            name = "jc3vr-tooling"
            [dev-dependencies]
            hud = { package = "jc3vr-hud", path = "../jc3vr-hud" }
        "#;
        let mut manifests = stack_with(Layer::Hud, hud);
        manifests.retain(|m| m.dir != "crates/jc3vr-tooling");
        manifests.push(manifest("crates/jc3vr-tooling", tooling));
        let violations = check_manifests(&manifests);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains(
            "the hud crate depends on the tooling crate (`jc3vr-tooling` in [dependencies])"
        ));
        assert!(violations[0].contains("siblings with no edge between them"));
        assert!(
            violations[1].contains(
                "the tooling crate depends on the hud crate (`hud` in [dev-dependencies])"
            )
        );
    }

    #[test]
    fn upward_dependencies_fail_in_build_and_target_sections() {
        let render = r#"
            [package]
            name = "jc3vr-render"
            [build-dependencies]
            jc3vr-gameplay = { path = "../jc3vr-gameplay" }
            [target.'cfg(windows)'.dev-dependencies]
            jc3vr-ui = { path = "../jc3vr-ui" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Render, render));
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains("(`jc3vr-gameplay` in [build-dependencies])"));
        assert!(violations[0].contains("but the render and gameplay crates are siblings"));
        assert!(violations[1].contains("[target.cfg(windows).dev-dependencies]"));
    }

    #[test]
    fn gameplay_may_not_depend_on_render() {
        let gameplay = r#"
            [package]
            name = "jc3vr-gameplay"
            [dependencies]
            jc3vr-view = { path = "../jc3vr-view" }
            jc3vr-render = { path = "../jc3vr-render" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Gameplay, gameplay));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].contains("the gameplay crate depends on the render crate"));
        assert!(violations[0].contains("move the registration type or publication cell"));
    }

    #[test]
    fn render_and_hud_may_not_depend_on_capture() {
        let render = r#"
            [package]
            name = "jc3vr-render"
            [dev-dependencies]
            jc3vr-capture = { path = "../jc3vr-capture" }
        "#;
        let hud = r#"
            [package]
            name = "jc3vr-hud"
            [dependencies]
            jc3vr-capture = { path = "../jc3vr-capture" }
        "#;
        let mut manifests = stack_with(Layer::Render, render);
        manifests.retain(|m| m.dir != "crates/jc3vr-hud");
        manifests.push(manifest("crates/jc3vr-hud", hud));
        let violations = check_manifests(&manifests);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains("the render crate depends on the capture crate"));
        assert!(violations[0].contains("through a probe table"));
        assert!(violations[1].contains("the hud crate depends on the capture crate"));
    }

    #[test]
    fn render_may_not_depend_on_body() {
        let render = r#"
            [package]
            name = "jc3vr-render"
            [dependencies]
            jc3vr-view = { path = "../jc3vr-view" }
            jc3vr-body = { path = "../jc3vr-body" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Render, render));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].contains("the render crate depends on the body crate"));
        assert!(violations[0].contains("but the render and body crates are siblings"));
    }

    #[test]
    fn body_may_not_depend_on_gameplay() {
        let body = r#"
            [package]
            name = "jc3vr-body"
            [dependencies]
            jc3vr-view = { path = "../jc3vr-view" }
            [dev-dependencies]
            jc3vr-gameplay = { path = "../jc3vr-gameplay" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Body, body));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].contains("the body crate depends on the gameplay crate"));
        assert!(violations[0].contains("it may only depend on core, input, view"));
    }

    #[test]
    fn physical_may_not_depend_on_gameplay_or_render() {
        let physical = r#"
            [package]
            name = "jc3vr-physical"
            [dependencies]
            jc3vr-body = { path = "../jc3vr-body" }
            jc3vr-render = { path = "../jc3vr-render" }
            [dev-dependencies]
            jc3vr-gameplay = { path = "../jc3vr-gameplay" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Physical, physical));
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].contains("the physical crate depends on the render crate"));
        assert!(violations[0].contains("but the render and physical crates are siblings"));
        assert!(violations[1].contains("the physical crate depends on the gameplay crate"));
        assert!(violations[1].contains("it may only depend on core, input, view, body"));
    }

    #[test]
    fn core_and_input_depend_on_no_layer() {
        let core = "[package]\nname = \"jc3vr-core\"\n[target.'cfg(windows)'.dependencies]\njc3vr-input = \"0\"\n";
        let violations = check_manifests(&stack_with(Layer::Core, core));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(
            violations[0].contains("may only depend on no layer crate"),
            "{}",
            violations[0]
        );
    }

    #[test]
    fn no_layer_crate_depends_on_the_payload_by_any_name() {
        let ui = r#"
            [package]
            name = "jc3vr-ui"
            [dependencies]
            jc3vr_payload = { path = "../../payload" }
            [dev-dependencies]
            top = { path = "../../payload/" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Ui, ui));
        assert_eq!(violations.len(), 2, "{violations:#?}");
        for violation in &violations {
            assert!(
                violation.contains("but nothing depends on the payload"),
                "{violation}"
            );
        }
        assert!(violations[1].contains("(`top` in [dev-dependencies])"));
    }

    #[test]
    fn an_aliased_path_cannot_hide_a_layer() {
        let gameplay = r#"
            [package]
            name = "jc3vr-gameplay"
            [dependencies]
            widgets = { package = "widgets", path = "../jc3vr-hud" }
        "#;
        let violations = check_manifests(&stack_with(Layer::Gameplay, gameplay));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].contains("depends on the hud crate (`widgets` in [dependencies])"));
    }

    #[test]
    fn the_payload_must_stay_a_cdylib() {
        let payload = "[package]\nname = \"jc3vr_payload\"\n";
        let violations = check_manifests(&stack_with(Layer::Payload, payload));
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].contains("must stay a `cdylib`"));
    }

    #[test]
    fn names_and_directories_must_match_a_layer() {
        let misnamed = "[package]\nname = \"jc3vr-hud\"\n";
        let mut manifests = stack_with(Layer::Tooling, misnamed);
        manifests.push(manifest("crates/jc3vr-widgets", misnamed));
        manifests.push(manifest("crates/jc3vr-payload", misnamed));
        let violations = check_manifests(&manifests);
        assert_eq!(violations.len(), 3, "{violations:#?}");
        assert!(violations[0].contains("must be named `jc3vr-tooling`; rename the package"));
        assert!(violations[1].contains("is not a layer crate"));
        assert!(violations[2].contains("is not a layer crate"));
    }

    #[test]
    fn unparseable_manifests_are_reported() {
        let violations = check_manifests(&stack_with(Layer::Ui, "[package\n"));
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("crates/jc3vr-ui/Cargo.toml: could not parse"));
    }

    fn root(members: &[&str]) -> String {
        let members: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
        format!("[workspace]\nmembers = [{}]\n", members.join(", "))
    }

    fn layer_dirs() -> Vec<String> {
        Layer::ALL.into_iter().map(layer_dir).collect()
    }

    #[test]
    fn only_the_payload_is_a_cdylib_in_the_workspace() {
        let mut dirs = layer_dirs();
        dirs.push("tools/dxbc-tool".to_owned());
        let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
        let mut members = stack();
        members.push(manifest(
            "tools/dxbc-tool",
            "[package]\nname = \"dxbc-tool\"\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n",
        ));
        members.retain(|m| m.dir != "crates/jc3vr-render");
        members.push(manifest(
            "crates/jc3vr-render",
            "[package]\nname = \"jc3vr-render\"\n[lib]\ncrate-type = [\"cdylib\"]\n",
        ));
        let violations = check_workspace(&root(&dirs), &members);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations.iter().all(|v| v.contains("only the payload")));
        assert!(violations.iter().any(|v| v.starts_with("tools/dxbc-tool/")));
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("crates/jc3vr-render/"))
        );
    }

    #[test]
    fn every_layer_crate_is_a_workspace_member() {
        let dirs: Vec<String> = layer_dirs()
            .into_iter()
            .filter(|dir| dir != "crates/jc3vr-hud")
            .collect();
        let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
        let violations = check_workspace(&root(&dirs), &stack());
        assert_eq!(
            violations,
            vec![
                "Cargo.toml: the hud crate is not a workspace member; add \"crates/jc3vr-hud\" to \
                 [workspace] members"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn glob_members_are_refused() {
        let violations = check_workspace(&root(&["crates/*"]), &[]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("is a glob"));
    }

    #[test]
    fn hooks_must_not_name_gameplay_or_hud() {
        let files = [
            SourceFile {
                path: "crates/jc3vr-view/src/hooks/camera/mod.rs".into(),
                text: "use jc3vr_core::hooks::patcher;\n// jc3vr_hud::pointer in a comment.\nuse jc3vr_gameplay::aim;".into(),
            },
            SourceFile {
                path: "crates/jc3vr-render/src/hooks/graphics_engine/mod.rs".into(),
                text: "fn f() { jc3vr_hud::egui_panel::draw_quad(); }".into(),
            },
        ];
        let violations = check_hooks(&files);
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].starts_with("crates/jc3vr-view/src/hooks/camera/mod.rs:3: jc3vr_gameplay::aim names the gameplay crate"));
        assert!(violations[0].contains("add a registration point in the crate that owns the hook"));
        assert!(violations[1].contains("jc3vr_hud::egui_panel::draw_quad names the hud crate"));
    }
}
