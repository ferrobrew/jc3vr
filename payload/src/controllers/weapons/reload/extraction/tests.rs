use glam::Quat;
use test_case::test_case;

use super::*;
use crate::controllers::grab::{Mark, MarkAt, resolve::resolve};

const CONFIG: ExtractionConfig = ExtractionConfig::new();

fn sample(holder: Hand, outward_m: f32) -> Sample {
    Sample {
        id: MagazineId {
            weapon: 1,
            gun: 0,
            holder,
        },
        expected: serial::Identity::test(),
        frame: Pose::IDENTITY,
        outward: Vec3::NEG_Y,
        grip: Vec3::NEG_Y * outward_m,
        world_scale: 1.0,
    }
}

#[test_case(Hand::Left; "left gun")]
#[test_case(Hand::Right; "right gun")]
fn only_a_claim_then_a_full_pull_extracts(holder: Hand) {
    let mut pull = Pull::new();
    assert!(!pull.step(Some(sample(holder, 0.0)), false, true, &CONFIG));
    assert_eq!(pull.hand(), None);
    assert!(!pull.step(Some(sample(holder, 0.0)), true, true, &CONFIG));
    assert_eq!(pull.hand(), Some(holder.other()));
    assert!(!pull.step(
        Some(sample(holder, CONFIG.pull_m * 0.9)),
        false,
        true,
        &CONFIG
    ));
    assert!(pull.step(Some(sample(holder, CONFIG.pull_m)), false, true, &CONFIG));
    assert_eq!(pull.hand(), None);
    assert!(!pull.step(Some(sample(holder, CONFIG.pull_m)), false, true, &CONFIG));
}

#[test]
fn returning_to_the_magazine_and_releasing_does_not_extract() {
    let mut pull = Pull::new();
    pull.step(Some(sample(Hand::Right, 0.0)), true, true, &CONFIG);
    pull.step(
        Some(sample(Hand::Right, CONFIG.pull_m * 0.5)),
        false,
        true,
        &CONFIG,
    );
    assert!(!pull.step(Some(sample(Hand::Right, 0.0)), false, true, &CONFIG));
    assert!(!pull.step(Some(sample(Hand::Right, 0.0)), false, false, &CONFIG));
    assert_eq!(pull.hand(), None);
    assert!(!pull.step(
        Some(sample(Hand::Right, CONFIG.pull_m)),
        false,
        true,
        &CONFIG
    ));
}

#[test]
fn the_pull_follows_the_rotating_and_translating_gun_not_world_motion() {
    let mut pull = Pull::new();
    pull.step(Some(sample(Hand::Right, 0.0)), true, true, &CONFIG);
    let rotation = Quat::from_rotation_z(0.8);
    let position = Vec3::new(1.0, 2.0, -3.0);
    let mut moved = sample(Hand::Right, 0.0);
    moved.frame = Pose {
        position,
        orientation: rotation,
    };
    moved.outward = rotation * Vec3::NEG_Y;
    moved.grip = position;
    assert!(!pull.step(Some(moved), false, true, &CONFIG));
    moved.grip += moved.outward * (CONFIG.pull_m + 0.001);
    assert!(pull.step(Some(moved), false, true, &CONFIG));
}

#[test]
fn sideways_motion_aborts_instead_of_detaching() {
    let mut pull = Pull::new();
    pull.step(Some(sample(Hand::Left, 0.0)), true, true, &CONFIG);
    let mut sideways = sample(Hand::Left, CONFIG.pull_m);
    sideways.grip.x = CONFIG.lateral_tolerance_m + 0.001;
    assert!(!pull.step(Some(sideways), false, true, &CONFIG));
    assert_eq!(pull.hand(), None);
}

#[test]
fn hand_loss_removal_identity_scale_and_mode_changes_require_a_new_claim() {
    let start = sample(Hand::Right, 0.0);
    let mut replacement = start;
    replacement.id.weapon += 1;
    let mut gun_change = start;
    gun_change.id.gun += 1;
    let mut hand_change = start;
    hand_change.id.holder = Hand::Left;
    let mut scale_change = start;
    scale_change.world_scale = 2.0;
    let mut invalid = start;
    invalid.grip.x = f32::NAN;
    for changed in [
        None,
        Some(replacement),
        Some(gun_change),
        Some(hand_change),
        Some(scale_change),
        Some(invalid),
    ] {
        let mut pull = Pull::new();
        pull.step(Some(start), true, true, &CONFIG);
        assert!(!pull.step(changed, false, true, &CONFIG));
        assert_eq!(pull.hand(), None);
        assert!(!pull.step(
            Some(sample(Hand::Right, CONFIG.pull_m)),
            false,
            true,
            &CONFIG
        ));
    }
    let mut pull = Pull::new();
    pull.step(Some(start), true, true, &CONFIG);
    pull.reset();
    assert!(!pull.step(
        Some(sample(Hand::Right, CONFIG.pull_m)),
        false,
        true,
        &CONFIG
    ));
    pull.step(Some(start), true, true, &CONFIG);
    let disabled = ExtractionConfig {
        enabled: false,
        ..CONFIG
    };
    assert!(!pull.step(Some(start), false, true, &disabled));
    assert_eq!(pull.hand(), None);
}

#[test]
fn only_an_inserted_detachable_magazine_is_eligible() {
    let mut reload = ReloadConfig::new();
    assert!(eligible(LoadKind::Magazine, false, &reload));
    assert!(!eligible(LoadKind::Magazine, true, &reload));
    for load in [LoadKind::Shell, LoadKind::Rocket, LoadKind::Cylinder] {
        assert!(!eligible(load, false, &reload));
    }
    reload.loose.enabled = false;
    assert!(!eligible(LoadKind::Magazine, false, &reload));
    reload.loose.enabled = true;
    reload.enabled = false;
    assert!(!eligible(LoadKind::Magazine, false, &reload));
}

fn target(volume: GrabVolume, centre: Vec3) -> GripTarget {
    GripTarget::sphere(
        volume,
        Some(Hand::Left),
        centre,
        0.2,
        Mark::ring(MarkAt::Cockpit(centre), 0.02),
    )
}

#[test]
fn direct_magazine_claim_beats_overlapping_brace_transfer_and_action() {
    let mut targets = vec![
        target(GrabVolume::Handle, Vec3::ZERO),
        target(GrabVolume::Foregrip, Vec3::ZERO),
        target(GrabVolume::WeaponAction, Vec3::ZERO),
        target(GrabVolume::InsertedMagazine, Vec3::X * 0.01),
    ];
    prioritize(&mut targets, [Some(Vec3::ZERO), None]);
    assert_eq!(
        resolve(&targets, [Some(Vec3::ZERO), None])[0]
            .chosen
            .unwrap()
            .volume,
        GrabVolume::InsertedMagazine
    );
}

#[test]
fn direct_action_claim_beats_unheld_brace_and_transfer_but_preserves_existing_support() {
    for hand in Hand::BOTH {
        let targets_for = |held| {
            let mut support = target(GrabVolume::Foregrip, Vec3::ZERO);
            support.hand = Some(hand);
            if held {
                support = support.held(hand);
            }
            let mut action = target(GrabVolume::WeaponAction, Vec3::X * 0.01);
            action.hand = Some(hand);
            let mut transfer = target(GrabVolume::Handle, Vec3::ZERO);
            transfer.hand = Some(hand);
            vec![support, transfer, action]
        };
        let grips = Hand::BOTH.map(|h| (h == hand).then_some(Vec3::ZERO));
        let mut targets = targets_for(false);
        prioritize(&mut targets, grips);
        assert_eq!(
            resolve(&targets, grips)[hand.index()]
                .chosen
                .unwrap()
                .volume,
            GrabVolume::WeaponAction
        );
        let mut targets = targets_for(true);
        prioritize(&mut targets, grips);
        assert_eq!(
            resolve(&targets, grips)[hand.index()]
                .chosen
                .unwrap()
                .volume,
            GrabVolume::Foregrip
        );
    }
}

#[test]
fn held_support_wins_and_a_missed_magazine_does_not_steal_support() {
    let support = target(GrabVolume::Foregrip, Vec3::ZERO).held(Hand::Left);
    let mut targets = vec![support, target(GrabVolume::InsertedMagazine, Vec3::ZERO)];
    prioritize(&mut targets, [Some(Vec3::ZERO), None]);
    assert_eq!(targets.len(), 2);
    let mut targets = vec![
        target(GrabVolume::Foregrip, Vec3::ZERO),
        target(GrabVolume::InsertedMagazine, Vec3::X),
    ];
    prioritize(&mut targets, [Some(Vec3::ZERO), None]);
    assert_eq!(targets.len(), 2);
    assert_eq!(
        resolve(&targets, [Some(Vec3::ZERO), None])[0]
            .chosen
            .unwrap()
            .volume,
        GrabVolume::Foregrip
    );
}
