use glam::{Quat, Vec3};
use pretty_assertions::assert_eq;

use super::*;
use crate::controllers::{
    fingers::grip::catalogue::GripPose, frame::Pose, hands::pose as hand_pose,
    weapons::regions::shape::MagazineWell,
};

const GRIP_RADIUS: f32 = 0.07;
const REACH: f32 = 0.05;

const REGIONS: [GunRegion; 6] = [
    GunRegion::Grip,
    GunRegion::Stock,
    GunRegion::Magazine,
    GunRegion::Action,
    GunRegion::Handguard,
    GunRegion::Barrel,
];

/// A rifle along -Z: the stock 0.3 m behind the grip, the muzzle 0.7 m ahead, a foregrip 0.35 m
/// out below the bore, a magazine below the receiver ahead of the grip, and a charging handle on
/// top.
fn rifle() -> GunShape {
    GunShape {
        min: Vec3::new(-0.04, -0.2, -0.72),
        max: Vec3::new(0.04, 0.1, 0.3),
        muzzle: Vec3::new(0.0, 0.05, -0.7),
        foregrip: Some(Vec3::new(0.0, -0.02, -0.35)),
        magazine: Some(MagazineWell {
            mouth: Vec3::new(0.0, -0.02, -0.12),
            axis: Vec3::Y,
        }),
        action: Some(Vec3::new(0.0, 0.08, 0.05)),
        long: true,
    }
}

/// A pistol: a short slide ahead of the grip, the magazine inside the grip.
fn pistol() -> GunShape {
    GunShape {
        min: Vec3::new(-0.02, -0.12, -0.17),
        max: Vec3::new(0.02, 0.05, 0.03),
        muzzle: Vec3::new(0.0, 0.03, -0.16),
        foregrip: None,
        magazine: Some(MagazineWell {
            mouth: Vec3::new(0.0, -0.1, 0.0),
            axis: Vec3::Y,
        }),
        action: Some(Vec3::new(0.0, 0.04, 0.02)),
        long: false,
    }
}

fn region_at(shape: &GunShape, point: Vec3) -> Option<GunRegion> {
    classify(shape, point, GRIP_RADIUS, REACH).map(|h| h.region)
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 1e-4
}

#[test]
fn a_rifle_has_every_region_along_it() {
    let r = rifle();
    assert_eq!(
        region_at(&r, Vec3::new(0.0, -0.05, 0.0)),
        Some(GunRegion::Grip)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, 0.0, 0.2)),
        Some(GunRegion::Stock)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, -0.12, -0.12)),
        Some(GunRegion::Magazine)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, 0.09, 0.06)),
        Some(GunRegion::Action)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, -0.02, -0.35)),
        Some(GunRegion::Handguard)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, 0.05, -0.62)),
        Some(GunRegion::Barrel)
    );
}

#[test]
fn a_hand_off_the_gun_touches_nothing() {
    let r = rifle();
    assert_eq!(region_at(&r, Vec3::new(0.3, 0.0, -0.3)), None);
    assert_eq!(region_at(&r, Vec3::new(0.0, 0.0, 0.5)), None);
    assert_eq!(region_at(&r, Vec3::new(0.0, 0.0, -0.9)), None);
    // Just outside the box is still on the gun.
    assert_eq!(
        region_at(&r, Vec3::new(0.08, 0.05, -0.6)),
        Some(GunRegion::Barrel)
    );
    assert_eq!(region_at(&r, Vec3::NAN), None);
}

#[test]
fn the_anchor_lies_on_the_line_the_region_runs_along() {
    let r = rifle();
    // A hand beside the barrel closes on the bore line through the muzzle.
    let hit = classify(&r, Vec3::new(0.06, 0.02, -0.6), GRIP_RADIUS, REACH).unwrap();
    assert_eq!(hit.region, GunRegion::Barrel);
    assert!(close(hit.anchor, Vec3::new(0.0, 0.05, -0.6)));
    assert!((hit.distance - hit.anchor.distance(Vec3::new(0.06, 0.02, -0.6))).abs() < 1e-6);
    // On the handguard, it closes on the line through the foregrip.
    let hit = classify(&r, Vec3::new(0.03, 0.0, -0.3), GRIP_RADIUS, REACH).unwrap();
    assert_eq!(hit.region, GunRegion::Handguard);
    assert!(close(hit.anchor, Vec3::new(0.0, -0.02, -0.3)));
    // On the stock, on the line back from the grip, never past the butt.
    let hit = classify(&r, Vec3::new(0.0, 0.02, 0.33), GRIP_RADIUS, REACH).unwrap();
    assert_eq!(hit.region, GunRegion::Stock);
    assert!(close(hit.anchor, Vec3::new(0.0, 0.0, 0.3)));
    // The grip's anchor is the attach point, where the canonical grip holds it.
    let hit = classify(&r, Vec3::new(0.0, -0.06, 0.01), GRIP_RADIUS, REACH).unwrap();
    assert_eq!(hit.anchor, Vec3::ZERO);
}

#[test]
fn the_barrel_never_reaches_past_the_muzzle() {
    let r = rifle();
    let hit = classify(&r, Vec3::new(0.0, 0.05, -0.74), GRIP_RADIUS, REACH).unwrap();
    assert_eq!(hit.region, GunRegion::Barrel);
    assert!(close(hit.anchor, r.muzzle));
}

#[test]
fn a_rifle_without_a_foregrip_or_wells_still_has_its_handguard_and_barrel() {
    let r = GunShape {
        foregrip: None,
        magazine: None,
        action: None,
        ..rifle()
    };
    assert_eq!(
        region_at(&r, Vec3::new(0.0, 0.0, -0.3)),
        Some(GunRegion::Handguard)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, 0.05, -0.6)),
        Some(GunRegion::Barrel)
    );
    assert_eq!(
        region_at(&r, Vec3::new(0.0, -0.12, -0.12)),
        Some(GunRegion::Handguard)
    );
}

#[test]
fn a_pistol_is_its_grip_its_slide_and_its_barrel() {
    let p = pistol();
    assert_eq!(
        region_at(&p, Vec3::new(0.0, -0.08, 0.0)),
        Some(GunRegion::Grip)
    );
    assert_eq!(
        region_at(&p, Vec3::new(0.0, 0.05, 0.02)),
        Some(GunRegion::Action)
    );
    assert_eq!(
        region_at(&p, Vec3::new(0.0, 0.03, -0.14)),
        Some(GunRegion::Barrel)
    );
    // No stock, no handguard, and no magazine apart from its grip.
    for point in [
        Vec3::new(0.0, 0.0, 0.05),
        Vec3::new(0.0, -0.11, 0.0),
        Vec3::new(0.0, 0.0, -0.08),
    ] {
        let region = region_at(&p, point);
        assert!(
            !matches!(
                region,
                Some(GunRegion::Stock | GunRegion::Handguard | GunRegion::Magazine)
            ),
            "{point}: {region:?}"
        );
    }
}

#[test]
fn an_unusable_shape_classifies_nothing() {
    let flat = GunShape {
        max: Vec3::new(0.04, 0.1, -0.71),
        ..rifle()
    };
    assert!(!flat.is_usable());
    assert_eq!(region_at(&flat, Vec3::new(0.0, 0.0, -0.71)), None);
    let inverted = GunShape {
        min: rifle().max,
        max: rifle().min,
        ..rifle()
    };
    assert_eq!(region_at(&inverted, Vec3::ZERO), None);
}

#[test]
fn only_the_grip_fires_and_only_the_parts_ahead_of_it_steer() {
    for region in REGIONS {
        assert_eq!(region.fires(), region == GunRegion::Grip, "{region:?}");
    }
    assert!(GunRegion::Handguard.steers());
    assert!(GunRegion::Barrel.steers());
    assert!(!GunRegion::Stock.steers());
    assert!(!GunRegion::Grip.steers());
    assert!(GunRegion::Handguard.slides() && GunRegion::Barrel.slides());
    assert!(!GunRegion::Magazine.slides());
}

#[test]
fn each_region_has_its_own_finger_pose() {
    assert_eq!(GunRegion::Grip.finger_pose(), GripPose::PistolGrip);
    assert_eq!(GunRegion::Handguard.finger_pose(), GripPose::Foregrip);
    assert_eq!(GunRegion::Magazine.finger_pose(), GripPose::MagazineCup);
    assert_eq!(GunRegion::Action.finger_pose(), GripPose::SlidePinch);
    let wrap = |region: GunRegion| match region.finger_pose() {
        GripPose::Cylinder { diameter_m } => diameter_m,
        other => panic!("{region:?}: {other:?}"),
    };
    // The stock is thicker than the barrel, so the fingers close less round it.
    assert!(wrap(GunRegion::Stock) > wrap(GunRegion::Barrel));
}

#[test]
fn a_carried_gun_sits_with_its_anchor_in_the_palm_turned_as_it_was_taken() {
    // The hand's grip puts the attach frame a little ahead of the wrist, turned.
    let grip = Pose {
        position: Vec3::new(0.0, -0.03, -0.08),
        orientation: Quat::from_rotation_x(0.4),
    };
    let wrist = Pose {
        position: Vec3::new(0.2, 1.2, -0.3),
        orientation: Quat::from_rotation_y(0.9),
    };
    let anchor = Vec3::new(0.0, 0.05, -0.55);
    let gun_on_wrist = Quat::from_rotation_z(1.2) * Quat::from_rotation_x(-0.3);
    let carry = Carry {
        region: GunRegion::Barrel,
        anchor,
        turn: gun_on_wrist,
    };
    let attach = hand_pose::mul(wrist, grip);
    let gun = hand_pose::mul(attach, carry_offset(&carry, grip));
    // The anchor is where the grip's attach point is.
    let anchor_world = hand_pose::mul(
        gun,
        Pose {
            position: anchor,
            orientation: Quat::IDENTITY,
        },
    )
    .position;
    assert!(close(anchor_world, attach.position));
    // The gun turns on the wrist as it did when the hand took it.
    let on_wrist = turn_on_wrist(wrist.orientation, gun.orientation);
    assert!(on_wrist.angle_between(gun_on_wrist) < 1e-4);
}

#[test]
fn a_carry_measured_on_the_wrist_frame_holds_the_gun_there_on_a_differently_oriented_bone() {
    // The skeleton's wrist bone is turned well away from the hand-tracking wrist frame.
    let bone = Pose {
        position: Vec3::new(0.2, 1.2, -0.3),
        orientation: Quat::from_rotation_y(0.9) * Quat::from_rotation_x(0.2),
    };
    let frame_on_bone = Quat::from_rotation_z(2.1) * Quat::from_rotation_y(-1.3);
    let wrist_frame = (bone.orientation * frame_on_bone).normalize();
    let grip = Pose {
        position: Vec3::new(0.06, -0.03, 0.004),
        orientation: Quat::from_rotation_x(0.4) * Quat::from_rotation_z(-1.1),
    };
    // The gun as the hand took it, measured on the wrist frame.
    let gun_world = Quat::from_rotation_x(-0.6) * Quat::from_rotation_y(2.4);
    let carry = Carry {
        region: GunRegion::Stock,
        anchor: Vec3::new(0.0, 0.0, 0.06),
        turn: turn_on_wrist(wrist_frame, gun_world),
    };
    let attach = hand_pose::mul(bone, grip);
    let placed = hand_pose::mul(
        attach,
        carry_offset(&carry_on_bone(carry, frame_on_bone), grip),
    );
    assert!(placed.orientation.angle_between(gun_world) < 1e-4);
    // Composed on the bone without the conversion, the gun turns by the bone's offset.
    let unconverted = hand_pose::mul(attach, carry_offset(&carry, grip));
    assert!(unconverted.orientation.angle_between(gun_world) > 0.5);
}

#[test]
fn a_point_round_trips_through_the_gun_frame_at_any_scale() {
    let gun = Pose {
        position: Vec3::new(1.0, 2.0, 3.0),
        orientation: Quat::from_rotation_y(0.7),
    };
    let point = Vec3::new(0.1, -0.2, -0.4);
    for scale in [1.0, 0.5, 2.0] {
        let there = out_of_gun(gun, point, scale);
        assert!(close(into_gun(gun, there, scale), point), "{scale}");
    }
}

/// The regions each hand holds its own gun by, by [`Hand::index`].
fn holding(left: Option<GunRegion>, right: Option<GunRegion>) -> [Option<GunRegion>; 2] {
    let mut holders = [None; 2];
    holders[Hand::Left.index()] = left;
    holders[Hand::Right.index()] = right;
    holders
}

#[test]
fn the_hands_on_a_gun_read_as_firing_bracing_or_carrying() {
    let hands = hands_on_guns(
        holding(None, Some(GunRegion::Grip)),
        Some((Hand::Left, GunRegion::Handguard)),
    );
    assert_eq!(
        hands,
        [
            Some(HandOnGun {
                region: GunRegion::Handguard,
                role: HandRole::Bracing
            }),
            Some(HandOnGun {
                region: GunRegion::Grip,
                role: HandRole::Firing
            }),
        ]
    );
    let carried = hands_on_guns(holding(Some(GunRegion::Barrel), None), None);
    assert_eq!(
        carried[Hand::Left.index()],
        Some(HandOnGun {
            region: GunRegion::Barrel,
            role: HandRole::Carrying
        })
    );
    assert_eq!(carried[Hand::Right.index()], None);
    // A brace by a hand holding a gun of its own is not a brace.
    let same = hands_on_guns(
        holding(Some(GunRegion::Grip), None),
        Some((Hand::Left, GunRegion::Barrel)),
    );
    assert_eq!(same[Hand::Left.index()].unwrap().role, HandRole::Firing);
}

#[test]
fn each_hand_carries_or_fires_its_own_gun() {
    // A rifle carried by its barrel in the left hand beside a pistol fired from the right.
    let hands = hands_on_guns(
        holding(Some(GunRegion::Barrel), Some(GunRegion::Grip)),
        None,
    );
    assert_eq!(hands[Hand::Left.index()].unwrap().role, HandRole::Carrying);
    assert_eq!(hands[Hand::Right.index()].unwrap().role, HandRole::Firing);
}

#[test]
fn a_carry_ends_when_its_hand_holds_another_gun_or_none() {
    assert_eq!(carry_gone(0x10, Some(0x10)), None);
    assert!(carry_gone(0x10, Some(0x20)).is_some());
    assert!(carry_gone(0x10, None).is_some());
}

#[test]
fn a_hand_slides_onto_the_grip_eased_and_ends_there() {
    let from = Vec3::new(0.0, 0.02, -0.3);
    let to = Vec3::ZERO;
    let over = GRIP_SLIDE;
    assert_eq!(slid_anchor(from, to, Duration::ZERO, over), from);
    assert_eq!(slid_anchor(from, to, over, over), to);
    assert_eq!(slid_anchor(from, to, over * 3, over), to);
    let half = slid_anchor(from, to, over / 2, over);
    assert!(half.distance(from.lerp(to, 0.5)) < 1e-5);
    // Eased: the first tenth of the time covers less than a tenth of the way.
    let early = slid_anchor(from, to, over / 10, over);
    assert!(early.distance(from) < 0.1 * from.distance(to));
    assert_eq!(slid_anchor(from, to, Duration::ZERO, Duration::ZERO), to);
}

#[test]
fn a_held_slide_moves_only_the_anchor() {
    let turn = Quat::from_rotation_x(0.7);
    let since = Instant::now();
    let hold = CarryHold {
        weapon: 1,
        carry: Carry {
            region: GunRegion::Grip,
            anchor: Vec3::ZERO,
            turn,
        },
        mark: 1,
        slide: Some(AnchorSlide {
            from: Vec3::new(0.0, 0.0, -0.3),
            since,
        }),
    };
    let start = hold.at(since);
    assert_eq!(start.anchor, Vec3::new(0.0, 0.0, -0.3));
    assert_eq!(start.turn, turn);
    assert_eq!(start.region, GunRegion::Grip);
    assert_eq!(hold.at(since + GRIP_SLIDE).anchor, Vec3::ZERO);
    // The gun body is seated again once the slide has ended, and not before.
    assert_eq!(hold.mark_at(since), hold.mark_at(since + GRIP_SLIDE / 2));
    assert_ne!(hold.mark_at(since), hold.mark_at(since + GRIP_SLIDE));
}

#[test]
fn a_gun_slid_onto_the_grip_keeps_pointing_the_way_it_pointed() {
    // The grip's attach frame on the wrist, and the gun's turn on the wrist when the hand braced.
    let grip = Pose {
        position: Vec3::new(0.01, -0.02, -0.07),
        orientation: Quat::from_rotation_z(0.4),
    };
    let turn = Quat::from_rotation_y(1.2) * Quat::from_rotation_x(-0.3);
    let carry = Carry {
        region: GunRegion::Grip,
        anchor: Vec3::ZERO,
        turn,
    };
    let held = hand_pose::mul(grip, carry_offset(&carry, grip));
    // The gun keeps its turn on the wrist, so its barrel points as it did, with its attach point
    // in the palm where the grip's is.
    assert!(held.orientation.dot(turn).abs() > 1.0 - 1e-5);
    assert!(held.position.distance(grip.position) < 1e-6);
    assert!(carry.region.fires());
}
