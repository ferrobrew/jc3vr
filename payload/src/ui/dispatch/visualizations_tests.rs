//! Renderer-free checks of the production feature visualization routes.

use std::collections::HashSet;

use super::*;
use crate::config;

#[test]
fn production_visualizations_show_each_canonical_row_only_under_its_owner() {
    let before = serde_json::to_value(config::get()).unwrap();
    let owners: [(FeatureId, &[&str]); 3] = [
        (FeatureId::RootWalls, &["Body mirror", "Controller gizmos"]),
        (
            FeatureId::PhysicalHands,
            &[
                "Ghost hands",
                "Grip markers",
                "Grab view",
                "Grab view: reach rays",
                "Grab view: target lines",
                "Grab view: palm state on wrists",
                "Grab view: refused grabs",
                "Grab view: nearby candidates",
                "Collider view",
            ],
        ),
        (
            FeatureId::Grapple,
            &["Grapple target highlight", "Grapple beam"],
        ),
    ];
    let inventory: HashSet<_> = owners
        .iter()
        .flat_map(|(_, labels)| labels.iter().copied())
        .collect();
    assert_eq!(inventory.len(), 13);
    for width in [420.0, 1400.0] {
        let mut union = HashSet::new();
        for (owner, expected) in owners {
            assert_eq!(route(owner), FeatureRoute::Controller);
            let labels = visualization_labels(owner, width);
            for label in &inventory {
                let count = labels.iter().filter(|text| text.as_str() == *label).count();
                assert_eq!(
                    count,
                    usize::from(expected.contains(label)),
                    "{owner:?}: {label}"
                );
                if count != 0 {
                    assert!(union.insert(*label), "duplicate owner for {label}");
                }
            }
            assert_no_global_controls(&labels);
            let rendered_text = labels.join(" ");
            if owner == FeatureId::RootWalls {
                for label in [
                    "Hide the game's reticles while the hands aim",
                    "Enabled",
                    "Gun readout",
                    "Body mirror distance (m)",
                    "Depth tolerance (m)",
                    "Depth tolerance (fraction of distance)",
                    "Minimum line width (px)",
                    "Ghost x-ray alpha",
                ] {
                    assert!(labels.iter().any(|text| text == label), "missing {label}");
                }
                assert!(rendered_text.contains("Primitives drawn per eye:"));
            }
            if owner == FeatureId::PhysicalHands {
                for label in [
                    "Target radius (m)",
                    "Candidate radius (m)",
                    "Most candidates",
                    "Radius (m)",
                    "Most bodies",
                    "Most primitives",
                    "Boxes",
                    "Shapes (spheres, capsules, convex hulls)",
                    "Static bodies",
                    "Contacts and the player's ground",
                    "The hands' and guns' joints",
                    "The body under the gaze",
                ] {
                    assert!(labels.iter().any(|text| text == label), "missing {label}");
                }
                for alternatives in [
                    ["Hide the grab view", "Show the grab view"],
                    ["Hide the colliders", "Show the colliders (Ctrl+Numpad0)"],
                ] {
                    assert!(
                        alternatives
                            .iter()
                            .any(|label| labels.iter().any(|text| text == label))
                    );
                }
            }
        }
        assert_eq!(union, inventory);
    }
    assert_eq!(serde_json::to_value(config::get()).unwrap(), before);
}

#[test]
fn specialized_visualizations_remain_separate_from_the_canonical_inventory() {
    for width in [420.0, 1400.0] {
        let previews = visualization_labels(FeatureId::Previews, width);
        assert_eq!(route(FeatureId::Previews), FeatureRoute::Previews);
        assert_eq!(
            previews,
            ["Unavailable: the DX11 renderer is not attached to this view."]
        );
        let diagnostics = visualization_labels(FeatureId::RenderDiagnostics, width);
        assert_eq!(
            route(FeatureId::RenderDiagnostics),
            FeatureRoute::RenderDiagnostics
        );
        assert_eq!(
            diagnostics,
            ["Per-eye render camera (differences in yellow)"]
        );
        for labels in [&previews, &diagnostics] {
            assert_no_global_controls(labels);
            assert!(!labels.iter().any(|text| text.starts_with("Inventory:")));
        }
    }
}

fn visualization_labels(feature_id: FeatureId, width: f32) -> Vec<String> {
    let context = egui::Context::default();
    let mut selection = NavigationState {
        feature: feature_id,
        section: FeatureSection::Visualizations,
    };
    let mut labels = Vec::new();
    for _ in 0..2 {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 5000.0),
                )),
                ..Default::default()
            },
            |ui| feature(ui, &mut selection, None),
        );
        labels.clear();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut labels);
        }
    }
    assert_eq!(selection.feature, feature_id);
    assert_eq!(selection.section, FeatureSection::Visualizations);
    labels
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

fn assert_no_global_controls(labels: &[String]) {
    for label in [
        "Shared overlay settings",
        "Hide registered views (including Gameplay cues)",
        "Restore registered views",
    ] {
        assert!(
            !labels.iter().any(|text| text == label),
            "unexpected global action: {label}"
        );
    }
}
