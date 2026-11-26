//! Renderer-free checks against the concrete visualization configuration fields.

use super::*;

fn serialized(config: &ControllersConfig) -> serde_json::Value {
    serde_json::to_value(config).expect("config serializes")
}

fn snapshot_with(config: ControllersConfig) -> Snapshot {
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
        (ViewId::BodyMirror, "/overlay/body_mirror"),
        (ViewId::BodyRig, "/overlay/body_rig"),
        (ViewId::BodyEstimate, "/overlay/body_estimate"),
        (ViewId::GrabView, "/physical_hands/grab_view/enabled"),
        (
            ViewId::ColliderView,
            "/physical_hands/collider_view/enabled",
        ),
        (ViewId::DockedGuns, "/overlay/docked_guns"),
    ];
    assert_eq!(paths.len(), ENTRIES.len());
    for (id, path) in paths {
        let mut config = ControllersConfig::new();
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
fn filtering_names_and_feature_names_preserves_the_saved_config() {
    let config = ControllersConfig::new();
    let before = serialized(&config);
    assert_eq!(matching(&config, "BODY MIRROR")[0].id, ViewId::BodyMirror);
    let hands = matching(&config, "physical hands");
    assert!(!hands.is_empty());
    assert!(
        hands
            .iter()
            .all(|entry| entry.feature == FeatureId::PhysicalHands)
    );
    assert!(matching(&config, "not a view").is_empty());
    assert_eq!(before, serialized(&config));
}

#[test]
fn drawer_shows_every_switch_once_with_the_global_controls() {
    let snapshot = snapshot_with(ControllersConfig::new());
    let before = serialized(&snapshot.config);
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
            |ui| controls(ui, &snapshot),
        );
        labels.clear();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut labels);
        }
    }
    for entry in ENTRIES {
        assert_eq!(
            labels.iter().filter(|label| *label == entry.label).count(),
            1,
            "{}",
            entry.label,
        );
    }
    for label in [
        "Overlay readout",
        "Hide registered views",
        "Restore registered views",
    ] {
        assert!(labels.iter().any(|text| text == label), "{label}");
    }
    assert!(labels.iter().any(|text| text.starts_with("Inventory:")));
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
    let mut config = ControllersConfig::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    config.overlay.body_mirror = true;
    config.overlay.docked_guns = true;
    let entries = matching(&config, "");
    assert_eq!(entries[0].id, ViewId::BodyMirror);
    assert_eq!(entries[1].id, ViewId::DockedGuns);
    assert!(
        entries[2..]
            .iter()
            .all(|entry| !entry.id.requested(&config))
    );
}

#[test]
fn requested_does_not_imply_a_verified_draw_or_active_count() {
    let mut snapshot = snapshot_with(ControllersConfig::new());
    snapshot.config.enabled = true;
    snapshot.config.overlay.body_mirror = true;
    assert_eq!(
        snapshot.state(ViewId::BodyMirror),
        ViewState::DrawUnverified
    );
    assert_eq!(snapshot.summary().active_count(), None);
    snapshot.config.enabled = false;
    assert_eq!(snapshot.summary().active_count(), Some(0));
    assert_eq!(snapshot.summary().blocked, snapshot.summary().requested);
}

#[test]
fn retained_overlay_error_does_not_block_requested_visualizations() {
    let mut config = ControllersConfig::new();
    for entry in ENTRIES {
        entry.id.set(&mut config, false);
    }
    config.enabled = true;
    config.overlay.body_rig = true;
    let before = serialized(&config);
    let mut snapshot = snapshot_with(config);
    snapshot.runtime.overlay.error = Some("previous draw failure".to_owned());

    assert_eq!(
        snapshot.state(ViewId::BodyRig),
        ViewState::DrawUnverified
    );
    assert_eq!(snapshot.summary().requested, 1);
    assert_eq!(snapshot.summary().blocked, 0);
    assert_eq!(snapshot.summary().draw_unverified, 1);
    assert_eq!(snapshot.summary().active_count(), None);
    assert_eq!(serialized(&snapshot.config), before);
}

#[test]
fn the_inventory_maps_every_switch_to_its_output_source() {
    let expected = [
        (ViewId::BodyMirror, Source::BodyMirror),
        (ViewId::BodyRig, Source::BodyRig),
        (ViewId::BodyEstimate, Source::BodyRig),
        (ViewId::GrabView, Source::GrabView),
        (ViewId::ColliderView, Source::ColliderView),
        (ViewId::DockedGuns, Source::DockedGuns),
    ];
    assert_eq!(expected.len(), ENTRIES.len());
    for (id, source) in expected {
        assert_eq!(ENTRIES.iter().filter(|entry| entry.id == id).count(), 1);
        assert_eq!(id.source(), source);
    }
    let config = ControllersConfig::new();
    let mask = TemporaryHide::default().hide(&config);
    for source in Source::ALL {
        assert_eq!(
            mask.contains(source),
            source == Source::BodyMirrorPouches
                || expected.iter().any(|(_, mapped)| *mapped == source)
        );
    }
}

#[test]
fn hide_and_restore_leave_configuration_and_intervening_edits_unchanged() {
    let mut config = ControllersConfig::new();
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
    assert!(mask.contains(Source::BodyMirrorPouches));
    assert!(mask.contains(Source::ColliderView));
    apply_edits(
        &mut config,
        &[
            Edit::Switch(ViewId::BodyMirror, false),
            Edit::Switch(ViewId::ColliderView, true),
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
    assert!(!mask.contains(Source::GrappleHighlight));
    let restored = temporary.restore();
    assert_eq!(restored, SourceMask::NONE);
    assert!(temporary.requested_at_hide.is_none());
    assert_eq!(serialized(&config), after_edit);
    assert!(!ViewId::BodyMirror.requested(&config));
    assert!(ViewId::ColliderView.requested(&config));
}

#[test]
fn requested_at_hide_is_diagnostic_and_new_requests_remain_hidden() {
    let mut config = ControllersConfig::new();
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
    ViewId::DockedGuns.set(&mut config, true);
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
        snapshot.state(ViewId::DockedGuns),
        ViewState::Blocked(Blocker::TemporaryHide)
    );
    assert_eq!(snapshot.summary().blocked, 1);
    assert_eq!(snapshot.summary().draw_unverified, 0);
}

#[test]
fn runtime_blockers_do_not_rewrite_requested_settings() {
    let mut snapshot = snapshot_with(ControllersConfig::new());
    snapshot.config.enabled = true;
    snapshot.config.overlay.body_rig = true;
    snapshot.runtime.controllers_scheme = false;
    assert_eq!(
        snapshot.state(ViewId::BodyRig),
        ViewState::Blocked(Blocker::Scheme)
    );
    snapshot.runtime.controllers_scheme = true;
    snapshot.runtime.gameplay = false;
    assert_eq!(
        snapshot.state(ViewId::BodyRig),
        ViewState::Blocked(Blocker::Gameplay)
    );
    snapshot.runtime.clean_view = true;
    assert_eq!(
        snapshot.state(ViewId::BodyRig),
        ViewState::Blocked(Blocker::CleanView)
    );
    assert!(snapshot.config.overlay.body_rig);
}
