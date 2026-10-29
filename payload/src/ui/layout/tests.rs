use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;

use super::*;

// A dock wire with finite geometry from a displayed dock; the tabs are replaced per test.
const DOCK_FIXTURE: &str = r#"{
    "surfaces": [
        {"Main": {"nodes": [{"Leaf": {
            "rect": {"min": {"x": 0.0, "y": 0.0}, "max": {"x": 1100.0, "y": 800.0}},
            "viewport": {"min": {"x": 0.0, "y": 24.0}, "max": {"x": 1100.0, "y": 800.0}},
            "tabs": ["Performance", "Render", "Previews", "Diagnostics", "Status", "Camera", "Game", "Hud", "Environment", "Vr", "Director", "Controllers", "Testing"],
            "active": 11, "scroll": 0.0, "collapsed": false
        }}], "focused_node": 0, "collapsed": false, "collapsed_leaf_count": 0}},
        {"Window": [{"nodes": [{"Leaf": {
            "rect": {"min": {"x": 50.0, "y": 50.0}, "max": {"x": 450.0, "y": 350.0}},
            "viewport": {"min": {"x": 50.0, "y": 74.0}, "max": {"x": 450.0, "y": 350.0}},
            "tabs": ["Performance"], "active": 0, "scroll": 0.0, "collapsed": false
        }}], "focused_node": 0, "collapsed": false, "collapsed_leaf_count": 0}, {
            "screen_rect": {"min": {"x": 50.0, "y": 50.0}, "max": {"x": 450.0, "y": 350.0}},
            "dragged": false, "next_position": null, "next_size": null,
            "expanded_height": null, "new": false, "minimized": false
        }]}
    ], "focused_surface": 1
}"#;

#[test]
fn corrupt_layout_is_not_silently_overwritten_and_reset_recovers() {
    let mut layout = Layout::decode(Some("invalid"));
    assert!(!layout.save_enabled);
    assert!(!layout.notice.is_empty());
    layout.reset();
    assert!(layout.save_enabled);
    assert_eq!(layout.selection, NavigationState::default());
    assert_eq!(layout.dock.iter_all_tabs().count(), 1);
}

fn current_wire() -> Value {
    let mut dock: Value = serde_json::from_str(DOCK_FIXTURE).expect("dock fixture JSON");
    dock["surfaces"][0]["Main"]["nodes"][0]["Leaf"]["tabs"] = json!(["Workspace"]);
    dock["surfaces"][1]["Window"][0]["nodes"][0]["Leaf"]["tabs"] = json!([
        {"Feature": {"key": "rendering.performance", "section": "Status"}},
        {"Feature": {"key": "removed.feature", "section": "Controls"}}
    ]);
    dock["surfaces"][1]["Window"][0]["nodes"][0]["Leaf"]["active"] = json!(1);
    json!({"version": 2, "feature": "removed.feature", "section": "Status", "dock": dock})
}

#[test]
fn current_layout_repairs_active_unknown_detached_tab_and_out_of_range_focus() {
    let mut wire = current_wire();
    wire["dock"]["focused_surface"] = json!(999);
    wire["dock"]["surfaces"][0]["Main"]["focused_node"] = json!(999);
    wire["dock"]["surfaces"][0]["Main"]["nodes"][0]["Leaf"]["active"] = json!(999);
    wire["dock"]["surfaces"][1]["Window"][0]["focused_node"] = json!(999);
    let layout = Layout::decode(Some(&wire.to_string()));
    assert!(layout.save_enabled, "{}", layout.notice);
    assert_eq!(layout.selection.feature, FeatureId::Session);
    assert_eq!(layout.dock.iter_all_tabs().count(), 2);
    let leaf = layout.dock[egui_dock::SurfaceIndex(1)]
        .root_node()
        .unwrap()
        .get_leaf()
        .unwrap();
    assert_eq!(leaf.active.0, 0);
    assert_eq!(leaf.tabs.len(), 1);
    assert_renderable(&layout.dock);
}

#[test]
fn per_section_views_of_one_feature_load_as_one_view() {
    let mut wire = current_wire();
    wire["dock"]["surfaces"][1]["Window"][0]["nodes"][0]["Leaf"]["tabs"] = json!([
        {"Feature": {"key": "rendering.performance", "section": "Status"}},
        {"Feature": {"key": "rendering.performance", "section": "Controls"}}
    ]);
    let layout = Layout::decode(Some(&wire.to_string()));
    assert!(layout.save_enabled, "{}", layout.notice);
    assert_eq!(layout.dock.iter_all_tabs().count(), 2);
    assert!(
        layout
            .dock
            .find_tab(&View::Feature {
                key: "rendering.performance".to_owned()
            })
            .is_some()
    );
    assert_renderable(&layout.dock);
}

#[test]
fn current_layout_restores_workspace_when_unknown_tabs_empty_the_main_tree() {
    let mut wire = current_wire();
    wire["dock"]["surfaces"][0]["Main"]["nodes"][0]["Leaf"]["tabs"] = json!([
        {"Feature": {"key": "removed.main", "section": "Status"}}
    ]);
    let layout = Layout::decode(Some(&wire.to_string()));
    assert!(layout.save_enabled, "{}", layout.notice);
    assert!(
        layout
            .dock
            .main_surface()
            .find_tab(&View::Workspace)
            .is_some()
    );
    assert_renderable(&layout.dock);
}

#[test]
fn malformed_dock_structures_are_rejected_before_rendering() {
    for kind in [
        "missing children",
        "wrong main",
        "unreachable node",
        "empty window",
    ] {
        let mut wire = current_wire();
        match kind {
            "missing children" => {
                wire["dock"]["surfaces"][0]["Main"]["nodes"][0] = json!({"Horizontal": {
                    "rect": {"min": {"x": 0.0, "y": 0.0}, "max": {"x": 10.0, "y": 10.0}},
                    "fraction": 0.5, "fully_collapsed": false, "collapsed_leaf_count": 0
                }})
            }
            "wrong main" => wire["dock"]["surfaces"][0] = json!("Empty"),
            "unreachable node" => {
                let node = wire["dock"]["surfaces"][0]["Main"]["nodes"][0].clone();
                wire["dock"]["surfaces"][0]["Main"]["nodes"]
                    .as_array_mut()
                    .unwrap()
                    .push(node);
            }
            _ => wire["dock"]["surfaces"][1]["Window"][0]["nodes"] = json!([]),
        }
        let layout = Layout::decode(Some(&wire.to_string()));
        assert!(!layout.save_enabled, "{kind}");
        assert!(!layout.notice.is_empty());
        assert_eq!(layout.dock.iter_all_tabs().count(), 1);
    }
}

#[test]
fn layout_read_errors_block_automatic_save_and_preserve_the_file() {
    let root = temporary_directory();
    let path = root.join("layout.json");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    let layout = Layout::load_path(Some(&path));
    assert!(!layout.save_enabled);
    assert!(layout.notice.contains(&path.display().to_string()));
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
    let directory = Layout::load_path(Some(&root));
    assert!(!directory.save_enabled);
    assert!(!directory.notice.is_empty());
    assert!(Layout::load_path(Some(&root.join("missing.json"))).save_enabled);
    fs::remove_dir_all(root).unwrap();
}

fn temporary_directory() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "jc3vr-layout-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn assert_renderable(dock: &egui_dock::DockState<View>) {
    let context = egui::Context::default();
    let mut dock = dock.clone();
    let mut selection = NavigationState::default();
    let mut reads = 0;
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        crate::ui::feature_dock(ui, &mut dock, &mut selection, false, |ui, _| {
            reads += 1;
            ui.label("Live snapshot");
        });
    });
    assert!(reads > 0);
}
