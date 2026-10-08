//! The status page's rows for the controller systems: the gun in the hand, the hand bodies, the
//! locomotion, and the procedural body.

use std::time::Instant;

use smol_str::{SmolStr, format_smolstr};

use crate::controllers::{
    body::procedural::{
        self, ProceduralStatus,
        authority::Case,
        config::ProceduralMode,
        ground::GroundTally,
        solver::{
            arms::Side,
            lower::{LowerPose, LowerStatus, SitLegs},
        },
    },
    frame::Hand,
    hands::grip::{self, GripRead},
    locomotion::{
        self,
        acts::{self, ActFilterStatus},
        anchor::{self, AnchorStatus},
    },
    menu::{
        read_config,
        status::{Health, Line},
    },
    physical_hands::physics::{self, BodyPhase, CallbackStatus, HandBodyStatus},
    weapons::{grip_watch, heft::HeftClass},
};

pub(super) fn rigid_grip() -> Line {
    if !read_config(|c| c.controllers.hands.rigid_grip) {
        return Line::off("");
    }
    let pinned = grip_watch::status().pinned;
    let pins = grip::status().pins;
    rigid_grip_line(pinned, pins)
}

pub(super) fn canonical_grip() -> Line {
    canonical_grip_line(grip::grip_reads())
}

pub(super) fn flip_watch() -> Line {
    let watch = grip_watch::status();
    let flips: u64 = watch.flips.iter().sum();
    let sweeps: u64 = watch.sweeps.iter().sum();
    let detail = format_smolstr!("{flips} FLIPS, {sweeps} SWEEPS");
    if flips > 0 {
        Line::failed(detail)
    } else {
        Line::ok(detail)
    }
}

pub(super) fn animated_deviation() -> Line {
    animated_deviation_line(grip_watch::status().by_class)
}

pub(super) fn bone_changes() -> Line {
    let [left, right] = grip_watch::status().bone_changes;
    Line::ok(format_smolstr!("L {left}, R {right}"))
}

pub(super) fn callback() -> Line {
    let enabled = read_config(|c| physics::enabled(&c.controllers.physical_hands));
    callback_line(enabled, physics::callback_status())
}

pub(super) fn callback_fires() -> Line {
    let status = physics::callback_status();
    let detail = format_smolstr!("{} FIRES", status.fires);
    match status.fires {
        0 if status.registered => Line::waiting(detail),
        0 => Line::off(detail),
        _ => Line::ok(detail),
    }
}

pub(super) fn left_body() -> Line {
    body(Hand::Left)
}

pub(super) fn right_body() -> Line {
    body(Hand::Right)
}

pub(super) fn body_errors() -> Line {
    let status = physics::status();
    let made = format_smolstr!("{} MADE, {} GONE", status.created, status.destroyed);
    match status.last_error {
        Some(error) => Line::failed("SEE LOG").with_note(error),
        None => Line::ok(made),
    }
}

pub(super) fn contact_events() -> Line {
    let hits = read_config(|c| {
        physics::enabled(&c.controllers.physical_hands)
            && c.controllers.physical_hands.bodies.hits.enabled
    });
    if !hits {
        return Line::off("");
    }
    let listening = physics::callback_status().listening;
    let status = physics::status();
    let hands = hands_label(listening);
    let detail = format_smolstr!("{hands}, {} CONTACTS", status.contacts);
    if status.dropped_contacts > 0 {
        Line::failed(format_smolstr!("{} DROPPED", status.dropped_contacts))
    } else if listening.contains(&true) {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

pub(super) fn blows() -> Line {
    let status = physics::status();
    match status.last_blow {
        Some(blow) => Line::ok(format_smolstr!(
            "{}, LAST {:.0} NS",
            status.blows,
            blow.impulse_ns
        )),
        None => Line::ok(format_smolstr!("{}", status.blows)),
    }
}

pub(super) fn joints() -> Line {
    let on = read_config(|c| {
        physics::enabled(&c.controllers.physical_hands)
            && c.controllers.physical_hands.bodies.joint_grabs
    });
    if !on {
        return Line::off("");
    }
    let joined = physics::status().hands.map(|hand| hand.joint.is_some());
    Line::ok(format_smolstr!(
        "{} ACTIVE, {}",
        joined.iter().filter(|&&joined| joined).count(),
        hands_label(joined)
    ))
}

pub(super) fn stick_velocity() -> Line {
    if !read_config(|c| c.controllers.locomotion.stick.enabled) {
        return Line::off("");
    }
    // The last tick's, not this one's: the movement task does not run while the game is paused,
    // which it is while the page is shown.
    let status = locomotion::status();
    match status.velocity {
        Some(velocity) => Line::ok(format_smolstr!("{:.1} M/S", velocity.length())),
        None if status.at.is_some() => Line::waiting("NOT DRIVING"),
        None => Line::waiting("NOT ON FOOT YET"),
    }
}

pub(super) fn act_filter() -> Line {
    if !read_config(|c| c.controllers.locomotion.acts.enabled) {
        return Line::off("");
    }
    act_filter_line(acts::status())
}

pub(super) fn act_watchdog() -> Line {
    if !read_config(|c| c.controllers.locomotion.acts.enabled) {
        return Line::off("");
    }
    let status = acts::status();
    let trips = format_smolstr!("{} TRIPS", status.releases);
    if status.released {
        Line::waiting(format_smolstr!("OPEN, {trips}"))
    } else {
        Line::ok(trips)
    }
}

pub(super) fn steady_view() -> Line {
    if !read_config(|c| c.controllers.locomotion.view.enabled) {
        return Line::off("");
    }
    steady_view_line(&anchor::status())
}

pub(super) fn neutral_head() -> Line {
    if !read_config(|c| c.controllers.locomotion.view.enabled) {
        return Line::off("");
    }
    match anchor::status().neutral {
        Some(neutral) => Line::ok(format_smolstr!("EYES {:.2} M", neutral.eyes.y))
            .with_note(format!("head {:.2} m", neutral.head.y)),
        None => Line::waiting("NOT MEASURED"),
    }
}

pub(super) fn procedural_mode() -> Line {
    let mode = read_config(|c| c.controllers.body.procedural.mode);
    procedural_mode_line(mode, &procedural::published_status(), Instant::now())
}

pub(super) fn pass_through() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::PassThrough {
        return Line::off("NOT IN TEST");
    }
    pass_through_line(procedural::published_status().round_trip)
}

pub(super) fn hand_over() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    hand_over_line(procedural::published_status().case)
}

pub(super) fn ground_casts() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    ground_line(procedural::published_status().ground)
}

pub(super) fn stance() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) == ProceduralMode::Off {
        return Line::off("");
    }
    if procedural::published_status().stance_measured {
        Line::ok("MEASURED")
    } else {
        Line::waiting("STAND STILL")
    }
}

pub(super) fn lower_body() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    let status = procedural::published_status();
    lower_body_line(status.case, &status.lower)
}

pub(super) fn lower_inputs() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::Full {
        return Line::off("");
    }
    let status = procedural::published_status();
    lower_inputs_line(&status.lower, status.crouch_depth)
}

/// The largest pass-through error that still proves the write path: metres, then degrees.
const PASS_THROUGH_TOLERANCE: (f32, f32) = (0.001, 0.1);

fn rigid_grip_line(pinned: [Option<bool>; 2], pins: u64) -> Line {
    let pins = format_smolstr!("{pins} PINS");
    let hands = |want: bool| pinned.map(|pinned| pinned == Some(want));
    if hands(true).contains(&true) {
        Line::ok(format_smolstr!("{}, {pins}", hands_label(hands(true))))
    } else if hands(false).contains(&true) {
        Line::waiting(format_smolstr!("{} NOT PINNED", hands_label(hands(false))))
    } else {
        Line::waiting("NO GUN").with_note(pins.to_string())
    }
}

fn canonical_grip_line(reads: [Option<GripRead>; 2]) -> Line {
    let failed = reads
        .iter()
        .zip(Hand::BOTH)
        .find_map(|(read, hand)| match read {
            Some(GripRead::BonesUnresolved) => Some((hand, "BONES MISSING")),
            Some(GripRead::DegenerateBindPose) => Some((hand, "BAD BIND POSE")),
            Some(GripRead::Read) | None => None,
        });
    if let Some((hand, reason)) = failed {
        return Line::failed(format_smolstr!("{} {reason}", hand_letter(hand)));
    }
    let read = reads.map(|read| read == Some(GripRead::Read));
    if read.contains(&true) {
        Line::ok(format_smolstr!("{} READ", hands_label(read)))
    } else {
        Line::waiting("NO GUN YET")
    }
}

/// The animated deviation seen for each weight class, in degrees, the classes seen so far only.
/// A diagnostic for whether the bind-pose grip is the pose the aiming clips hold each kind of gun
/// in, which is unverified; it shows numbers rather than a verdict.
fn animated_deviation_line(by_class: [Option<f32>; 4]) -> Line {
    let mut detail = SmolStr::default();
    for (class, deviation) in HeftClass::ALL.iter().zip(by_class) {
        let Some(deviation) = deviation else {
            continue;
        };
        let separator = if detail.is_empty() { "" } else { ", " };
        detail = format_smolstr!(
            "{detail}{separator}{} {deviation:.0}°",
            class.label().to_uppercase()
        );
    }
    if detail.is_empty() {
        Line::waiting("NO GUN YET")
    } else {
        Line::ok(detail)
    }
}

fn callback_line(enabled: bool, status: CallbackStatus) -> Line {
    if !status.registered {
        return if enabled {
            Line::waiting("NOT REGISTERED")
        } else {
            Line::off("")
        };
    }
    if status.firing_thread == 0 {
        Line::waiting("NOT FIRED")
    } else if status.firing_thread == status.game_thread {
        Line::ok("GAME THREAD")
    } else {
        Line::ok(format_smolstr!("THREAD {}", status.firing_thread))
    }
}

fn body(hand: Hand) -> Line {
    let enabled = read_config(|c| physics::enabled(&c.controllers.physical_hands));
    body_line(enabled, &physics::status().hands[hand.index()])
}

fn body_line(enabled: bool, status: &HandBodyStatus) -> Line {
    let id = status
        .body_id
        .map_or(SmolStr::default(), |id| format_smolstr!(" #{id}"));
    match status.phase {
        BodyPhase::Acting => Line::ok(format_smolstr!("IN WORLD{id}")),
        BodyPhase::Parked => Line::ok(format_smolstr!("PARKED{id}")),
        BodyPhase::Absent if enabled => Line::waiting("NO BODY"),
        BodyPhase::Absent => Line::off(""),
    }
}

fn act_filter_line(status: ActFilterStatus) -> Line {
    let detail = format_smolstr!("{} DROP, {} SWAP", status.dropped, status.replaced);
    if status.refused > 0 {
        Line::ok(format_smolstr!("{detail}, {} REF", status.refused))
            .with_note(format!("{} swaps refused", status.refused))
    } else {
        Line::ok(detail)
    }
}

fn steady_view_line(status: &AnchorStatus) -> Line {
    if status.weight >= 1.0 {
        Line::ok("ENGAGED")
    } else if status.weight > 0.0 {
        Line::ok(format_smolstr!("BLENDING {:.0}%", status.weight * 100.0))
    } else if status.eligible && status.neutral.is_none() {
        if status.capture_progress > 0.0 {
            Line::waiting(format_smolstr!(
                "MEASURING {:.1} S",
                status.capture_progress
            ))
        } else {
            Line::waiting("STAND STILL")
        }
    } else if status.eligible {
        Line::waiting("READY")
    } else {
        Line::ok("IDLE")
    }
}

fn procedural_mode_line(mode: ProceduralMode, status: &ProceduralStatus, now: Instant) -> Line {
    let label = match mode {
        ProceduralMode::Off => return Line::off(""),
        ProceduralMode::PassThrough => "TEST",
        ProceduralMode::Full => "FULL BODY",
    };
    if let Some(error) = status.rig_error {
        return Line::failed("RIG, SEE LOG").with_note(error.to_string());
    }
    // Whether the gate has run in this mode, however long ago: it does not run while the game is
    // paused, which it is while the page is shown.
    match status.at {
        Some(at) if status.mode == mode => Line::ok(label).with_note(format!(
            "last gate {:.1} s ago",
            now.saturating_duration_since(at).as_secs_f32()
        )),
        _ => Line::waiting(format_smolstr!("{label} IDLE")),
    }
}

fn pass_through_line(round_trip: Option<(f32, f32)>) -> Line {
    let Some((metres, degrees)) = round_trip else {
        return Line::waiting("NOT MEASURED");
    };
    let detail = format_smolstr!("{:.1} MM, {degrees:.2} DEG", metres * 1000.0);
    let (max_m, max_deg) = PASS_THROUGH_TOLERANCE;
    if metres <= max_m && degrees <= max_deg {
        Line::ok(detail)
    } else {
        Line::failed(detail)
    }
}

fn hand_over_line(case: Case) -> Line {
    let word = match case {
        Case::NotInHeadset => "NO HEADSET",
        Case::SwitchedOff => "SWITCHED OFF",
        Case::NotInGameplay => "NOT IN GAMEPLAY",
        Case::Sequence => "SEQUENCE",
        Case::Dead => "DEAD",
        Case::Ragdoll => "RAGDOLL",
        Case::GettingUp => "GETTING UP",
        Case::Vehicle => "VEHICLE",
        Case::Traversal => "TRAVERSAL",
        Case::LedgeClimb => "LEDGE CLIMB",
        Case::Swimming => "SWIMMING",
        Case::Flight => "AIRBORNE",
        Case::HitReaction => "HIT REACTION",
        Case::HandClimb => "HAND CLIMB",
        Case::Jump => "JUMP",
        Case::AuthoredMotion => "AUTHORED MOTION",
        Case::NoMovementTask => "NO MOVE TASK",
        Case::OnFoot => "ON FOOT",
    };
    // The animation has the body in every case but on foot, which is what the hand-over is for.
    let health = match case {
        Case::OnFoot => Health::Ok,
        _ => Health::Waiting,
    };
    Line::new(health, word).with_note(case.label())
}

/// The lower body's pose: what the legs are doing on foot, and the kneel's side and weight.
fn lower_body_line(case: Case, lower: &LowerStatus) -> Line {
    if case != Case::OnFoot {
        return Line::waiting("NOT ON FOOT");
    }
    match lower.pose {
        LowerPose::Stand => Line::ok("STAND"),
        LowerPose::Crouch => Line::ok("CROUCH"),
        LowerPose::Kneel => Line::ok(format_smolstr!(
            "KNEEL {}, {:.0}%",
            lower.kneel_side.map_or("?", side_letter),
            lower.kneel_weight * 100.0
        )),
        LowerPose::Sit => Line::ok(format_smolstr!(
            "SIT {}, {:.0}%",
            match lower.sit_legs {
                Some(SitLegs::Crossed) => "CROSSED",
                Some(SitLegs::KneesUp) => "KNEES UP",
                None => "?",
            },
            lower.sit_weight * 100.0
        )),
    }
}

/// What the lower body's pose is picked from: the hips' height the head asks for against the
/// squat's floor, the crouch's depth, how long the head has been low and still enough to sit, and
/// whether the body is told to move, the player plays seated, or the stick stood the body up and a
/// sit waits for the head to rise.
fn lower_inputs_line(lower: &LowerStatus, crouch: f32) -> Line {
    let moving = if lower.moving { ", MOVING" } else { "" };
    let seated = if lower.standing_play { "" } else { ", SEATED" };
    let disarmed = if lower.sit_armed { "" } else { ", RISE TO SIT" };
    Line::ok(format_smolstr!(
        "HIPS {:.2} M, FLOOR {:.2} M, CROUCH {:.2}, STILL {:.1} S{moving}{seated}{disarmed}",
        lower.wanted_hips_m,
        lower.floor_m,
        crouch,
        lower.still_s
    ))
    .with_note(format!(
        "head speed {:.2} m/s, sit weight {:.2}",
        lower.head_speed_mps, lower.sit_weight
    ))
}

fn side_letter(side: Side) -> &'static str {
    match side {
        Side::Left => "L",
        Side::Right => "R",
    }
}

fn ground_line(tally: GroundTally) -> Line {
    let detail = format_smolstr!("{}/{} HIT", tally.hits, tally.casts);
    if tally.errors > 0 {
        Line::failed(format_smolstr!("{} ERR, {detail}", tally.errors))
    } else if tally.casts == 0 {
        Line::waiting("NO CASTS")
    } else {
        Line::ok(detail)
    }
}

/// The hands a per-hand flag is set for: `L`, `R`, `L+R`, or `NONE`.
fn hands_label(set: [bool; 2]) -> &'static str {
    match set {
        [true, true] => "L+R",
        [true, false] => "L",
        [false, true] => "R",
        [false, false] => "NONE",
    }
}

fn hand_letter(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "L",
        Hand::Right => "R",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec3;

    use super::*;
    use crate::controllers::locomotion::view::Neutral;

    #[test]
    fn the_animated_deviation_lists_the_classes_seen() {
        assert_eq!(
            animated_deviation_line([Some(12.4), None, Some(31.6), None]).text(),
            "OK, SIDEARM 12°, RIFLE 32°"
        );
        assert_eq!(
            animated_deviation_line([None; 4]).text(),
            "WAITING, NO GUN YET"
        );
    }

    #[test]
    fn the_rigid_grip_names_the_pinned_hands() {
        assert_eq!(
            rigid_grip_line([None, Some(true)], 40).text(),
            "OK, R, 40 PINS"
        );
        assert_eq!(
            rigid_grip_line([Some(false), None], 0).text(),
            "WAITING, L NOT PINNED"
        );
        assert_eq!(rigid_grip_line([None, None], 3).text(), "WAITING, NO GUN");
    }

    #[test]
    fn a_failed_grip_read_wins_over_a_good_one() {
        assert_eq!(
            canonical_grip_line([Some(GripRead::Read), Some(GripRead::BonesUnresolved)]).text(),
            "FAILED, R BONES MISSING"
        );
        assert_eq!(
            canonical_grip_line([Some(GripRead::Read), None]).text(),
            "OK, L READ"
        );
        assert_eq!(canonical_grip_line([None, None]).health, Health::Waiting);
    }

    #[test]
    fn the_callback_reports_its_thread() {
        let fired = |firing_thread| CallbackStatus {
            registered: true,
            game_thread: 7,
            firing_thread,
            ..CallbackStatus::default()
        };
        assert_eq!(callback_line(true, fired(7)).text(), "OK, GAME THREAD");
        assert_eq!(callback_line(true, fired(9)).text(), "OK, THREAD 9");
        assert_eq!(callback_line(true, fired(0)).health, Health::Waiting);
        assert_eq!(
            callback_line(false, CallbackStatus::default()).health,
            Health::Off
        );
    }

    #[test]
    fn bodies_read_their_phase() {
        let acting = HandBodyStatus {
            phase: BodyPhase::Acting,
            body_id: Some(12),
            ..HandBodyStatus::default()
        };
        assert_eq!(body_line(true, &acting).text(), "OK, IN WORLD #12");
        assert_eq!(
            body_line(true, &HandBodyStatus::default()).health,
            Health::Waiting
        );
        assert_eq!(
            body_line(false, &HandBodyStatus::default()).health,
            Health::Off
        );
    }

    #[test]
    fn the_steady_view_walks_its_states() {
        let neutral = Neutral {
            head: Vec3::Y,
            neck: Vec3::Y,
            eyes: Vec3::Y,
        };
        let status = |weight, eligible, measured: bool, capture_progress| AnchorStatus {
            weight,
            eligible,
            neutral: measured.then_some(neutral),
            capture_progress,
            ..AnchorStatus::default()
        };
        let text = |s: AnchorStatus| steady_view_line(&s).text();
        assert_eq!(text(status(1.0, true, true, 0.0)), "OK, ENGAGED");
        assert_eq!(text(status(0.5, true, true, 0.0)), "OK, BLENDING 50%");
        assert_eq!(
            text(status(0.0, true, false, 1.24)),
            "WAITING, MEASURING 1.2 S"
        );
        assert_eq!(text(status(0.0, true, false, 0.0)), "WAITING, STAND STILL");
        assert_eq!(text(status(0.0, false, true, 0.0)), "OK, IDLE");
    }

    #[test]
    fn the_lower_body_names_the_pose_and_its_inputs() {
        let mut lower = LowerStatus::new();
        assert_eq!(
            lower_body_line(Case::Jump, &lower).text(),
            "WAITING, NOT ON FOOT"
        );
        assert_eq!(lower_body_line(Case::OnFoot, &lower).text(), "OK, STAND");
        lower.pose = LowerPose::Kneel;
        lower.kneel_side = Some(Side::Right);
        lower.kneel_weight = 0.5;
        assert_eq!(
            lower_body_line(Case::OnFoot, &lower).text(),
            "OK, KNEEL R, 50%"
        );
        lower.pose = LowerPose::Sit;
        lower.sit_legs = Some(SitLegs::Crossed);
        lower.sit_weight = 1.0;
        assert_eq!(
            lower_body_line(Case::OnFoot, &lower).text(),
            "OK, SIT CROSSED, 100%"
        );
        lower.wanted_hips_m = 0.42;
        lower.floor_m = 0.5;
        lower.still_s = 0.4;
        lower.moving = true;
        lower.standing_play = true;
        assert_eq!(
            lower_inputs_line(&lower, 1.0).text(),
            "OK, HIPS 0.42 M, FLOOR 0.50 M, CROUCH 1.00, STILL 0.4 S, MOVING"
        );
        lower.standing_play = false;
        lower.sit_armed = false;
        assert_eq!(
            lower_inputs_line(&lower, 1.0).text(),
            "OK, HIPS 0.42 M, FLOOR 0.50 M, CROUCH 1.00, STILL 0.4 S, MOVING, SEATED, RISE TO SIT"
        );
    }

    #[test]
    fn the_procedural_mode_must_have_run_in_it() {
        let now = Instant::now();
        let ran = ProceduralStatus {
            at: Some(now),
            mode: ProceduralMode::Full,
            ..ProceduralStatus::default()
        };
        assert_eq!(
            procedural_mode_line(ProceduralMode::Full, &ran, now).text(),
            "OK, FULL BODY"
        );
        assert_eq!(
            procedural_mode_line(ProceduralMode::PassThrough, &ran, now).text(),
            "WAITING, TEST IDLE"
        );
        let later = now + Duration::from_secs(60);
        assert_eq!(
            procedural_mode_line(ProceduralMode::Full, &ran, later)
                .note
                .as_deref(),
            Some("last gate 60.0 s ago")
        );
        assert_eq!(
            procedural_mode_line(ProceduralMode::Full, &ProceduralStatus::default(), now).health,
            Health::Waiting
        );
        assert_eq!(
            procedural_mode_line(ProceduralMode::Off, &ran, now).health,
            Health::Off
        );
    }

    #[test]
    fn the_pass_through_fails_past_its_tolerance() {
        assert_eq!(
            pass_through_line(Some((0.0002, 0.01))).text(),
            "OK, 0.2 MM, 0.01 DEG"
        );
        assert_eq!(pass_through_line(Some((0.02, 0.01))).health, Health::Failed);
        assert_eq!(pass_through_line(None).health, Health::Waiting);
    }

    #[test]
    fn the_hand_over_is_live_on_foot() {
        assert_eq!(hand_over_line(Case::OnFoot).text(), "OK, ON FOOT");
        let climb = hand_over_line(Case::LedgeClimb);
        assert_eq!(climb.text(), "WAITING, LEDGE CLIMB");
        assert_eq!(climb.note.as_deref(), Some(Case::LedgeClimb.label()));
    }

    #[test]
    fn ground_errors_fail_the_casts() {
        let tally = |casts, hits, errors| GroundTally {
            casts,
            hits,
            errors,
            ..GroundTally::new()
        };
        assert_eq!(ground_line(tally(4, 3, 0)).text(), "OK, 3/4 HIT");
        assert_eq!(ground_line(tally(4, 3, 1)).text(), "FAILED, 1 ERR, 3/4 HIT");
        assert_eq!(ground_line(tally(0, 0, 0)).health, Health::Waiting);
    }
}
