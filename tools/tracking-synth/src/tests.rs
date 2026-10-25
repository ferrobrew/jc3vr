//! The built-in scripts' recordings: that they read back through the file's decoder, and that the
//! poses and inputs they hold are the ones the scripts describe.

use glam::{Quat, Vec2, Vec3};
use pretty_assertions::assert_eq;
use tracking_format::{
    Frame, Recording,
    model::{ActionValue, Button, Hand, RecenterKind, SemanticSet},
};

use crate::{
    rig::PLAYER,
    schema::Schema,
    scripts,
    synth::{self, DEFAULT_RATE_HZ, Options, SynthError},
};

fn recording(name: &str) -> Recording {
    let script = scripts::named(name).expect("a built-in script");
    let schema = Schema::payload().expect("the schema reads");
    synth::recording(&script, Options::default(), &schema).expect("the script samples")
}

/// The frame nearest `t` seconds in.
fn at(recording: &Recording, t: f32) -> &Frame {
    let index = (t * DEFAULT_RATE_HZ).round() as usize;
    &recording.frames[index.min(recording.frames.len() - 1)]
}

/// The centre eye's position.
fn head(frame: &Frame) -> Vec3 {
    let eyes = frame.head.expect("the head is located").eyes;
    (eyes[0].pose.position + eyes[1].pose.position) * 0.5
}

fn gaze(frame: &Frame) -> Vec3 {
    frame.head.expect("the head is located").eyes[0]
        .pose
        .orientation
        * Vec3::NEG_Z
}

fn grip(frame: &Frame, hand: Hand) -> Vec3 {
    frame
        .controllers
        .as_ref()
        .expect("the controllers are sampled")
        .hands[hand.index()]
    .grip
    .pose
    .position
}

/// The on-foot semantic action `name` on `hand`.
fn on_foot(frame: &Frame, name: &str, hand: Hand) -> ActionValue {
    let schema = Schema::payload().expect("the schema reads");
    let (index, _) = schema.find(SemanticSet::OnFoot, name).expect("an action");
    let semantic = frame
        .semantic
        .as_ref()
        .expect("the semantic actions are sampled");
    assert_eq!(semantic.mode.set, SemanticSet::OnFoot);
    semantic.mode.actions[index][hand.index()].value
}

/// Within the sway, the bob, and the tremor of an exact pose.
const LOOSE_M: f32 = 0.03;

#[test]
fn every_built_in_recording_reads_back_as_written() {
    for script in scripts::all() {
        let schema = Schema::payload().expect("the schema reads");
        let recording =
            synth::recording(&script, Options::default(), &schema).expect("the script samples");
        let bytes = synth::encode(&recording);
        assert_eq!(recording.version, 4);
        assert_eq!(&bytes[8..10], &4u16.to_le_bytes());
        let first_record = tracking_format::encode_latest_preamble(&recording.header).len();
        assert_eq!(bytes[first_record], tracking_format::TAG_FRAME);
        let decoded = tracking_format::decode(&bytes).expect("the file reads");
        assert_eq!(decoded, recording, "{}", script.name);
        assert_eq!(synth::encode(&decoded), bytes, "{}", script.name);
    }
}

#[test]
fn a_recording_is_sampled_at_its_rate_from_a_recenter() {
    let recording = recording("stand_still");
    let script = scripts::named("stand_still").expect("a built-in script");
    let expected = (script.duration_s() * DEFAULT_RATE_HZ).ceil() as usize + 1;
    assert_eq!(recording.frames.len(), expected);
    for pair in recording.frames.windows(2) {
        let step = pair[1].time_ns - pair[0].time_ns;
        assert!((11_111_110..=11_111_112).contains(&step), "{step}");
    }
    assert_eq!(recording.frames[0].recenters.len(), 1);
    assert_eq!(recording.frames[0].recenters[0].kind, RecenterKind::Full);
    assert!(recording.frames[1..].iter().all(|f| f.recenters.is_empty()));
    let header = &recording.header;
    assert_eq!(header.dominant_hand, Hand::Right);
    assert_eq!(header.start.position(), Vec3::ZERO);
    assert_eq!(
        header.action_schema,
        Schema::payload().expect("the schema reads").version
    );
}

#[test]
fn a_rate_outside_a_headset_s_is_refused() {
    let script = scripts::named("stand_still").expect("a built-in script");
    let schema = Schema::payload().expect("the schema reads");
    let error =
        synth::recording(&script, Options { rate_hz: 0.0 }, &schema).expect_err("no frame rate");
    assert_eq!(error, SynthError::Rate(0.0));
}

#[test]
fn standing_still_sways_within_a_few_centimetres_with_the_hands_at_the_sides() {
    let recording = recording("stand_still");
    let mut moved = 0.0f32;
    for frame in &recording.frames {
        let offset = head(frame).length();
        assert!(offset < LOOSE_M, "{offset}");
        moved = moved.max(offset);
        let height = frame.stage.head_height.expect("the floor is known");
        assert!((height - PLAYER.eye_height).abs() < LOOSE_M);
        assert!(grip(frame, Hand::Left).x < -0.2 && grip(frame, Hand::Right).x > 0.2);
    }
    assert!(moved > 0.002, "the head does not sway: {moved}");
}

#[test]
fn walking_forward_reaches_a_metre_and_a_half_and_comes_back() {
    let recording = recording("walk_room_forward");
    let far = at(&recording, 4.2);
    assert!(
        head(far).distance(Vec3::new(0.0, 0.0, -1.5)) < LOOSE_M,
        "{}",
        head(far)
    );
    // The hands came along with the body.
    assert!(grip(far, Hand::Left).z < -1.3 && grip(far, Hand::Right).z < -1.3);
    let end = recording.frames.last().expect("frames");
    assert!(head(end).length() < LOOSE_M, "{}", head(end));
    // The body walked: the head bobbed while it moved.
    let heights: Vec<f32> = (0..60)
        .map(|i| head(at(&recording, 1.6 + i as f32 / 60.0)).y)
        .collect();
    let range = heights.iter().copied().fold(f32::MIN, f32::max)
        - heights.iter().copied().fold(f32::MAX, f32::min);
    assert!(range > 0.01, "{range}");
}

#[test]
fn the_square_turns_at_each_corner_and_closes() {
    let recording = recording("walk_room_square");
    // After the first side and its turn, the body faces +X at the first corner.
    let corner = at(&recording, 1.0 + 2.2 + 0.3 + 1.2);
    assert!(gaze(corner).x > 0.95, "{}", gaze(corner));
    let axis = Vec3::new(0.0, 0.0, -1.5 + PLAYER.eye_ahead);
    assert!(head(corner).distance(axis + Vec3::X * PLAYER.eye_ahead) < LOOSE_M);
    let end = recording.frames.last().expect("frames");
    assert!(head(end).length() < LOOSE_M, "{}", head(end));
    assert!(gaze(end).z < -0.99, "{}", gaze(end));
}

#[test]
fn the_lean_puts_the_head_over_the_table() {
    let recording = recording("lean_forward");
    let leaning = head(at(&recording, 4.5));
    assert!(
        leaning.distance(Vec3::new(0.0, -0.20, -0.35)) < LOOSE_M,
        "{leaning}"
    );
    // The hanging hands come forward and down with the shoulders.
    let standing = grip(at(&recording, 0.0), Hand::Right);
    let leant = grip(at(&recording, 4.5), Hand::Right);
    assert!(
        leant.z < standing.z - 0.2 && leant.y < standing.y,
        "{leant}"
    );
}

#[test]
fn the_squat_lowers_the_head_by_about_half_a_metre() {
    let recording = recording("crouch");
    let low = at(&recording, 3.9);
    let drop = PLAYER.eye_height - low.stage.head_height.expect("the floor is known");
    assert!((0.42..0.55).contains(&drop), "{drop}");
    assert!(head(recording.frames.last().expect("frames")).length() < LOOSE_M);
}

#[test]
fn the_turn_faces_left_and_back() {
    let recording = recording("turn_in_place");
    assert!(gaze(at(&recording, 3.6)).x < -0.98);
    assert!(gaze(recording.frames.last().expect("frames")).z < -0.99);
}

#[test]
fn looking_around_turns_the_head_and_leaves_the_hands() {
    let recording = recording("look_around");
    let left = gaze(at(&recording, 3.0));
    let expected = Quat::from_rotation_y(70f32.to_radians()) * Vec3::NEG_Z;
    assert!(left.dot(expected) > 0.999, "{left}");
    let down = gaze(at(&recording, 11.25));
    assert!(down.y < -0.8, "{down}");
    let still = grip(at(&recording, 0.0), Hand::Left);
    assert!(grip(at(&recording, 3.0), Hand::Left).distance(still) < 0.02);
}

#[test]
fn the_left_stick_walks_at_full_then_half_and_drives_the_move_action() {
    let recording = recording("stick_walk");
    for (t, forward) in [(1.0, 0.0), (3.0, 1.0), (6.0, 0.5), (9.0, 0.0)] {
        let frame = at(&recording, t);
        let stick = frame.controllers.as_ref().expect("controllers").hands[0]
            .inputs
            .thumbstick;
        assert!(
            stick.distance(Vec2::new(0.0, forward)) < 1e-4,
            "{t}: {stick}"
        );
        assert_eq!(
            on_foot(frame, "move", Hand::Left),
            ActionValue::Vec2(stick),
            "{t}"
        );
        assert_eq!(
            on_foot(frame, "move", Hand::Right),
            ActionValue::Vec2(Vec2::ZERO)
        );
    }
    // The body stays put while the stick walks.
    for frame in &recording.frames {
        assert!(head(frame).length() < LOOSE_M);
    }
}

#[test]
fn the_right_stick_flicks_and_holds_the_turn_action() {
    let recording = recording("stick_strafe_and_turn");
    assert_eq!(
        on_foot(at(&recording, 6.6), "turn", Hand::Right),
        ActionValue::Float(1.0)
    );
    assert_eq!(
        on_foot(at(&recording, 7.2), "turn", Hand::Right),
        ActionValue::Float(0.0)
    );
    let held = on_foot(at(&recording, 10.0), "turn", Hand::Right);
    assert_eq!(held, ActionValue::Float(-0.7));
    let strafe = at(&recording, 2.0)
        .controllers
        .as_ref()
        .expect("controllers")
        .hands[0]
        .inputs
        .thumbstick;
    assert!(strafe.distance(Vec2::X) < 1e-4, "{strafe}");
    let frame = at(&recording, 6.6);
    let right = &frame.controllers.as_ref().expect("controllers").hands[1];
    assert!(right.buttons.contains(Button::ThumbstickTouch));
}

#[test]
fn the_velocities_are_the_poses_rates() {
    let recording = recording("walk_room_square");
    let dt = 1.0 / DEFAULT_RATE_HZ;
    for index in (1..recording.frames.len() - 1).step_by(37) {
        let hand = |i: usize| {
            recording.frames[i]
                .controllers
                .as_ref()
                .expect("controllers")
                .hands[1]
                .grip
        };
        let (before, now, after) = (hand(index - 1), hand(index), hand(index + 1));
        let rate = (after.pose.position - before.pose.position) / (2.0 * dt);
        let velocity = now.linear_velocity.expect("a velocity");
        assert!(velocity.distance(rate) < 0.05, "{index}: {velocity} {rate}");
        let turn = (after.pose.orientation * before.pose.orientation.inverse()).to_scaled_axis();
        let spin = now.angular_velocity.expect("an angular velocity");
        assert!(spin.distance(turn / (2.0 * dt)) < 0.1, "{index}: {spin}");
    }
}
