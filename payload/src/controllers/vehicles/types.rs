//! The vehicle handling's public readout types: what the tick's two halves publish, and what the
//! debug UI and the status page read back.

use glam::{Vec2, Vec3};

use crate::controllers::{
    grab::{GrabVolume, GripTarget},
    mode::VehicleSeat,
    vehicles::{
        arm_steering::{ArmPose, PitchInput},
        fit::FitReadout,
        mounted::MountedGun,
        physical::{ControlCue, ControlEvent, ControlKind, ControlState, Outputs},
        wheel::WheelFrame,
    },
};

/// The mounted gun as the last input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MountedReadout {
    pub gun: MountedGun,
    /// The barrel direction the hands ask for, in the world.
    pub direction: Option<Vec3>,
    /// The aim point handed to the gun.
    pub target: Option<Vec3>,
}

/// The arm steering as the last input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmReadout {
    pub pose: ArmPose,
    /// The spread, as a share of the arm span, the arms need this tick to steer: the engaging
    /// spread, or the narrower one while they already steer.
    pub needed_spread: f32,
    /// The shoulders' height in the cockpit frame, which the pose's height is measured from.
    pub shoulder_y: f32,
    /// The pitch input the configured measure read.
    pub pitch: PitchInput,
    /// The arms' own stick vector, `None` while they are not spread. While the grapple is aimed,
    /// the vector held from when the aim began.
    pub arms: Option<Vec2>,
    /// Whether the arms are held where they were because the grapple is aimed.
    pub held_for_grapple: bool,
    /// The player's stick (+X right, +Y forward).
    pub stick: Vec2,
    /// The stick vector written to the move half-axes: the stick and the arms together.
    pub written: Vec2,
}

/// The grip targets [`begin_tick`] provides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickVolumes {
    pub targets: Vec<GripTarget>,
}

/// The parachute handles as the last input tick saw them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandlesReadout {
    /// Whether each hand holds its handle, indexed by [`Hand::index`].
    pub held: [bool; 2],
    /// Each hand's pull, `0..=1`.
    pub pulls: [f32; 2],
    /// The handles' own stick vector, `None` while neither is held.
    pub handles: Option<Vec2>,
    /// The stick vector written to the move half-axes: the stick and the handles together, `None`
    /// while the tables' stick writes stand.
    pub written: Option<Vec2>,
}

/// The vehicles' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VehiclesStatus {
    /// Whether the last input tick ran the vehicle handling.
    pub active: bool,
    /// The seat, while in a vehicle.
    pub seat: Option<VehicleSeat>,
    /// The physical controls the seat offers with their toggles on.
    pub offered: &'static [ControlKind],
    /// The grab volume each hand is in, indexed by [`Hand::index`].
    pub volumes: [Option<GrabVolume>; 2],
    /// The control each hand holds.
    pub holds: [Option<ControlKind>; 2],
    /// The controls' moving parts.
    pub controls: ControlState,
    /// The values the held controls wrote on the last tick.
    pub outputs: Outputs,
    /// The most recent grabs and releases, newest last.
    pub events: Vec<ControlEvent>,
    pub mounted: Option<MountedReadout>,
    pub arms: Option<ArmReadout>,
    /// The parachute handles, while they act.
    pub handles: Option<HandlesReadout>,
    /// What the wheel and handlebars fit found, while driving.
    pub fit: Option<FitReadout>,
    /// The wheel as placed, in the cockpit frame.
    pub wheel: Option<WheelFrame>,
    /// The angle the rendered wheel is turned to, radians, while it is turned.
    pub rendered_angle: Option<f32>,
    /// The stick's or lever's cue that fired last, and how many have fired.
    pub last_cue: Option<ControlCue>,
    pub cues: u64,
    /// How many times the throttle went through its afterburner gate.
    pub afterburner_lights: u64,
    /// How many times a hand pressed a cockpit button or twisted a twist grip.
    pub button_presses: u64,
    /// How many times a hand took or let go of a mounted gun's handle.
    pub mounted_grip_events: u64,
    /// Whether each hand holds its mounted gun's handle, indexed by [`Hand::index`].
    pub mounted_holds: [bool; 2],
}
/// What the status page shows of the vehicles: [`status`] without its event list.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct VehicleSummary {
    pub active: bool,
    pub seat: Option<VehicleSeat>,
    pub offered: &'static [ControlKind],
    pub holds: [Option<ControlKind>; 2],
    pub outputs: Outputs,
    pub cues: u64,
    pub afterburner_lights: u64,
    pub button_presses: u64,
    /// Whether a mounted gun is manned, and whether each hand holds its handle.
    pub mounted: bool,
    pub mounted_holds: [bool; 2],
    pub mounted_grip_events: u64,
}
