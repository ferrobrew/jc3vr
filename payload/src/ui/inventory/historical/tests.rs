use super::*;
use crate::ui::inventory::{FeatureId, Surface, fingerprint, source_files};

#[test]
fn frozen_baseline_has_reviewed_current_source_targets() {
    let (baseline, mapping) = fixtures();
    assert_eq!(baseline.schema, 1);
    assert_eq!(mapping.schema, 1);
    assert_eq!(baseline.commit, "6eeb8e17e62f19f8b63fc649a6ad4be9923f97f8");
    assert_eq!(mapping.baseline_commit, baseline.commit);
    assert_eq!(baseline.scanner, "jc3vr-actionable-witness-v1");
    assert_eq!(mapping.scanner, "jc3vr-actionable-witness-v2");
    assert_eq!(baseline.controls.len(), 1405);
    assert_eq!(baseline.source_sha256.len(), 64);
    assert_eq!(
        baseline
            .controls
            .iter()
            .map(|row| &row.id)
            .collect::<HashSet<_>>()
            .len(),
        1405
    );
    assert_eq!(mapping.assignments.len(), 1405);
    assert_eq!(mapping.consolidations.len(), 4);
    assert!(baseline.source_sha256.values().all(|hash| hash.len() == 64));
    let sources: Vec<_> = source_files().copied().collect();
    let issues = parity(&baseline, &mapping, &sources);
    assert!(
        issues.is_empty(),
        "historical UI source parity: {issues:#?}"
    );
}

#[test]
fn reviewed_turn_choices_allow_only_the_equivalent_radio_setting() {
    let (baseline, mapping) = fixtures();
    for fingerprint in ["84fa260ad8330072", "2e4d621e225b242a"] {
        let row = mapping
            .assignments
            .iter()
            .find(|row| row.old_id.contains(fingerprint))
            .expect("reviewed turn choice");
        let old = baseline
            .controls
            .iter()
            .find(|old| old.id == row.old_id)
            .expect("historical turn choice");
        assert!(reviewed_kind(old, row, &mapping));
        let mut changed = row.clone();
        changed.consolidation = None;
        assert!(!reviewed_kind(old, &changed, &mapping));
        changed = row.clone();
        changed.kind = "Command".to_owned();
        assert!(!reviewed_kind(old, &changed, &mapping));
        changed = row.clone();
        changed.semantic.call = changed.semantic.call.replace("turn . mode", "turn . other");
        assert!(!reviewed_kind(old, &changed, &mapping));
        changed = row.clone();
        changed.semantic.effect = "{ another_action ( ) ; }".to_owned();
        assert!(!reviewed_kind(old, &changed, &mapping));
    }
}

#[test]
fn frozen_files_require_deliberate_review_to_change() {
    // These hashes freeze both obligations and reviewed owners independently of current sources.
    assert_eq!(
        fingerprint(include_bytes!("baseline.json")),
        BASELINE_FINGERPRINT
    );
    assert_eq!(
        fingerprint(include_bytes!("mapping.json")),
        MAPPING_FINGERPRINT
    );
}

#[test]
fn reviewed_owner_routes_retain_explicit_config_helper_roots() {
    let mapping: serde_json::Value =
        serde_json::from_str(include_str!("mapping.json")).expect("reviewed UI assignments");
    let review = &mapping["owner_route_review"];
    let changes = review["changes"]
        .as_array()
        .expect("reviewed owner changes");
    assert_eq!(review["changed_assignments"].as_u64(), Some(138));
    assert_eq!(changes.len(), 138);
    assert_eq!(
        changes
            .iter()
            .filter(|change| change["old_owners"] != change["new_owners"])
            .count(),
        137
    );
    assert_eq!(review["witness_changed_assignments"].as_u64(), Some(24));
    assert_eq!(
        changes
            .iter()
            .filter(|change| change["witness_changed"] == true)
            .count(),
        24
    );
    assert_eq!(review["config_root_assignments"].as_u64(), Some(21));
    let roots: Vec<_> = mapping["assignments"]
        .as_array()
        .expect("reviewed assignments")
        .iter()
        .filter(|row| row.get("config_root_review").is_some())
        .collect();
    assert_eq!(roots.len(), 21);
    let mut helpers = HashSet::new();
    for row in roots {
        let root = &row["config_root_review"];
        let path = root["path"].as_str().expect("config helper path");
        let function = root["function"].as_str().expect("config helper function");
        assert_eq!(root["path"], row["path"]);
        assert_eq!(root["root"], "config::Config");
        let signature = format!("fn {function}(ui: &mut egui::Ui, cfg: &mut Config)");
        let call = format!("{function}(ui, &mut config::edit());");
        assert_eq!(root["signature"], signature);
        assert_eq!(root["call"], call);
        let source = source_files()
            .find(|source| source.path == path)
            .expect("registered config helper source");
        assert!(source.text.contains(&signature), "{path}: {signature}");
        assert!(source.text.contains(&call), "{path}: {call}");
        let targets = root["targets"].as_array().expect("config helper targets");
        assert!(!targets.is_empty());
        for target in targets {
            assert!(
                row["semantic"]["call"]
                    .as_str()
                    .expect("retained primitive call")
                    .contains(target.as_str().expect("retained config target"))
            );
        }
        helpers.insert(function);
    }
    assert_eq!(helpers.len(), 5);
}

#[test]
fn scanner_witnesses_ignore_comments_and_function_names() {
    let old = witnesses(
        "fn old() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); }",
    );
    let moved = witnesses(
        "// ui.button(\"not a control\");\nfn relocated() { let c=&mut cfg.controllers; ui . checkbox ( & mut c . enabled , \"Enabled\" ); }",
    );
    assert_eq!(old, moved);
    assert_ne!(
        old,
        witnesses(
            "fn relocated() { let c = &mut cfg.other; ui.checkbox(&mut c.enabled, \"Enabled\"); }"
        )
    );
}

#[test]
fn macro_setting_targets_are_witnessed() {
    let before = witnesses("fn page() { config_toggle!(\"Enabled\", |c| c.controllers.enabled); }");
    let changed =
        witnesses("fn renamed() { config_toggle!(\"Enabled\", |c| c.controllers.other); }");
    assert_eq!(before.len(), 1);
    assert_ne!(before, changed);
}

#[test]
fn captured_pose_reset_checks_the_handler_after_the_hover_match() {
    let camera = include_str!("../../camera.rs");
    assert!(camera.contains(".on_hover_text(match vr::pose_control::base() {"));
    let (mut baseline, mut mapping) = fixtures();
    mapping.assignments.retain(|row| {
        row.path == "payload/src/ui/camera.rs"
            && row.semantic.call == "button ( \"Reset to captured pose\" )"
    });
    assert_eq!(mapping.assignments.len(), 1);
    let row = &mapping.assignments[0];
    assert!(row.semantic.effect.contains(". clicked ( ) {"));
    assert!(
        row.semantic
            .effect
            .contains("vr : : pose_control : : reset ( ) ;")
    );
    baseline.controls.retain(|old| old.id == row.old_id);
    mapping.consolidations.clear();
    let camera_source = SourceFile {
        path: "payload/src/ui/camera.rs",
        text: camera,
        surface: Surface::Debug,
        destinations: &[FeatureId::HeadView],
    };
    assert!(parity(&baseline, &mapping, &[camera_source]).is_empty());
    assert_eq!(camera.matches("vr::pose_control::reset();").count(), 1);
    for handler in ["", "vr::pose_control::set_current(values);"] {
        let changed = Box::leak(
            camera
                .replace("vr::pose_control::reset();", handler)
                .into_boxed_str(),
        );
        assert_eq!(
            parity(
                &baseline,
                &mapping,
                &[SourceFile {
                    text: changed,
                    ..camera_source
                }],
            ),
            vec![Issue::MissingTarget(mapping.assignments[0].old_id.clone())],
        );
    }
}

#[test]
fn nested_setter_closures_remain_part_of_the_witness() {
    let original = "fn page() { ui.add(Slider::new(&mut value, 0..=1).custom_formatter(|n, _| { format!(\"{n}\") })); }";
    let changed = original.replace("format!", "other_format!");
    let before = witnesses(original);
    assert!(before[0].1.effect.contains("format !"));
    assert_ne!(before, witnesses(&changed));
}

#[test]
fn source_mutations_cannot_remove_or_retarget_historical_controls() {
    let (baseline, mapping) = small_fixture();
    for changed in [
        "fn moved() { let c = &mut cfg.controllers; }",
        "fn moved() { let c = &mut cfg.controllers; ui.checkbox(&mut c.other, \"Enabled\"); if ui.button(\"Capture\").clicked() { wrong::capture(); } }",
        "fn moved() { let c = &mut cfg.other; ui.checkbox(&mut c.enabled, \"Enabled\"); if ui.button(\"Capture\").clicked() { sink::capture(); } }",
        "fn moved() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); if ui.button(\"Capture\").clicked() {} }",
    ] {
        assert!(
            !parity(&baseline, &mapping, &[source(changed)]).is_empty(),
            "{changed}"
        );
    }
    let moved = "fn moved() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); if ui.button(\"Capture\").clicked() { sink::capture(); } }";
    assert!(parity(&baseline, &mapping, &[source(moved)]).is_empty());
}

#[test]
fn assignments_and_consolidations_are_not_self_derived() {
    let (baseline, mut mapping) = small_fixture();
    let retained = mapping.assignments.pop().expect("command assignment");
    assert!(
        parity(&baseline, &mapping, &[source(ORIGINAL)])
            .contains(&Issue::MissingAssignment(retained.old_id.clone()))
    );
    mapping.assignments.push(retained.clone());
    mapping.assignments.push(retained.clone());
    assert!(
        parity(&baseline, &mapping, &[source(ORIGINAL)])
            .contains(&Issue::DuplicateAssignment(retained.old_id.clone()))
    );
    mapping.assignments.pop();
    mapping.assignments[0].old_id = "not-a-historical-id".to_owned();
    assert!(
        parity(&baseline, &mapping, &[source(ORIGINAL)])
            .iter()
            .any(|issue| matches!(issue, Issue::UnknownId(_)))
    );
}

#[test]
fn duplicate_current_targets_require_the_exact_approved_old_id_set() {
    let (mut baseline, mut mapping) = small_fixture();
    baseline.controls.push(Obligation {
        id: "duplicate-old-setting".to_owned(),
        path: "fixture.rs".to_owned(),
        kind: "Setting".to_owned(),
        semantic: baseline.controls[0].semantic.clone(),
    });
    let mut duplicate = mapping.assignments[0].clone();
    duplicate.old_id = "duplicate-old-setting".to_owned();
    mapping.assignments.push(duplicate);
    assert!(
        parity(&baseline, &mapping, &[source(ORIGINAL)])
            .iter()
            .any(|issue| matches!(issue, Issue::UnapprovedConsolidation(_)))
    );
    mapping.assignments[0].consolidation = Some("reviewed-duplicate".to_owned());
    mapping.assignments[2].consolidation = Some("reviewed-duplicate".to_owned());
    mapping.consolidations.push(Consolidation {
        id: "reviewed-duplicate".to_owned(),
        old_ids: vec!["setting".to_owned(), "duplicate-old-setting".to_owned()],
        reason: "Both old editors change the same setting.".to_owned(),
    });
    assert!(parity(&baseline, &mapping, &[source(ORIGINAL)]).is_empty());
    mapping.assignments[2].owners[0].section = Section::Status;
    assert!(
        parity(&baseline, &mapping, &[source(ORIGINAL)])
            .iter()
            .any(|issue| matches!(issue, Issue::UnapprovedConsolidation(_)))
    );
}

#[test]
fn extra_matching_current_owner_is_not_silently_selected() {
    let (baseline, mapping) = small_fixture();
    let duplicate = "fn first() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); if ui.button(\"Capture\").clicked() { sink::capture(); } } fn second() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); }";
    assert!(
        parity(&baseline, &mapping, &[source(duplicate)])
            .iter()
            .any(|issue| matches!(issue, Issue::TargetCount(_, 1, 2)))
    );
}

const ORIGINAL: &str = "fn old() { let c = &mut cfg.controllers; ui.checkbox(&mut c.enabled, \"Enabled\"); if ui.button(\"Capture\").clicked() { sink::capture(); } }";
const BASELINE_FINGERPRINT: u64 = 0x9b6099490bea7ce7;
const MAPPING_FINGERPRINT: u64 = 0x4919210693aa27c7;

fn source(text: &'static str) -> SourceFile {
    SourceFile {
        path: "fixture.rs",
        text,
        surface: Surface::Debug,
        destinations: &[FeatureId::Devices],
    }
}

fn small_fixture() -> (Baseline, Mapping) {
    let controls = witnesses(ORIGINAL);
    let obligations = controls
        .iter()
        .zip(["setting", "command"])
        .map(|((kind, semantic), id)| Obligation {
            id: id.to_owned(),
            path: "fixture.rs".to_owned(),
            kind: kind.clone(),
            semantic: semantic.clone(),
        })
        .collect::<Vec<_>>();
    let assignments = obligations
        .iter()
        .map(|old| Assignment {
            old_id: old.id.clone(),
            path: old.path.clone(),
            kind: old.kind.clone(),
            semantic: old.semantic.clone(),
            occurrence: 1,
            expected_matches: 1,
            owners: vec![Owner {
                feature: "Devices".to_owned(),
                section: Section::Controls,
            }],
            route_evidence: vec!["Reviewed fixture call.".to_owned()],
            review: "Same setting binding and command sink.".to_owned(),
            consolidation: None,
        })
        .collect();
    (
        Baseline {
            schema: 1,
            commit: "fixture".to_owned(),
            scanner: "fixture".to_owned(),
            source_sha256: HashMap::from([("fixture.rs".to_owned(), "hash".to_owned())]),
            controls: obligations,
        },
        Mapping {
            schema: 1,
            baseline_commit: "fixture".to_owned(),
            scanner: "fixture".to_owned(),
            consolidations: Vec::new(),
            assignments,
        },
    )
}
