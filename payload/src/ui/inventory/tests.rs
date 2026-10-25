use std::collections::HashSet;

use super::*;

#[test]
fn actual_control_inventory_has_unique_ids_and_source_evidence() {
    let inventory = existing_controls();
    assert!(!inventory.is_empty());
    let ids: HashSet<_> = inventory.iter().map(|control| &control.id).collect();
    assert_eq!(ids.len(), inventory.len());
    for control in &inventory {
        assert!(control.line > 0);
        assert!(!control.function.is_empty());
        assert!(!control.arguments.is_empty());
        assert!(!control.candidate_destinations.is_empty());
    }
    for (path, needle, kind) in [
        ("controllers/settings.rs", "c.enabled", ControlKind::Setting),
        ("director.rs", "Save bookmark here", ControlKind::Command),
        ("testing.rs", "Start recording", ControlKind::Command),
        (
            "controllers/body/menu/pages.rs",
            "MATCH CHARACTER HEIGHT",
            ControlKind::Setting,
        ),
    ] {
        assert!(
            inventory
                .iter()
                .any(|control| control.source.ends_with(path)
                    && control.arguments.contains(needle)
                    && control.kind == kind),
            "{path}: {needle}"
        );
    }
    assert!(
        inventory
            .iter()
            .any(|control| control.surface == Surface::Pause)
    );
    assert!(source_files().any(|source| source.text.contains("#[cfg(feature = \"profiler\")]")));
}

#[test]
fn actual_constructor_controls_are_present_and_required_by_migration_parity() {
    let inventory = existing_controls();
    let mut constructor_ids = HashSet::new();
    for (path, needle, kind) in [
        (
            "controllers/diagnostics.rs",
            "Capture controller diagnostics",
            ControlKind::Command,
        ),
        ("controllers/sights.rs", "\"Stop\"", ControlKind::Command),
        ("controllers/sights.rs", "\"Capture\"", ControlKind::Command),
        (
            "controllers/sights.rs",
            "Reset to the table",
            ControlKind::Command,
        ),
        (
            "controllers/sights.rs",
            "Reset to the fire point",
            ControlKind::Command,
        ),
        (
            "controllers/weapons.rs",
            "h.second_weapon",
            ControlKind::Setting,
        ),
        (
            "controllers/weapons.rs",
            "h.per_hand_tick",
            ControlKind::Setting,
        ),
        (
            "render/corrections.rs",
            "cfg.stereo.share_prepasses",
            ControlKind::Setting,
        ),
        ("testing.rs", "\"Replay\"", ControlKind::Command),
        ("testing.rs", "\"Run\"", ControlKind::Command),
    ] {
        let matching: Vec<_> = inventory
            .iter()
            .filter(|control| {
                control.source.ends_with(path)
                    && control.arguments.contains(needle)
                    && control.kind == kind
            })
            .collect();
        assert_eq!(matching.len(), 1, "{path}: {needle}");
        constructor_ids.insert(matching[0].id.clone());
    }
    assert_eq!(constructor_ids.len(), 10);
    let assignments: Vec<_> = inventory
        .iter()
        .map(|control| MigrationAssignment {
            control: control.id.clone(),
            destination: control.candidate_destinations[0],
            section: FeatureSection::Controls,
        })
        .collect();
    assert!(migration_parity(&inventory, &assignments).is_empty());
    for omitted in constructor_ids {
        let retained: Vec<_> = assignments
            .iter()
            .filter(|assignment| assignment.control != omitted)
            .cloned()
            .collect();
        assert_eq!(
            migration_parity(&inventory, &retained),
            vec![ParityIssue::Missing(omitted)]
        );
    }
}

#[test]
fn scanner_maps_qualified_and_imported_widget_constructors_without_expanding_code() {
    let source = SourceFile {
        path: "fixture.rs",
        text: r###"fn show() {
            // egui::Button::new("Not a control");
            let text = "Checkbox::new(&mut state, label)";
            ui.add_enabled(ready, egui::Button::new("Qualified command"));
            ui.add(Button::new("Imported command"));
            ui.add(egui::Checkbox::new(&mut state, "Qualified setting"));
            ui.add(Checkbox::new(&mut state, "Imported setting"));
        }"###,
        surface: Surface::Debug,
        destinations: &[FeatureId::Devices],
    };
    let inventory = scan_source(&source);
    assert_eq!(inventory.len(), 4);
    assert_eq!(
        inventory
            .iter()
            .filter(|control| control.kind == ControlKind::Command)
            .count(),
        2
    );
    assert_eq!(
        inventory
            .iter()
            .filter(|control| control.kind == ControlKind::Setting)
            .count(),
        2
    );
    assert!(inventory.iter().all(|control| control.function == "show"));
}

#[test]
fn control_ids_survive_whitespace_and_unrelated_controls() {
    let source = SourceFile {
        path: "fixture.rs",
        text: "fn show() { ui.checkbox(&mut c.enabled, \"Enabled\"); }",
        surface: Surface::Debug,
        destinations: &[FeatureId::Devices],
    };
    let changed = SourceFile {
        text: "// ui.button(\"not a control\");\nfn show() { ui.button(\"Other\");\n ui . checkbox ( & mut c . enabled , \"Enabled\" ) ; }",
        ..source
    };
    let before = scan_source(&source);
    let after = scan_source(&changed);
    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 2);
    assert_eq!(before[0].id, after[1].id);
}

#[test]
fn scanner_ignores_comments_and_strings_with_control_syntax() {
    let source = SourceFile {
        path: "fixture.rs",
        text: r###"fn show() {
            /* ui.button("not a control"); /* nested */ */
            let text = r#"ui.checkbox(&mut state, "not a control")"#;
            ui.button("Actual");
        }"###,
        surface: Surface::Debug,
        destinations: &[FeatureId::Devices],
    };
    let inventory = scan_source(&source);
    assert_eq!(inventory.len(), 1);
    assert!(inventory[0].arguments.contains("Actual"));
}

#[test]
fn migration_parity_requires_each_existing_control_once() {
    let source = SourceFile {
        path: "fixture.rs",
        text: "fn show() { ui.button(\"Go\"); ui.checkbox(&mut enabled, \"Enable\"); }",
        surface: Surface::Debug,
        destinations: &[FeatureId::DirectorSpawn],
    };
    let inventory = scan_source(&source);
    assert_eq!(migration_parity(&inventory, &[]).len(), 2);
    let assignments: Vec<_> = inventory
        .iter()
        .map(|control| MigrationAssignment {
            control: control.id.clone(),
            destination: FeatureId::DirectorSpawn,
            section: FeatureSection::Controls,
        })
        .collect();
    assert!(migration_parity(&inventory, &assignments).is_empty());
    assert_eq!(assignments[0].section, FeatureSection::Controls);
    let mut invalid = assignments.clone();
    invalid.push(assignments[0].clone());
    assert!(matches!(
        migration_parity(&inventory, &invalid).as_slice(),
        [ParityIssue::Duplicate(_)]
    ));
    invalid[0].destination = FeatureId::Reload;
    assert!(
        migration_parity(&inventory, &invalid)
            .iter()
            .any(|issue| matches!(
                issue,
                ParityIssue::UnexpectedDestination(_, FeatureId::Reload)
            ))
    );
}

#[test]
fn approved_feature_catalog_has_unique_stable_keys_and_source_coverage() {
    let keys: HashSet<_> = FEATURES.iter().map(|feature| feature.key).collect();
    assert_eq!(keys.len(), FEATURES.len());
    for page in PageId::ALL {
        assert!(FEATURES.iter().any(|feature| feature.page == page));
    }
    for feature in FEATURES {
        assert_eq!(FeatureId::from_key(feature.key), Some(feature.id));
        assert!(
            source_files().any(|source| source.destinations.contains(&feature.id))
                || feature.id == FeatureId::ActiveTools,
            "{}",
            feature.key
        );
    }
    for feature in [
        FeatureId::DirectorSafety,
        FeatureId::DirectorSpawn,
        FeatureId::Environment,
        FeatureId::Sandbox,
        FeatureId::Engine,
    ] {
        assert_eq!(feature.definition().page, PageId::Capture);
    }
}

#[test]
fn source_registration_covers_current_ui_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let registered: HashSet<_> = source_files().map(|source| source.path).collect();
    for relative in legacy_files(&root, &root) {
        assert!(
            registered.contains(format!("payload/src/ui/{relative}").as_str()),
            "{relative}"
        );
    }
}

fn legacy_files(root: &std::path::Path, directory: &std::path::Path) -> Vec<String> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(directory).expect("UI source directory") {
        let path = entry.expect("source entry").path();
        let relative = path
            .strip_prefix(root)
            .expect("source under UI root")
            .to_string_lossy()
            .replace('\\', "/");
        if matches!(
            relative.split('/').next(),
            Some(
                "capture"
                    | "inventory"
                    | "navigation"
                    | "layout"
                    | "dispatch"
                    | "overview"
                    | "visualizations"
            )
        ) {
            continue;
        }
        if path.is_dir() {
            result.extend(legacy_files(root, &path));
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !matches!(
                relative.as_str(),
                "capture.rs"
                    | "inventory.rs"
                    | "navigation.rs"
                    | "layout.rs"
                    | "dispatch.rs"
                    | "util.rs"
            )
        {
            result.push(relative);
        }
    }
    result
}
