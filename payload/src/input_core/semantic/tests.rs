use std::collections::HashSet;

use super::*;

#[test]
fn semantic_names_are_valid_and_unique_per_set() {
    let valid = |name: &str| {
        !name.is_empty()
            && name.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    };
    let mut sets = HashSet::new();
    for set in SemanticSet::ALL {
        assert!(valid(set.name()), "{}", set.name());
        assert!(sets.insert(set.name()), "{}", set.name());
        let mut names = HashSet::new();
        for def in set.actions() {
            assert!(valid(def.name), "{}.{}", set.name(), def.name);
            assert!(names.insert(def.name), "{}.{}", set.name(), def.name);
            assert!(!def.localized_name.is_empty());
        }
    }
}

#[test]
fn every_mode_selects_a_distinct_set_and_global_is_none_of_them() {
    let selected: HashSet<SemanticSet> = ActionSetKind::ALL
        .iter()
        .map(|&k| SemanticSet::of(k))
        .collect();
    assert_eq!(selected.len(), ActionSetKind::ALL.len());
    assert!(!selected.contains(&SemanticSet::Global));
    for kind in ActionSetKind::ALL {
        assert_eq!(kind.name(), SemanticSet::of(kind).name());
        assert_eq!(ActionSetKind::ALL[kind.index()], kind);
    }
    for (i, set) in SemanticSet::ALL.iter().enumerate() {
        assert_eq!(set.index(), i);
    }
}
