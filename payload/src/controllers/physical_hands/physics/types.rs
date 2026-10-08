//! The hand bodies' public readout types: the phase and the per-step status each hand's body
//! shows, the callback's live counters, and what a hand's grip holds through a joint.

use std::num::NonZeroUsize;

use glam::{Vec2, Vec3};

use crate::controllers::physical_hands::physics::blows::{BlowReport, ShoveReport};

/// What a hand body is doing.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum BodyPhase {
    /// No body.
    #[default]
    Absent,
    /// A body out of the simulation, waiting for its hand to act again.
    Parked,
    /// A body in the simulation, driven toward its hand.
    Acting,
}

impl BodyPhase {
    pub const fn label(self) -> &'static str {
        match self {
            BodyPhase::Absent => "no body",
            BodyPhase::Parked => "parked",
            BodyPhase::Acting => "acting",
        }
    }
}

/// One hand body on the last step, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandBodyStatus {
    pub phase: BodyPhase,
    pub body_id: Option<u32>,
    /// The body's collision-filter word as the world holds it.
    pub filter_info: Option<u32>,
    /// How far the body is short of where it was sent, metres.
    pub lag_m: f32,
    pub speed_mps: f32,
    /// The share of the hand's strongest pull used, `0..=1`.
    pub force_share: f32,
    /// The share of the hand's strongest twist used, `0..=1`.
    pub torque_share: f32,
    /// Times the body fell too far behind and was put back on its hand.
    pub resets: u64,
    /// The id of the constraint that joins the body to what its grip holds.
    pub joint: Option<u32>,
}

/// The hand bodies' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandBodiesStatus {
    /// Whether the pre-simulation callback is registered.
    pub registered: bool,
    /// Pre-simulation steps the callback has run.
    pub steps: u64,
    /// The player's collision system group the bodies share.
    pub system_group: Option<u32>,
    /// Indexed by [`Hand::index`].
    pub hands: [HandBodyStatus; 2],
    /// The push the hands give the player, world `(x, z)` metres per second.
    pub push: Vec2,
    pub created: u64,
    pub destroyed: u64,
    /// Contacts the bodies started, as the collision listener reported them.
    pub contacts: u64,
    /// Contacts dropped because more arrived between two frame ticks than the queue holds.
    pub dropped_contacts: u64,
    /// Blows dealt to characters.
    pub blows: u64,
    pub last_blow: Option<BlowReport>,
    /// Shoves and yanks the hands gave characters they held.
    pub shoves: u64,
    /// Those that reached a knockdown.
    pub knockdowns: u64,
    pub last_shove: Option<ShoveReport>,
    pub last_error: Option<String>,
}

/// The pre-simulation callback's live state, read from atomics only, so any thread may read it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CallbackStatus {
    /// Whether the callback is registered with the physics system.
    pub registered: bool,
    /// Times the callback has fired.
    pub fires: u64,
    /// The game thread's id, as the registration saw it; zero before it.
    pub game_thread: u32,
    /// The id of the thread the callback last fired on; zero before it first fired.
    pub firing_thread: u32,
    /// Whether each hand's body has contact events on, indexed by [`Hand::index`].
    pub listening: [bool; 2],
}

/// An object's body a hand's grip holds through a joint.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HoldTarget {
    pub object: NonZeroUsize,
    pub body: u32,
    /// How far the hand's goal is moved so that the object's centre stays out of the player's
    /// body, world space.
    pub keep_out: Vec3,
    /// Whether both hands hold the object.
    pub shared: bool,
}
