//! Renderer-free checks against the concrete visualization configuration fields.

use super::*;

fn serialized(config: &ControllersConfig) -> serde_json::Value {
    serde_json::to_value(config).expect("config serializes")
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
            apply_edits(&mut config, &[(id, value)]);
            assert_eq!(serialized(&config), expected, "{path}");
            assert_eq!(id.requested(&config), value, "{path}");
        }
    }
}

#[test]
fn filtering_matches_names_and_feature_names() {
    let config = ControllersConfig::new();
    assert_eq!(matching(&config, "BODY MIRROR")[0].id, ViewId::BodyMirror);
    let hands = matching(&config, "physical hands");
    assert!(!hands.is_empty());
    assert!(
        hands
            .iter()
            .all(|entry| entry.feature == FeatureId::PhysicalHands)
    );
    assert!(matching(&config, "not a view").is_empty());
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
fn the_hide_mask_covers_every_switch_source_and_nothing_else() {
    let mask = hide_mask();
    for source in Source::ALL {
        assert_eq!(
            mask.contains(source),
            source == Source::BodyMirrorPouches
                || ENTRIES.iter().any(|entry| entry.id.source() == source)
        );
    }
}
