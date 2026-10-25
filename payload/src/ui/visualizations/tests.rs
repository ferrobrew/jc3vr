//! Renderer-free checks against the concrete visualization configuration fields.

use std::collections::HashSet;

use super::*;

fn serialized(config: &Config) -> serde_json::Value {
    serde_json::to_value(config).expect("config serializes")
}

fn snapshot_with(config: Config) -> Snapshot {
    Snapshot {
        config,
        runtime: Runtime {
            controllers_scheme: true,
            gameplay: true,
            ..Runtime::default()
        },
    }
}

#[test]
fn each_inventory_switch_edits_only_its_concrete_config_field() {
    let paths = [
        (ViewId::BodyMirror, "/controllers/overlay/body_mirror"),
        (
            ViewId::ControllerGizmos,
            "/controllers/overlay/debug_gizmos",
        ),
        (ViewId::GhostHands, "/controllers/overlay/ghost_hands"),
        (ViewId::GripMarkers, "/controllers/overlay/markers"),
        (
            ViewId::GrappleHighlight,
            "/controllers/overlay/grapple_highlight",
        ),
        (ViewId::GrappleBeam, "/controllers/overlay/grapple_beam"),
        (
            ViewId::GrabView,
            "/controllers/physical_hands/grab_view/enabled",
        ),
        (
            ViewId::GrabRays,
            "/controllers/physical_hands/grab_view/rays",
        ),
        (
            ViewId::GrabTargets,
            "/controllers/physical_hands/grab_view/targets",
        ),
        (
            ViewId::GrabWrist,
            "/controllers/physical_hands/grab_view/wrist",
        ),
        (
            ViewId::GrabRefusals,
            "/controllers/physical_hands/grab_view/refusals",
        ),
        (
            ViewId::GrabCandidates,
            "/controllers/physical_hands/grab_view/candidates",
        ),
        (
            ViewId::ColliderView,
            "/controllers/physical_hands/collider_view/enabled",
        ),
    ];
    assert_eq!(paths.len(), ENTRIES.len());
    let mapping: serde_json::Value =
        serde_json::from_str(include_str!("../inventory/historical/mapping.json"))
            .expect("reviewed historical assignments");
    let baseline: serde_json::Value =
        serde_json::from_str(include_str!("../inventory/historical/baseline.json"))
            .expect("immutable historical obligations");
    let aliases: Vec<_> = mapping["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row.get("visualization_review").is_some())
        .collect();
    assert_eq!(aliases.len(), paths.len());
    let expected_history = [
        (ViewId::BodyMirror, "f593aeed6a82e727", "o . body_mirror"),
        (
            ViewId::ControllerGizmos,
            "18ac1a5979ed17e8",
            "o . debug_gizmos",
        ),
        (ViewId::GhostHands, "eb116c75ebb8f05d", "o . ghost_hands"),
        (ViewId::GripMarkers, "e5631bede5185e9e", "o . markers"),
        (
            ViewId::GrappleHighlight,
            "e371f60c3ea3de4e",
            "o . grapple_highlight",
        ),
        (ViewId::GrappleBeam, "0950e10e1028fb7b", "o . grapple_beam"),
        (ViewId::GrabView, "48a58f29f46b59fb", "v . enabled"),
        (ViewId::GrabRays, "1031d7c328c933e0", "v . rays"),
        (ViewId::GrabTargets, "dcc09445898c8c30", "v . targets"),
        (ViewId::GrabWrist, "39bf24b5bc1905db", "v . wrist"),
        (ViewId::GrabRefusals, "26066b0b6adc71bf", "v . refusals"),
        (ViewId::GrabCandidates, "614fb9037437d012", "v . candidates"),
        (ViewId::ColliderView, "d4d9719026b08225", "v . enabled"),
    ];
    assert_eq!(expected_history.len(), paths.len());
    for (id, fingerprint, old_target) in expected_history {
        let rows: Vec<_> = aliases
            .iter()
            .filter(|row| row["old_id"].as_str().unwrap().contains(fingerprint))
            .collect();
        assert_eq!(rows.len(), 1, "{id:?}");
        let row = rows[0];
        let review = &row["visualization_review"];
        assert_eq!(review["view"], format!("{id:?}"));
        assert_eq!(
            review["config_path"],
            paths.iter().find(|(view, _)| *view == id).unwrap().1
        );
        let entry = ENTRIES.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(review["owner"], format!("{:?}", entry.feature));
        assert_eq!(row["path"], "payload/src/ui/visualizations.rs");
        assert_eq!(row["kind"], "Setting");
        assert_eq!(row["consolidation"], "canonical-visualization-switches");
        let old = baseline["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|old| old["id"] == row["old_id"])
            .expect("retained historical obligation");
        assert!(
            old["semantic"]["call"]
                .as_str()
                .unwrap()
                .contains(old_target)
        );
    }
    for (id, path) in paths {
        let mut config = Config::new();
        for value in [false, true] {
            let mut expected = serialized(&config);
            *expected.pointer_mut(path).expect("concrete config field") = value.into();
            apply_edits(&mut config, &[Edit::Switch(id, value)]);
            assert_eq!(serialized(&config), expected, "{path}");
            assert_eq!(id.requested(&config), value, "{path}");
        }
    }
}

#[test]
fn filtering_names_groups_and_feature_subsets_preserves_the_saved_config() {
    let config = Config::new();
    let before = serialized(&config);
    assert_eq!(
        matching(&config, "BODY MIRROR", None)[0].id,
        ViewId::BodyMirror
    );
    let cues = matching(&config, "gameplay cues", None);
    assert!(!cues.is_empty());
    assert!(cues.iter().all(|entry| entry.group == Group::GameplayCues));
    let hands = matching(&config, "", Some(FeatureId::PhysicalHands));
    assert!(!hands.is_empty());
    assert!(
        hands
            .iter()
            .all(|entry| entry.feature == FeatureId::PhysicalHands)
    );
    assert!(matching(&config, "not a view", None).is_empty());
    assert_eq!(before, serialized(&config));
}

#[test]
fn feature_subsets_are_exact_disjoint_and_cover_the_inventory() {
    let config = Config::new();
    let expected: [(FeatureId, &[ViewId]); 3] = [
        (
            FeatureId::RootWalls,
            &[ViewId::BodyMirror, ViewId::ControllerGizmos],
        ),
        (
            FeatureId::PhysicalHands,
            &[
                ViewId::GhostHands,
                ViewId::GripMarkers,
                ViewId::GrabView,
                ViewId::GrabRays,
                ViewId::GrabTargets,
                ViewId::GrabWrist,
                ViewId::GrabRefusals,
                ViewId::GrabCandidates,
                ViewId::ColliderView,
            ],
        ),
        (
            FeatureId::Grapple,
            &[ViewId::GrappleHighlight, ViewId::GrappleBeam],
        ),
    ];
    let mut union = HashSet::new();
    for (feature, ids) in expected {
        let entries = matching(&config, "", Some(feature));
        let actual: HashSet<_> = entries.iter().map(|entry| entry.id).collect();
        assert_eq!(entries.len(), ids.len(), "{feature:?}");
        assert_eq!(actual, ids.iter().copied().collect(), "{feature:?}");
        for entry in entries {
            assert_eq!(entry.feature, feature);
            assert!(union.insert(entry.id), "duplicate owner for {:?}", entry.id);
        }
    }
    assert_eq!(union.len(), 13);
    assert_eq!(union, ENTRIES.iter().map(|entry| entry.id).collect());
    for feature in [FeatureId::Previews, FeatureId::RenderDiagnostics] {
        assert!(matching(&config, "", Some(feature)).is_empty());
    }
}

#[test]
fn shared_controls_show_only_owner_rows_without_global_controls_for_a_feature() {
    let snapshot = snapshot_with(Config::new());
    let before = serialized(&snapshot.config);
    for feature in [
        Some(FeatureId::RootWalls),
        Some(FeatureId::PhysicalHands),
        Some(FeatureId::Grapple),
        None,
    ] {
        let context = egui::Context::default();
        let mut labels = Vec::new();
        for _ in 0..2 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 4000.0),
                    )),
                    ..Default::default()
                },
                |ui| controls(ui, &snapshot, feature),
            );
            labels.clear();
            for clipped in &output.shapes {
                collect_text(&clipped.shape, &mut labels);
            }
        }
        for entry in ENTRIES {
            assert_eq!(
                labels.iter().filter(|label| *label == entry.label).count(),
                usize::from(feature.is_none_or(|feature| feature == entry.feature)),
                "{feature:?}: {}",
                entry.label,
            );
        }
        for label in [
            "Shared overlay settings",
            "Hide registered views (including Gameplay cues)",
            "Restore registered views",
        ] {
            assert_eq!(
                labels.iter().any(|text| text == label),
                feature.is_none(),
                "{feature:?}: {label}"
            );
        }
        assert_eq!(
            labels.iter().any(|text| text.starts_with("Inventory:")),
            feature.is_none()
        );
    }
    assert_eq!(serialized(&snapshot.config), before);
}

fn collect_text(shape: &egui::Shape, labels: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => labels.push(text.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, labels);
            }
        }
        _ => {}
    }
}

#[test]
fn requested_switches_precede_disabled_switches() {
    let mut config = Config::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    config.controllers.overlay.body_mirror = true;
    config.controllers.overlay.grapple_beam = true;
    let entries = matching(&config, "", None);
    assert_eq!(entries[0].id, ViewId::BodyMirror);
    assert_eq!(entries[1].id, ViewId::GrappleBeam);
    assert!(
        entries[2..]
            .iter()
            .all(|entry| !entry.id.requested(&config))
    );
}

#[test]
fn checked_children_report_the_actual_parent_blocker() {
    let mut snapshot = snapshot_with(Config::new());
    snapshot.config.controllers.enabled = true;
    snapshot.config.controllers.overlay.enabled = true;
    snapshot.config.controllers.physical_hands.grab_view.enabled = false;
    snapshot.config.controllers.physical_hands.grab_view.rays = true;
    assert_eq!(
        snapshot.state(ViewId::GrabRays),
        ViewState::Blocked(Blocker::GrabView)
    );
    snapshot.config.controllers.overlay.enabled = false;
    assert_eq!(
        snapshot.state(ViewId::GrabRays),
        ViewState::Blocked(Blocker::Overlay)
    );
    snapshot.config.controllers.enabled = false;
    assert_eq!(
        snapshot.state(ViewId::GrabRays),
        ViewState::Blocked(Blocker::Controllers)
    );
}

#[test]
fn requested_does_not_imply_a_verified_draw_or_active_count() {
    let mut snapshot = snapshot_with(Config::new());
    snapshot.config.controllers.enabled = true;
    snapshot.config.controllers.overlay.enabled = true;
    snapshot.config.controllers.overlay.body_mirror = true;
    assert_eq!(
        snapshot.state(ViewId::BodyMirror),
        ViewState::DrawUnverified
    );
    assert_eq!(snapshot.summary().active_count(), None);
    snapshot.config.controllers.overlay.enabled = false;
    assert_eq!(snapshot.summary().active_count(), Some(0));
    assert_eq!(snapshot.summary().blocked, snapshot.summary().requested);
}

#[test]
fn retained_overlay_error_does_not_block_requested_visualizations() {
    let mut config = Config::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    config.controllers.enabled = true;
    config.controllers.overlay.enabled = true;
    config.controllers.overlay.debug_gizmos = true;
    let before = serialized(&config);
    let mut snapshot = snapshot_with(config);
    snapshot.runtime.overlay.error = Some("previous draw failure".to_owned());

    assert_eq!(
        snapshot.state(ViewId::ControllerGizmos),
        ViewState::DrawUnverified
    );
    assert_eq!(snapshot.summary().requested, 1);
    assert_eq!(snapshot.summary().blocked, 0);
    assert_eq!(snapshot.summary().draw_unverified, 1);
    assert_eq!(snapshot.summary().active_count(), None);
    assert_eq!(serialized(&snapshot.config), before);
}

#[test]
fn the_inventory_maps_every_switch_to_the_exact_eight_output_sources() {
    let expected = [
        (ViewId::BodyMirror, Source::BodyMirror),
        (ViewId::ControllerGizmos, Source::DebugGizmos),
        (ViewId::GhostHands, Source::GhostHands),
        (ViewId::GripMarkers, Source::GrabMarkers),
        (ViewId::GrappleHighlight, Source::GrappleHighlight),
        (ViewId::GrappleBeam, Source::GrappleBeam),
        (ViewId::GrabView, Source::GrabView),
        (ViewId::GrabRays, Source::GrabView),
        (ViewId::GrabTargets, Source::GrabView),
        (ViewId::GrabWrist, Source::GrabView),
        (ViewId::GrabRefusals, Source::GrabView),
        (ViewId::GrabCandidates, Source::GrabView),
        (ViewId::ColliderView, Source::ColliderView),
    ];
    assert_eq!(ENTRIES.len(), 13);
    assert_eq!(expected.len(), ENTRIES.len());
    for (id, source) in expected {
        assert_eq!(ENTRIES.iter().filter(|entry| entry.id == id).count(), 1);
        assert_eq!(id.source(), source);
    }
    assert_eq!(
        ENTRIES
            .iter()
            .filter(|entry| entry.id.source() == Source::GrabView)
            .count(),
        6
    );
    let config = Config::new();
    let mask = TemporaryHide::default().hide(&config);
    assert_eq!(
        Source::ALL
            .into_iter()
            .filter(|source| mask.contains(*source))
            .count(),
        8
    );
    for source in Source::ALL {
        assert_eq!(
            mask.contains(source),
            expected.iter().any(|(_, mapped)| *mapped == source)
        );
    }
}

#[test]
fn hide_and_restore_leave_configuration_and_intervening_edits_unchanged() {
    let mut config = Config::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    ViewId::BodyMirror.set(&mut config, true);
    let before_hide = serialized(&config);
    let mut temporary = TemporaryHide::default();
    let mask = temporary.hide(&config);
    assert_eq!(serialized(&config), before_hide);
    assert_eq!(
        temporary.requested_at_hide.expect("hide snapshot").count(),
        1
    );
    assert!(mask.contains(Source::BodyMirror));
    assert!(mask.contains(Source::ColliderView));
    apply_edits(
        &mut config,
        &[
            Edit::Switch(ViewId::BodyMirror, false),
            Edit::Switch(ViewId::ColliderView, true),
            Edit::GunReadout(GunReadout::Laser),
        ],
    );
    let after_edit = serialized(&config);
    assert_eq!(
        temporary
            .requested_at_hide
            .expect("retained hide snapshot")
            .count(),
        1
    );
    assert!(mask.contains(ViewId::ColliderView.source()));
    assert!(!mask.contains(Source::GunReadout));
    let restored = temporary.restore();
    assert_eq!(restored, SourceMask::NONE);
    assert!(temporary.requested_at_hide.is_none());
    assert_eq!(serialized(&config), after_edit);
    assert!(!ViewId::BodyMirror.requested(&config));
    assert!(ViewId::ColliderView.requested(&config));
}

#[test]
fn requested_at_hide_is_diagnostic_and_new_requests_remain_hidden() {
    let mut config = Config::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    let mut temporary = TemporaryHide::default();
    let mask = temporary.hide(&config);
    assert_eq!(
        temporary
            .requested_at_hide
            .expect("empty request snapshot")
            .count(),
        0
    );
    ViewId::GrappleBeam.set(&mut config, true);
    assert_eq!(temporary.hide(&config), mask);
    assert_eq!(
        temporary
            .requested_at_hide
            .expect("original request snapshot")
            .count(),
        0
    );
    let mut snapshot = snapshot_with(config);
    snapshot.runtime.temporarily_hidden = true;
    assert_eq!(
        snapshot.state(ViewId::GrappleBeam),
        ViewState::Blocked(Blocker::TemporaryHide)
    );
    assert_eq!(snapshot.summary().blocked, 1);
    assert_eq!(snapshot.summary().draw_unverified, 0);
}

#[test]
fn runtime_blockers_do_not_rewrite_requested_settings() {
    let mut snapshot = snapshot_with(Config::new());
    snapshot.config.controllers.enabled = true;
    snapshot.config.controllers.overlay.enabled = true;
    snapshot.config.controllers.overlay.debug_gizmos = true;
    snapshot.runtime.controllers_scheme = false;
    assert_eq!(
        snapshot.state(ViewId::ControllerGizmos),
        ViewState::Blocked(Blocker::Scheme)
    );
    snapshot.runtime.controllers_scheme = true;
    snapshot.runtime.gameplay = false;
    assert_eq!(
        snapshot.state(ViewId::ControllerGizmos),
        ViewState::Blocked(Blocker::Gameplay)
    );
    snapshot.runtime.cinematic = true;
    assert_eq!(
        snapshot.state(ViewId::ControllerGizmos),
        ViewState::Blocked(Blocker::Cinematic)
    );
    assert!(snapshot.config.controllers.overlay.debug_gizmos);
}
