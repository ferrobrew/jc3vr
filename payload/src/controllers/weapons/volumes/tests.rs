use super::*;
use crate::controllers::{body::BodyDimensions, grab::grab_volume};

const DIMS: BodyDimensions = BodyDimensions::CHARACTER_ESTIMATE;
/// A body a little shorter than the character's.
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
const BODIES: [BodyDimensions; 3] = [DIMS, BodyDimensions::GENERIC_ADULT, TALL];
const POUCHES: PouchesConfig = PouchesConfig::new();

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

fn frame_of(skeleton: SkeletonLandmarks) -> BodyFrame {
    BodyFrame::on(skeleton).expect("a body to place the volumes on")
}

fn standing(dims: &BodyDimensions) -> BodyFrame {
    frame_of(ideal_skeleton(dims, 0.0))
}

fn landmarks(frame: &BodyFrame) -> Vec<Candidate> {
    socket_landmarks(frame, &HolstersConfig::new())
}

/// Every volume, the sockets and the pouches, for a player whose dominant hand is `dominant`.
fn every_volume_for(frame: &BodyFrame, pouches: &PouchesConfig, dominant: Hand) -> Vec<Candidate> {
    let mut c = landmarks(frame);
    c.extend(grenade_pouch(
        frame,
        pouches,
        dominant,
        ChestGrabbers::Either,
    ));
    c.extend(c4_pouch(frame, pouches, dominant, ChestGrabbers::Either));
    c.push(magazine_pouch(
        frame,
        pouches,
        dominant,
        Some(dominant.other()),
    ));
    c
}

fn every_volume(frame: &BodyFrame) -> Vec<Candidate> {
    every_volume_for(frame, &POUCHES, Hand::Right)
}

fn find(c: &[Candidate], volume: GrabVolume) -> Candidate {
    *c.iter().find(|c| c.volume == volume).unwrap()
}

fn centre(c: &[Candidate], volume: GrabVolume) -> Vec3 {
    find(c, volume).centre
}

const RIGHT_HIP: GrabVolume = GrabVolume::Socket(Socket::Hip(Hand::Right));
const LEFT_HIP: GrabVolume = GrabVolume::Socket(Socket::Hip(Hand::Left));
const RIGHT_SHOULDER: GrabVolume = GrabVolume::Socket(Socket::Shoulder(Hand::Right));
const CHEST: GrabVolume = GrabVolume::Socket(Socket::Chest);

/// The least gap the defaults leave between two volumes, metres, so that a hand at one does not
/// brush the other.
const MIN_GAP_M: f32 = 0.01;

/// The least gap the defaults leave between a pouch and a socket, metres: a hand feeling for a
/// pouch lands a little off it more often than not, and a press that lands in a socket instead
/// draws or switches a weapon.
const MIN_POUCH_HOLSTER_GAP_M: f32 = 0.04;

fn assert_apart(c: &[Candidate], context: &str) {
    for (i, a) in c.iter().enumerate() {
        for b in &c[i + 1..] {
            let gap = a.centre.distance(b.centre) - a.radius - b.radius;
            let socket = |c: &Candidate| matches!(c.volume, GrabVolume::Socket(_));
            let pouch = |c: &Candidate| {
                matches!(
                    c.volume,
                    GrabVolume::GrenadePouch | GrabVolume::MagazinePouch | GrabVolume::C4Pouch
                )
            };
            let pouch_and_socket = (pouch(a) && socket(b)) || (socket(a) && pouch(b));
            let min = if pouch_and_socket {
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
fn the_sockets_sit_where_the_body_has_them() {
    let c = landmarks(&standing(&DIMS));
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
    assert!(chest.x.abs() < 1e-6);
}

#[test]
fn the_default_volumes_keep_apart() {
    let swapped = PouchesConfig {
        grenade_side: PouchSide::OffHand,
        ..POUCHES
    };
    for dims in BODIES {
        for yaw in [0.0, -0.6] {
            let frame = frame_of(ideal_skeleton(&dims, yaw));
            for dominant in Hand::BOTH {
                for pouches in [POUCHES, swapped] {
                    let c = every_volume_for(&frame, &pouches, dominant);
                    assert_apart(
                        &c,
                        &format!("height {}, yaw {yaw}, dominant {dominant:?}", dims.height),
                    );
                }
            }
        }
    }
}

#[test]
fn the_pouches_sit_on_the_front_of_the_body_by_handedness() {
    for dims in BODIES {
        let c = every_volume(&standing(&dims));
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        let c4 = centre(&c, GrabVolume::C4Pouch);
        let magazine = centre(&c, GrabVolume::MagazinePouch);
        let shoulders = centre(&c, RIGHT_SHOULDER);
        let hip = centre(&c, LEFT_HIP);
        // A right-handed player: the grenades on the dominant side of the chest, the C4 on the
        // other, and the magazines at the front of the off-hand's hip.
        assert!(grenade.x > 0.0 && c4.x < 0.0 && magazine.x < 0.0);
        assert!((grenade.x + c4.x).abs() < 1e-5 && (grenade.y - c4.y).abs() < 1e-5);
        // The chest pouches on the upper chest, below the shoulders and above the belt; the
        // magazine pouch at the waist, above the hip socket.
        assert!(grenade.y < shoulders.y - 0.1 && grenade.y > magazine.y + 0.2);
        assert!(magazine.y > hip.y && magazine.y < -0.5, "{magazine}");
        // In front of the body, a few centimetres ahead of the eyes' vertical line rather than
        // out in the air.
        for pouch in [grenade, c4, magazine] {
            assert!(pouch.z < 0.0 && pouch.z > -0.12, "{pouch}");
        }
        // The magazine pouch is at the front of the hip, inside the hip socket.
        assert!(magazine.x > hip.x + 0.05 && magazine.z < hip.z - 0.1);
    }
}

#[test]
fn a_left_handed_layout_mirrors_a_right_handed_one() {
    let frame = standing(&DIMS);
    let right = every_volume_for(&frame, &POUCHES, Hand::Right);
    let left = every_volume_for(&frame, &POUCHES, Hand::Left);
    for volume in [
        GrabVolume::GrenadePouch,
        GrabVolume::C4Pouch,
        GrabVolume::MagazinePouch,
    ] {
        let (r, l) = (centre(&right, volume), centre(&left, volume));
        assert!(
            (r.x + l.x).abs() < 1e-5 && (r.y - l.y).abs() < 1e-5 && (r.z - l.z).abs() < 1e-5,
            "{volume:?}"
        );
    }
}

#[test]
fn the_pouches_are_laid_out_around_the_gun_hand_not_the_dominant_hand() {
    let frame = standing(&DIMS);
    // A left-handed player whose gun rides the right hand: the free hand is the left.
    let side = gun_side(Some(Hand::Right), None, Hand::Left);
    assert_eq!(side, Hand::Right);
    let magazine = magazine_pouch(&frame, &POUCHES, side, Some(Hand::Left));
    assert!(magazine.centre.x < 0.0, "{}", magazine.centre);
    assert_eq!(magazine.hand, Some(Hand::Left));
    // The free hand decides the side even against the gun side, and with no hand free the pouch
    // is on the side away from the guns.
    let free_right = magazine_pouch(&frame, &POUCHES, Hand::Right, Some(Hand::Right));
    assert!(free_right.centre.x > 0.0);
    let both_held = magazine_pouch(&frame, &POUCHES, Hand::Right, None);
    assert!(both_held.centre.x < 0.0);
    // The grenades go on the gun hand's side by default.
    let grenade = grenade_pouch(&frame, &POUCHES, side, ChestGrabbers::Either).unwrap();
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
        let c = every_volume_for(&standing(&DIMS), &swapped, dominant);
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        assert_eq!(grenade.x > 0.0, dominant == Hand::Left);
    }
}

#[test]
fn either_hand_takes_from_the_chest_pouches_and_only_the_free_hand_from_the_magazines() {
    let c = every_volume(&standing(&DIMS));
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

#[test]
fn a_hand_at_a_volume_grabs_it_and_a_hand_in_front_grabs_nothing() {
    let frame = standing(&DIMS);
    let c = every_volume(&frame);
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
    // A hand hanging relaxed at the side, a little forward of the socket, still reaches it.
    let c = landmarks(&frame);
    let hip = centre(&c, RIGHT_HIP);
    assert!(grab_volume(hip + Vec3::new(0.0, 0.0, -0.08), Hand::Right, &c).is_some());
}

#[test]
fn the_volumes_ride_the_body_in_any_orientation() {
    let upright = every_volume(&standing(&DIMS));
    // Turned in the room, pitched face down as in a skydive, rolled onto a side, and moved.
    let turns = [
        Quat::from_rotation_y(1.2),
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        Quat::from_rotation_z(0.9) * Quat::from_rotation_y(-0.4),
    ];
    let shift = Vec3::new(0.3, -0.2, 0.5);
    for turn in turns {
        let skeleton = ideal_skeleton(&DIMS, 0.0).map(|p| turn * p + shift);
        let moved = every_volume(&frame_of(skeleton));
        for (a, b) in upright.iter().zip(&moved) {
            assert_eq!(a.volume, b.volume);
            assert!(
                (turn * a.centre + shift).distance(b.centre) < 1e-4,
                "{:?} under {turn}",
                a.volume
            );
        }
    }
}

#[test]
fn the_hip_volumes_ride_the_pelvis_and_the_shoulder_volumes_ride_the_chest() {
    let mut skeleton = ideal_skeleton(&DIMS, 0.0);
    let before = every_volume(&frame_of(skeleton));
    // The chest turns 30 degrees over a still pelvis.
    let chest = skeleton.chest();
    let twist = Quat::from_rotation_y(0.5);
    skeleton.shoulders = skeleton.shoulders.map(|s| chest + twist * (s - chest));
    let after = every_volume(&frame_of(skeleton));
    for (a, b) in before.iter().zip(&after) {
        let moved = a.centre.distance(b.centre) > 1e-3;
        let on_chest = matches!(
            a.volume,
            GrabVolume::Socket(Socket::Shoulder(_) | Socket::Chest)
                | GrabVolume::GrenadePouch
                | GrabVolume::C4Pouch
        );
        assert_eq!(moved, on_chest, "{:?}", a.volume);
    }
}

#[test]
fn a_bent_spine_tilts_the_girdles_with_it() {
    // Bending forward at the hips: the shoulders come forward and down over a still pelvis, and
    // the chest's pouches stay on the front of the chest rather than hanging below it.
    let mut skeleton = ideal_skeleton(&DIMS, 0.0);
    let hips = skeleton.hip_centre();
    let bend = Quat::from_rotation_x(-0.6);
    skeleton.shoulders = skeleton.shoulders.map(|s| hips + bend * (s - hips));
    let frame = frame_of(skeleton);
    let spine = (skeleton.chest() - hips).normalize();
    assert!((frame.chest * Vec3::Y).distance(spine) < 1e-5);
    let grenade = centre(&every_volume(&frame), GrabVolume::GrenadePouch);
    let ahead = (grenade - skeleton.chest()).dot(frame.chest * Vec3::NEG_Z);
    assert!(ahead > 0.0, "{grenade}");
}

#[test]
fn a_degenerate_skeleton_places_nothing() {
    let skeleton = ideal_skeleton(&DIMS, 0.0);
    // The shoulders on one point.
    let pinched = SkeletonLandmarks {
        shoulders: [skeleton.chest(); 2],
        ..skeleton
    };
    assert_eq!(BodyFrame::on(pinched), None);
    // The spine along the line through the hips.
    let along = SkeletonLandmarks {
        shoulders: skeleton
            .hips
            .map(|h| h + (skeleton.hips[1] - skeleton.hips[0])),
        ..skeleton
    };
    assert_eq!(BodyFrame::on(along), None);
    let broken = SkeletonLandmarks {
        pelvis: Vec3::NAN,
        ..skeleton
    };
    assert_eq!(BodyFrame::on(broken), None);
}

#[test]
fn only_a_free_hand_takes_from_the_chest_pouches_while_a_gun_is_drawn() {
    let frame = standing(&DIMS);
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
        grenade_pouch(&frame, &POUCHES, Hand::Right, only_left).unwrap(),
        c4_pouch(&frame, &POUCHES, Hand::Right, only_left).unwrap(),
    ] {
        assert_eq!(pouch.hand, Some(Hand::Left));
        let c = [pouch];
        assert_eq!(grab_volume(pouch.centre, Hand::Right, &c), None);
        assert_eq!(
            grab_volume(pouch.centre, Hand::Left, &c),
            Some(pouch.volume)
        );
    }
    assert!(grenade_pouch(&frame, &POUCHES, Hand::Right, ChestGrabbers::Neither).is_none());
    assert!(c4_pouch(&frame, &POUCHES, Hand::Right, ChestGrabbers::Neither).is_none());
}

#[test]
fn with_both_hands_empty_only_the_dominant_hand_draws_a_long_gun() {
    let frame = standing(&DIMS);
    let shoulders = |c: &[Candidate]| -> Vec<Option<Hand>> {
        c.iter()
            .filter(|c| matches!(c.volume, GrabVolume::Socket(Socket::Shoulder(_))))
            .map(|c| c.hand)
            .collect()
    };
    for dominant in Hand::BOTH {
        let mut empty = landmarks(&frame);
        let count = empty.len();
        reserve_long_guns(&mut empty, dominant, true);
        assert_eq!(
            empty.len(),
            count,
            "the volumes stay, for the dominant hand"
        );
        assert_eq!(shoulders(&empty), vec![Some(dominant); 2]);
        // The hips and the chest are either hand's still.
        assert!(
            empty
                .iter()
                .filter(|c| !matches!(c.volume, GrabVolume::Socket(Socket::Shoulder(_))))
                .all(|c| c.hand.is_none())
        );
        // A long gun docked under the off-hand's touch is not offered to it.
        let mut touched = vec![Candidate {
            volume: GrabVolume::Socket(Socket::Shoulder(Hand::Left)),
            centre: Vec3::ZERO,
            radius: 0.1,
            hand: Some(dominant.other()),
        }];
        reserve_long_guns(&mut touched, dominant, true);
        assert!(touched.is_empty());
        // With a gun in a hand, either hand takes one, as a second weapon.
        let mut armed = landmarks(&frame);
        reserve_long_guns(&mut armed, dominant, false);
        assert_eq!(shoulders(&armed), vec![None; 2]);
    }
}
