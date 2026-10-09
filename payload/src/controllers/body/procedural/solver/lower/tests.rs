use super::*;
use crate::controllers::body::procedural::{
    rig::tests::skeleton,
    solver::{SolveInput, pose::RigPose, solve},
};

const DT: f32 = 1.0 / 30.0;

/// The test rig, its measures, a standing input, and the default tuning.
fn setup() -> (LowerRig, LowerInput, LowerTuning) {
    let (_, rig, input, tuning) = setup_with_rig();
    (rig, input, tuning)
}

fn setup_with_rig() -> (Rig, LowerRig, LowerInput, LowerTuning) {
    let rig = Rig::from_skeleton(&skeleton()).unwrap();
    let config = ProceduralBodyConfig::new();
    let stance = [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| FootPose {
        position: rig.reference(bone).translation,
        rotation: rig.reference(bone).rotation,
    });
    let input = LowerInput {
        world: Mat4::from_translation(Vec3::new(3.0, 1.0, -2.0)),
        head: rig.reference(RigBone::Head).translation,
        hips_head_y: rig.reference(RigBone::Head).translation.y,
        head_world: Vec3::ZERO,
        crouch: 0.0,
        hips_xz: Vec2::ZERO,
        hips_yaw: 0.0,
        feet: stance,
        stance,
        grounded: true,
        moving: false,
        stick: false,
        standing_play: true,
        off_side: Side::Left,
        dt: DT,
    };
    let lower = LowerRig::measure(&rig, &config);
    (rig, lower, input, LowerTuning::of(&config))
}

fn hold(body: &mut LowerBody, rig: &LowerRig, input: &LowerInput, tuning: &LowerTuning, s: f32) {
    for _ in 0..(s / DT).round() as usize {
        body.update(rig, input, tuning);
    }
}

/// Put the head where it asks for the hips at `hips`, with the crouch's depth that follows.
fn lower_to(rig: &LowerRig, input: &mut LowerInput, hips: f32) {
    input.head.y = hips + rig.head_over_hips;
    input.hips_head_y = input.head.y;
    let floored = hips.clamp(rig.lowest_hips, rig.standing_hips);
    input.crouch = (rig.standing_hips - floored) / (rig.standing_hips - rig.lowest_hips);
}

#[test]
fn the_head_picks_standing_and_crouching() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    assert_eq!(body.update(&rig, &input, &tuning), None);
    assert_eq!(body.status().pose, LowerPose::Stand);
    lower_to(&rig, &mut input, 0.8);
    assert_eq!(
        body.update(&rig, &input, &tuning),
        Some(PoseChange {
            from: LowerPose::Stand,
            to: LowerPose::Crouch,
            sit_end: None,
        })
    );
    // Within the hysteresis, a crouch stays a crouch.
    lower_to(&rig, &mut input, rig.standing_hips - 0.06);
    assert_eq!(body.update(&rig, &input, &tuning), None);
    assert_eq!(body.status().pose, LowerPose::Crouch);
    lower_to(&rig, &mut input, rig.standing_hips);
    body.update(&rig, &input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Stand);
}

#[test]
fn a_head_bobbing_short_of_the_crouch_stays_standing() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    // A standing player's head bobbing a few centimetres, past what the deadband takes, reads as
    // a shallow crouch depth that stays standing.
    for hips in [0.07, 0.03, 0.08, 0.02, 0.07] {
        lower_to(&rig, &mut input, rig.standing_hips - hips);
        assert_eq!(body.update(&rig, &input, &tuning), None);
    }
    assert_eq!(body.status().pose, LowerPose::Stand);
}

#[test]
fn a_head_below_the_squats_floor_kneels_after_a_moment_on_the_off_hands_side() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    body.update(&rig, &input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Crouch);
    hold(
        &mut body,
        &rig,
        &input,
        &tuning,
        kneel::KNEEL_AFTER_S + 0.05,
    );
    let status = body.status();
    assert_eq!(status.pose, LowerPose::Kneel);
    assert_eq!(status.kneel_side, Some(Side::Left));
    input.off_side = Side::Right;
    let mut other = LowerBody::new();
    hold(
        &mut other,
        &rig,
        &input,
        &tuning,
        kneel::KNEEL_AFTER_S + 0.1,
    );
    assert_eq!(other.status().kneel_side, Some(Side::Right));
}

#[test]
fn the_trailing_foot_kneels() {
    let (rig, mut input, tuning) = setup();
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    // The right foot behind (+Z is back).
    input.feet[1].position.z += 0.2;
    let mut body = LowerBody::new();
    hold(&mut body, &rig, &input, &tuning, 0.5);
    assert_eq!(body.status().kneel_side, Some(Side::Right));
    // Turned half round, the same feet put the left one behind.
    input.hips_yaw = std::f32::consts::PI;
    let mut turned = LowerBody::new();
    hold(&mut turned, &rig, &input, &tuning, 0.5);
    assert_eq!(turned.status().kneel_side, Some(Side::Left));
}

#[test]
fn moving_or_rising_ends_the_kneel_and_moving_never_starts_one() {
    let (rig, mut input, tuning) = setup();
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    let mut body = LowerBody::new();
    hold(&mut body, &rig, &input, &tuning, 0.5);
    assert_eq!(body.status().pose, LowerPose::Kneel);
    input.moving = true;
    body.update(&rig, &input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Crouch);
    hold(&mut body, &rig, &input, &tuning, 2.0);
    assert_eq!(body.status().pose, LowerPose::Crouch);
    input.moving = false;
    hold(&mut body, &rig, &input, &tuning, 0.5);
    assert_eq!(body.status().pose, LowerPose::Kneel);
    // Just above the floor holds the kneel; further up ends it.
    lower_to(&rig, &mut input, rig.lowest_hips + 0.01);
    body.update(&rig, &input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Kneel);
    lower_to(&rig, &mut input, rig.lowest_hips + 0.1);
    body.update(&rig, &input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Crouch);
    // Off the ground, and switched off, it does not start.
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    input.grounded = false;
    hold(&mut body, &rig, &input, &tuning, 1.0);
    assert_ne!(body.status().pose, LowerPose::Kneel);
    input.grounded = true;
    let off = LowerTuning {
        kneel: false,
        ..tuning
    };
    hold(&mut body, &rig, &input, &off, 1.0);
    assert_ne!(body.status().pose, LowerPose::Kneel);
}

#[test]
fn the_kneel_eases_in_and_out() {
    let (rig, mut input, tuning) = setup();
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    let mut body = LowerBody::new();
    let mut last = 0.0;
    let mut weights = vec![];
    for _ in 0..60 {
        body.update(&rig, &input, &tuning);
        let w = body.status().kneel_weight;
        assert!(w >= last - 1e-6, "the weight fell on the way in");
        last = w;
        weights.push(w);
    }
    assert_eq!(last, 1.0);
    // Eased: the first steps of the blend are small.
    let first = weights.iter().copied().find(|w| *w > 0.0).unwrap();
    assert!(first < 0.05, "{first}");
    assert!(body.targets(&rig, &input).is_some());
    lower_to(&rig, &mut input, rig.standing_hips);
    for _ in 0..60 {
        body.update(&rig, &input, &tuning);
        let w = body.status().kneel_weight;
        assert!(w <= last + 1e-6, "the weight rose on the way out");
        last = w;
    }
    assert_eq!(last, 0.0);
    assert!(body.targets(&rig, &input).is_none());
}

#[test]
fn the_kneeling_knee_is_on_the_ground_and_the_hips_stay_over_it_as_the_root_moves() {
    let (rig, mut input, tuning) = setup();
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    let mut body = LowerBody::new();
    hold(&mut body, &rig, &input, &tuning, 1.0);
    let targets = body.targets(&rig, &input).unwrap();
    assert_eq!(targets.weight, 1.0);
    // The hips are at the height the head asks for, between the kneel's limits.
    assert!((targets.hips.y - (rig.lowest_hips - 0.1)).abs() < 1e-4);
    // The kneeling (left) foot is behind the hips with its toes down, the other ahead.
    let [left, right] = targets.feet;
    assert!(left.position.z > targets.hips.z && right.position.z < targets.hips.z);
    assert!((left.rotation * Vec3::NEG_Z).y < -0.5);
    assert!((right.position.y - input.stance[1].position.y).abs() < 1e-5);
    // The kneeling leg's lengths put the knee on the ground: hip joint, knee, ankle.
    let hip = targets.hips - Vec3::X * rig.hip_half_width - Vec3::Y * rig.hip_joint_drop;
    let span = hip.distance(left.position);
    assert!(span <= rig.thigh + rig.shin + 1e-4);
    // The root moves a little under the held hips: the targets stay put in the world.
    let world = input.world;
    input.world = world * Mat4::from_translation(Vec3::new(0.05, 0.0, 0.03));
    let moved = body.targets(&rig, &input).unwrap();
    let in_world = |w: &Mat4, p: Vec3| w.transform_point3(p);
    assert!(in_world(&input.world, moved.hips).abs_diff_eq(in_world(&world, targets.hips), 1e-4));
    // A head lower still sits back no further than the kneel's lowest.
    lower_to(&rig, &mut input, 0.0);
    body.update(&rig, &input, &tuning);
    let low = body.targets(&rig, &input).unwrap();
    assert!((low.hips.y - rig.kneel_low()).abs() < 1e-4);
}

/// The solve's input for a standing reference pose with the feet at the stance.
fn standing_solve_input(full: &Rig, input: &LowerInput) -> SolveInput {
    let animated = RigPose {
        bones: full.reference,
    };
    SolveInput {
        animated,
        head: input.head,
        hips_head_y: None,
        head_rotation: Quat::IDENTITY,
        torso_yaw: Some(0.0),
        feet: input.stance.map(|f| f.position),
        foot_rotations: input.stance.map(|f| f.rotation),
        support: (input.stance[0].position + input.stance[1].position) * 0.5,
        planted: [true; 2],
        heel_lifts: [0.0; 2],
        hips_xz: None,
        hips_drop: 0.0,
        lower: None,
    }
}

#[test]
fn the_solve_puts_the_kneeling_knee_on_the_ground_and_the_other_up() {
    let (full, rig, mut input, tuning) = setup_with_rig();
    let config = ProceduralBodyConfig::new();
    input.world = Mat4::IDENTITY;
    lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
    let mut body = LowerBody::new();
    hold(&mut body, &rig, &input, &tuning, 1.0);
    let targets = body.targets(&rig, &input).unwrap();
    let mut solve_input = standing_solve_input(&full, &input);
    solve_input.hold(targets);
    let pose = solve(&full, &solve_input, &config);
    assert!(pose.position(RigBone::Hips).abs_diff_eq(targets.hips, 1e-3));
    let knee = pose.position(RigBone::LeftLeg);
    assert!(
        (knee.y - kneel::KNEE_GROUND_M).abs() < 0.01,
        "the knee is at {knee}"
    );
    assert!(
        pose.position(RigBone::LeftFoot)
            .abs_diff_eq(targets.feet[0].position, 2e-3)
    );
    // The standing leg's knee is up, ahead of its hip and above its ankle.
    let front = pose.position(RigBone::RightLeg);
    assert!(
        front.y > pose.position(RigBone::RightFoot).y + 0.2,
        "{front}"
    );
    assert!(front.z < pose.position(RigBone::RightUpLeg).z);
    // Half the weight is half the way from standing.
    let mut half = standing_solve_input(&full, &input);
    half.hold(LowerTargets {
        weight: 0.5,
        ..targets
    });
    let between = solve(&full, &half, &config);
    let standing = pose_without(&full, &input, &config);
    let mid = (standing.position(RigBone::Hips) + targets.hips) * 0.5;
    assert!(between.position(RigBone::Hips).abs_diff_eq(mid, 1e-3));
}

fn pose_without(full: &Rig, input: &LowerInput, config: &ProceduralBodyConfig) -> RigPose {
    solve(full, &standing_solve_input(full, input), config)
}

/// Sit the head down at a sitting height and hold it still for `seconds`.
fn sit_down(body: &mut LowerBody, rig: &LowerRig, input: &mut LowerInput, tuning: &LowerTuning) {
    lower_to(rig, input, rig.sit_hips());
    hold(body, rig, input, tuning, tuning.sit_after_s + 0.2);
}

#[test]
fn a_low_still_head_sits_in_standing_play_and_kneels_first() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    lower_to(&rig, &mut input, rig.sit_hips());
    // The kneel's shorter wait comes first.
    hold(&mut body, &rig, &input, &tuning, 0.4);
    assert_eq!(body.status().pose, LowerPose::Kneel);
    hold(&mut body, &rig, &input, &tuning, tuning.sit_after_s);
    let status = body.status();
    assert_eq!(status.pose, LowerPose::Sit);
    assert!(status.holds_stick());
    // The kneel lets go as the sit takes over.
    hold(&mut body, &rig, &input, &tuning, 1.0);
    assert_eq!(body.status().kneel_weight, 0.0);
    assert_eq!(body.status().sit_weight, 1.0);
    // Played seated, the same head only kneels.
    input.standing_play = false;
    let mut seated = LowerBody::new();
    hold(&mut seated, &rig, &input, &tuning, 3.0);
    assert_eq!(seated.status().pose, LowerPose::Kneel);
    // Switched off, it only kneels too.
    input.standing_play = true;
    let off = LowerTuning {
        sit: false,
        ..tuning
    };
    let mut unsat = LowerBody::new();
    hold(&mut unsat, &rig, &input, &off, 3.0);
    assert_eq!(unsat.status().pose, LowerPose::Kneel);
}

#[test]
fn a_moving_head_does_not_sit() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    lower_to(&rig, &mut input, rig.sit_hips());
    for i in 0..120 {
        // The head sways at about half a metre a second.
        input.head_world = Vec3::new((i as f32 * 0.2).sin() * 0.1, 0.0, 0.0);
        body.update(&rig, &input, &tuning);
        assert_ne!(body.status().pose, LowerPose::Sit, "tick {i}");
    }
    assert!(body.status().head_speed_mps > 0.25);
}

#[test]
fn the_stick_stands_the_body_up_and_it_sits_again_only_after_the_head_rises() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    sit_down(&mut body, &rig, &mut input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Sit);
    input.stick = true;
    input.moving = true;
    let change = body.update(&rig, &input, &tuning).unwrap();
    assert_eq!(change.from, LowerPose::Sit);
    assert_eq!(change.to, LowerPose::Crouch);
    assert_eq!(change.sit_end, Some(SitEnd::Stick));
    // Getting up holds the stick until the sit has let go.
    assert!(body.status().holds_stick());
    hold(&mut body, &rig, &input, &tuning, tuning.sit_blend_s + 0.1);
    assert!(!body.status().holds_stick());
    // Still low and still, it kneels rather than sitting again.
    input.stick = false;
    input.moving = false;
    hold(&mut body, &rig, &input, &tuning, 3.0);
    assert_eq!(body.status().pose, LowerPose::Kneel);
    assert!(!body.status().sit_armed);
    // Once the head has risen, it may sit again.
    lower_to(&rig, &mut input, rig.sit_below() + 0.15);
    body.update(&rig, &input, &tuning);
    assert!(body.status().sit_armed);
    sit_down(&mut body, &rig, &mut input, &tuning);
    assert_eq!(body.status().pose, LowerPose::Sit);
}

#[test]
fn raising_the_head_stands_up_through_a_crouch() {
    let (rig, mut input, tuning) = setup();
    let mut body = LowerBody::new();
    sit_down(&mut body, &rig, &mut input, &tuning);
    hold(&mut body, &rig, &input, &tuning, tuning.sit_blend_s);
    // A small lift stays sat.
    lower_to(&rig, &mut input, rig.sit_below() + 0.05);
    assert_eq!(body.update(&rig, &input, &tuning), None);
    lower_to(&rig, &mut input, rig.lowest_hips + 0.1);
    let change = body.update(&rig, &input, &tuning).unwrap();
    assert_eq!(change.to, LowerPose::Crouch);
    assert_eq!(change.sit_end, Some(SitEnd::HeadRose));
    // The sit eases out rather than letting go at once.
    let weight = body.status().sit_weight;
    assert!(weight > 0.5 && weight < 1.0, "{weight}");
    // Leaving the ground ends it too.
    let mut other = LowerBody::new();
    sit_down(&mut other, &rig, &mut input, &tuning);
    input.grounded = false;
    let change = other.update(&rig, &input, &tuning).unwrap();
    assert_eq!(change.sit_end, Some(SitEnd::Interrupted));
}

#[test]
fn the_head_forward_of_the_stance_crosses_the_legs() {
    let (rig, mut input, tuning) = setup();
    let mut upright = LowerBody::new();
    sit_down(&mut upright, &rig, &mut input, &tuning);
    assert_eq!(upright.status().sit_legs, Some(SitLegs::KneesUp));
    let mut forward = LowerBody::new();
    let mut leaning = input;
    leaning.head.z -= 0.3;
    sit_down(&mut forward, &rig, &mut leaning, &tuning);
    assert_eq!(forward.status().sit_legs, Some(SitLegs::Crossed));
    // Leaning forward for a moment once sat crosses the knees-up legs; a glance forward does not.
    let mut lean = input;
    lean.head.z -= 0.3;
    hold(&mut upright, &rig, &lean, &tuning, 0.5);
    assert_eq!(upright.status().sit_legs, Some(SitLegs::KneesUp));
    hold(&mut upright, &rig, &lean, &tuning, 0.7);
    assert_eq!(upright.status().sit_legs, Some(SitLegs::Crossed));
}

#[test]
fn the_solve_sits_the_hips_on_the_ground_with_the_legs_in_front() {
    let (full, rig, mut input, tuning) = setup_with_rig();
    let config = ProceduralBodyConfig::new();
    input.world = Mat4::IDENTITY;
    let mut body = LowerBody::new();
    sit_down(&mut body, &rig, &mut input, &tuning);
    hold(&mut body, &rig, &input, &tuning, 1.0);
    let targets = body.targets(&rig, &input).unwrap();
    assert_eq!(targets.weight, 1.0);
    let mut solve_input = standing_solve_input(&full, &input);
    solve_input.hold(targets);
    let pose = solve(&full, &solve_input, &config);
    let hips = pose.position(RigBone::Hips);
    assert!((hips.y - rig.sit_hips()).abs() < 1e-3, "{hips}");
    // Knees up: the feet ahead of the hips, the knees well above both.
    for (up, leg, foot) in [
        (RigBone::LeftUpLeg, RigBone::LeftLeg, RigBone::LeftFoot),
        (RigBone::RightUpLeg, RigBone::RightLeg, RigBone::RightFoot),
    ] {
        assert!(pose.position(foot).z < hips.z - 0.3);
        assert!(pose.position(leg).y > pose.position(up).y + 0.2, "{leg:?}");
    }
    // Crossed: each ankle over on the other side, each knee out to its own.
    let mut crossed = LowerBody::new();
    let mut leaning = input;
    leaning.head.z -= 0.3;
    sit_down(&mut crossed, &rig, &mut leaning, &tuning);
    hold(&mut crossed, &rig, &leaning, &tuning, 1.0);
    let targets = crossed.targets(&rig, &leaning).unwrap();
    let mut solve_input = standing_solve_input(&full, &leaning);
    solve_input.hold(targets);
    let pose = solve(&full, &solve_input, &config);
    let hips = pose.position(RigBone::Hips);
    assert!(pose.position(RigBone::LeftFoot).x > hips.x);
    assert!(pose.position(RigBone::RightFoot).x < hips.x);
    assert!(pose.position(RigBone::LeftLeg).x < pose.position(RigBone::LeftUpLeg).x - 0.1);
    assert!(pose.position(RigBone::RightLeg).x > pose.position(RigBone::RightUpLeg).x + 0.1);
    for bone in [RigBone::LeftFoot, RigBone::RightFoot] {
        assert!(
            pose.position(bone)
                .abs_diff_eq(targets.feet[bone_side(bone)].position, 2e-3)
        );
    }
}

fn bone_side(bone: RigBone) -> usize {
    match bone {
        RigBone::LeftFoot => 0,
        _ => 1,
    }
}

#[test]
fn a_sit_over_a_kneel_layers_their_weights() {
    let (_, input, _) = setup();
    let base = LowerTargets {
        weight: 0.5,
        hips: Vec3::ZERO,
        feet: input.stance,
        poles: [Vec3::NEG_Z; 2],
    };
    let top = LowerTargets {
        weight: 0.5,
        hips: Vec3::Y,
        ..base
    };
    let both = top.over(&base);
    // Over the solver's own at the origin: a quarter left, a quarter the kneel, a half the sit.
    assert!((both.weight - 0.75).abs() < 1e-6);
    let own = Vec3::new(4.0, 0.0, 0.0);
    let mixed = own.lerp(both.hips, both.weight);
    let layered = own.lerp(base.hips, base.weight).lerp(top.hips, top.weight);
    assert!(mixed.abs_diff_eq(layered, 1e-5), "{mixed} {layered}");
}
