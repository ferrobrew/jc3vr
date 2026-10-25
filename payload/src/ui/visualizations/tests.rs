//! Renderer-free checks against the concrete visualization configuration fields.

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
