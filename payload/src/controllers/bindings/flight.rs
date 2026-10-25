//! Wingsuit gesture integration after hand interactions and before final action resolution.

use std::time::Instant;

use jc3gi::input::input_action_map::Action::OPEN_PARACHUTE;

use crate::{
    controllers::{
        bindings::hold::{Desired, MapKind},
        body::BodyDimensions,
        config::{ControllersConfig, GripOffset},
        fingers::grip,
        frame::{ControllerFrame, Hand},
        grab::GrabVolume,
        loadout::live,
        mode::{Detection, Mode},
        vehicles::wingsuit_gestures::{
            GestureCommands, GestureInput, HandClaims, WingsuitGestures,
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
    pub dt: f32,
}

pub(super) fn evaluate(
    state: &mut FlightState,
    tick: &Tick<'_>,
    desired: &[Desired],
) -> GestureCommands {
    state.calibrate(Calibration {
        grip_offset: tick.config.hands.grip,
        dimensions: *tick.dimensions,
    });
    let now = Instant::now();
    let Some(packet) = flight_pose::for_frame(tick.frame, now) else {
        state.reset();
        return GestureCommands::default();
    };
    let pose = packet.measure(
        &FlightCalibration {
            grip_offset: &tick.config.hands.grip,
            dimensions: tick.dimensions,
        },
        now,
    );
    state.gestures.step_live(
        GestureInput {
            sample: packet.sample(),
            mode: tick.detection.mode,
            gameplay: available(tick),
            canopy_requested: canopy(desired),
            pose,
            occupied: [false; 2],
            claims: claims(tick, now),
        },
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
    let now = Instant::now();
    let eligible = !canopy(desired) && available(tick);
    let packet_current = eligible && flight_pose::for_frame(tick.frame, now).is_some();
    let claims = if packet_current {
        claims(tick, now)
    } else {
        [HandClaims::default(); 2]
    };
    finalize(
        state,
        commands,
        tick.detection.mode,
        Finalization {
            available: eligible,
            packet_current,
            claims,
        },
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

struct Finalization {
    available: bool,
    packet_current: bool,
    claims: [HandClaims; 2],
}

fn finalize(
    state: &mut FlightState,
    commands: GestureCommands,
    mode: Mode,
    finalization: Finalization,
    desired: &mut Vec<Desired>,
) -> bool {
    if canopy(desired)
        || !finalization.available
        || !finalization.packet_current
        || finalization
            .claims
            .into_iter()
            .any(|claim| claim.grip || claim.trigger || claim.mechanical || claim.grapple)
    {
        state.reset();
        return false;
    }
    match flight_pose::desired_for(commands, mode, false) {
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
    let gun = weapons::capabilities::gun_owners_now();
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let grip = grip::current(hand, now, grip::max_age(tick.dt)).is_some();
        HandClaims {
            grip: grip
                || live::held_kind(hand).is_some()
                || tick.grabs[i].is_some()
                || foregrip.is_some_and(|hold| hold.holder.other() == hand),
            trigger: gun[i].is_some(),
            mechanical: reload.fire_held[i]
                || reload
                    .insert_hands
                    .is_some_and(|(a, b)| a == hand || b == hand),
            grapple: tick.grapple || tick.detection.traversal.is_some(),
        }
    })
}

#[cfg(test)]
mod tests;
