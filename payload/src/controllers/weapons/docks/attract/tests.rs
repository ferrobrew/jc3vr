use glam::Quat;

use super::*;
use crate::controllers::body::rig::frames::SkeletonLandmarks;

/// A standing body facing -Z: shoulders at 1.45 m, hips at 0.95 m, 0.4 m and 0.2 m wide.
fn body(yaw: f32) -> BodyFrame {
    let turn = Quat::from_rotation_y(yaw);
    let at = |p: Vec3| turn * p;
    BodyFrame::on(SkeletonLandmarks {
        head: at(Vec3::new(0.0, 1.65, 0.0)),
        pelvis: at(Vec3::new(0.0, 0.95, 0.0)),
        hips: [
            at(Vec3::new(-0.1, 0.95, 0.0)),
            at(Vec3::new(0.1, 0.95, 0.0)),
        ],
        shoulders: [
            at(Vec3::new(-0.2, 1.45, 0.0)),
            at(Vec3::new(0.2, 1.45, 0.0)),
        ],
    })
    .expect("a body")
}

/// The cockpit frame's place in the world, for the tests: a plain shift.
fn offset(point: Vec3) -> Vec3 {
    point + Vec3::new(100.0, 0.0, -50.0)
}

fn dock(socket: Socket, hand: Hand, centre: Vec3, radius: f32) -> (Candidate, SocketUse) {
    (
        Candidate {
            volume: GrabVolume::Socket(socket),
            centre,
            radius,
            hand: Some(hand),
        },
        SocketUse::Dock,
    )
}

/// The right shoulder's socket behind the shoulder blade and the right hip's beside the thigh,
/// each free for the right hand.
fn free_sockets() -> Vec<(Candidate, SocketUse)> {
    vec![
        dock(
            Socket::Shoulder(Hand::Right),
            Hand::Right,
            Vec3::new(0.12, 1.3, 0.15),
            0.17,
        ),
        dock(
            Socket::Hip(Hand::Right),
            Hand::Right,
            Vec3::new(0.2, 0.85, 0.0),
            0.12,
        ),
    ]
}

fn attracted(grip: Vec3, speed: f32) -> Option<Socket> {
    let body = body(0.0);
    let attractors = attractors(&free_sockets(), Some(&body), offset, &ATTRACTION);
    attract(&attractors, Hand::Right, grip, speed, &ATTRACTION).map(|a| a.socket)
}

#[test]
fn a_hand_let_open_over_the_shoulder_docks_on_the_back() {
    // Above the shoulder, well outside the socket's own reach behind the shoulder blade.
    let over = Vec3::new(0.18, 1.62, 0.0);
    assert!(over.distance(Vec3::new(0.12, 1.3, 0.15)) > 0.17);
    assert_eq!(attracted(over, 0.5), Some(Socket::Shoulder(Hand::Right)));
    // Beside the head on that side and a little behind it.
    assert_eq!(
        attracted(Vec3::new(0.25, 1.55, 0.1), 0.5),
        Some(Socket::Shoulder(Hand::Right))
    );
}

#[test]
fn a_weapon_let_go_of_in_front_of_the_body_falls() {
    // In front of the chest, within the back socket's radius but out in front of the shoulders.
    let front = Vec3::new(0.2, 1.4, -0.25);
    assert!(front.distance(Vec3::new(0.12, 1.3, 0.15)) <= ATTRACTION.shoulder_m);
    assert_eq!(attracted(front, 0.2), None);
    // Held out over the belly, within the hip's radius.
    let belly = Vec3::new(0.15, 0.9, -0.27);
    assert!(belly.distance(Vec3::new(0.2, 0.85, 0.0)) <= ATTRACTION.hip_m);
    assert_eq!(attracted(belly, 0.2), None);
    // Out at the side of the chest, beyond the hip's radius.
    assert_eq!(attracted(Vec3::new(0.45, 1.15, -0.1), 0.2), None);
    // Out at arm's length, nowhere near a socket.
    assert_eq!(attracted(Vec3::new(0.3, 1.2, -0.6), 0.2), None);
}

#[test]
fn a_throw_is_never_attracted() {
    let over = Vec3::new(0.18, 1.62, 0.0);
    assert_eq!(attracted(over, ATTRACTION.back_max_speed_mps + 0.5), None);
    let beside = Vec3::new(0.32, 0.95, 0.05);
    assert_eq!(attracted(beside, ATTRACTION.hip_max_speed_mps + 0.5), None);
    assert_eq!(attracted(over, f32::NAN), None);
    assert_eq!(attracted(Vec3::NAN, 0.0), None);
}

#[test]
fn a_weapon_swung_back_over_the_shoulder_docks() {
    // The 10 November 17:07 session's holsters over the shoulder let go at 3.6 and 3.9 m/s, behind
    // the line through the shoulder joints.
    let behind = Vec3::new(0.2, 1.6, 0.12);
    assert_eq!(attracted(behind, 3.9), Some(Socket::Shoulder(Hand::Right)));
}

#[test]
fn a_pistol_turned_down_onto_the_front_of_the_thigh_docks() {
    // In front of the hip joints by about the depth of the thigh, at the belt, moving as a hand
    // brings a pistol down onto the hip.
    let thigh = Vec3::new(0.28, 1.0, -0.19);
    assert_eq!(attracted(thigh, 2.7), Some(Socket::Hip(Hand::Right)));
}

#[test]
fn an_attraction_sends_the_grip_to_the_sockets_centre_in_the_world() {
    let body = body(0.0);
    let attractors = attractors(&free_sockets(), Some(&body), offset, &ATTRACTION);
    let over = Vec3::new(0.18, 1.62, 0.0);
    let attraction = attract(&attractors, Hand::Right, over, 0.5, &ATTRACTION).unwrap();
    let centre = Vec3::new(0.12, 1.3, 0.15);
    assert!(attraction.toward.distance(offset(centre)) < 1e-5);
    assert!((attraction.distance - over.distance(centre)).abs() < 1e-5);
}

#[test]
fn a_hand_beside_the_thigh_docks_on_the_hip() {
    let beside = Vec3::new(0.32, 0.95, 0.05);
    assert!(beside.distance(Vec3::new(0.2, 0.85, 0.0)) > 0.12);
    assert_eq!(attracted(beside, 1.0), Some(Socket::Hip(Hand::Right)));
}

#[test]
fn the_socket_the_hand_is_deepest_inside_wins() {
    // A hip socket riding high, so that its attraction meets the shoulder's above the back's floor.
    let hip_centre = Vec3::new(0.2, 1.0, 0.0);
    let shoulder_centre = Vec3::new(0.12, 1.3, 0.15);
    let sockets = vec![
        dock(
            Socket::Shoulder(Hand::Right),
            Hand::Right,
            shoulder_centre,
            0.17,
        ),
        dock(Socket::Hip(Hand::Right), Hand::Right, hip_centre, 0.12),
    ];
    let attractors = attractors(&sockets, Some(&body(0.0)), offset, &ATTRACTION);
    let grip = Vec3::new(0.2, 1.17, 0.0);
    let hip_distance = grip.distance(hip_centre);
    let shoulder_distance = grip.distance(shoulder_centre);
    assert!(hip_distance < shoulder_distance);
    assert!(hip_distance <= ATTRACTION.hip_m);
    assert!(shoulder_distance / ATTRACTION.shoulder_m < hip_distance / ATTRACTION.hip_m);
    let attraction = attract(&attractors, Hand::Right, grip, 0.2, &ATTRACTION).unwrap();
    assert_eq!(attraction.socket, Socket::Shoulder(Hand::Right));
}

#[test]
fn a_hand_down_by_the_thigh_is_never_drawn_onto_the_back() {
    // Within the shoulder's wide radius but well below the shoulders, fast enough that the hip
    // takes it as a throw: it falls rather than docking on the back.
    let beside = Vec3::new(0.32, 0.95, 0.05);
    assert!(beside.distance(Vec3::new(0.12, 1.3, 0.15)) <= ATTRACTION.shoulder_m);
    assert_eq!(attracted(beside, ATTRACTION.hip_max_speed_mps + 0.5), None);
}

#[test]
fn the_front_limits_turn_with_the_body() {
    // The same body and sockets turned a quarter about +Y: the front is now -X.
    let yaw = std::f32::consts::FRAC_PI_2;
    let turn = Quat::from_rotation_y(yaw);
    let sockets: Vec<_> = free_sockets()
        .into_iter()
        .map(|(c, usage)| {
            (
                Candidate {
                    centre: turn * c.centre,
                    ..c
                },
                usage,
            )
        })
        .collect();
    let body = body(yaw);
    let attractors = attractors(&sockets, Some(&body), offset, &ATTRACTION);
    let over = turn * Vec3::new(0.18, 1.62, 0.0);
    let front = turn * Vec3::new(0.2, 1.4, -0.25);
    assert_eq!(
        attract(&attractors, Hand::Right, over, 0.5, &ATTRACTION).map(|a| a.socket),
        Some(Socket::Shoulder(Hand::Right))
    );
    assert_eq!(
        attract(&attractors, Hand::Right, front, 0.5, &ATTRACTION),
        None
    );
}

#[test]
fn only_free_fitting_sockets_attract_and_only_their_hand() {
    let body = body(0.0);
    let mut sockets = free_sockets();
    sockets[0].1 = SocketUse::NoFit;
    sockets[1].1 = SocketUse::Draw;
    assert!(attractors(&sockets, Some(&body), offset, &ATTRACTION).is_empty());
    sockets[1].1 = SocketUse::Dock;
    let attractors = attractors(&sockets, Some(&body), offset, &ATTRACTION);
    assert_eq!(attractors.len(), 1);
    assert_eq!(attractors[0].socket, Socket::Hip(Hand::Right));
    let over = Vec3::new(0.18, 1.62, 0.0);
    assert_eq!(
        attract(&attractors, Hand::Right, over, 0.5, &ATTRACTION),
        None
    );
    let all = attractors_for_both();
    assert_eq!(
        attract(
            &all,
            Hand::Left,
            Vec3::new(0.32, 0.95, 0.05),
            0.5,
            &ATTRACTION
        ),
        None
    );
}

fn attractors_for_both() -> Vec<Attractor> {
    attractors(&free_sockets(), Some(&body(0.0)), offset, &ATTRACTION)
}

#[test]
fn without_a_body_no_socket_attracts() {
    assert!(attractors(&free_sockets(), None, offset, &ATTRACTION).is_empty());
}

#[test]
fn an_attraction_never_reaches_less_than_the_sockets_own_reach() {
    let wide = vec![dock(
        Socket::Shoulder(Hand::Right),
        Hand::Right,
        Vec3::ZERO,
        0.5,
    )];
    let attractors = attractors(&wide, Some(&body(0.0)), offset, &ATTRACTION);
    assert_eq!(attractors[0].radius, ATTRACTION.shoulder_m.max(0.5));
}

#[test]
fn the_travel_takes_longer_from_farther_within_its_bounds() {
    let snap = 0.12;
    assert_eq!(travel_s(0.0, snap, &ATTRACTION), snap);
    let near = travel_s(0.25, snap, &ATTRACTION);
    let far = travel_s(0.4, snap, &ATTRACTION);
    assert!(near > snap && far > near);
    assert_eq!(travel_s(10.0, snap, &ATTRACTION), ATTRACTION.max_travel_s);
    assert_eq!(travel_s(f32::NAN, snap, &ATTRACTION), snap);
}

#[test]
fn coming_into_an_attraction_ticks_once_until_the_hand_leaves_it() {
    let attractors = attractors_for_both();
    let mut watch = ZoneWatch::new();
    let step = |watch: &mut ZoneWatch, grip: Option<Vec3>| {
        watch.step(Hand::Right, &attractors, grip, &ATTRACTION)
    };
    let centre = Vec3::new(0.12, 1.3, 0.15);
    let up = Vec3::Y;
    // Out of every attraction, then into the back's: one tick.
    assert!(!step(&mut watch, Some(centre + up * 0.8)));
    assert!(step(&mut watch, Some(centre + up * 0.44)));
    // Staying, and drifting just past the edge within the margin: no repeat.
    assert!(!step(&mut watch, Some(centre + up * 0.3)));
    assert!(!step(&mut watch, Some(centre + up * 0.47)));
    assert!(!step(&mut watch, Some(centre + up * 0.44)));
    // Out past the margin, then back in: it ticks again.
    assert!(!step(&mut watch, Some(centre + up * 0.8)));
    assert!(step(&mut watch, Some(centre + up * 0.44)));
}

#[test]
fn the_tick_keeps_the_front_limit_and_stops_with_nothing_to_dock() {
    let attractors = attractors_for_both();
    let mut watch = ZoneWatch::new();
    let step = |watch: &mut ZoneWatch, grip: Option<Vec3>| {
        watch.step(Hand::Right, &attractors, grip, &ATTRACTION)
    };
    // In front of the chest, within the back's radius: no tick.
    assert!(!step(&mut watch, Some(Vec3::new(0.2, 1.4, -0.25))));
    // Over the shoulder: a tick. Losing the grip (the gun docked, braced, or the attraction off)
    // forgets the zone, so the next time in ticks again.
    let over = Some(Vec3::new(0.18, 1.62, 0.0));
    assert!(step(&mut watch, over));
    assert!(!step(&mut watch, None));
    assert!(step(&mut watch, over));
    // The other hand has no attraction here.
    assert!(!watch.step(Hand::Left, &attractors, over, &ATTRACTION));
}

#[test]
fn moving_from_one_attraction_straight_into_another_ticks_for_the_new_socket() {
    let attractors = attractors_for_both();
    let mut watch = ZoneWatch::new();
    let step = |watch: &mut ZoneWatch, grip: Vec3| {
        watch.step(Hand::Right, &attractors, Some(grip), &ATTRACTION)
    };
    assert!(step(&mut watch, Vec3::new(0.32, 0.95, 0.05)));
    // Up to the shoulder, out of the hip's attraction by more than the margin.
    assert!(step(&mut watch, Vec3::new(0.18, 1.62, 0.0)));
}

#[test]
fn a_taken_back_socket_draws_in_a_swap_and_a_free_one_comes_first() {
    let body = body(0.0);
    let taken = |socket: Socket, centre: Vec3| {
        let (candidate, _) = dock(socket, Hand::Right, centre, 0.17);
        (candidate, SocketUse::NoFit)
    };
    let others = vec![
        taken(Socket::Shoulder(Hand::Left), Vec3::new(-0.12, 1.3, 0.15)),
        taken(Socket::Hip(Hand::Left), Vec3::new(-0.2, 0.85, 0.0)),
    ];
    let swaps = |_: Hand, socket: Socket| matches!(socket, Socket::Shoulder(_));
    let swapping = swap_attractors(&others, swaps, Some(&body), offset, &ATTRACTION);
    assert_eq!(swapping.len(), 1);
    assert!(swapping[0].swap);
    assert_eq!(swapping[0].socket, Socket::Shoulder(Hand::Left));
    // A hand let open over the left shoulder swaps there.
    let over_left = Vec3::new(-0.15, 1.45, 0.2);
    let attraction = attract(&swapping, Hand::Right, over_left, 0.1, &ATTRACTION).unwrap();
    assert!(attraction.swap);
    // With a free socket in reach as well, the free one takes the weapon, however deep the hand is
    // in the taken one's attraction.
    let mut all = attractors(&free_sockets(), Some(&body), offset, &ATTRACTION);
    all.extend(swapping);
    let attraction = attract(&all, Hand::Right, over_left, 0.1, &ATTRACTION).unwrap();
    assert!(!attraction.swap);
    assert_eq!(attraction.socket, Socket::Shoulder(Hand::Right));
}

#[test]
fn the_hand_feels_the_free_socket_a_release_would_take_before_a_swap() {
    // A pistol held by the right hip (riding high), which is free, is also within the right
    // shoulder's wide attraction while that shoulder is taken: the release docks on the hip, so the hand is in the
    // hip's zone, not the shoulder's swap (the grab hint showed a swap onto the back here).
    let body = body(0.0);
    let (shoulder, _) = dock(
        Socket::Shoulder(Hand::Right),
        Hand::Right,
        Vec3::new(0.12, 1.3, 0.15),
        0.17,
    );
    let (hip, hip_use) = dock(
        Socket::Hip(Hand::Right),
        Hand::Right,
        Vec3::new(0.2, 1.0, 0.0),
        0.12,
    );
    let mut all = attractors(&[(hip, hip_use)], Some(&body), offset, &ATTRACTION);
    all.extend(swap_attractors(
        &[(shoulder, SocketUse::NoFit)],
        |_, _| true,
        Some(&body),
        offset,
        &ATTRACTION,
    ));
    let grip = Vec3::new(0.25, 1.17, 0.05);
    let share = |centre: Vec3, radius: f32| grip.distance(centre) / radius;
    assert!(
        share(Vec3::new(0.12, 1.3, 0.15), ATTRACTION.shoulder_m)
            < share(Vec3::new(0.2, 1.0, 0.0), ATTRACTION.hip_m)
    );
    let mut watch = ZoneWatch::new();
    assert!(watch.step(Hand::Right, &all, Some(grip), &ATTRACTION));
    assert_eq!(watch.inside(Hand::Right), Some(Socket::Hip(Hand::Right)));
    // Starting in the shoulder's swap alone, then coming within the free hip's reach: the hand
    // moves to the hip, and ticks for it.
    let mut watch = ZoneWatch::new();
    assert!(watch.step(
        Hand::Right,
        &all,
        Some(Vec3::new(0.25, 1.5, 0.1)),
        &ATTRACTION
    ));
    assert_eq!(
        watch.inside(Hand::Right),
        Some(Socket::Shoulder(Hand::Right))
    );
    assert!(watch.step(Hand::Right, &all, Some(grip), &ATTRACTION));
    assert_eq!(watch.inside(Hand::Right), Some(Socket::Hip(Hand::Right)));
}
