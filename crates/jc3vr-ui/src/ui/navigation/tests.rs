use super::*;

#[test]
fn saved_selection_recovers_known_keys_and_defaults_unknown_ones() {
    assert_eq!(
        NavigationState::recover(Some("removed-feature")),
        NavigationState::default()
    );
    assert_eq!(NavigationState::recover(None), NavigationState::default());
    for feature in FEATURES {
        assert_eq!(
            NavigationState::recover(Some(feature.key)).feature,
            feature.id
        );
    }
}
