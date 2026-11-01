//! Wingsuit gesture integration after hand interactions and before final action resolution.
//!
//! Each change of the recognizer's phase, and each transition it requests, is logged at info level
//! with the gate that blocks it and the arm measures it saw, while the player is falling or in the
//! suit, so a session's log shows what the recognizer made of the player's arms.

use std::{fmt, time::Instant};

use jc3gi::input::input_action_map::Action::OPEN_PARACHUTE;
use tracking_format::event::{Label, WingsuitGesture};

use crate::{
    controllers::{
        bindings::hold::{Desired, MapKind},
        body::BodyDimensions,
        config::{ControllersConfig, GripOffset},
        fingers::grip::{self, GripSource},
        frame::{ControllerFrame, Hand},
        grab::GrabVolume,
        mode::{AirborneVariant, Detection, Mode},
        vehicles::{
            config::WingsuitGesturesConfig,
            wingsuit_gestures::{
                Blocked, FlightPose, GestureAvailability, GestureCommands, GestureInput,
                GesturePhase, HandClaims, SuitTransition, WingsuitGestures,
            },
        },
        weapons,
    },
    recorder::tracking::events::{self, EventKind},
    vr::input::flight_pose::{self, FlightCalibration},
};

/// Recognition and the calibration that interprets its pose history belong to one binding state.
#[derive(Debug, Default)]
pub(super) struct FlightState {
    gestures: WingsuitGestures,
    calibration: Option<Calibration>,
    /// The phase and its cause last logged, or `None` while the player is neither falling nor in
    /// the suit.
    logged: Option<(GesturePhase, Option<Why>)>,
}

impl FlightState {
    /// Clear pose history without lowering the gesture sample watermark.
    pub(super) fn reset(&mut self) {
        self.gestures.reset();
    }

    pub(super) fn current(&self) -> GestureAvailability {
        self.gestures.current()
    }

    fn calibrate(&mut self, calibration: Calibration) {
        if self
            .calibration
            .is_none_or(|last| !last.same_bits(calibration))
        {
            self.reset();
        }
        self.calibration = Some(calibration);
    }
}

pub(super) struct Tick<'a> {
    pub frame: &'a ControllerFrame,
    pub config: &'a ControllersConfig,
    pub dimensions: &'a BodyDimensions,
    pub detection: Detection,
    pub ready: bool,
    pub ragdoll: bool,
    pub grapple: bool,
    pub grabs: [Option<GrabVolume>; 2],
    /// What each hand is within reach of this tick, by [`Hand::index`].
    pub reaches: [&'a [GrabVolume]; 2],
    pub dt: f32,
}

pub(super) fn evaluate(
    state: &mut FlightState,
    tick: &Tick<'_>,
    desired: &[Desired],
) -> GestureCommands {
    let input = observation(tick, desired, Instant::now());
    let config = &tick.config.vehicles.wingsuit_gestures;
    let commands = evaluate_input(
        state,
        Calibration {
            grip_offset: tick.config.hands.grip,
            dimensions: *tick.dimensions,
        },
        input,
        config,
    );
    note(state, tick, input, config, Stage::Evaluate);
    if let Some(transition) = commands.transition {
        tracing::info!(
            target: "controllers",
            "wingsuit gestures: {} the suit {}",
            match transition {
                SuitTransition::Deploy => "deploying",
                SuitTransition::Close => "closing",
            },
            Measures(input.and_then(|input| input.pose)),
        );
    }
    commands
}

/// State actions can add a canopy request or acquire a hand after gesture evaluation.
pub(super) fn commit(
    state: &mut FlightState,
    commands: GestureCommands,
    tick: &Tick<'_>,
    desired: &mut Vec<Desired>,
) -> bool {
    let calibration = Calibration {
        grip_offset: tick.config.hands.grip,
        dimensions: *tick.dimensions,
    };
    state.calibrate(calibration);
    let input = observation(tick, desired, Instant::now());
    let config = &tick.config.vehicles.wingsuit_gestures;
    let reserve_pitch = commit_input(state, commands, input, config, desired);
    note(state, tick, input, config, Stage::Commit);
    reserve_pitch
}

/// Why the recognizer is unavailable, as far as the binding owner can tell.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Why {
    /// No live head and controller packet matched this tick's controller frame.
    NoSample,
    /// The binding owner suspended gameplay input for the gestures.
    Suspended(Suspension),
    /// The recognizer's own gate.
    Blocked(Blocked),
    /// The samples went back in time or paused past the largest gap, which cancels the gestures.
    Interrupted,
    /// The observation changed between the gesture evaluation and the action commit.
    Revalidated,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSample => f.write_str("no live head and controller sample matches the tick"),
            Self::Suspended(suspension) => fmt::Display::fmt(suspension, f),
            Self::Blocked(blocked) => fmt::Display::fmt(blocked, f),
            Self::Interrupted => f.write_str("the samples went back or paused"),
            Self::Revalidated => {
                f.write_str("the sample changed before the actions were committed")
            }
        }
    }
}

/// The binding owner's reason to suspend gameplay input for the gestures.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Suspension {
    MaskedHands,
    Menu,
    ModeChanging,
    Ragdoll,
    Stashed,
    FireBlocked,
}

impl fmt::Display for Suspension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MaskedHands => "a recovery mask holds a hand",
            Self::Menu => "a menu is open",
            Self::ModeChanging => "the mode is changing",
            Self::Ragdoll => "the player is ragdolling",
            Self::Stashed => "the inventory is stashed",
            Self::FireBlocked => "weapon actions are blocked",
        })
    }
}

/// Where in the tick a phase is noted.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Stage {
    Evaluate,
    Commit,
}

/// Log the recognizer's phase when it, or the gate behind it, changed since the last log. Outside
/// falling and the suit, only the first tick there is logged.
fn note(
    state: &mut FlightState,
    tick: &Tick<'_>,
    input: Option<GestureInput>,
    config: &WingsuitGesturesConfig,
    stage: Stage,
) {
    let airborne = matches!(
        tick.detection.mode,
        Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Wingsuit)
    );
    let phase = state.gestures.phase();
    let entry = airborne.then(|| {
        let why = (phase == GesturePhase::Unavailable).then(|| match input {
            None => Why::NoSample,
            Some(input) => suspension(tick)
                .map(Why::Suspended)
                .or_else(|| input.blocked(config).map(Why::Blocked))
                .unwrap_or(match stage {
                    Stage::Evaluate => Why::Interrupted,
                    Stage::Commit => Why::Revalidated,
                }),
        });
        (phase, why)
    });
    if entry == state.logged {
        return;
    }
    state.logged = entry;
    if events::collecting() {
        events::note(EventKind::WingsuitGesture(Box::new(WingsuitGesture {
            phase: entry.map(|(phase, _)| Label::Owned(phase.to_string())),
            why: entry
                .and_then(|(_, why)| why)
                .map(|why| Label::Owned(why.to_string())),
        })));
    }
    match entry {
        None => tracing::info!(
            target: "controllers",
            "wingsuit gestures: idle, the player is neither falling nor in the suit"
        ),
        Some((phase, Some(why))) => tracing::info!(
            target: "controllers",
            "wingsuit gestures: {phase} ({why}) {}",
            Measures(input.and_then(|input| input.pose)),
        ),
        Some((phase, None)) => tracing::info!(
            target: "controllers",
            "wingsuit gestures: {phase} {}",
            Measures(input.and_then(|input| input.pose)),
        ),
    }
}

/// The arm measures of a pose, for the log.
struct Measures(Option<FlightPose>);

impl fmt::Display for Measures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(pose) = self.0 else {
            return f.write_str("with no arm pose");
        };
        write!(
            f,
            "with lateral [{:.2}, {:.2}] reach [{:.2}, {:.2}] height [{:.2}, {:.2}] spans, \
             arm angle [{:.0}, {:.0}] and palm roll [{:.0}, {:.0}] degrees",
            pose.lateral[0],
            pose.lateral[1],
            pose.reach[0],
            pose.reach[1],
            pose.height[0],
            pose.height[1],
            pose.angle_deg[0],
            pose.angle_deg[1],
            pose.palm_roll_deg[0],
            pose.palm_roll_deg[1],
        )
    }
}

#[derive(Debug, Copy, Clone)]
struct Calibration {
    grip_offset: GripOffset,
    dimensions: BodyDimensions,
}

impl Calibration {
    fn same_bits(self, other: Self) -> bool {
        self.values().map(f32::to_bits) == other.values().map(f32::to_bits)
            && self.dimensions.seated_eye_height.map(f32::to_bits)
                == other.dimensions.seated_eye_height.map(f32::to_bits)
    }

    fn values(self) -> [f32; 14] {
        let grip = self.grip_offset;
        let body = self.dimensions;
        [
            grip.position_m.x,
            grip.position_m.y,
            grip.position_m.z,
            grip.rotation_deg.x,
            grip.rotation_deg.y,
            grip.rotation_deg.z,
            body.height,
            body.eye_height,
            body.shoulder_height,
            body.shoulder_width,
            body.arm_span,
            body.upper_arm,
            body.forearm,
            body.hand,
        ]
    }
}

fn observation(tick: &Tick<'_>, desired: &[Desired], now: Instant) -> Option<GestureInput> {
    let packet = flight_pose::for_frame(tick.frame, now)?;
    Some(GestureInput {
        sample: packet.sample(),
        mode: tick.detection.mode,
        gameplay: suspension(tick).is_none(),
        canopy_requested: canopy(desired),
        pose: packet.measure(
            &FlightCalibration {
                grip_offset: &tick.config.hands.grip,
                dimensions: tick.dimensions,
            },
            now,
        ),
        claims: claims(tick, now),
    })
}

fn evaluate_input(
    state: &mut FlightState,
    calibration: Calibration,
    input: Option<GestureInput>,
    config: &WingsuitGesturesConfig,
) -> GestureCommands {
    state.calibrate(calibration);
    let Some(input) = input else {
        state.reset();
        return GestureCommands::default();
    };
    state.gestures.step(input, config)
}

fn commit_input(
    state: &mut FlightState,
    commands: GestureCommands,
    input: Option<GestureInput>,
    config: &WingsuitGesturesConfig,
    desired: &mut Vec<Desired>,
) -> bool {
    let Some(input) = input else {
        state.reset();
        return false;
    };
    if state.gestures.revalidate(input, config) == GestureAvailability::Unavailable {
        state.reset();
        return false;
    }
    match flight_pose::desired_for(commands, input.mode, canopy(desired)) {
        Ok(inputs) => {
            desired.extend(inputs);
            commands.reserve_pitch
        }
        Err(_) => {
            state.reset();
            false
        }
    }
}

/// The first reason the binding owner suspends gameplay input for the gestures, if any.
fn suspension(tick: &Tick<'_>) -> Option<Suspension> {
    if !tick.ready {
        Some(Suspension::MaskedHands)
    } else if tick.detection.mode == Mode::Ui {
        Some(Suspension::Menu)
    } else if tick.detection.held.is_some() {
        Some(Suspension::ModeChanging)
    } else if tick.ragdoll {
        Some(Suspension::Ragdoll)
    } else if weapons::inventory::status().stashed {
        Some(Suspension::Stashed)
    } else if weapons::status().fire_blocked {
        Some(Suspension::FireBlocked)
    } else {
        None
    }
}

fn canopy(desired: &[Desired]) -> bool {
    desired
        .iter()
        .any(|input| input.target.map == MapKind::Player && input.target.action == OPEN_PARACHUTE)
}

fn claims(tick: &Tick<'_>, now: Instant) -> [HandClaims; 2] {
    let reload = weapons::reload::status();
    let foregrip = weapons::foregrip::two_hand_grip();
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let grip = grip::current(hand, now, grip::max_age(tick.dt))
            .is_some_and(|grip| !holds_item(grip.source));
        HandClaims {
            grip: grip
                || tick.grabs[i].is_some()
                || foregrip.is_some_and(|hold| hold.holder.other() == hand),
            slot: tick.reaches[i].iter().copied().any(on_body),
            mechanical: reload.fire_held[i]
                || reload
                    .insert_hands
                    .is_some_and(|(a, b)| a == hand || b == hand),
            grapple: tick.grapple || tick.detection.traversal.is_some(),
        }
    })
}

/// Whether a grip of `source` is a held item, which rides along in the hand rather than claiming
/// it. A magazine is the reload's, which claims the hand itself.
const fn holds_item(source: GripSource) -> bool {
    matches!(
        source,
        GripSource::Gun | GripSource::Grenade | GripSource::C4 | GripSource::Pickup
    )
}

/// Whether `volume` is a slot on the body: a socket, or a pouch.
const fn on_body(volume: GrabVolume) -> bool {
    matches!(
        volume,
        GrabVolume::Socket(_)
            | GrabVolume::GrenadePouch
            | GrabVolume::MagazinePouch
            | GrabVolume::C4Pouch
    )
}

#[cfg(test)]
mod tests;
