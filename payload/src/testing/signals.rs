//! The signals a scenario can check: typed reads of the snapshots the mod's systems publish for
//! the `VR: STATUS` page and the debug UI, and of the page's rows themselves.
//!
//! Every read takes the publishing module's leaf slot or atomics only, as the status page's rows
//! do, so the runner may read them from the game thread at the top of the frame.

use std::fmt;

use crate::{
    controllers::{
        body::procedural::{self, authority::Case},
        frame::{self as controller_frame, Hand},
        hands::grip,
        locomotion,
        menu::status::{self as status_page, Health},
        mode::{self, Mode, OnFootVariant},
        physical_hands::{
            climb,
            physics::{self, BodyPhase},
        },
        scheme,
        weapons::{self, c4, grenade, grip_watch, hold::second, reload::loose},
    },
    frame_timing,
    testing::replay,
};

/// A value a probe reads on one frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Bool(bool),
    Number(f64),
    /// The signal has no value on this frame: not measured yet, or nothing to measure.
    Missing,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Missing => write!(f, "missing"),
        }
    }
}

/// What kind of value a signal reads.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SignalKind {
    /// A yes or no.
    Flag,
    /// A count since the payload loaded, which only grows; a scenario usually checks how far it
    /// grew during the replay.
    Counter,
    /// A measurement.
    Measure,
}

macro_rules! signals {
    ($($variant:ident => $name:literal, $kind:ident, $doc:literal;)*) => {
        /// A published signal a scenario can check.
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub(crate) enum Signal {
            $(#[doc = $doc] $variant,)*
        }

        impl Signal {
            /// Every signal, in the order the documentation lists them.
            pub(crate) const ALL: &[Signal] = &[$(Signal::$variant,)*];

            /// The name a scenario file uses.
            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Signal::$variant => $name,)*
                }
            }

            pub(crate) const fn kind(self) -> SignalKind {
                match self {
                    $(Signal::$variant => SignalKind::$kind,)*
                }
            }

            /// What the signal reads.
            pub(crate) const fn description(self) -> &'static str {
                match self {
                    $(Signal::$variant => $doc,)*
                }
            }
        }
    };
}

signals! {
    ReplayDriving => "replay.driving", Flag, "A replay is publishing its frames.";
    VrRendering => "vr.rendering", Flag, "A VR frame is rendering to the headset.";
    Recenters => "vr.recenters", Counter, "Recenters of the cockpit baseline.";
    FrameTimeP95Ms => "frame.p95_ms", Measure, "The 95th percentile of the recent frame intervals, milliseconds.";
    ControllerFrameFresh => "controllers.frame_fresh", Flag, "A controller frame is published and fresh.";
    ControllerScheme => "controllers.scheme", Flag, "The controller scheme is selected rather than the gamepad.";
    OnFoot => "controllers.on_foot", Flag, "The mode detector has the player on foot on the ground.";
    Swimming => "controllers.swimming", Flag, "The mode detector has the player swimming.";
    InVehicle => "controllers.in_vehicle", Flag, "The mode detector has the player in a vehicle seat.";
    Airborne => "controllers.airborne", Flag, "The mode detector has the player falling, in the wingsuit, or under the parachute.";
    HandBodiesRegistered => "hand_bodies.registered", Flag, "The hand bodies' pre-simulation callback is registered.";
    HandBodiesOnGameThread => "hand_bodies.on_game_thread", Flag, "The callback has fired, on the thread that registered it.";
    HandBodiesFires => "hand_bodies.fires", Counter, "Times the callback fired.";
    LeftBodyInWorld => "hand_bodies.left_in_world", Flag, "The left hand's body is acting in the world.";
    RightBodyInWorld => "hand_bodies.right_in_world", Flag, "The right hand's body is acting in the world.";
    HandBodiesCreated => "hand_bodies.created", Counter, "Hand bodies made.";
    HandBodiesContacts => "hand_bodies.contacts", Counter, "Contacts the hand bodies started.";
    HandBodiesDroppedContacts => "hand_bodies.dropped_contacts", Counter, "Contacts dropped because the queue was full.";
    HandBlows => "hand_bodies.blows", Counter, "Blows the hands dealt to characters.";
    HandBodiesError => "hand_bodies.error", Flag, "The hand bodies hold an error.";
    Climbing => "climb.climbing", Flag, "A hand grips the world.";
    Mantling => "climb.mantling", Flag, "The mantle is carrying the body over a ledge.";
    ClimbGrips => "climb.grips", Counter, "Grips of the world.";
    ClimbReleases => "climb.releases", Counter, "Grips let go.";
    ClimbMantles => "climb.mantles", Counter, "Mantles completed.";
    ClimbDrops => "climb.drops", Counter, "Grips ended against the player's wish.";
    ClimbError => "climb.error", Flag, "The climbing holds an error.";
    ProceduralRigError => "procedural.rig_error", Flag, "The procedural body could not rig the skeleton.";
    PassThroughErrorM => "procedural.pass_through_error_m", Measure, "The pass-through's largest read-back error, metres.";
    PassThroughErrorDeg => "procedural.pass_through_error_deg", Measure, "The pass-through's largest read-back error, degrees.";
    ProceduralOnFoot => "procedural.on_foot", Flag, "The hand-over table's case is on foot.";
    StanceMeasured => "procedural.stance_measured", Flag, "The neutral stance has been measured.";
    Wielded => "weapons.wielded", Flag, "A weapon is wielded.";
    WeaponInLeftHand => "weapons.in_left_hand", Flag, "The wielded weapon's first gun rides the left hand.";
    WeaponInRightHand => "weapons.in_right_hand", Flag, "The wielded weapon's first gun rides the right hand.";
    TwoHanded => "weapons.two_handed", Flag, "The wielded weapon is held in both hands' reach.";
    SecondInLeftHand => "weapons.second_in_left_hand", Flag, "A second weapon is held in the left hand.";
    SecondInRightHand => "weapons.second_in_right_hand", Flag, "A second weapon is held in the right hand.";
    WeaponGrabs => "weapons.grabs", Counter, "Grabs the weapon handling saw.";
    GrenadePins => "grenade.pins_pulled", Counter, "Pins pulled from grenades taken from the pouch.";
    GrenadesThrown => "grenade.thrown", Counter, "Grenades thrown from the pouch.";
    GrenadesCaught => "grenade.caught", Counter, "Live grenades caught.";
    GrenadesThrownOn => "grenade.thrown_on", Counter, "Caught grenades thrown on.";
    C4Thrown => "c4.thrown", Counter, "Charges thrown or pressed on from the pouch.";
    C4Stuck => "c4.stuck", Counter, "Charges pressed onto a surface by hand.";
    MagazinesDropped => "magazines.dropped", Counter, "Magazine props dropped into the world and noted.";
    MagazinesPickedUp => "magazines.picked_up", Counter, "Magazines picked up from the world.";
    MagazinesSeated => "magazines.seated", Counter, "Magazines from the world seated in a gun.";
    GripFlips => "grip.flips", Counter, "Snaps of a gun on the wrist.";
    GripPins => "grip.pins", Counter, "Attach bones pinned.";
    StickSpeed => "locomotion.stick_speed_mps", Measure, "The speed the stick drives the body at, metres per second.";
}

impl Signal {
    /// The signal named `name` in a scenario file.
    pub(crate) fn parse(name: &str) -> Option<Signal> {
        Signal::ALL.iter().copied().find(|s| s.name() == name)
    }

    /// Read the signal now.
    pub(crate) fn read(self) -> Value {
        let flag = Value::Bool;
        let count = |n: u64| Value::Number(n as f64);
        match self {
            Signal::ReplayDriving => flag(replay::is_driving()),
            Signal::VrRendering => flag(crate::vr::render_params(0).is_some()),
            Signal::Recenters => count(u64::from(crate::vr::recenter_count())),
            Signal::FrameTimeP95Ms => frame_timing::frame_percentiles()
                .map_or(Value::Missing, |p| Value::Number(f64::from(p.p95_ms))),
            Signal::ControllerFrameFresh => flag(controller_frame::latest().is_some()),
            Signal::ControllerScheme => flag(scheme::is_controllers()),
            Signal::OnFoot => flag(mode::current().mode == Mode::OnFoot(OnFootVariant::Ground)),
            Signal::Swimming => flag(mode::current().mode == Mode::OnFoot(OnFootVariant::Swimming)),
            Signal::InVehicle => flag(matches!(mode::current().mode, Mode::Vehicle(_))),
            Signal::Airborne => flag(matches!(mode::current().mode, Mode::Airborne(_))),
            Signal::HandBodiesRegistered => flag(physics::callback_status().registered),
            Signal::HandBodiesOnGameThread => {
                let status = physics::callback_status();
                flag(status.firing_thread != 0 && status.firing_thread == status.game_thread)
            }
            Signal::HandBodiesFires => count(physics::callback_status().fires),
            Signal::LeftBodyInWorld => flag(physics::status().hands[0].phase == BodyPhase::Acting),
            Signal::RightBodyInWorld => flag(physics::status().hands[1].phase == BodyPhase::Acting),
            Signal::HandBodiesCreated => count(physics::status().created),
            Signal::HandBodiesContacts => count(physics::status().contacts),
            Signal::HandBodiesDroppedContacts => count(physics::status().dropped_contacts),
            Signal::HandBlows => count(physics::status().blows),
            Signal::HandBodiesError => flag(physics::status().last_error.is_some()),
            Signal::Climbing => flag(climb::status().climbing),
            Signal::Mantling => flag(climb::status().mantling),
            Signal::ClimbGrips => count(climb::status().grips),
            Signal::ClimbReleases => count(climb::status().releases),
            Signal::ClimbMantles => count(climb::status().mantles),
            Signal::ClimbDrops => count(climb::status().drops),
            Signal::ClimbError => flag(climb::status().last_error.is_some()),
            Signal::ProceduralRigError => flag(procedural::published_status().rig_error.is_some()),
            Signal::PassThroughErrorM => procedural::published_status()
                .round_trip
                .map_or(Value::Missing, |(m, _)| Value::Number(f64::from(m))),
            Signal::PassThroughErrorDeg => procedural::published_status()
                .round_trip
                .map_or(Value::Missing, |(_, deg)| Value::Number(f64::from(deg))),
            Signal::ProceduralOnFoot => flag(procedural::published_status().case == Case::OnFoot),
            Signal::StanceMeasured => flag(procedural::published_status().stance_measured),
            Signal::Wielded => flag(weapons::status().wielded.is_some()),
            Signal::WeaponInLeftHand => flag(wielded_holder() == Some(Hand::Left)),
            Signal::WeaponInRightHand => flag(wielded_holder() == Some(Hand::Right)),
            Signal::TwoHanded => flag(
                weapons::status()
                    .wielded
                    .is_some_and(|w| w.weapon.two_handed),
            ),
            Signal::SecondInLeftHand => flag(second::hand() == Some(Hand::Left)),
            Signal::SecondInRightHand => flag(second::hand() == Some(Hand::Right)),
            Signal::WeaponGrabs => count(weapons::status().grabs),
            Signal::GrenadePins => count(grenade::status().pins_pulled),
            Signal::GrenadesThrown => count(grenade::status().thrown),
            Signal::GrenadesCaught => count(grenade::status().catch.caught),
            Signal::GrenadesThrownOn => count(grenade::status().catch.thrown),
            Signal::C4Thrown => count(c4::status().thrown),
            Signal::C4Stuck => count(c4::status().stuck),
            Signal::MagazinesDropped => count(loose::counts().dropped),
            Signal::MagazinesPickedUp => count(loose::counts().picked_up),
            Signal::MagazinesSeated => count(loose::counts().seated),
            Signal::GripFlips => count(grip_watch::status().flips.iter().sum()),
            Signal::GripPins => count(grip::status().pins),
            Signal::StickSpeed => locomotion::status()
                .velocity
                .map_or(Value::Missing, |v| Value::Number(f64::from(v.length()))),
        }
    }
}

/// A `VR: STATUS` row's line, by its label, for a scenario's status-row probe.
pub(crate) fn status_row(label: &str) -> Option<status_page::Line> {
    status_page::read_row(label)
}

/// The `VR: STATUS` row labelled `label`, matched without regard to case, as the page spells it.
pub(crate) fn status_row_label(label: &str) -> Option<&'static str> {
    status_page::row_labels().find(|row| row.eq_ignore_ascii_case(label.trim()))
}

/// The health word a scenario file names, in the page's own spelling or lowercase.
pub(crate) fn parse_health(word: &str) -> Option<Health> {
    [Health::Ok, Health::Off, Health::Waiting, Health::Failed]
        .into_iter()
        .find(|h| h.word().eq_ignore_ascii_case(word))
}

/// The hand the wielded weapon's first gun rides.
fn wielded_holder() -> Option<Hand> {
    weapons::status().wielded.and_then(|w| w.weapon.holder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signal_parses_back_from_its_name() {
        for &signal in Signal::ALL {
            assert_eq!(Signal::parse(signal.name()), Some(signal));
        }
        assert_eq!(Signal::parse("climb.climbin"), None);
    }

    #[test]
    fn signal_names_are_distinct() {
        let mut names: Vec<_> = Signal::ALL.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Signal::ALL.len());
    }

    #[test]
    fn status_rows_are_found_by_their_label_in_any_case() {
        assert_eq!(status_row_label("callback"), Some("CALLBACK"));
        assert_eq!(
            status_row_label("PASS-THROUGH ERROR"),
            Some("PASS-THROUGH ERROR")
        );
        assert_eq!(status_row_label("NO SUCH ROW"), None);
    }

    #[test]
    fn health_words_parse_in_either_case() {
        assert_eq!(parse_health("OK"), Some(Health::Ok));
        assert_eq!(parse_health("waiting"), Some(Health::Waiting));
        assert_eq!(parse_health("fine"), None);
    }
}
