//! Versioned dock persistence, explicit legacy migration, and detached-view recovery.

use std::{fmt::Display, fs, io, path::Path};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    config,
    ui::{
        Tab,
        catalog::{FeatureId, FeatureSection},
        navigation::NavigationState,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum View {
    Workspace,
    Feature {
        key: String,
        section: FeatureSection,
    },
}

#[derive(Serialize, Deserialize)]
struct SavedLayout {
    version: u32,
    feature: String,
    section: FeatureSection,
    dock: egui_dock::DockState<View>,
}

pub(crate) struct Layout {
    pub selection: NavigationState,
    pub dock: egui_dock::DockState<View>,
    pub drawer: bool,
    pub notice: String,
    legacy: Option<egui_dock::DockState<Tab>>,
    save_enabled: bool,
    frames: u32,
    saved: Option<String>,
}

pub(crate) static STATE: Mutex<Option<Layout>> = Mutex::new(None);

impl Default for Layout {
    fn default() -> Self {
        Self {
            selection: NavigationState::default(),
            dock: egui_dock::DockState::new(vec![View::Workspace]),
            drawer: false,
            notice: String::new(),
            legacy: None,
            save_enabled: true,
            frames: 0,
            saved: None,
        }
    }
}

impl Layout {
    pub(crate) fn load() -> Self {
        Self::load_paths(
            config::paths::file("ui_layout_v2.json").as_deref(),
            config::paths::file("ui_layout.json").as_deref(),
        )
    }

    fn load_paths(current: Option<&Path>, legacy: Option<&Path>) -> Self {
        let current = match read_layout(current) {
            Ok(text) => text,
            Err(notice) => return Self::blocked(notice),
        };
        if current.is_some() {
            return Self::decode(current.as_deref(), None);
        }
        match read_layout(legacy) {
            Ok(legacy) => Self::decode(None, legacy.as_deref()),
            Err(notice) => Self::blocked(notice),
        }
    }

    pub(crate) fn decode(current: Option<&str>, legacy: Option<&str>) -> Self {
        if let Some(json) = current {
            return match decode_current(json) {
                Ok(saved) if saved.version == 2 => {
                    let mut layout = Self {
                        selection: NavigationState {
                            feature: FeatureId::from_key(&saved.feature).unwrap_or(FeatureId::Session),
                            section: saved.section,
                        },
                        dock: saved.dock,
                        saved: Some(json.to_owned()),
                        ..Self::default()
                    };
                    layout.dock.retain_tabs(|view| match view {
                        View::Workspace => true,
                        View::Feature { key, .. } => FeatureId::from_key(key).is_some(),
                    });
                    normalize_tabs(&mut layout.dock);
                    if layout.dock.main_surface().num_tabs() == 0 {
                        *layout.dock.main_surface_mut() = egui_dock::Tree::default();
                    }
                    layout.ensure_workspace();
                    layout
                }
                Ok(_) => Self::blocked("UI layout: unsupported saved version. Restore the default layout to replace it.".to_owned()),
                Err(error) => Self::blocked(format!("UI layout: unsafe or invalid saved layout: {error}. Restore the default layout to replace it.")),
            };
        }
        if let Some(json) = legacy {
            return match decode_dock(json) {
                Ok(dock) => Self {
                    legacy: Some(dock),
                    save_enabled: false,
                    notice: "An old dock layout is available. Migrate old layout preserves detached geometry; Restore default layout starts with one workspace. The old file remains unchanged.".to_owned(),
                    ..Self::default()
                },
                Err(error) => Self::blocked(format!("UI layout: unsafe or invalid old layout: {error}. Restore the default layout to start with one workspace; the old file remains unchanged.")),
            };
        }
        Self::default()
    }

    pub(crate) fn migrate_legacy(&mut self) {
        if let Some(legacy) = self.legacy.take() {
            self.dock = legacy.map_tabs(|tab| {
                let selection = NavigationState::recover(Some(&tab.to_string()));
                View::Feature {
                    key: selection.feature.definition().key.to_owned(),
                    section: legacy_section(*tab),
                }
            });
            normalize_tabs(&mut self.dock);
            self.ensure_workspace();
            self.save_enabled = true;
            self.notice =
                "The old layout is migrated. The original ui_layout.json remains unchanged."
                    .to_owned();
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn recover(&mut self) {
        let mut views = vec![View::Workspace];
        for (_, view) in self.dock.iter_all_tabs() {
            if !views.contains(view) {
                views.push(view.clone());
            }
        }
        self.dock = egui_dock::DockState::new(views);
        self.save_enabled = true;
        self.notice =
            "All views are in the main dock. Drag a tab to detach or re-dock it.".to_owned();
    }

    pub(crate) fn detach(&mut self, selection: NavigationState) {
        let view = View::Feature {
            key: selection.feature.definition().key.to_owned(),
            section: selection.section,
        };
        if self.dock.find_tab(&view).is_none() {
            self.dock.add_window(vec![view]);
        }
    }

    pub(crate) fn save(&mut self) {
        if !self.save_enabled {
            return;
        }
        self.frames += 1;
        if self.frames < 60 {
            return;
        }
        self.frames = 0;
        let saved = SavedLayout {
            version: 2,
            feature: self.selection.feature.definition().key.to_owned(),
            section: self.selection.section,
            dock: self.dock.clone(),
        };
        let json = match serde_json::to_string(&saved) {
            Ok(json) => json,
            Err(error) => {
                self.notice = format!("UI layout: could not serialize the layout: {error}");
                return;
            }
        };
        if self.saved.as_deref() == Some(json.as_str()) {
            return;
        }
        let Some(path) = config::paths::file("ui_layout_v2.json") else {
            return;
        };
        self.write_json(&path, json, config::write::replace);
    }

    fn write_json<E: Display>(
        &mut self,
        path: &Path,
        json: String,
        replace: impl FnOnce(&Path, &[u8]) -> Result<(), E>,
    ) {
        match replace(path, json.as_bytes()) {
            Ok(()) => self.saved = Some(json),
            Err(error) => {
                self.notice = format!("UI layout: could not replace {}: {error}", path.display())
            }
        }
    }

    pub(crate) fn has_legacy(&self) -> bool {
        self.legacy.is_some()
    }

    fn blocked(notice: String) -> Self {
        Self {
            notice,
            save_enabled: false,
            ..Self::default()
        }
    }

    fn ensure_workspace(&mut self) {
        if self.dock.find_tab(&View::Workspace).is_none() {
            self.dock
                .main_surface_mut()
                .push_to_first_leaf(View::Workspace);
        }
    }
}

fn read_layout(path: Option<&Path>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "UI layout: could not read {}: {error}. Automatic saving is blocked until explicit recovery or reset.",
            path.display()
        )),
    }
}

fn decode_current(json: &str) -> Result<SavedLayout, String> {
    let mut wire: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    normalize_wire(wire.get_mut("dock").ok_or("missing dock")?)?;
    serde_json::from_value(wire).map_err(|error| error.to_string())
}

fn decode_dock<T: DeserializeOwned>(json: &str) -> Result<egui_dock::DockState<T>, String> {
    let mut wire: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    normalize_wire(&mut wire)?;
    serde_json::from_value(wire).map_err(|error| error.to_string())
}

// Focus references are private to egui-dock and become stale when retain_tabs compacts surfaces.
fn normalize_wire(dock: &mut Value) -> Result<(), String> {
    let dock = dock.as_object_mut().ok_or("dock is not an object")?;
    dock.insert("focused_surface".to_owned(), Value::Null);
    let surfaces = dock
        .get_mut("surfaces")
        .and_then(Value::as_array_mut)
        .ok_or("missing surfaces")?;
    if surfaces.is_empty() || surfaces.len() > 256 {
        return Err("invalid surface count".to_owned());
    }
    for (index, surface) in surfaces.iter_mut().enumerate() {
        if index != 0 && surface.as_str() == Some("Empty") {
            continue;
        }
        let tree = if index == 0 {
            surface
                .get_mut("Main")
                .ok_or("the first surface is not Main")?
        } else {
            surface
                .get_mut("Window")
                .and_then(Value::as_array_mut)
                .filter(|window| window.len() == 2)
                .and_then(|window| window.first_mut())
                .ok_or("a detached surface is not a window")?
        };
        let object = tree.as_object_mut().ok_or("tree is not an object")?;
        object.insert("focused_node".to_owned(), Value::Null);
        object.insert("collapsed".to_owned(), Value::Bool(false));
        object.insert("collapsed_leaf_count".to_owned(), Value::from(0));
        let nodes = object
            .get_mut("nodes")
            .and_then(Value::as_array_mut)
            .ok_or("missing nodes")?;
        if nodes.len() > 4096 {
            return Err("too many dock nodes".to_owned());
        }
        for node_index in 0..nodes.len() {
            let parent_is_split = node_index == 0
                || nodes[(node_index - 1) / 2].get("Horizontal").is_some()
                || nodes[(node_index - 1) / 2].get("Vertical").is_some();
            let node = &mut nodes[node_index];
            if node.as_str() == Some("Empty") {
                if node_index == 0 || parent_is_split {
                    return Err("empty node in a live dock branch".to_owned());
                }
                continue;
            }
            if !parent_is_split {
                return Err("unreachable dock node".to_owned());
            }
            if let Some(leaf) = node.get_mut("Leaf") {
                let leaf = leaf.as_object_mut().ok_or("invalid leaf")?;
                let count = leaf
                    .get("tabs")
                    .and_then(Value::as_array)
                    .ok_or("missing leaf tabs")?
                    .len();
                if count == 0 {
                    return Err("empty dock leaf".to_owned());
                }
                let active = leaf.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
                leaf.insert(
                    "active".to_owned(),
                    Value::from(if active < count { active } else { 0 }),
                );
                leaf.insert("collapsed".to_owned(), Value::Bool(false));
            } else {
                let kind = if node.get("Horizontal").is_some() {
                    "Horizontal"
                } else {
                    "Vertical"
                };
                let split = node.get_mut(kind).ok_or("unknown dock node")?;
                let split = split.as_object_mut().ok_or("invalid split")?;
                let fraction = split
                    .get("fraction")
                    .and_then(Value::as_f64)
                    .ok_or("invalid split fraction")?;
                if !(0.0..=1.0).contains(&fraction) {
                    return Err("split fraction is out of range".to_owned());
                }
                split.insert("fully_collapsed".to_owned(), Value::Bool(false));
                split.insert("collapsed_leaf_count".to_owned(), Value::from(0));
                if node_index * 2 + 2 >= nodes.len() {
                    return Err("split is missing children".to_owned());
                }
            }
        }
        if index != 0
            && !nodes.iter().any(|node| {
                node.get("Leaf")
                    .and_then(|leaf| leaf.get("tabs"))
                    .and_then(Value::as_array)
                    .is_some_and(|tabs| !tabs.is_empty())
            })
        {
            return Err("empty detached tree".to_owned());
        }
    }
    Ok(())
}

fn normalize_tabs<T>(dock: &mut egui_dock::DockState<T>) {
    for surface in dock.iter_surfaces_mut() {
        if let Some(tree) = surface.node_tree_mut() {
            tree.set_focused_node(egui_dock::NodeIndex(usize::MAX));
            for node in tree.iter_mut() {
                if let Some(leaf) = node.get_leaf_mut()
                    && leaf.active.0 >= leaf.tabs.len()
                {
                    leaf.active = egui_dock::TabIndex(0);
                }
            }
        }
    }
}

fn legacy_section(tab: Tab) -> FeatureSection {
    match tab {
        Tab::Performance | Tab::Status | Tab::Diagnostics => FeatureSection::Status,
        Tab::Previews => FeatureSection::Visualizations,
        _ => FeatureSection::Controls,
    }
}

#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;
