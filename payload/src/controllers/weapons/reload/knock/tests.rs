use glam::{Quat, Vec3};

use super::*;
use crate::controllers::{
    physical_hands::rigid::BodyPose,
    weapons::reload::wells::{TableAction, WellGrade, WellKind},
};

/// A rifle's magazine hanging 12 cm out of a well pushed up (+Y) into the gun at the origin.
fn rifle() -> KnockZone {
    KnockZone::of(Vec3::ZERO, Vec3::Y, 0.12, 1.0).unwrap()
}

/// Two ticks of the tip: from `from` to `to` over `dt`, starting with no approach.
fn hit(zone: &KnockZone, from: Vec3, to: Vec3, dt: f32, min: f32) -> Option<Strike> {
    let (none, previous) = strike(zone, None, from, dt, min);
    assert_eq!(none, None, "the first tick has no approach");
    strike(zone, previous, to, dt, min).0
}

#[test]
fn a_fast_strike_on_the_base_knocks_the_magazine_out_along_the_well() {
    let zone = rifle();
    let s = hit(
        &zone,
        Vec3::new(0.0, -0.19, 0.0),
        Vec3::new(0.0, -0.15, 0.0),
        0.02,
        1.0,
    )
    .expect("a 2 m/s strike on the base knocks");
    assert!((s.closing_mps - 2.0).abs() < 1e-3, "{s:?}");
    assert!((s.velocity - Vec3::Y * 2.0).length() < 1e-3, "{s:?}");
    // Struck at the base, straight through the magazine's centre.
    assert!(
        (s.contact - Vec3::new(0.0, -0.06, 0.0)).length() < 1e-4,
        "{s:?}"
    );
}

#[test]
fn a_gentle_touch_does_nothing() {
    let zone = rifle();
    let from = Vec3::new(0.0, -0.19, 0.0);
    let to = Vec3::new(0.0, -0.15, 0.0);
    // 0.2 m/s, a hand bringing the magazine to the gun to load.
    assert_eq!(hit(&zone, from, to, 0.2, 1.0), None);
}

#[test]
fn the_threshold_is_on_the_closing_speed() {
    let zone = rifle();
    let from = Vec3::new(0.0, -0.19, 0.0);
    let to = Vec3::new(0.0, -0.15, 0.0);
    // 2 m/s closing.
    assert!(hit(&zone, from, to, 0.02, 1.99).is_some());
    assert!(hit(&zone, from, to, 0.02, 2.01).is_none());
    // The same strike at a higher threshold from the debug UI.
    assert!(hit(&zone, from, to, 0.01, 3.9).is_some());
}

#[test]
fn a_tip_already_against_the_magazine_does_not_knock_however_fast_it_moves() {
    let zone = rifle();
    let s = hit(
        &zone,
        Vec3::new(0.0, -0.13, 0.0),
        Vec3::new(0.0, -0.08, 0.0),
        0.01,
        1.0,
    );
    assert_eq!(s, None);
}

#[test]
fn a_fast_pass_beside_the_magazine_does_not_knock() {
    let zone = rifle();
    let s = hit(
        &zone,
        Vec3::new(0.1, -0.06, 0.0),
        Vec3::new(0.06, -0.06, 0.0),
        0.01,
        1.0,
    );
    assert_eq!(s, None);
}

#[test]
fn a_swipe_through_the_magazine_between_two_ticks_knocks() {
    let zone = rifle();
    let s = hit(
        &zone,
        Vec3::new(0.08, -0.06, 0.0),
        Vec3::new(-0.08, -0.06, 0.0),
        0.02,
        1.0,
    )
    .expect("the swept path crosses the magazine");
    assert!((s.closing_mps - 8.0).abs() < 1e-2, "{s:?}");
}

#[test]
fn a_strike_on_the_side_near_the_well_meets_it_above_the_centre() {
    let zone = rifle();
    let s = hit(
        &zone,
        Vec3::new(0.08, -0.01, 0.0),
        Vec3::new(0.03, -0.01, 0.0),
        0.02,
        1.0,
    )
    .expect("a 2.5 m/s strike on the side knocks");
    assert!(
        (s.contact - Vec3::new(0.0, 0.05, 0.0)).length() < 1e-4,
        "{s:?}"
    );
    assert!(
        (s.velocity - Vec3::new(-2.5, 0.0, 0.0)).length() < 1e-3,
        "{s:?}"
    );
}

#[test]
fn the_gun_moving_with_the_hand_is_not_a_strike() {
    // The gun and the tip move up together at 3 m/s: no motion relative to the magazine.
    let shift = Vec3::Y * 0.06;
    let before = rifle();
    let after = KnockZone::of(shift, Vec3::Y, 0.12, 1.0).unwrap();
    let tip = Vec3::new(0.0, -0.17, 0.0);
    let (_, previous) = strike(&before, None, tip, 0.02, 1.0);
    let (s, _) = strike(&after, previous, tip + shift, 0.02, 1.0);
    assert_eq!(s, None);
}

#[test]
fn the_strike_is_reported_in_the_cockpit_frame() {
    // A well whose axis points along +X.
    let zone = KnockZone::of(Vec3::ONE, Vec3::X, 0.12, 1.0).unwrap();
    let s = hit(
        &zone,
        Vec3::ONE - Vec3::X * 0.19,
        Vec3::ONE - Vec3::X * 0.15,
        0.02,
        1.0,
    )
    .unwrap();
    assert!((s.velocity - Vec3::X * 2.0).length() < 1e-3, "{s:?}");
    assert!((s.contact + Vec3::X * 0.06).length() < 1e-4, "{s:?}");
}

#[test]
fn the_zone_follows_the_player_scale_and_ends_at_the_base() {
    let zone = KnockZone::of(Vec3::ZERO, Vec3::Y, 0.12, 2.0).unwrap();
    assert!((zone.radius - KNOCK.radius_m / 2.0).abs() < 1e-6);
    let (top, base) = zone.ends();
    assert!((top - Vec3::Y * KNOCK.lip_m / 2.0).length() < 1e-6);
    assert!((base + Vec3::Y * 0.06).length() < 1e-6);
    assert!(KnockZone::of(Vec3::ZERO, Vec3::ZERO, 0.12, 1.0).is_none());
    assert!(KnockZone::of(Vec3::ZERO, Vec3::Y, 0.12, 0.0).is_none());
}

#[test]
fn only_a_magazine_in_a_table_well_can_be_knocked_out() {
    let gun = WellFrame::Gun {
        mouth: Vec3::ZERO,
        axis: Vec3::Y,
    };
    let placed = Some((Vec3::ZERO, Vec3::Y));
    assert!(zone_for(LoadKind::Magazine, gun, false, placed, 0.1, 1.0).is_some());
    assert!(zone_for(LoadKind::Magazine, gun, true, placed, 0.1, 1.0).is_none());
    assert!(zone_for(LoadKind::Cylinder, gun, false, placed, 0.1, 1.0).is_none());
    assert!(zone_for(LoadKind::Magazine, gun, false, None, 0.1, 1.0).is_none());
    let wrist = WellFrame::Wrist { offset: Vec3::ZERO };
    assert!(zone_for(LoadKind::Magazine, wrist, false, placed, 0.1, 1.0).is_none());
}

#[test]
fn a_magazine_part_hangs_its_length_and_a_grip_magazine_none() {
    let entry = |part: bool, length: Option<f32>| WellEntry {
        weapon: "w999_placeholder".to_owned(),
        weapon_hash: 9,
        kind: WellKind::Magazine,
        mouth_m: Some(Vec3::ZERO),
        insert_axis: Some(Vec3::Y),
        magazine_part: part,
        part_top_m: part.then_some(0.07),
        part_length_m: length,
        action: TableAction::Slide,
        handle_m: None,
        travel_m: None,
        grade: WellGrade::Mesh,
    };
    assert_eq!(below_m(Some(&entry(true, Some(0.14)))), 0.14);
    assert_eq!(below_m(Some(&entry(false, Some(0.19)))), 0.0);
    assert_eq!(below_m(Some(&entry(false, None))), 0.0);
    assert_eq!(below_m(None), 0.0);
}

fn magazine_body() -> RigidBody {
    RigidBody {
        pose: BodyPose {
            centre: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::IDENTITY,
        },
        // The drop gave it the gun's velocity.
        velocities: Velocities {
            linear: Vec3::new(0.5, 0.0, 0.0),
            angular: Vec3::ZERO,
        },
        inverse_mass: 1.0 / 0.4,
        inverse_inertia: Vec3::splat(1.0 / 0.0006),
    }
}

#[test]
fn a_base_strike_through_the_centre_sends_the_magazine_on_without_spin() {
    let body = magazine_body();
    let contact = body.pose.centre - Vec3::Y * 0.06;
    let after = knocked(body, contact, Vec3::Y * 2.0, &KNOCK);
    let expected = Vec3::new(0.5, 2.0 * (1.0 + KNOCK.restitution), 0.0);
    assert!((after.linear - expected).length() < 1e-3, "{after:?}");
    assert!(after.angular.length() < 1e-4, "{after:?}");
}

#[test]
fn a_side_strike_near_the_well_turns_the_magazine_as_it_goes() {
    let body = magazine_body();
    let contact = body.pose.centre + Vec3::Y * 0.05;
    let after = knocked(body, contact, Vec3::NEG_X * 2.5, &KNOCK);
    assert!(after.linear.x < 0.5, "{after:?}");
    // Struck above the centre toward -X, it turns about +Z.
    assert!(after.angular.z > 0.0, "{after:?}");
    assert!(after.angular.length() <= KNOCK.max_spin_radps + 1e-3);
}

#[test]
fn a_knock_is_capped_and_an_immovable_body_is_left_alone() {
    let body = magazine_body();
    let contact = body.pose.centre - Vec3::Y * 0.06;
    let after = knocked(body, contact, Vec3::Y * 40.0, &KNOCK);
    let change = after.linear - body.velocities.linear;
    assert!(
        (change.length() - KNOCK.max_knock_mps).abs() < 1e-3,
        "{after:?}"
    );
    let fixed = RigidBody {
        inverse_mass: 0.0,
        inverse_inertia: Vec3::ZERO,
        ..body
    };
    assert_eq!(
        knocked(fixed, contact, Vec3::Y * 2.0, &KNOCK),
        fixed.velocities
    );
    assert_eq!(knocked(body, contact, Vec3::ZERO, &KNOCK), body.velocities);
}
