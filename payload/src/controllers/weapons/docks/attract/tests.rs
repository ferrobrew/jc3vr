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

/// The right shoulder's socket behind the shoulder blade, the right hip's beside the thigh, and the
/// chest's in front of the sternum, each free for the right hand.
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
        dock(Socket::Chest, Hand::Right, Vec3::new(0.0, 1.3, -0.12), 0.09),
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
    let belly = Vec3::new(0.12, 0.9, -0.18);
    assert!(belly.distance(Vec3::new(0.2, 0.85, 0.0)) <= ATTRACTION.hip_m);
    assert_eq!(attracted(belly, 0.2), None);
    // Out at arm's length, nowhere near a socket.
    assert_eq!(attracted(Vec3::new(0.3, 1.2, -0.6), 0.2), None);
}

#[test]
fn a_throw_is_never_attracted() {
    let over = Vec3::new(0.18, 1.62, 0.0);
    assert_eq!(attracted(over, ATTRACTION.max_speed_mps + 0.5), None);
    assert_eq!(attracted(over, f32::NAN), None);
    assert_eq!(attracted(Vec3::NAN, 0.0), None);
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
    // Between the chest socket and the back's: nearer the chest socket as a share of its radius.
    assert_eq!(
        attracted(Vec3::new(0.03, 1.31, -0.08), 0.2),
        Some(Socket::Chest)
    );
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
    let attractors = attractors(&sockets, Some(&body), offset, &ATTRACTION);
    assert_eq!(attractors.len(), 1);
    assert_eq!(attractors[0].socket, Socket::Chest);
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
fn without_a_body_only_the_chest_attracts() {
    let attractors = attractors(&free_sockets(), None, offset, &ATTRACTION);
    assert_eq!(
        attractors.iter().map(|a| a.socket).collect::<Vec<_>>(),
        vec![Socket::Chest]
    );
}

#[test]
fn an_attraction_never_reaches_less_than_the_sockets_own_reach() {
    let wide = vec![dock(Socket::Chest, Hand::Right, Vec3::ZERO, 0.5)];
    let attractors = attractors(&wide, None, offset, &ATTRACTION);
    assert_eq!(attractors[0].radius, 0.5);
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
    assert!(!step(&mut watch, Some(centre + up * 0.6)));
    assert!(step(&mut watch, Some(centre + up * 0.44)));
    // Staying, and drifting just past the edge within the margin: no repeat.
    assert!(!step(&mut watch, Some(centre + up * 0.3)));
    assert!(!step(&mut watch, Some(centre + up * 0.47)));
    assert!(!step(&mut watch, Some(centre + up * 0.44)));
    // Out past the margin, then back in: it ticks again.
    assert!(!step(&mut watch, Some(centre + up * 0.5)));
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
