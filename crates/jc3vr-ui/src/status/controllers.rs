//! The status rows for the controller systems: the gun in the hand, the hand bodies, the
//! locomotion, and the procedural body.

use jc3vr_body::controllers::{
    body::{
        procedural,
        procedural::{ProceduralStatus, authority::Case, twist::TwistStatus},
    },
    hands::{grip, grip::GripRead},
    held::{
        guns as held_guns,
        guns::{GunBodyStatus, HeftClass},
    },
    locomotion,
    locomotion::{acts, acts::ActFilterStatus, anchor, anchor::AnchorStatus},
};
use jc3vr_core::published::Stamp;
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::{
    physics,
    physics::{BodyPhase, CallbackStatus, HandBodyStatus},
};
use jc3vr_weapons::controllers::weapons::grip_watch;
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Health, Line, Row};

pub(crate) const RIGID_GRIP: Row = Row::new("Rigid grip", rigid_grip);
pub(crate) const CANONICAL_GRIP: Row = Row::new("Canonical grip", canonical_grip);
pub(crate) const FLIP_WATCH: Row = Row::new("Flip watch", flip_watch);
pub(crate) const ANIMATED_DEVIATION: Row = Row::new("Animated grip offset", animated_deviation);
pub(crate) const CALLBACK: Row = Row::new("Callback", callback);
pub(crate) const LEFT_BODY: Row = Row::new("Left body", left_body);
pub(crate) const RIGHT_BODY: Row = Row::new("Right body", right_body);
pub(crate) const BODY_ERRORS: Row = Row::new("Body errors", body_errors);
pub(crate) const CONTACT_EVENTS: Row = Row::new("Contact events", contact_events);
pub(crate) const LEFT_GUN_BODY: Row = Row::new("Left gun body", left_gun_body);
pub(crate) const RIGHT_GUN_BODY: Row = Row::new("Right gun body", right_gun_body);
pub(crate) const STICK_VELOCITY: Row = Row::new("Stick velocity", stick_velocity);
pub(crate) const ACT_FILTER: Row = Row::new("Act filter", act_filter);
pub(crate) const ACT_WATCHDOG: Row = Row::new("Act watchdog", act_watchdog);
pub(crate) const STEADY_VIEW: Row = Row::new("Steady view", steady_view);
pub(crate) const NEUTRAL_HEAD: Row = Row::new("Neutral head", neutral_head);
pub(crate) const PROCEDURAL_MODE: Row = Row::new("Mode", procedural_mode);
pub(crate) const HAND_OVER: Row = Row::new("Hand-over case", hand_over);
pub(crate) const TWIST_BONES: Row = Row::new("Twist bones", twist_bones);

fn rigid_grip() -> Line {
    let pinned = grip_watch::status().pinned;
    let pins = grip::status().pins;
    rigid_grip_line(pinned, pins)
}

fn canonical_grip() -> Line {
    canonical_grip_line(grip::grip_reads())
}

fn flip_watch() -> Line {
    let watch = grip_watch::status();
    let flips: u64 = watch.flips.iter().sum();
    let sweeps: u64 = watch.sweeps.iter().sum();
    let detail = format_smolstr!("{} flips, {} sweeps", flips, sweeps);
    if flips > 0 {
        Line::failed(detail)
    } else {
        Line::ok(detail)
    }
}

fn animated_deviation() -> Line {
    animated_deviation_line(grip_watch::status().by_class)
}

fn callback() -> Line {
    callback_line(physics::callback_status())
}

fn left_body() -> Line {
    body(Hand::Left)
}

fn right_body() -> Line {
    body(Hand::Right)
}

fn body_errors() -> Line {
    let status = physics::status();
    let made = format_smolstr!("{} made, {} gone", status.created, status.destroyed);
    match status.last_error {
        Some(error) => Line::failed("See log").with_note(error),
        None => Line::ok(made),
    }
}

fn contact_events() -> Line {
    let listening = physics::callback_status().listening;
    let status = physics::status();
    let hands = hands_label(listening);
    let detail = format_smolstr!("{}, {} contacts", hands, status.contacts);
    if status.dropped_contacts > 0 {
        Line::failed(format_smolstr!("{} dropped", status.dropped_contacts))
    } else if listening.contains(&true) {
        Line::ok(detail)
    } else {
        Line::waiting(detail)
    }
}

fn stick_velocity() -> Line {
    if !CONTROLLERS_CONFIG.query(|c| c.locomotion.stick.enabled) {
        return Line::off("");
    }
    // The last tick's, not this one's: the movement task does not run while the game is paused,
    // which it is while the page is shown.
    let status = locomotion::status();
    match status.velocity {
        Some(velocity) => Line::ok(format_smolstr!("{:.1} m/s", velocity.length())),
        None if status.at.is_some() => Line::waiting("Not driving"),
        None => Line::waiting("Not on foot yet"),
    }
}

fn act_filter() -> Line {
    act_filter_line(acts::status())
}

fn act_watchdog() -> Line {
    let status = acts::status();
    let trips = format_smolstr!("{} trips", status.releases);
    if status.released {
        Line::waiting(format_smolstr!("Open, {}", trips))
    } else {
        Line::ok(trips)
    }
}

fn steady_view() -> Line {
    steady_view_line(&anchor::status())
}

fn neutral_head() -> Line {
    match anchor::status().neutral {
        Some(neutral) => Line::ok(format_smolstr!("Eyes {:.2} m", neutral.eyes.y))
            .with_note(format!("head {:.2} m", neutral.head.y)),
        None => Line::waiting("Not measured"),
    }
}

fn procedural_mode() -> Line {
    procedural_mode_line(&procedural::published_status(), Stamp::now())
}

fn hand_over() -> Line {
    hand_over_line(procedural::published_status().case)
}

fn twist_bones() -> Line {
    twist_line(&procedural::twist::status())
}

/// The twist bones found and the largest twist they carried on the last pass.
fn twist_line(status: &TwistStatus) -> Line {
    match status.bones {
        None => Line::waiting("Not read"),
        Some((0, _)) => Line::failed("None found"),
        Some((bones, segments)) => {
            let detail = format_smolstr!(
                "{} on {}, {:.0}°",
                bones,
                segments,
                status.largest_twist.to_degrees()
            );
            if status.wrote {
                Line::ok(detail)
            } else {
                Line::waiting(detail)
            }
        }
    }
}

fn rigid_grip_line(pinned: [Option<bool>; 2], pins: u64) -> Line {
    let pins = format_smolstr!("{} pins", pins);
    let hands = |want: bool| pinned.map(|pinned| pinned == Some(want));
    if hands(true).contains(&true) {
        Line::ok(format_smolstr!("{}, {}", hands_label(hands(true)), pins))
    } else if hands(false).contains(&true) {
        Line::waiting(format_smolstr!("{} not pinned", hands_label(hands(false))))
    } else {
        Line::waiting("No gun").with_note(pins)
    }
}

fn canonical_grip_line(reads: [Option<GripRead>; 2]) -> Line {
    let failed = reads
        .iter()
        .zip(Hand::BOTH)
        .find_map(|(read, hand)| match read {
            Some(GripRead::BonesUnresolved) => Some((hand, SmolStr::new_static("bones missing"))),
            Some(GripRead::DegenerateBindPose) => {
                Some((hand, SmolStr::new_static("bad bind pose")))
            }
            Some(GripRead::Read) | None => None,
        });
    if let Some((hand, reason)) = failed {
        return Line::failed(format_smolstr!("{} {}", hand_letter(hand), reason));
    }
    let read = reads.map(|read| read == Some(GripRead::Read));
    if read.contains(&true) {
        Line::ok(format_smolstr!("{} read", hands_label(read)))
    } else {
        Line::waiting("No gun yet")
    }
}

/// The animated deviation seen for each weight class, in degrees, the classes seen so far only.
/// A diagnostic for whether the bind-pose grip is the pose the aiming clips hold each kind of gun
/// in, which is unverified; it shows numbers rather than a verdict.
fn animated_deviation_line(by_class: [Option<f32>; 4]) -> Line {
    let seen: Vec<String> = HeftClass::ALL
        .iter()
        .zip(by_class)
        .filter_map(|(class, deviation)| Some(format!("{} {:.0}°", class.label(), deviation?)))
        .collect();
    let detail = seen.join(", ");
    if detail.is_empty() {
        Line::waiting("No gun yet")
    } else {
        Line::ok(detail)
    }
}

fn callback_line(status: CallbackStatus) -> Line {
    if !status.registered {
        return Line::waiting("Not registered");
    }
    if status.firing_thread == 0 {
        Line::waiting("Not fired")
    } else if status.firing_thread == status.game_thread {
        Line::ok(format_smolstr!("Game thread, {} fires", status.fires))
    } else {
        Line::ok(format_smolstr!(
            "Thread {}, {} fires",
            status.firing_thread,
            status.fires
        ))
    }
}

fn left_gun_body() -> Line {
    gun_body(Hand::Left)
}

fn right_gun_body() -> Line {
    gun_body(Hand::Right)
}

fn gun_body(hand: Hand) -> Line {
    let status = held_guns::status();
    gun_body_line(status.hands[hand.index()], status.contacts[hand.index()])
}

/// One hand's gun body: whether it is in the world, its mass, its joints, and its contacts.
fn gun_body_line(status: Option<GunBodyStatus>, contacts: u64) -> Line {
    let Some(gun) = status else {
        return Line::waiting("No gun body");
    };
    let joints = match (gun.holder_joint, gun.brace_joint) {
        (Some(_), Some(_)) => SmolStr::new_static("held+braced"),
        (Some(_), None) => SmolStr::new_static("held"),
        (None, _) => return Line::failed("Not joined"),
    };
    if !gun.in_world {
        return Line::failed("Not in world");
    }
    Line::ok(format_smolstr!(
        "#{} {:.1} kg, {}, {} hits",
        gun.body_id,
        gun.mass_kg,
        joints,
        contacts
    ))
    .with_note(format!(
        "mass from {}, {:.0} mm and {:.1} degrees off its seat",
        gun.mass_source.label(),
        gun.deviation_m * 1000.0,
        gun.deviation_rad.to_degrees()
    ))
}

fn body(hand: Hand) -> Line {
    body_line(&physics::status().hands[hand.index()])
}

fn body_line(status: &HandBodyStatus) -> Line {
    let id = status
        .body_id
        .map_or(SmolStr::default(), |id| format_smolstr!(" #{id}"));
    match status.phase {
        BodyPhase::Acting => Line::ok(format_smolstr!("In world{}", id)),
        BodyPhase::Parked => Line::ok(format_smolstr!("Parked{}", id)),
        BodyPhase::Absent => Line::waiting("No body"),
    }
}

fn act_filter_line(status: ActFilterStatus) -> Line {
    let detail = format_smolstr!("{} drop, {} swap", status.dropped, status.replaced);
    if status.refused > 0 {
        Line::ok(format_smolstr!("{}, {} ref", detail, status.refused))
            .with_note(format!("{} swaps refused", status.refused))
    } else {
        Line::ok(detail)
    }
}

fn steady_view_line(status: &AnchorStatus) -> Line {
    if status.weight >= 1.0 {
        Line::ok("Engaged")
    } else if status.weight > 0.0 {
        Line::ok(format_smolstr!("Blending {:.0}%", status.weight * 100.0))
    } else if status.eligible && status.neutral.is_none() {
        if status.capture_progress > 0.0 {
            Line::waiting(format_smolstr!(
                "Measuring {:.1} s",
                status.capture_progress
            ))
        } else {
            Line::waiting("Stand still")
        }
    } else if status.eligible {
        Line::waiting("Ready")
    } else {
        Line::ok("Idle")
    }
}

fn procedural_mode_line(status: &ProceduralStatus, now: Stamp) -> Line {
    let label = SmolStr::new_static("Full body");
    if let Some(error) = status.rig_error {
        return Line::failed("Rig, see log").with_note(error.to_string());
    }
    // Whether the gate has run, however long ago: it does not run while the game is paused, which
    // it is while the page is shown.
    match status.at {
        Some(at) => Line::ok(label).with_note(format!(
            "last gate {:.1} s ago",
            now.saturating_duration_since(at).as_secs_f32()
        )),
        _ => Line::waiting(format_smolstr!("{} idle", label)),
    }
}

fn hand_over_line(case: Case) -> Line {
    let word = format_smolstr!(
        "{}",
        match case {
            Case::NotInHeadset => "No headset",
            Case::NotInGameplay => "Not in gameplay",
            Case::Sequence => "Sequence",
            Case::Dead => "Dead",
            Case::Ragdoll => "Ragdoll",
            Case::GettingUp => "Getting up",
            Case::Seated => "Seated",
            Case::StuntStanding => "On a roof",
            Case::StuntHanging => "On a helicopter rail",
            Case::Vehicle => "Vehicle",
            Case::GrappleAttach => "Wall attach",
            Case::Traversal => "Traversal",
            Case::GrappleLock => "Line locked",
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
        }
    );
    // The animation has the body in every case but on foot, which is what the hand-over is for.
    let health = match case {
        Case::OnFoot => Health::Ok,
        _ => Health::Waiting,
    };
    Line::new(health, word).with_note(case.label())
}

/// The hands a per-hand flag is set for: `L`, `R`, `L+R`, or `NONE`.
fn hands_label(set: [bool; 2]) -> SmolStr {
    match set {
        [true, true] => SmolStr::new("L+R"),
        [true, false] => SmolStr::new("L"),
        [false, true] => SmolStr::new("R"),
        [false, false] => SmolStr::new_static("none"),
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
    use jc3vr_body::controllers::locomotion::view::Neutral;

    use super::*;

    #[test]
    fn a_gun_body_shows_its_mass_joints_and_hits() {
        let gun = GunBodyStatus {
            weapon: 1,
            body_id: 812,
            in_world: true,
            mass_kg: 3.96,
            mass_source: held_guns::MassSource::Body,
            holder_joint: Some(4),
            brace_joint: Some(5),
            deviation_m: 0.004,
            deviation_rad: 0.01,
            speed_mps: 0.0,
        };
        assert_eq!(
            gun_body_line(Some(gun), 3).text(),
            "Ok, #812 4.0 kg, held+braced, 3 hits"
        );
        assert_eq!(
            gun_body_line(Some(gun), 3).detail,
            "#812 4.0 kg, held+braced, 3 hits"
        );
        let one_hand = GunBodyStatus {
            brace_joint: None,
            ..gun
        };
        assert_eq!(
            gun_body_line(Some(one_hand), 0).text(),
            "Ok, #812 4.0 kg, held, 0 hits"
        );
        let loose = GunBodyStatus {
            holder_joint: None,
            ..gun
        };
        assert_eq!(gun_body_line(Some(loose), 0).text(), "Failed, Not joined");
        let out = GunBodyStatus {
            in_world: false,
            ..gun
        };
        assert_eq!(gun_body_line(Some(out), 0).text(), "Failed, Not in world");
        assert_eq!(gun_body_line(None, 0).text(), "Waiting, No gun body");
    }

    #[test]
    fn the_animated_deviation_lists_the_classes_seen() {
        assert_eq!(
            animated_deviation_line([Some(12.4), None, Some(31.6), None]).text(),
            "Ok, sidearm 12°, rifle 32°"
        );
        assert_eq!(
            animated_deviation_line([None; 4]).text(),
            "Waiting, No gun yet"
        );
    }

    #[test]
    fn the_rigid_grip_names_the_pinned_hands() {
        assert_eq!(
            rigid_grip_line([None, Some(true)], 40).text(),
            "Ok, R, 40 pins"
        );
        assert_eq!(
            rigid_grip_line([Some(false), None], 0).text(),
            "Waiting, L not pinned"
        );
        assert_eq!(rigid_grip_line([None, None], 3).text(), "Waiting, No gun");
    }

    #[test]
    fn a_failed_grip_read_wins_over_a_good_one() {
        assert_eq!(
            canonical_grip_line([Some(GripRead::Read), Some(GripRead::BonesUnresolved)]).text(),
            "Failed, R bones missing"
        );
        assert_eq!(
            canonical_grip_line([Some(GripRead::Read), None]).text(),
            "Ok, L read"
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
        assert_eq!(callback_line(fired(7)).text(), "Ok, Game thread, 0 fires");
        assert_eq!(callback_line(fired(9)).text(), "Ok, Thread 9, 0 fires");
        assert_eq!(callback_line(fired(0)).health, Health::Waiting);
    }

    #[test]
    fn bodies_read_their_phase() {
        let acting = HandBodyStatus {
            phase: BodyPhase::Acting,
            body_id: Some(12),
            ..HandBodyStatus::default()
        };
        assert_eq!(body_line(&acting).text(), "Ok, In world #12");
        assert_eq!(
            body_line(&HandBodyStatus::default()).health,
            Health::Waiting
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
        assert_eq!(text(status(1.0, true, true, 0.0)), "Ok, Engaged");
        assert_eq!(text(status(0.5, true, true, 0.0)), "Ok, Blending 50%");
        assert_eq!(
            text(status(0.0, true, false, 1.24)),
            "Waiting, Measuring 1.2 s"
        );
        assert_eq!(text(status(0.0, true, false, 0.0)), "Waiting, Stand still");
        assert_eq!(text(status(0.0, false, true, 0.0)), "Ok, Idle");
    }

    #[test]
    fn the_twist_bones_line_counts_them_and_names_the_twist() {
        let mut status = TwistStatus::default();
        assert_eq!(twist_line(&status).text(), "Waiting, Not read");
        status.bones = Some((0, 0));
        assert_eq!(twist_line(&status).text(), "Failed, None found");
        status.bones = Some((4, 4));
        status.largest_twist = 1.0;
        assert_eq!(twist_line(&status).text(), "Waiting, 4 on 4, 57°");
        status.wrote = true;
        assert_eq!(twist_line(&status).text(), "Ok, 4 on 4, 57°");
    }

    #[test]
    fn the_procedural_mode_must_have_run_in_it() {
        let now = Stamp::now();
        let ran = ProceduralStatus {
            at: Some(now),
            ..ProceduralStatus::default()
        };
        assert_eq!(procedural_mode_line(&ran, now).text(), "Ok, Full body");
        let later = now + Duration::from_secs(60);
        assert_eq!(
            procedural_mode_line(&ran, later).note.as_deref(),
            Some("last gate 60.0 s ago")
        );
        assert_eq!(
            procedural_mode_line(&ProceduralStatus::default(), now).health,
            Health::Waiting
        );
    }

    #[test]
    fn the_hand_over_is_live_on_foot() {
        assert_eq!(hand_over_line(Case::OnFoot).text(), "Ok, On foot");
        let climb = hand_over_line(Case::LedgeClimb);
        assert_eq!(climb.text(), "Waiting, Ledge climb");
        assert_eq!(climb.note.as_deref(), Some(Case::LedgeClimb.label()));
    }
}
