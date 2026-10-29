//! Wingsuit gesture integration after hand interactions and before final action resolution.

use std::time::Instant;

use jc3gi::input::input_action_map::Action::OPEN_PARACHUTE;

use crate::{
    controllers::{
        bindings::hold::{Desired, MapKind},
        body::BodyDimensions,
        config::{ControllersConfig, GripOffset},
        fingers::grip::{self, GripSource},
        frame::{ControllerFrame, Hand},
        grab::GrabVolume,
        mode::{Detection, Mode},
        vehicles::{
            config::WingsuitGesturesConfig,
            wingsuit_gestures::{
                GestureAvailability, GestureCommands, GestureInput, HandClaims, WingsuitGestures,
            },
        },
        weapons,
    },
    vr::input::flight_pose::{self, FlightCalibration},
};

/// Recognition and the calibration that interprets its pose history belong to one binding state.
#[derive(Debug, Default)]
pub(super) struct FlightState {
    gestures: WingsuitGestures,
    calibration: Option<Calibration>,
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
    evaluate_input(
        state,
        Calibration {
            grip_offset: tick.config.hands.grip,
            dimensions: *tick.dimensions,
        },
        observation(tick, desired, Instant::now()),
        &tick.config.vehicles.wingsuit_gestures,
    )
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
    commit_input(
        state,
        commands,
        observation(tick, desired, Instant::now()),
        &tick.config.vehicles.wingsuit_gestures,
        desired,
    )
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
        gameplay: available(tick),
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

fn available(tick: &Tick<'_>) -> bool {
    tick.ready
        && tick.detection.mode != Mode::Ui
        && tick.detection.held.is_none()
        && !tick.ragdoll
        && !weapons::inventory::status().stashed
        && !weapons::status().fire_blocked
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
