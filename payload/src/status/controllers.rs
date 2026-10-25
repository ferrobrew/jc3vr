//! The status rows for the controller systems: the gun in the hand, the hand bodies, the
//! locomotion, and the procedural body.

use std::time::Instant;

use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        body::procedural::{
            self, ProceduralStatus,
            authority::Case,
            config::ProceduralMode,
            ground::GroundTally,
            solver::{
                arms::Side,
                lower::{LowerPose, LowerStatus, SitLegs},
            },
            twist::TwistStatus,
        },
        frame::Hand,
        hands::grip::{self, GripRead},
        locomotion::{
            self,
            acts::{self, ActFilterStatus},
            anchor::{self, AnchorStatus},
        },
        menu::read_config,
        physical_hands::{
            collider_view::{self, ColliderViewStatus},
            physics::{
                self, BodyPhase, CallbackStatus, HandBodyStatus,
                guns::{self, GunBodyStatus},
            },
        },
        weapons::{grip_watch, heft::HeftClass},
    },
    status::{Health, Line, Text, prose, text},
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
    let detail = text!(flips.to_string(), " flips, ", sweeps.to_string(), " sweeps");
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
    let detail = text!(status.fires.to_string(), " fires");
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
    let made = text!(
        status.created.to_string(),
        " made, ",
        status.destroyed.to_string(),
        " gone"
    );
    match status.last_error {
        Some(error) => Line::failed(prose("See log")).with_note(error),
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
    let detail = text!(hands, ", ", status.contacts.to_string(), " contacts");
    if status.dropped_contacts > 0 {
        Line::failed(text!(status.dropped_contacts.to_string(), " dropped"))
    } else if listening.contains(&true) {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

pub(super) fn collider_view() -> Line {
    collider_view_line(&collider_view::status())
}

pub(super) fn blows() -> Line {
    let status = physics::status();
    match status.last_blow {
        Some(blow) => Line::ok(text!(
            status.blows.to_string(),
            ", last ",
            format!("{:.0}", blow.impulse_ns),
            " Ns"
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
    Line::ok(text!(
        joined.iter().filter(|&&joined| joined).count().to_string(),
        " active, ",
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
        Some(velocity) => Line::ok(text!(format!("{:.1}", velocity.length()), " m/s")),
        None if status.at.is_some() => Line::waiting(prose("Not driving")),
        None => Line::waiting(prose("Not on foot yet")),
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
    let trips = text!(status.releases.to_string(), " trips");
    if status.released {
        Line::waiting(text!("Open, ", trips))
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
        Some(neutral) => Line::ok(text!("Eyes ", format!("{:.2}", neutral.eyes.y), " m"))
            .with_note(format!("head {:.2} m", neutral.head.y)),
        None => Line::waiting(prose("Not measured")),
    }
}

pub(super) fn procedural_mode() -> Line {
    let mode = read_config(|c| c.controllers.body.procedural.mode);
    procedural_mode_line(mode, &procedural::published_status(), Instant::now())
}

pub(super) fn pass_through() -> Line {
    if read_config(|c| c.controllers.body.procedural.mode) != ProceduralMode::PassThrough {
        return Line::off(prose("Not in test"));
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
        Line::ok(prose("Measured"))
    } else {
        Line::waiting(prose("Stand still"))
    }
}

pub(super) fn twist_bones() -> Line {
    let (enabled, mode) = read_config(|c| {
        (
            c.controllers.body.procedural.twist_bones,
            c.controllers.body.procedural.mode,
        )
    });
    if !enabled || mode != ProceduralMode::Full {
        return Line::off("");
    }
    twist_line(&procedural::twist::status())
}

/// The twist bones found and the largest twist they carried on the last pass.
fn twist_line(status: &TwistStatus) -> Line {
    match status.bones {
        None => Line::waiting(prose("Not read")),
        Some((0, _)) => Line::failed(prose("None found")),
        Some((bones, segments)) => {
            let detail = text!(
                bones.to_string(),
                " on ",
                segments.to_string(),
                ", ",
                format!("{:.0}°", status.largest_twist.to_degrees())
            );
            if status.wrote {
                Line::ok(detail)
            } else {
                Line::waiting(detail)
            }
        }
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
    let pins = text!(pins.to_string(), " pins");
    let hands = |want: bool| pinned.map(|pinned| pinned == Some(want));
    if hands(true).contains(&true) {
        Line::ok(text!(hands_label(hands(true)), ", ", pins))
    } else if hands(false).contains(&true) {
        Line::waiting(text!(hands_label(hands(false)), " not pinned"))
    } else {
        Line::waiting(prose("No gun")).with_note(pins)
    }
}

fn canonical_grip_line(reads: [Option<GripRead>; 2]) -> Line {
    let failed = reads
        .iter()
        .zip(Hand::BOTH)
        .find_map(|(read, hand)| match read {
            Some(GripRead::BonesUnresolved) => Some((hand, prose("bones missing"))),
            Some(GripRead::DegenerateBindPose) => Some((hand, prose("bad bind pose"))),
            Some(GripRead::Read) | None => None,
        });
    if let Some((hand, reason)) = failed {
        return Line::failed(text!(hand_letter(hand), " ", reason));
    }
    let read = reads.map(|read| read == Some(GripRead::Read));
    if read.contains(&true) {
        Line::ok(text!(hands_label(read), " read"))
    } else {
        Line::waiting(prose("No gun yet"))
    }
}

/// The animated deviation seen for each weight class, in degrees, the classes seen so far only.
/// A diagnostic for whether the bind-pose grip is the pose the aiming clips hold each kind of gun
/// in, which is unverified; it shows numbers rather than a verdict.
fn animated_deviation_line(by_class: [Option<f32>; 4]) -> Line {
    let mut detail = Text::default();
    for (class, deviation) in HeftClass::ALL.iter().zip(by_class) {
        let Some(deviation) = deviation else {
            continue;
        };
        let separator = if detail.is_empty() { "" } else { ", " };
        detail.push(text!(
            separator,
            prose(class.label()),
            " ",
            format!("{deviation:.0}°")
        ));
    }
    if detail.is_empty() {
        Line::waiting(prose("No gun yet"))
    } else {
        Line::ok(detail)
    }
}

fn callback_line(enabled: bool, status: CallbackStatus) -> Line {
    if !status.registered {
        return if enabled {
            Line::waiting(prose("Not registered"))
        } else {
            Line::off("")
        };
    }
    if status.firing_thread == 0 {
        Line::waiting(prose("Not fired"))
    } else if status.firing_thread == status.game_thread {
        Line::ok(prose("Game thread"))
    } else {
        Line::ok(text!("Thread ", status.firing_thread.to_string()))
    }
}

pub(super) fn left_gun_body() -> Line {
    gun_body(Hand::Left)
}

pub(super) fn right_gun_body() -> Line {
    gun_body(Hand::Right)
}

fn gun_body(hand: Hand) -> Line {
    let enabled = read_config(|c| guns::enabled(&c.controllers.physical_hands));
    let status = guns::status();
    gun_body_line(
        enabled,
        status.hands[hand.index()],
        status.contacts[hand.index()],
    )
}

/// One hand's gun body: whether it is in the world, its mass, its joints, and its contacts.
fn gun_body_line(enabled: bool, status: Option<GunBodyStatus>, contacts: u64) -> Line {
    if !enabled {
        return Line::off("");
    }
    let Some(gun) = status else {
        return Line::waiting(prose("No gun body"));
    };
    let joints = match (gun.holder_joint, gun.brace_joint) {
        (Some(_), Some(_)) => prose("held+braced"),
        (Some(_), None) => prose("held"),
        (None, _) => return Line::failed(prose("Not joined")),
    };
    if !gun.in_world {
        return Line::failed(prose("Not in world"));
    }
    Line::ok(text!(
        format!("#{} {:.1}", gun.body_id, gun.mass_kg),
        " kg, ",
        joints,
        ", ",
        contacts.to_string(),
        " hits"
    ))
    .with_note(format!(
        "mass from {}, {:.0} mm and {:.1} degrees off its seat",
        gun.mass_source.label(),
        gun.deviation_m * 1000.0,
        gun.deviation_rad.to_degrees()
    ))
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
        BodyPhase::Acting => Line::ok(text!("In world", id)),
        BodyPhase::Parked => Line::ok(text!("Parked", id)),
        BodyPhase::Absent if enabled => Line::waiting(prose("No body")),
        BodyPhase::Absent => Line::off(""),
    }
}

fn act_filter_line(status: ActFilterStatus) -> Line {
    let detail = text!(
        status.dropped.to_string(),
        " drop, ",
        status.replaced.to_string(),
        " swap"
    );
    if status.refused > 0 {
        Line::ok(text!(detail, ", ", status.refused.to_string(), " ref"))
            .with_note(format!("{} swaps refused", status.refused))
    } else {
        Line::ok(detail)
    }
}

fn steady_view_line(status: &AnchorStatus) -> Line {
    if status.weight >= 1.0 {
        Line::ok(prose("Engaged"))
    } else if status.weight > 0.0 {
        Line::ok(text!("Blending ", format!("{:.0}%", status.weight * 100.0)))
    } else if status.eligible && status.neutral.is_none() {
        if status.capture_progress > 0.0 {
            Line::waiting(text!(
                "Measuring ",
                format!("{:.1}", status.capture_progress),
                " s"
            ))
        } else {
            Line::waiting(prose("Stand still"))
        }
    } else if status.eligible {
        Line::waiting(prose("Ready"))
    } else {
        Line::ok(prose("Idle"))
    }
}

fn procedural_mode_line(mode: ProceduralMode, status: &ProceduralStatus, now: Instant) -> Line {
    let label = match mode {
        ProceduralMode::Off => return Line::off(""),
        ProceduralMode::PassThrough => prose("Test"),
        ProceduralMode::Full => prose("Full body"),
    };
    if let Some(error) = status.rig_error {
        return Line::failed(prose("Rig, see log")).with_note(error.to_string());
    }
    // Whether the gate has run in this mode, however long ago: it does not run while the game is
    // paused, which it is while the page is shown.
    match status.at {
        Some(at) if status.mode == mode => Line::ok(label).with_note(format!(
            "last gate {:.1} s ago",
            now.saturating_duration_since(at).as_secs_f32()
        )),
        _ => Line::waiting(text!(label, " idle")),
    }
}

fn pass_through_line(round_trip: Option<(f32, f32)>) -> Line {
    let Some((metres, degrees)) = round_trip else {
        return Line::waiting(prose("Not measured"));
    };
    let detail = text!(
        format!("{:.1}", metres * 1000.0),
        " mm, ",
        format!("{degrees:.2}"),
        " deg"
    );
    let (max_m, max_deg) = PASS_THROUGH_TOLERANCE;
    if metres <= max_m && degrees <= max_deg {
        Line::ok(detail)
    } else {
        Line::failed(detail)
    }
}

fn hand_over_line(case: Case) -> Line {
    let word = prose(match case {
        Case::NotInHeadset => "No headset",
        Case::SwitchedOff => "Switched off",
        Case::NotInGameplay => "Not in gameplay",
        Case::Sequence => "Sequence",
        Case::Dead => "Dead",
        Case::Ragdoll => "Ragdoll",
        Case::GettingUp => "Getting up",
        Case::Seated => "Seated",
        Case::StuntStanding => "On a roof",
        Case::Vehicle => "Vehicle",
        Case::Traversal => "Traversal",
        Case::LedgeClimb => "Ledge climb",
        Case::Swimming => "Swimming",
        Case::Wingsuit => "Wingsuit",
        Case::Parachute => "Parachute",
        Case::Flight => "Falling",
        Case::HitReaction => "Hit reaction",
        Case::HandClimb => "Hand climb",
        Case::Jump => "Jump",
        Case::AuthoredMotion => "Authored motion",
        Case::NoMovementTask => "No move task",
        Case::OnFoot => "On foot",
    });
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
        return Line::waiting(prose("Not on foot"));
    }
    match lower.pose {
        LowerPose::Stand => Line::ok(prose("Stand")),
        LowerPose::Crouch => Line::ok(prose("Crouch")),
        LowerPose::Kneel => Line::ok(text!(
            "Kneel ",
            lower.kneel_side.map_or("?", side_letter),
            ", ",
            format!("{:.0}%", lower.kneel_weight * 100.0)
        )),
        LowerPose::Sit => Line::ok(text!(
            "Sit ",
            prose(match lower.sit_legs {
                Some(SitLegs::Crossed) => "crossed",
                Some(SitLegs::KneesUp) => "knees up",
                None => "?",
            }),
            ", ",
            format!("{:.0}%", lower.sit_weight * 100.0)
        )),
    }
}

/// What the lower body's pose is picked from: the hips' height the head asks for against the
/// squat's floor, the crouch's depth, how long the head has been low and still enough to sit, and
/// whether the body is told to move, the player plays seated, or the stick stood the body up and a
/// sit waits for the head to rise.
fn lower_inputs_line(lower: &LowerStatus, crouch: f32) -> Line {
    let moving = prose(if lower.moving { ", moving" } else { "" });
    let seated = prose(if lower.standing_play { "" } else { ", seated" });
    let disarmed = prose(if lower.sit_armed { "" } else { ", rise to sit" });
    Line::ok(text!(
        "Hips ",
        format!("{:.2}", lower.wanted_hips_m),
        " m, floor ",
        format!("{:.2}", lower.floor_m),
        " m, crouch ",
        format!("{crouch:.2}"),
        ", still ",
        format!("{:.1}", lower.still_s),
        " s",
        moving,
        seated,
        disarmed
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
    let detail = text!(format!("{}/{}", tally.hits, tally.casts), " hit");
    if tally.errors > 0 {
        Line::failed(text!(tally.errors.to_string(), " err, ", detail))
    } else if tally.casts == 0 {
        Line::waiting(prose("No casts"))
    } else {
        Line::ok(detail)
    }
}

/// The hands a per-hand flag is set for: `L`, `R`, `L+R`, or `NONE`.
fn hands_label(set: [bool; 2]) -> Text {
    match set {
        [true, true] => Text::from("L+R"),
        [true, false] => Text::from("L"),
        [false, true] => Text::from("R"),
        [false, false] => prose("none"),
    }
}

fn hand_letter(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "L",
        Hand::Right => "R",
    }
}

/// The collider view's row: off with the key that shows it, waiting for the step counter or a
/// read, or the bodies drawn and those the budget left out.
fn collider_view_line(status: &ColliderViewStatus) -> Line {
    if !status.enabled {
        return Line::off(prose("Ctrl+Numpad 0 shows it"));
    }
    if let Some(error) = &status.last_error
        && status.reads == 0
    {
        return Line::failed(prose("See log")).with_note(error.clone());
    }
    if !status.registered || status.reads == 0 {
        return Line::waiting(prose("No physics step yet"));
    }
    let detail = text!(
        status.drawn.bodies.to_string(),
        " of ",
        status.in_range.to_string(),
        " bodies, ",
        status.primitives.to_string(),
        " primitives"
    );
    if status.drawn.dropped > 0 {
        Line::ok(text!(
            detail,
            ", ",
            status.drawn.dropped.to_string(),
            " over budget"
        ))
    } else {
        Line::ok(detail)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec3;

    use super::*;
    use crate::controllers::{
        locomotion::view::Neutral, physical_hands::collider_view::draw::Drawn,
    };

    #[test]
    fn the_collider_view_row_says_how_to_show_it_and_what_it_draws() {
        let off = ColliderViewStatus::default();
        assert_eq!(
            collider_view_line(&off).text(),
            "OFF, CTRL+NUMPAD 0 SHOWS IT"
        );
        let waiting = ColliderViewStatus {
            enabled: true,
            registered: true,
            ..ColliderViewStatus::default()
        };
        assert_eq!(collider_view_line(&waiting).health, Health::Waiting);
        let drawing = ColliderViewStatus {
            enabled: true,
            registered: true,
            reads: 3,
            in_range: 40,
            primitives: 1200,
            drawn: Drawn {
                bodies: 30,
                dropped: 10,
                ..Drawn::default()
            },
            ..ColliderViewStatus::default()
        };
        assert_eq!(
            collider_view_line(&drawing).text(),
            "OK, 30 OF 40 BODIES, 1200 PRIMITIVES, 10 OVER BUDGET"
        );
    }

    #[test]
    fn a_gun_body_shows_its_mass_joints_and_hits() {
        let gun = GunBodyStatus {
            weapon: 1,
            body_id: 812,
            in_world: true,
            mass_kg: 3.96,
            mass_source: guns::MassSource::Body,
            holder_joint: Some(4),
            brace_joint: Some(5),
            deviation_m: 0.004,
            deviation_rad: 0.01,
            speed_mps: 0.0,
        };
        assert_eq!(
            gun_body_line(true, Some(gun), 3).text(),
            "OK, #812 4.0 KG, HELD+BRACED, 3 HITS"
        );
        assert_eq!(
            gun_body_line(true, Some(gun), 3).display_detail,
            "#812 4.0 kg, held+braced, 3 hits"
        );
        let one_hand = GunBodyStatus {
            brace_joint: None,
            ..gun
        };
        assert_eq!(
            gun_body_line(true, Some(one_hand), 0).text(),
            "OK, #812 4.0 KG, HELD, 0 HITS"
        );
        let loose = GunBodyStatus {
            holder_joint: None,
            ..gun
        };
        assert_eq!(
            gun_body_line(true, Some(loose), 0).text(),
            "FAILED, NOT JOINED"
        );
        let out = GunBodyStatus {
            in_world: false,
            ..gun
        };
        assert_eq!(
            gun_body_line(true, Some(out), 0).text(),
            "FAILED, NOT IN WORLD"
        );
        assert_eq!(gun_body_line(true, None, 0).text(), "WAITING, NO GUN BODY");
        assert_eq!(gun_body_line(false, Some(gun), 0).text(), "OFF");
    }

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
    fn the_twist_bones_line_counts_them_and_names_the_twist() {
        let mut status = TwistStatus::default();
        assert_eq!(twist_line(&status).text(), "WAITING, NOT READ");
        status.bones = Some((0, 0));
        assert_eq!(twist_line(&status).text(), "FAILED, NONE FOUND");
        status.bones = Some((4, 4));
        status.largest_twist = 1.0;
        assert_eq!(twist_line(&status).text(), "WAITING, 4 ON 4, 57°");
        status.wrote = true;
        assert_eq!(twist_line(&status).text(), "OK, 4 ON 4, 57°");
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
