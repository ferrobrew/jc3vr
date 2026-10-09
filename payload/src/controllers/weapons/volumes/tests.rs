use super::*;
use crate::controllers::grab::grab_volume;

const DIMS: BodyDimensions = BodyDimensions::GENERIC_ADULT;
/// A taller player's own body, as a calibration would save it.
const TALL: BodyDimensions = BodyDimensions {
    height: 1.80,
    eye_height: 1.68,
    shoulder_height: 1.48,
    shoulder_width: 0.40,
    arm_span: 1.80,
    upper_arm: 0.34,
    forearm: 0.26,
    hand: 0.19,
    seated_eye_height: None,
};
const BODIES: [BodyDimensions; 3] = [
    BodyDimensions::GENERIC_ADULT,
    BodyDimensions::CHARACTER_ESTIMATE,
    TALL,
];
const STANDING: BodyFrame = BodyFrame {
    origin: Vec3::ZERO,
    seated: false,
    anchor: Anchor::Body { yaw: 0.0 },
};
const SEATED: BodyFrame = BodyFrame {
    origin: Vec3::ZERO,
    seated: true,
    anchor: Anchor::Body { yaw: 0.0 },
};
const POUCHES: PouchesConfig = PouchesConfig::new();

fn holsters(frame: &BodyFrame, dims: &BodyDimensions) -> Vec<Candidate> {
    holster_candidates(frame, dims, &HolstersConfig::new())
}

/// Every volume, holsters and pouches, for a player whose dominant hand is `dominant`.
fn every_volume_for(
    frame: &BodyFrame,
    dims: &BodyDimensions,
    pouches: &PouchesConfig,
    dominant: Hand,
) -> Vec<Candidate> {
    let mut c = holsters(frame, dims);
    c.extend(grenade_pouch(
        frame,
        dims,
        pouches,
        dominant,
        ChestGrabbers::Either,
    ));
    c.extend(c4_pouch(
        frame,
        dims,
        pouches,
        dominant,
        ChestGrabbers::Either,
    ));
    c.push(magazine_pouch(
        frame,
        dims,
        pouches,
        dominant,
        Some(dominant.other()),
    ));
    c
}

fn every_volume(frame: &BodyFrame, dims: &BodyDimensions) -> Vec<Candidate> {
    every_volume_for(frame, dims, &POUCHES, Hand::Right)
}

fn find(c: &[Candidate], volume: GrabVolume) -> Candidate {
    *c.iter().find(|c| c.volume == volume).unwrap()
}

fn centre(c: &[Candidate], volume: GrabVolume) -> Vec3 {
    find(c, volume).centre
}

const RIGHT_HIP: GrabVolume = GrabVolume::Holster(HolsterSite::Hip(Hand::Right));
const LEFT_HIP: GrabVolume = GrabVolume::Holster(HolsterSite::Hip(Hand::Left));
const RIGHT_SHOULDER: GrabVolume = GrabVolume::Holster(HolsterSite::Shoulder(Hand::Right));
const CHEST: GrabVolume = GrabVolume::Holster(HolsterSite::Chest);

/// The least gap the defaults leave between two volumes, metres, so that a hand at one does not
/// brush the other.
const MIN_GAP_M: f32 = 0.01;

/// The least gap the defaults leave between a pouch and a holster, metres: a hand feeling for a
/// pouch lands a little off it more often than not, and a press that lands in a holster instead
/// draws or switches a weapon.
const MIN_POUCH_HOLSTER_GAP_M: f32 = 0.04;

fn assert_apart(c: &[Candidate], context: &str) {
    for (i, a) in c.iter().enumerate() {
        for b in &c[i + 1..] {
            let gap = a.centre.distance(b.centre) - a.radius - b.radius;
            let holster = |c: &Candidate| matches!(c.volume, GrabVolume::Holster(_));
            let pouch = |c: &Candidate| {
                matches!(
                    c.volume,
                    GrabVolume::GrenadePouch | GrabVolume::MagazinePouch | GrabVolume::C4Pouch
                )
            };
            let pouch_and_holster = (pouch(a) && holster(b)) || (holster(a) && pouch(b));
            let min = if pouch_and_holster {
                MIN_POUCH_HOLSTER_GAP_M
            } else {
                MIN_GAP_M
            };
            assert!(
                gap >= min,
                "{:?} and {:?} are {gap} m apart ({context})",
                a.volume,
                b.volume
            );
        }
    }
}

#[test]
fn the_holsters_sit_where_the_body_has_them() {
    let c = holsters(&STANDING, &DIMS);
    let right_hip = centre(&c, RIGHT_HIP);
    let left_hip = centre(&c, LEFT_HIP);
    let right_shoulder = centre(&c, RIGHT_SHOULDER);
    let chest = centre(&c, CHEST);
    assert!(right_hip.x > 0.0 && left_hip.x < 0.0);
    assert!((right_hip.x + left_hip.x).abs() < 1e-6, "the hips mirror");
    // The hips are well below the eyes, the shoulders just below, the chest between.
    assert!(right_hip.y < -0.6 && right_hip.y > -0.8);
    assert!(right_shoulder.y > -0.2 && right_shoulder.y < 0.0);
    assert!(chest.y < right_shoulder.y && chest.y > right_hip.y);
    // Over the shoulder is behind it; the chest is in front.
    assert!(right_shoulder.z > body::EYES_FORWARD_OF_SHOULDERS);
    assert!(chest.z < 0.0);
    assert_eq!(chest.x, 0.0);
}

#[test]
fn seated_hips_are_higher_and_forward_and_the_rest_follow_the_torso() {
    for dims in BODIES {
        let standing = holsters(&STANDING, &dims);
        let seated = holsters(&SEATED, &dims);
        let (up, sh) = (centre(&standing, RIGHT_HIP), centre(&seated, RIGHT_HIP));
        // At the belt line beside the waist: about half a metre below the eyes, close to the
        // body, and in front of the eyes over the thighs.
        assert!(sh.y > up.y + 0.1, "{sh} vs {up}");
        assert!(sh.y < -0.4 && sh.y > -0.65, "{sh}");
        assert!(sh.z < 0.0 && sh.z > -0.15, "{sh}");
        assert_eq!(sh.x, up.x);
        // The shoulders come forward a little, clear of a backrest; the chest stays.
        let (us, ss) = (
            centre(&standing, RIGHT_SHOULDER),
            centre(&seated, RIGHT_SHOULDER),
        );
        assert!(ss.z < us.z && ss.z > 0.0 && ss.y == us.y);
        assert_eq!(centre(&standing, CHEST), centre(&seated, CHEST));
    }
}

#[test]
fn the_default_volumes_keep_apart() {
    let swapped = PouchesConfig {
        grenade_side: PouchSide::OffHand,
        ..POUCHES
    };
    for frame in [STANDING, SEATED] {
        for dims in BODIES {
            for dominant in Hand::BOTH {
                for pouches in [POUCHES, swapped] {
                    let c = every_volume_for(&frame, &dims, &pouches, dominant);
                    assert_apart(
                        &c,
                        &format!(
                            "seated {}, height {}, dominant {dominant:?}",
                            frame.seated, dims.height
                        ),
                    );
                }
            }
        }
    }
}

#[test]
fn a_seated_hand_at_the_waist_grabs_the_hip_holster() {
    let dims = BodyDimensions::CHARACTER_ESTIMATE;
    // A hand resting at the side of the waist, over the front of the seat: half a metre below
    // the eyes, a hand's width out from the body, a little in front of the eyes.
    let hand = Vec3::new(0.22, -0.5, -0.08);
    let seated = holsters(&SEATED, &dims);
    assert_eq!(grab_volume(hand, Hand::Right, &seated), Some(RIGHT_HIP));
    let standing = holsters(&STANDING, &dims);
    assert_eq!(grab_volume(hand, Hand::Right, &standing), None);
}

#[test]
fn the_pouches_sit_on_the_front_of_the_body_by_handedness() {
    for dims in BODIES {
        let c = every_volume(&STANDING, &dims);
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        let c4 = centre(&c, GrabVolume::C4Pouch);
        let magazine = centre(&c, GrabVolume::MagazinePouch);
        let shoulders = centre(&c, RIGHT_SHOULDER);
        let hip = centre(&c, LEFT_HIP);
        // A right-handed player: the grenades on the dominant side of the chest, the C4 on the
        // other, and the magazines at the front of the off-hand's hip.
        assert!(grenade.x > 0.0 && c4.x < 0.0 && magazine.x < 0.0);
        assert!((grenade.x + c4.x).abs() < 1e-5 && grenade.y == c4.y);
        // The chest pouches on the upper chest, below the shoulders and above the belt; the
        // magazine pouch at the waist, above the hip holster.
        assert!(grenade.y < shoulders.y - 0.1 && grenade.y > magazine.y + 0.2);
        assert!(magazine.y > hip.y && magazine.y < -0.5, "{magazine}");
        // In front of the body, a few centimetres ahead of the eyes' vertical line rather than
        // out in the air.
        for pouch in [grenade, c4, magazine] {
            assert!(pouch.z < 0.0 && pouch.z > -0.12, "{pouch}");
        }
        // The magazine pouch is at the front of the hip, inside the hip holster.
        assert!(magazine.x > hip.x + 0.05 && magazine.z < hip.z - 0.1);
    }
}

#[test]
fn a_left_handed_layout_mirrors_a_right_handed_one() {
    for frame in [STANDING, SEATED] {
        let right = every_volume_for(&frame, &DIMS, &POUCHES, Hand::Right);
        let left = every_volume_for(&frame, &DIMS, &POUCHES, Hand::Left);
        for volume in [
            GrabVolume::GrenadePouch,
            GrabVolume::C4Pouch,
            GrabVolume::MagazinePouch,
        ] {
            let (r, l) = (centre(&right, volume), centre(&left, volume));
            assert!(
                (r.x + l.x).abs() < 1e-5 && r.y == l.y && r.z == l.z,
                "{volume:?}"
            );
        }
    }
}

#[test]
fn the_pouches_are_laid_out_around_the_gun_hand_not_the_dominant_hand() {
    // A left-handed player whose gun rides the right hand: the free hand is the left.
    let side = gun_side(Some(Hand::Right), None, Hand::Left);
    assert_eq!(side, Hand::Right);
    let magazine = magazine_pouch(&STANDING, &DIMS, &POUCHES, side, Some(Hand::Left));
    assert!(magazine.centre.x < 0.0, "{}", magazine.centre);
    assert_eq!(magazine.hand, Some(Hand::Left));
    // The free hand decides the side even against the gun side, and with no hand free the pouch
    // is on the side away from the guns.
    let free_right = magazine_pouch(&STANDING, &DIMS, &POUCHES, Hand::Right, Some(Hand::Right));
    assert!(free_right.centre.x > 0.0);
    let both_held = magazine_pouch(&STANDING, &DIMS, &POUCHES, Hand::Right, None);
    assert!(both_held.centre.x < 0.0);
    // The grenades go on the gun hand's side by default.
    let grenade = grenade_pouch(&STANDING, &DIMS, &POUCHES, side, ChestGrabbers::Either).unwrap();
    assert!(grenade.centre.x > 0.0);
    // Holstered, the layout stays where the last gun had it, and falls back on the dominant hand
    // before any gun is drawn.
    assert_eq!(gun_side(None, Some(Hand::Right), Hand::Left), Hand::Right);
    assert_eq!(gun_side(None, None, Hand::Left), Hand::Left);
    assert_eq!(
        gun_side(Some(Hand::Left), Some(Hand::Right), Hand::Right),
        Hand::Left
    );
}

#[test]
fn the_grenade_side_can_be_overridden_and_the_c4_takes_the_other() {
    for dominant in Hand::BOTH {
        let sides = chest_sides(&POUCHES, dominant);
        assert_eq!((sides.grenade, sides.c4), (dominant, dominant.other()));
        let swapped = PouchesConfig {
            grenade_side: PouchSide::OffHand,
            ..POUCHES
        };
        let sides = chest_sides(&swapped, dominant);
        assert_eq!((sides.grenade, sides.c4), (dominant.other(), dominant));
        let c = every_volume_for(&STANDING, &DIMS, &swapped, dominant);
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        assert_eq!(grenade.x > 0.0, dominant == Hand::Left);
    }
}

#[test]
fn either_hand_takes_from_the_chest_pouches_and_only_the_free_hand_from_the_magazines() {
    for frame in [STANDING, SEATED] {
        let c = every_volume(&frame, &DIMS);
        for volume in [GrabVolume::GrenadePouch, GrabVolume::C4Pouch] {
            let pouch = find(&c, volume);
            assert_eq!(pouch.hand, None);
            for hand in Hand::BOTH {
                assert_eq!(grab_volume(pouch.centre, hand, &c), Some(volume));
            }
        }
        let magazine = find(&c, GrabVolume::MagazinePouch);
        assert_eq!(magazine.hand, Some(Hand::Left));
        assert_eq!(
            grab_volume(magazine.centre, Hand::Left, &c),
            Some(GrabVolume::MagazinePouch)
        );
        assert_ne!(
            grab_volume(magazine.centre, Hand::Right, &c),
            Some(GrabVolume::MagazinePouch)
        );
    }
}

#[test]
fn a_hand_at_a_volume_grabs_it_and_a_hand_in_front_grabs_nothing() {
    for frame in [STANDING, SEATED] {
        let c = every_volume(&frame, &DIMS);
        for candidate in &c {
            assert_eq!(
                grab_volume(candidate.centre, Hand::Left, &c),
                Some(candidate.volume),
                "{:?}",
                candidate.volume
            );
        }
        // A hand held out in front at chest height, as when firing the grapple.
        let out_front = Vec3::new(0.0, -0.3, -0.45);
        assert_eq!(grab_volume(out_front, Hand::Left, &c), None);
    }
    // A hand hanging relaxed at the side, a little forward of the holster, still reaches it.
    let c = holsters(&STANDING, &DIMS);
    let hip = centre(&c, RIGHT_HIP);
    assert!(grab_volume(hip + Vec3::new(0.0, 0.0, -0.08), Hand::Right, &c).is_some());
}

/// A head at `position` looking level, with no previous tick.
fn level(position: Vec3) -> HeadDrop {
    HeadDrop {
        position,
        pitch_down: 0.0,
        previous_y: None,
    }
}

#[test]
fn bending_over_to_look_at_the_belt_does_not_move_the_pouches() {
    let upright = Anchor::Body { yaw: 0.0 };
    let y = |position: Vec3, pitch_deg: f32, previous_y| {
        body_frame(
            HeadDrop {
                position,
                pitch_down: pitch_deg.to_radians(),
                previous_y,
            },
            Stance::Standing,
            0.15,
            upright,
        )
        .origin
        .y
    };
    let bent = Vec3::new(0.0, -0.4, -0.15);
    // Looking well down at the belt, the head drops past the deadband but the frame holds.
    assert_eq!(y(bent, 55.0, Some(0.0)), 0.0);
    // Looking level, the same drop is a crouch.
    assert!((y(bent, 0.0, Some(0.0)) + 0.25).abs() < 1e-6);
    // In between, it follows part of the way.
    let part = y(bent, 30.0, Some(0.0));
    assert!(part < 0.0 && part > -0.25, "{part}");
    // Crouched, then looking down at the belt: the frame stays at the crouch.
    assert!((y(Vec3::new(0.0, -0.6, 0.0), 60.0, Some(-0.45)) + 0.45).abs() < 1e-6);
    // Standing up while still looking down: the frame rises with the head at once.
    assert_eq!(y(Vec3::new(0.0, -0.05, 0.0), 60.0, Some(-0.45)), 0.0);
    // The first tick, and a non-finite pitch, follow the head.
    assert!((y(bent, 55.0, None) + 0.25).abs() < 1e-6);
    assert!((y(bent, f32::NAN, Some(0.0)) + 0.25).abs() < 1e-6);
}

#[test]
fn the_body_drops_only_past_the_deadband() {
    let upright = Anchor::Body { yaw: 0.0 };
    let origin = |head, stance| body_frame(level(head), stance, 0.15, upright).origin;
    for stance in [Stance::Standing, Stance::Seated] {
        assert_eq!(origin(Vec3::new(0.3, -0.1, 0.2), stance), Vec3::ZERO);
        assert_eq!(origin(Vec3::new(0.0, 0.2, 0.0), stance), Vec3::ZERO);
        let crouched = origin(Vec3::new(0.0, -0.5, 0.0), stance);
        assert!((crouched.y + 0.35).abs() < 1e-6);
        assert_eq!(origin(Vec3::NAN, stance), Vec3::ZERO);
    }
    assert!(body_frame(level(Vec3::ZERO), Stance::Seated, 0.15, upright).seated);
    assert!(!body_frame(level(Vec3::ZERO), Stance::Standing, 0.15, upright).seated);
    assert_eq!(
        body_frame(
            level(Vec3::ZERO),
            Stance::Standing,
            0.15,
            Anchor::Body { yaw: f32::NAN }
        )
        .anchor,
        upright
    );
}

/// A skeleton built to the dimensions, with its eyes at the origin, turned by `yaw`.
fn ideal_skeleton(dims: &BodyDimensions, yaw: f32) -> SkeletonLandmarks {
    let turn = Quat::from_rotation_y(yaw);
    let shoulders = body::shoulder_positions(Vec3::ZERO, turn, dims);
    let pelvis = turn
        * Vec3::new(
            0.0,
            dims.height * HIP_HEIGHT_FRACTION - dims.eye_height,
            body::EYES_FORWARD_OF_SHOULDERS,
        );
    let half = turn * Vec3::new(dims.shoulder_width * 0.22, 0.0, 0.0);
    SkeletonLandmarks {
        head: Vec3::ZERO,
        pelvis,
        hips: [pelvis - half, pelvis + half],
        shoulders,
    }
}

#[test]
fn a_skeleton_built_to_the_dimensions_places_the_volumes_as_the_body_does() {
    for dims in BODIES {
        for seated in [false, true] {
            let body = BodyFrame { seated, ..STANDING };
            let rico = BodyFrame {
                anchor: Anchor::Skeleton(ideal_skeleton(&dims, 0.0)),
                ..body
            };
            for (a, b) in every_volume(&body, &dims)
                .iter()
                .zip(every_volume(&rico, &dims))
            {
                assert_eq!(a.volume, b.volume);
                assert!(
                    a.centre.distance(b.centre) < 1e-4,
                    "{:?}: {} vs {} (seated {seated})",
                    a.volume,
                    a.centre,
                    b.centre
                );
            }
        }
    }
}

#[test]
fn the_volumes_turn_with_the_torso_about_the_eyes() {
    let yaw = 1.2;
    let turn = Quat::from_rotation_y(yaw);
    for seated in [false, true] {
        let ahead = every_volume(&BodyFrame { seated, ..STANDING }, &DIMS);
        let turned_body = BodyFrame {
            seated,
            anchor: Anchor::Body { yaw },
            ..STANDING
        };
        let turned_rico = BodyFrame {
            seated,
            anchor: Anchor::Skeleton(ideal_skeleton(&DIMS, yaw)),
            ..STANDING
        };
        for frame in [turned_body, turned_rico] {
            for (a, b) in ahead.iter().zip(every_volume(&frame, &DIMS)) {
                assert!(
                    (turn * a.centre).distance(b.centre) < 1e-4,
                    "{:?} under {}",
                    a.volume,
                    frame.anchor.label()
                );
            }
        }
    }
}

#[test]
fn the_hip_volumes_ride_the_pelvis_and_the_shoulder_volumes_ride_the_chest() {
    let mut skeleton = ideal_skeleton(&DIMS, 0.0);
    let before = every_volume(
        &BodyFrame {
            anchor: Anchor::Skeleton(skeleton),
            ..STANDING
        },
        &DIMS,
    );
    // The chest turns 30 degrees over a still pelvis.
    let chest = skeleton.chest();
    let twist = Quat::from_rotation_y(0.5);
    skeleton.shoulders = skeleton.shoulders.map(|s| chest + twist * (s - chest));
    let after = every_volume(
        &BodyFrame {
            anchor: Anchor::Skeleton(skeleton),
            ..STANDING
        },
        &DIMS,
    );
    for (a, b) in before.iter().zip(&after) {
        let moved = a.centre.distance(b.centre) > 1e-3;
        let on_chest = matches!(
            a.volume,
            GrabVolume::Holster(HolsterSite::Shoulder(_) | HolsterSite::Chest)
                | GrabVolume::GrenadePouch
                | GrabVolume::C4Pouch
        );
        assert_eq!(moved, on_chest, "{:?}", a.volume);
    }
}

#[test]
fn the_volumes_on_the_skeleton_keep_apart() {
    for dims in BODIES {
        for seated in [false, true] {
            let frame = BodyFrame {
                seated,
                anchor: Anchor::Skeleton(ideal_skeleton(&dims, -0.6)),
                ..STANDING
            };
            assert_apart(&every_volume(&frame, &dims), &format!("seated {seated}"));
        }
    }
}

#[test]
fn a_degenerate_girdle_keeps_the_cockpit_axes() {
    assert_eq!(girdle_basis([Vec3::ZERO, Vec3::Y]), Quat::IDENTITY);
    let basis = girdle_basis([Vec3::new(-0.2, 0.0, 0.0), Vec3::new(0.2, 0.1, 0.0)]);
    assert!((basis * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-5);
}

#[test]
fn only_a_free_hand_takes_from_the_chest_pouches_while_a_gun_is_drawn() {
    assert_eq!(ChestGrabbers::for_guns([false; 2]), ChestGrabbers::Either);
    assert_eq!(
        ChestGrabbers::for_guns([false, true]),
        ChestGrabbers::Only(Hand::Left)
    );
    assert_eq!(
        ChestGrabbers::for_guns([true, false]),
        ChestGrabbers::Only(Hand::Right)
    );
    assert_eq!(ChestGrabbers::for_guns([true; 2]), ChestGrabbers::Neither);
    let only_left = ChestGrabbers::Only(Hand::Left);
    for pouch in [
        grenade_pouch(&STANDING, &DIMS, &POUCHES, Hand::Right, only_left).unwrap(),
        c4_pouch(&STANDING, &DIMS, &POUCHES, Hand::Right, only_left).unwrap(),
    ] {
        assert_eq!(pouch.hand, Some(Hand::Left));
        let c = [pouch];
        assert_eq!(grab_volume(pouch.centre, Hand::Right, &c), None);
        assert_eq!(
            grab_volume(pouch.centre, Hand::Left, &c),
            Some(pouch.volume)
        );
    }
    assert!(
        grenade_pouch(
            &STANDING,
            &DIMS,
            &POUCHES,
            Hand::Right,
            ChestGrabbers::Neither
        )
        .is_none()
    );
    assert!(
        c4_pouch(
            &STANDING,
            &DIMS,
            &POUCHES,
            Hand::Right,
            ChestGrabbers::Neither
        )
        .is_none()
    );
}
