//! Test-only native-rest model and candidate prototype. Production uses the shoulder-based
//! placement; these fixtures do not establish callback scheduling or authorize native restoration.

use glam::Quat;

use super::*;
use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    vehicles::{
        config::ParachuteHandlesConfig,
        parachute::{HandleEvent, HandleHold, HandleState, StepInputs},
        physical::Release,
    },
};

mod parachute {
    use super::*;
    pub(super) use crate::controllers::vehicles::parachute::{anchor_point, step};

    pub(super) fn candidates(
        config: &ParachuteHandlesConfig,
        state: &HandleState,
        centres: [Option<Vec3>; 2],
        grips: [Option<Vec3>; 2],
    ) -> [Option<Candidate>; 2] {
        Hand::BOTH.map(|hand| {
            let i = hand.index();
            let centre = match state.holds[i] {
                Some(hold) => anchor_point(config, hold, grips[i].filter(|g| g.is_finite())?),
                None => centres[i]?,
            };
            centre.is_finite().then_some(Candidate {
                volume: GrabVolume::ParachuteHandle(hand),
                centre,
                radius: config.radius_m,
                hand: Some(hand),
            })
        })
    }

    pub(super) fn reach_candidates(
        config: &ParachuteHandlesConfig,
        state: &HandleState,
        centres: [Option<Vec3>; 2],
    ) -> impl Iterator<Item = Candidate> {
        let radius = config.reach_m.max(config.radius_m);
        Hand::BOTH
            .into_iter()
            .filter(move |hand| state.holds[hand.index()].is_none())
            .filter_map(move |hand| {
                Some(Candidate {
                    volume: GrabVolume::ParachuteHandle(hand),
                    centre: centres[hand.index()].filter(|p| p.is_finite())?,
                    radius,
                    hand: Some(hand),
                })
            })
    }
}

const CONFIG: ParachuteHandlesConfig = ParachuteHandlesConfig::new();
const OWNER: OwnerIdentity = OwnerIdentity {
    character: 10,
    control_block: 20,
};

fn cockpit() -> CockpitTransform {
    CockpitTransform {
        head_world: Vec3::new(8.0, -2.0, 3.0),
        head_cockpit: Vec3::new(0.2, 1.6, -0.4),
        body: Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.3),
        world_scale: 1.7,
    }
}

fn sample(session: Session, now: Instant) -> NativeRestSnapshot {
    NativeRestSnapshot {
        session,
        sequence: 1,
        sampled_at: now,
        rig: RigIdentity {
            object: 30,
            control_block: 31,
            model: 40,
            controller: 50,
        },
        character_world: Mat4::from_rotation_translation(
            Quat::from_rotation_y(-0.8),
            Vec3::new(9.0, -3.0, 4.0),
        ),
        rig_world: Some(Mat4::from_rotation_translation(
            Quat::from_rotation_z(0.6) * Quat::from_rotation_x(0.4),
            Vec3::new(12.0, 5.0, -7.0),
        )),
        rest_model: [
            Some(Vec3::new(-0.5, 1.2, 0.3)),
            Some(Vec3::new(0.5, 1.1, 0.4)),
        ],
    }
}

fn state_with_sample(now: Instant) -> State {
    let mut state = State::new();
    state.configure(Some(OWNER), now);
    state.record(sample(state.active.unwrap().0, now));
    state
}

#[test]
fn sampled_rig_space_passes_through_world_into_the_current_cockpit() {
    let now = Instant::now();
    let mut state = state_with_sample(now);
    let mut snapshot = state.snapshot.unwrap();
    let rig_rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let rig_translation = Vec3::new(10.0, 20.0, 30.0);
    snapshot.sequence += 1;
    snapshot.rig_world = Some(Mat4::from_rotation_translation(
        rig_rotation,
        rig_translation,
    ));
    snapshot.character_world = Mat4::from_translation(Vec3::new(-3.0, 7.0, 5.0));
    snapshot.rest_model = [
        Some(Vec3::new(-1.0, 2.0, 3.0)),
        Some(Vec3::new(2.0, -3.0, 4.0)),
    ];
    state.record(snapshot);
    let cockpit = CockpitTransform {
        head_world: Vec3::new(4.0, 6.0, 8.0),
        head_cockpit: Vec3::new(1.0, 2.0, 3.0),
        body: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        world_scale: 2.0,
    };
    // The rig maps (x, y, z) to (-y, x, z); the cockpit inverse maps it to (x, z, -y).
    // Including translation and scale gives (4 - y/2, 13 + z/2, -4 - x/2).
    let expected = [Vec3::new(3.0, 14.5, -3.5), Vec3::new(5.5, 15.0, -5.0)];
    let points = state.centres(&cockpit, now);
    for ((point, native), expected) in points.into_iter().zip(snapshot.rest_model).zip(expected) {
        let native = native.unwrap();
        assert!(point.unwrap().distance(expected) < 1.0e-5);
        let wrong_root = cockpit.head_cockpit
            + cockpit.body.inverse()
                * (snapshot.character_world.transform_point3(native) - cockpit.head_world)
                / cockpit.world_scale;
        let wrong_order = cockpit.head_cockpit
            + (rig_rotation * (cockpit.body.inverse() * native)
                + cockpit.body.inverse() * (rig_translation - cockpit.head_world))
                / cockpit.world_scale;
        assert!(expected.distance(wrong_root) > 1.0);
        assert!(expected.distance(wrong_order) > 2.0);
    }
    let moved = CockpitTransform {
        head_world: Vec3::new(7.0, 11.0, 13.0),
        head_cockpit: Vec3::new(-2.0, 1.0, 4.0),
        body: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        world_scale: 0.5,
    };
    // The new cockpit inverse maps (x, y, z) to (-z, y, x), without resampling the rig.
    let expected = [Vec3::new(-42.0, 17.0, 6.0), Vec3::new(-44.0, 23.0, 16.0)];
    let current = state.centres(&moved, now);
    for (point, expected) in current.into_iter().zip(expected) {
        assert!(point.unwrap().distance(expected) < 1.0e-5);
    }
    assert!(current[0].unwrap().distance(points[0].unwrap()) > 1.0);
}

#[test]
fn missing_stale_future_and_unverified_samples_offer_no_native_point() {
    let now = Instant::now();
    let cockpit = cockpit();
    let mut state = State::new();
    assert_eq!(
        state.centres(&cockpit, now),
        [Err(RestUnavailable::Inactive); 2]
    );
    state.configure(Some(OWNER), now);
    assert_eq!(
        state.centres(&cockpit, now),
        [Err(RestUnavailable::MissingSample); 2]
    );
    let mut snapshot = sample(state.active.unwrap().0, now);
    state.record(snapshot);
    assert_eq!(
        state.centres(&cockpit, now + STALE_AFTER * 2),
        [Err(RestUnavailable::Stale); 2]
    );
    assert_eq!(
        state.centres(&cockpit, now - Duration::from_millis(1)),
        [Err(RestUnavailable::FutureSample); 2]
    );
    snapshot.rig_world = None;
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit, now),
        [Err(RestUnavailable::TransformUnavailable); 2]
    );
    snapshot.rig_world = Some(Mat4::ZERO);
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit, now),
        [Err(RestUnavailable::InvalidFrames); 2]
    );
    snapshot.rig_world = Some(Mat4::IDENTITY);
    snapshot.character_world = Mat4::ZERO;
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit, now),
        [Err(RestUnavailable::InvalidCharacterRoot); 2]
    );
}

#[test]
fn a_missing_or_invalid_side_does_not_hide_the_other_native_rest() {
    let now = Instant::now();
    let state = state_with_sample(now);
    let mut snapshot = state.snapshot.unwrap();
    snapshot.rest_model[0] = None;
    let points = snapshot.centres(snapshot.session, &cockpit(), now);
    assert_eq!(points[0], Err(RestUnavailable::MissingJoint));
    assert!(points[1].is_ok());
    snapshot.rest_model[0] = Some(Vec3::NAN);
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit(), now)[0],
        Err(RestUnavailable::InvalidPoint)
    );
}

#[test]
fn sampling_starts_without_a_held_marker_and_continues_with_both_handles_held() {
    let now = Instant::now();
    let mut state = State::new();
    state.configure(Some(OWNER), now);
    let (session, restore) = state.request(Some(OWNER), now).unwrap();
    assert_eq!(restore, [false; 2]);
    state.record(sample(session, now));
    assert!(state.centres(&cockpit(), now).iter().all(Result::is_ok));
    state.holds = Some(([true; 2], now));
    let (session, restore) = state.request(Some(OWNER), now).unwrap();
    assert_eq!(restore, [false; 2]);
    let mut next = sample(session, now);
    next.sequence = 2;
    next.rest_model = [Some(Vec3::X), Some(-Vec3::X)];
    state.record(next);
    assert_eq!(state.snapshot.unwrap().rest_model, next.rest_model);
    state.holds = Some(([false; 2], now));
    assert_eq!(state.request(Some(OWNER), now).unwrap().1, [true; 2]);
}

#[test]
fn mode_owner_reuse_and_input_gaps_require_a_new_sample() {
    let now = Instant::now();
    let cockpit = cockpit();
    let mut state = state_with_sample(now);
    let old = state.snapshot.unwrap();
    state.configure(None, now);
    state.configure(Some(OWNER), now);
    state.record(old);
    assert_eq!(
        state.centres(&cockpit, now),
        [Err(RestUnavailable::MissingSample); 2]
    );
    assert_eq!(
        old.centres(state.active.unwrap().0, &cockpit, now),
        [Err(RestUnavailable::SessionChanged); 2]
    );
    let reused = OwnerIdentity {
        control_block: 21,
        ..OWNER
    };
    state.configure(Some(reused), now);
    state.record(old);
    assert!(state.request(Some(OWNER), now).is_none());
    assert_eq!(
        old.centres(state.active.unwrap().0, &cockpit, now),
        [Err(RestUnavailable::OwnerChanged); 2]
    );
    assert_eq!(
        state.centres(&cockpit, now),
        [Err(RestUnavailable::MissingSample); 2]
    );
    let session = state.active.unwrap().0;
    state.record(sample(session, now));
    let later = now + STALE_AFTER * 2;
    assert!(state.request(Some(reused), later).is_none());
    state.configure(Some(reused), later);
    assert_ne!(state.active.unwrap().0, session);
    assert_eq!(
        state.centres(&cockpit, later),
        [Err(RestUnavailable::MissingSample); 2]
    );
}

#[test]
fn an_old_sync_cannot_replace_a_newer_sample_or_retain_a_missing_side() {
    let now = Instant::now();
    let mut state = state_with_sample(now);
    let old = state.snapshot.unwrap();
    let mut next = old;
    next.sequence = 3;
    next.rig.model += 1;
    next.rest_model[1] = None;
    state.holds = Some(([true; 2], now));
    assert!(state.record(next));
    assert!(!state.record(old));
    assert!(state.snapshot.is_none());
    assert!(state.holds.is_none());
    let replacement = state.active.unwrap().0;
    assert_ne!(replacement.generation, old.session.generation);
    next.session = replacement;
    next.sequence = 4;
    assert!(state.record(next));
    assert_eq!(state.snapshot.unwrap().sequence, 4);
    assert_eq!(
        state.centres(&cockpit(), now)[1],
        Err(RestUnavailable::MissingJoint)
    );
}

#[test]
fn a_reused_rig_address_with_a_new_control_block_retires_the_session() {
    let now = Instant::now();
    let mut state = state_with_sample(now);
    let old = state.snapshot.unwrap();
    let mut reused = old;
    reused.sequence += 1;
    reused.rig.control_block += 1;
    assert!(state.record(reused));
    assert!(state.snapshot.is_none());
    assert_ne!(state.active.unwrap().0.generation, old.session.generation);
    assert!(!state.record(old));
}

#[test]
fn a_projective_or_overflowed_matrix_cannot_place_a_resting_handle() {
    let now = Instant::now();
    let state = state_with_sample(now);
    let mut snapshot = state.snapshot.unwrap();
    let mut projective = Mat4::IDENTITY;
    projective.x_axis.w = 0.1;
    snapshot.rig_world = Some(projective);
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit(), now),
        [Err(RestUnavailable::InvalidFrames); 2]
    );
    snapshot.rig_world = Some(Mat4::from_scale(Vec3::splat(f32::MAX)));
    assert_eq!(
        snapshot.centres(snapshot.session, &cockpit(), now),
        [Err(RestUnavailable::InvalidFrames); 2]
    );
}

#[test]
fn free_held_and_released_handles_use_native_rest_grip_and_native_rest() {
    let now = Instant::now();
    let native = state_with_sample(now)
        .centres(&cockpit(), now)
        .map(Result::ok);
    let mut state = HandleState::new();
    let free = parachute::candidates(&CONFIG, &state, native, [None; 2]);
    assert_eq!(free.map(|c| c.unwrap().centre), native.map(Option::unwrap));
    assert_eq!(
        parachute::reach_candidates(&CONFIG, &state, native).count(),
        2
    );
    let rests = [Vec3::new(-0.2, 0.8, -0.4), Vec3::new(0.3, 0.9, -0.5)];
    state.holds = rests.map(|rest| Some(HandleHold { rest }));
    let grips = rests.map(|rest| Some(rest - Vec3::Y * 0.1));
    let held = parachute::candidates(&CONFIG, &state, [None; 2], grips);
    assert_eq!(held.map(|c| c.unwrap().centre), grips.map(Option::unwrap));
    assert_eq!(
        parachute::reach_candidates(&CONFIG, &state, native).count(),
        0
    );
    let mut events = Vec::new();
    parachute::step(
        &mut state,
        &CONFIG,
        &StepInputs {
            grips,
            held: [false; 2],
            grabs: [false; 2],
        },
        &mut events,
    );
    assert_eq!(
        events,
        vec![
            HandleEvent::Released(Hand::Left, Release::GripReleased),
            HandleEvent::Released(Hand::Right, Release::GripReleased),
        ]
    );
    let released = parachute::candidates(&CONFIG, &state, native, grips);
    assert_eq!(
        released.map(|c| c.unwrap().centre),
        native.map(Option::unwrap)
    );
    assert_eq!(
        parachute::reach_candidates(&CONFIG, &state, native).count(),
        2
    );
}

#[test]
fn unavailable_free_rest_never_substitutes_a_shoulder_or_a_held_rest() {
    let mut state = HandleState::new();
    assert_eq!(
        parachute::candidates(&CONFIG, &state, [None; 2], [Some(Vec3::ZERO); 2]),
        [None; 2]
    );
    assert_eq!(
        parachute::reach_candidates(&CONFIG, &state, [None; 2]).count(),
        0
    );
    state.holds[0] = Some(HandleHold { rest: Vec3::Y });
    let native = [None, Some(Vec3::X)];
    let mixed = parachute::candidates(&CONFIG, &state, native, [Some(Vec3::Y), None]);
    assert_eq!(mixed[0].unwrap().centre, Vec3::Y);
    assert_eq!(mixed[1].unwrap().centre, Vec3::X);
    let reach: Vec<_> = parachute::reach_candidates(&CONFIG, &state, native).collect();
    assert_eq!(reach.len(), 1);
    assert_eq!(reach[0].hand, Some(Hand::Right));
    assert!(parachute::candidates(&CONFIG, &state, [Some(Vec3::ZERO); 2], [None; 2])[0].is_none());
}

#[test]
fn only_fresh_unheld_handles_are_restored_to_their_links() {
    let now = Instant::now();
    assert_eq!(unplaced(None, now), [false; 2]);
    assert_eq!(unplaced(Some(([false, true], now)), now), [true, false]);
    assert_eq!(unplaced(Some(([true; 2], now)), now), [false; 2]);
    assert_eq!(
        unplaced(Some(([false; 2], now)), now + STALE_AFTER * 2),
        [false; 2]
    );
    assert_eq!(
        unplaced(Some(([false; 2], now)), now - Duration::from_millis(1)),
        [false; 2]
    );
}
