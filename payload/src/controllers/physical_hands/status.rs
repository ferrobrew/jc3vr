//! The physical hands' readout: why the palms did nothing on the last tick, and what each one did
//! when they acted.

use glam::{Vec2, Vec3};

use crate::controllers::physical_hands::{
    grasp::Reach,
    palm::PalmBox,
    pickup::{HeldReport, PickupStatus},
    state::STATE,
};

/// Why the palms did nothing on the last aim update.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The physical hands are off, or the controller scheme is not in effect.
    Disabled,
    /// Not on foot on the ground or falling.
    Mode,
    /// No tracked hands.
    NoHands,
    /// The aim update ran on another thread than the input tick.
    OffThread,
}

impl Idle {
    pub const fn label(self) -> &'static str {
        match self {
            Idle::Disabled => "off",
            Idle::Mode => "not on foot on the ground or falling",
            Idle::NoHands => "no tracked hands",
            Idle::OffThread => "the aim update is off the main thread",
        }
    }
}

/// One palm's state on the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandStatus {
    /// Whether the palm acted.
    pub acting: bool,
    pub palm: Option<PalmBox>,
    /// The deepest press into a solid surface, metres, and whether the surface is an object.
    pub press: Option<(f32, bool)>,
    /// The push this palm asked for, world `(x, z)` metres per second.
    pub push: Vec2,
    /// The hand's speed, metres per second.
    pub speed: f32,
    /// What the hand could take with its grip.
    pub reach: Option<Reach>,
    /// What the hand holds.
    pub held: Option<HeldReport>,
    /// The palm's shove of the object it presses into.
    pub shove: Option<ShoveReport>,
    /// Whether the hand grips a latched door.
    pub latched: bool,
}

/// One palm's shove of a loose object on a tick, for the readout and the overlay.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShoveReport {
    /// Where the palm touches the object, world space.
    pub at: Vec3,
    /// The impulse applied, world space, newton seconds.
    pub impulse: Vec3,
    /// The share of the palm's strongest push used, `0..=1`.
    pub load: f32,
    /// The share of the push the object does not give, `0..=1`, which pushes the body.
    pub resistance: f32,
}

/// The physical hands' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhysicalHandsStatus {
    pub idle: Option<Idle>,
    /// Indexed by [`crate::controllers::frame::Hand::index`].
    pub hands: [HandStatus; 2],
    /// The combined push, world `(x, z)` metres per second.
    pub push: Vec2,
    /// Casts made on the last tick.
    pub casts: u32,
    /// Object knocks made by fast hits.
    pub object_pushes: u64,
    /// Ticks a palm shoved an object.
    pub shoves: u64,
    /// Ticks the aim update ran off the main thread.
    pub off_thread: u64,
    /// The last cast or engine failure.
    pub last_error: Option<String>,
    pub pickup: PickupStatus,
}

pub fn status() -> PhysicalHandsStatus {
    STATE.lock().status.clone()
}
