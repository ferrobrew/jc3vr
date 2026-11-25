//! Versioned dock persistence and detached-view recovery.

mod writer;

use std::{fs, io, path::Path};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config,
    ui::{catalog::FeatureId, navigation::NavigationState},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum View {
    Workspace,
    Feature { key: String },
}

#[derive(Serialize, Deserialize)]
struct SavedLayout {
    version: u32,
    feature: String,
    dock: egui_dock::DockState<View>,
}

pub(crate) struct Layout {
    pub selection: NavigationState,
    pub dock: egui_dock::DockState<View>,
    pub drawer: bool,
    pub notice: String,
    save_enabled: bool,
    frames: u32,
    saved: Option<String>,
    saved_generation: u64,
}

pub(crate) static STATE: Mutex<Option<Layout>> = Mutex::new(None);

#[must_use = "an unfinished layout writer requires the payload to stay mapped"]
pub(crate) fn shutdown() -> bool {
    writer::shutdown()
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            selection: NavigationState::default(),
            dock: egui_dock::DockState::new(vec![View::Workspace]),
            drawer: false,
            notice: String::new(),
            save_enabled: true,
            frames: 0,
            saved: None,
            saved_generation: 0,
        }
    }
}

impl Layout {
    pub(crate) fn load() -> Self {
        Self::load_path(config::paths::file("ui_layout_v2.json").as_deref())
    }

    fn load_path(path: Option<&Path>) -> Self {
        match read_layout(path) {
            Ok(text) => Self::decode(text.as_deref()),
            Err(notice) => Self::blocked(notice),
        }
    }

    pub(crate) fn decode(current: Option<&str>) -> Self {
        if let Some(json) = current {
            return match decode_current(json) {
                Ok(saved) if saved.version == 2 => {
                    let mut layout = Self {
                        selection: NavigationState::recover(Some(&saved.feature)),
                        dock: saved.dock,
                        saved: Some(json.to_owned()),
                        ..Self::default()
                    };
                    // Layouts saved with per-section views can hold one feature more than once.
                    let mut seen = Vec::new();
                    layout.dock.retain_tabs(|view| {
                        let known = match view {
                            View::Workspace => true,
                            View::Feature { key } => FeatureId::from_key(key).is_some(),
                        };
                        let first = !seen.contains(view);
                        if known && first {
                            seen.push(view.clone());
                        }
                        known && first
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
        Self::default()
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
        };
        if self.dock.find_tab(&view).is_none() {
            self.dock.add_window(vec![view]);
        }
    }

    pub(crate) fn save(&mut self) {
        self.acknowledge(writer::acknowledgement());
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
            dock: self.dock.clone(),
        };
        let json = match serde_json::to_string(&saved) {
            Ok(json) => json,
            Err(error) => {
                self.notice = format!("UI layout: could not serialize the layout: {error}");
                return;
            }
        };
        let Some(path) = config::paths::file("ui_layout_v2.json") else {
            return;
        };
        if let Err(error) = writer::submit(path, json) {
            self.notice = error;
        }
    }

    fn acknowledge(&mut self, acknowledgement: writer::Acknowledgement) {
        if let Some(saved) = acknowledgement.saved
            && saved.generation > self.saved_generation
        {
            self.saved_generation = saved.generation;
            self.saved = Some(saved.text.to_string());
        }
        if let Some(error) = acknowledgement.error {
            self.notice = error;
        }
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

#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;
