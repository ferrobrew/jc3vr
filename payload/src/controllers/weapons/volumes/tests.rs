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

/// One unclaimed socket volume per socket with a place on the body, for the draw restriction.
fn sockets() -> Vec<Candidate> {
    [
        Socket::Hip(Hand::Left),
        Socket::Hip(Hand::Right),
        Socket::Shoulder(Hand::Left),
        Socket::Shoulder(Hand::Right),
        Socket::Chest,
    ]
    .into_iter()
    .map(|socket| Candidate {
        volume: GrabVolume::Socket(socket),
        centre: Vec3::ZERO,
        radius: 0.1,
        hand: None,
    })
    .collect()
}

/// Every pouch, for a player whose dominant hand is `dominant`.
fn every_volume_for(frame: &BodyFrame, pouches: &PouchTuning, dominant: Hand) -> Vec<Candidate> {
    let mut c = Vec::new();
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
        dominant.other(),
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

/// The least gap the defaults leave between two volumes, metres, so that a hand at one does not
/// brush the other.
const MIN_GAP_M: f32 = 0.01;

fn assert_apart(c: &[Candidate], context: &str) {
    for (i, a) in c.iter().enumerate() {
        for b in &c[i + 1..] {
            let gap = a.centre.distance(b.centre) - a.radius - b.radius;
            assert!(
                gap >= MIN_GAP_M,
                "{:?} and {:?} are {gap} m apart ({context})",
                a.volume,
                b.volume
            );
        }
    }
}

#[test]
fn the_default_volumes_keep_apart() {
    for dims in BODIES {
        for yaw in [0.0, -0.6] {
            let frame = frame_of(ideal_skeleton(&dims, yaw));
            for dominant in Hand::BOTH {
                let c = every_volume_for(&frame, &POUCHES, dominant);
                assert_apart(
                    &c,
                    &format!("height {}, yaw {yaw}, dominant {dominant:?}", dims.height),
                );
            }
        }
    }
}

#[test]
fn the_pouches_sit_on_the_front_of_the_body_by_handedness() {
    for dims in BODIES {
        let skeleton = ideal_skeleton(&dims, 0.0);
        let c = every_volume(&frame_of(skeleton));
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        let c4 = centre(&c, GrabVolume::C4Pouch);
        let magazine = centre(&c, GrabVolume::MagazinePouch);
        let shoulders = skeleton.shoulders[Hand::Right.index()];
        let hip = skeleton.hips[Hand::Left.index()];
        // A right-handed player: the grenades on the dominant side of the chest, the C4 on the
        // other, and the magazines at the front of the off-hand's hip.
        assert!(grenade.x > 0.0 && c4.x < 0.0 && magazine.x < 0.0);
        assert!((grenade.x + c4.x).abs() < 1e-5 && (grenade.y - c4.y).abs() < 1e-5);
        // The chest pouches on the upper chest, below the shoulders and above the belt; the
        // magazine pouch at the waist, above the hip joints.
        assert!(grenade.y < shoulders.y - 0.1 && grenade.y > magazine.y + 0.2);
        assert!(magazine.y > hip.y && magazine.y < -0.5, "{magazine}");
        // The magazine pouch in front of the belly, a few centimetres ahead of the eyes' vertical
        // line rather than out in the air; the chest pouches back at the chest's surface, ahead of
        // the shoulder joints' line.
        assert!(magazine.z < 0.0 && magazine.z > -0.12, "{magazine}");
        for pouch in [grenade, c4] {
            assert!(
                pouch.z < shoulders.z && pouch.z > shoulders.z - 0.1,
                "{pouch}"
            );
        }
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
fn the_magazine_pouch_is_laid_out_around_the_gun_hand_not_the_dominant_hand() {
    let frame = standing(&DIMS);
    // A left-handed player whose gun rides the right hand: the free hand is the left.
    let side = gun_side(Some(Hand::Right), None, Hand::Left);
    assert_eq!(side, Hand::Right);
    let magazine = magazine_pouch(&frame, &POUCHES, side.other(), Some(Hand::Left));
    assert!(magazine.centre.x < 0.0, "{}", magazine.centre);
    assert_eq!(magazine.hand, Some(Hand::Left));
    // The pouch sits on the side it is placed on, whichever hand takes from it.
    let right = magazine_pouch(&frame, &POUCHES, Hand::Right, None);
    assert!(right.centre.x > 0.0);
    assert_eq!(right.hand, None);
    let left = magazine_pouch(&frame, &POUCHES, Hand::Left, Some(Hand::Right));
    assert!(left.centre.x < 0.0);
    // The chest pouches stay on the dominant hand's sides whichever hand holds the gun.
    let grenade = grenade_pouch(&frame, &POUCHES, Hand::Left, ChestGrabbers::Either).unwrap();
    assert!(grenade.centre.x < 0.0);
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
fn the_grenade_takes_the_dominant_side_and_the_c4_the_other() {
    for dominant in Hand::BOTH {
        let sides = chest_sides(dominant);
        assert_eq!((sides.grenade, sides.c4), (dominant, dominant.other()));
        let c = every_volume_for(&standing(&DIMS), &POUCHES, dominant);
        let grenade = centre(&c, GrabVolume::GrenadePouch);
        assert_eq!(grenade.x > 0.0, dominant == Hand::Right);
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
fn the_belt_pouch_rides_the_pelvis_and_the_chest_pouches_ride_the_chest() {
    let mut skeleton = ideal_skeleton(&DIMS, 0.0);
    let before = every_volume(&frame_of(skeleton));
    // The chest turns 30 degrees over a still pelvis.
    let chest = skeleton.chest();
    let twist = Quat::from_rotation_y(0.5);
    skeleton.shoulders = skeleton.shoulders.map(|s| chest + twist * (s - chest));
    let after = every_volume(&frame_of(skeleton));
    for (a, b) in before.iter().zip(&after) {
        let moved = a.centre.distance(b.centre) > 1e-3;
        let on_chest = matches!(a.volume, GrabVolume::GrenadePouch | GrabVolume::C4Pouch);
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
    let shoulders = |c: &[Candidate]| -> Vec<Option<Hand>> {
        c.iter()
            .filter(|c| matches!(c.volume, GrabVolume::Socket(Socket::Shoulder(_))))
            .map(|c| c.hand)
            .collect()
    };
    for dominant in Hand::BOTH {
        let mut empty = sockets();
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
        let mut armed = sockets();
        reserve_long_guns(&mut armed, dominant, false);
        assert_eq!(shoulders(&armed), vec![None; 2]);
    }
}

/// The ideal skeleton with its upper body bent forward by `bend` radians about the middle of the
/// hip joints.
fn bent_forward(bend: f32) -> SkeletonLandmarks {
    let mut skeleton = ideal_skeleton(&DIMS, 0.0);
    let hips = skeleton.hip_centre();
    let fold = |p: Vec3| hips + Quat::from_rotation_x(-bend) * (p - hips);
    skeleton.shoulders = skeleton.shoulders.map(fold);
    skeleton.head = fold(skeleton.head);
    skeleton
}

fn chest_pouches(frame: &BodyFrame) -> [Vec3; 2] {
    let c = every_volume(frame);
    [
        centre(&c, GrabVolume::GrenadePouch),
        centre(&c, GrabVolume::C4Pouch),
    ]
}

#[test]
fn a_look_down_keeps_the_chest_pouches_off_the_bend_it_causes() {
    let upright = chest_pouches(&standing(&DIMS));
    let bent = frame_of(bent_forward(0.3));
    let down = Quat::from_rotation_x(-1.2);
    // Looking down, the pouches stay where the upright chest has them.
    for (a, b) in upright.iter().zip(chest_pouches(&bent.looking(down))) {
        assert!(a.distance(b) < 1e-3, "{a} and {b}");
    }
    // With the head level, a lean carries them with the chest.
    let leaning = chest_pouches(&bent.looking(Quat::IDENTITY));
    assert_eq!(leaning, chest_pouches(&bent));
    for (a, b) in upright.iter().zip(leaning) {
        assert!(b.z < a.z - 0.05, "{a} and {b}");
    }
    // The magazine pouch rides the pelvis either way.
    let magazine = |frame: &BodyFrame| centre(&every_volume(frame), GrabVolume::MagazinePouch);
    assert_eq!(magazine(&bent.looking(down)), magazine(&bent));
}

#[test]
fn a_body_folded_past_a_look_down_keeps_its_chest_pouches_on_its_chest() {
    let down = Quat::from_rotation_x(-1.2);
    let folded = frame_of(bent_forward(1.2));
    assert_eq!(chest_pouches(&folded.looking(down)), chest_pouches(&folded));
    // Lying face down in a skydive, looking down at the ground.
    let lying = frame_of(
        ideal_skeleton(&DIMS, 0.0).map(|p| Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) * p),
    );
    assert_eq!(chest_pouches(&lying.looking(down)), chest_pouches(&lying));
}

#[test]
fn the_back_sockets_reach_lower_and_further_out_than_their_weapons_sit() {
    let frame = standing(&DIMS);
    let chest = frame.skeleton.chest();
    let at = |socket: Socket, x: f32| Candidate {
        volume: GrabVolume::Socket(socket),
        centre: chest + Vec3::new(x, 0.2, 0.15),
        radius: SOCKETS.shoulder_radius_m,
        hand: None,
    };
    let sockets = [
        at(Socket::Shoulder(Hand::Left), -0.1),
        at(Socket::Shoulder(Hand::Right), 0.1),
        at(Socket::BackOuter, 0.12),
        at(Socket::Hip(Hand::Right), 0.2),
    ];
    let mut reached = sockets;
    reach_back_sockets(&mut reached, &frame);
    for (before, after) in sockets.iter().zip(&reached) {
        let moved = after.centre - before.centre;
        if matches!(before.volume, GrabVolume::Socket(Socket::Hip(_))) {
            assert_eq!(moved, Vec3::ZERO);
            continue;
        }
        assert!((moved.y + 0.12).abs() < 1e-4, "{moved}");
        assert!(
            (moved.x.abs() - 0.05).abs() < 1e-4 && moved.x.signum() == before.centre.x.signum(),
            "{moved}"
        );
        assert_eq!(after.radius, before.radius);
    }
}
