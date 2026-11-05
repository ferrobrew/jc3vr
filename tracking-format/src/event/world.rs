//! The world's version 8 records: where the cockpit and the camera sit in the world, the grapple's
//! line, the vehicle, the frame's timing, and the physics bodies around Rico with what he holds and
//! the forces the mod applies to them.

use glam::{Quat, Vec3};

use crate::{
    event::Label,
    model::{Hand, Pose},
};

/// Where the cockpit frame sits in the world for one game frame, and the camera the frame rendered
/// from. A cockpit point `p` is the world point `head_world + view * ((p - head_cockpit) *
/// world_scale)`, as the mod composes the hands.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    /// The published head pose, world space: what the mod composes the cockpit through.
    pub head_world: Pose,
    /// The head's position in the cockpit frame at the same moment.
    pub head_cockpit: Vec3,
    /// The view frame's rotation, cockpit to world.
    pub view: Quat,
    /// World metres per cockpit metre.
    pub world_scale: f32,
    /// The active camera, when it was readable.
    pub camera: Option<CameraState>,
}

/// The active camera's transforms for one rendered frame: the two tick poses the engine blends, the
/// fraction it blended them by, and the blend it rendered, world space.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct CameraState {
    pub previous: Pose,
    pub current: Pose,
    /// The render update's sub-tick fraction.
    pub fraction: f32,
    pub rendered: Pose,
}

/// The local player's grapple while a wire is out.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GrappleState {
    /// The grappling hook's state and its active wire's state, as the game names them.
    pub hook_state: Label,
    pub wire_state: Label,
    /// The longest the wire may run, metres.
    pub max_length_m: f32,
    /// The hook's end and the device's end.
    pub hook: WireEndSample,
    pub device: WireEndSample,
    /// The object the hook's end targets, a correlation identity only.
    pub target_object: Option<u64>,
    /// The snatch's held line, while it swings its target.
    pub sling: Option<SlingState>,
}

/// One end of a grapple wire.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct WireEndSample {
    pub state: Label,
    pub position: Vec3,
    /// Whether the end is fixed to a physics proxy.
    pub attached: bool,
}

/// The snatch's held line.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct SlingState {
    pub length_m: f32,
    /// The line's tension, and the most it holds on this target with this brace, newtons.
    pub tension_n: f32,
    pub max_tension_n: f32,
    pub mass_kg: f32,
    pub braced: bool,
    pub tethered: bool,
    /// Where the line is anchored, and the target's centre, world space.
    pub anchor: Vec3,
    pub centre: Vec3,
}

/// The vehicle the player rides, while in one.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    /// The vehicle class the bindings resolved, and whether the player drives.
    pub class: Label,
    pub driver: bool,
    /// The vehicle, a correlation identity only.
    pub object: Option<u64>,
    /// The vehicle's transform at the end of the last simulation step, and as the frame rendered
    /// it, world space.
    pub transform: Option<Pose>,
    pub rendered: Option<Pose>,
    /// How far the last simulation step moved it, world metres.
    pub step_m: Option<Vec3>,
    /// The steering the vehicle last applied: acceleration, brake, and turn.
    pub steering: Option<Vec3>,
    /// Whether the vehicle handling ran on the last input tick.
    pub handling: bool,
    /// The control each hand holds, indexed by [`Hand::index`].
    pub holds: [Option<Label>; 2],
    /// Whether a mounted gun is manned, whether each hand holds its handle, and its aim point.
    pub mounted: bool,
    pub mounted_holds: [bool; 2],
    pub mounted_aim: Option<Vec3>,
}

/// The frame's place in the engine's frame sequence and how long the last game frame took.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct FrameState {
    /// The engine's render frame index.
    pub render_frame: Option<u32>,
    /// The counter of the mirror's latest present when the state was read, the number the frame
    /// stamp draws into the mirror's video (wrapping at its bits); this frame's own present follows
    /// it. Absent before the mirror presents.
    pub mirror_frame: Option<u64>,
    /// The last complete game frame's interval, milliseconds, and the sim ticks it ran.
    pub interval_ms: Option<f32>,
    pub ticks: u32,
    /// The haptics each hand last played, `0..=1`.
    pub haptics: [Option<f32>; 2],
}

/// The dynamic bodies near Rico after one physics step, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct BodiesSample {
    /// The steps the recorder has counted since the payload loaded.
    pub step: u64,
    /// The world point the bodies' positions are relative to: Rico's root.
    pub origin: Vec3,
    /// The body slots this step's walk went through, the world's peak slot, and how long the walk
    /// and the reads took, microseconds.
    pub walked: u32,
    pub peak: u32,
    pub cost_us: u32,
    /// The kept bodies, less those at rest since an earlier step: a body that comes to rest is
    /// written once, and again when it moves. A body that left the set is simply absent.
    pub bodies: Vec<BodySample>,
}

/// One body's state after a step. Positions keep a millimetre within 32 metres of the origin,
/// velocities a centimetre or a hundredth of a radian per second.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct BodySample {
    pub id: u32,
    /// The body's position relative to the sample's origin, and its orientation, world axes.
    pub offset: Vec3,
    pub orientation: Quat,
    /// World metres and radians per second.
    pub linear: Vec3,
    pub angular: Vec3,
    /// Whether both its velocities read zero.
    pub resting: bool,
    /// Which hand holds it, if either.
    pub held: [bool; 2],
}

/// A body's identity and shape, noted when it joins the near set and repeated now and then while
/// it stays, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct BodyIntro {
    pub id: u32,
    /// The game object that owns it, a correlation identity only.
    pub object: Option<u64>,
    /// What it is: a prop, a weapon, a vehicle part, a character's proxy, a ragdoll, or one of the
    /// mod's own bodies.
    pub kind: Label,
    pub mass_kg: Option<f32>,
    /// Its shape's kind, and its broad-phase box's half extents when it joined, world metres.
    pub shape: Label,
    pub half_extents: Vec3,
    /// The collision-filter word.
    pub filter: u32,
    pub repeat: bool,
}

/// What a hand holds through the physical hands after one simulation tick, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HeldSample {
    pub hand: Hand,
    /// The kind of hold: a prop, something hinged, a door, or a ragdoll.
    pub kind: Label,
    /// The held object, a correlation identity only, and its body.
    pub object: u64,
    pub body: u32,
    /// The body's pose and velocities, world space, when it was readable.
    pub motion: Option<HeldMotion>,
    /// The hold's own report, when the hold published one.
    pub report: Option<HoldReport>,
}

/// A held body's pose and velocities.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HeldMotion {
    pub pose: Pose,
    pub linear: Vec3,
    pub angular: Vec3,
}

/// The hold's report: where the grab point is, where the hand wants it, and how hard it works.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HoldReport {
    pub mass_kg: f32,
    /// The body's centre of mass, the grab point on it, and where the hand would have the grab
    /// point, world space.
    pub centre: Vec3,
    pub anchor: Vec3,
    pub target: Vec3,
    pub lag_m: f32,
    /// The shares of the hand's strongest pull and twist used, `0..=1`.
    pub load: f32,
    pub twist: f32,
    pub joined: bool,
    pub shared: bool,
    pub liftable: bool,
}

/// A velocity write or an impulse the mod applied to a body, from version 8.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Force {
    /// The source file and line that asked for it.
    pub source: Label,
    pub line: u32,
    pub kind: ForceKind,
    pub body: u32,
    /// The body's owner, a correlation identity only.
    pub object: Option<u64>,
    /// The body's mass, kilograms, and its velocities before the write, world space.
    pub mass_kg: Option<f32>,
    pub before: Option<crate::event::WorldMotion>,
}

/// What a [`Force`] did.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ForceKind {
    /// The body's velocities were set, world metres and radians per second.
    Velocity { linear: Vec3, angular: Vec3 },
    /// An impulse at a world point, newton seconds.
    Impulse { impulse: Vec3, at: Vec3 },
}
