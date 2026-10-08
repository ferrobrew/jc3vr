//! The hand climbing's public readout types: a gripped surface, the per-hand and whole-climb
//! status, a mantle's landing on a moving body, and what the palms' tick publishes.

use glam::{Quat, Vec3};

/// A surface a hand can grip: the static world, or with [`ClimbConfig::moving`], a moving body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hold {
    /// Where the surface is, game space.
    pub point: Vec3,
    /// The surface's normal, toward the hand.
    pub normal: Vec3,
    /// The surface's body.
    pub body: u32,
    /// Whether the body moves, keyframed or dynamic, rather than being of the static world.
    pub moving: bool,
}

/// One hand's climbing on the last tick, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ClimbHandStatus {
    /// The static surface in the hand's reach.
    pub reach: Option<Hold>,
    /// The surface the hand grips.
    pub grip: Option<Hold>,
}

/// The climbing's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClimbStatus {
    /// Whether a hand grips the world.
    pub climbing: bool,
    /// Whether the mantle is carrying the body over a ledge.
    pub mantling: bool,
    /// Indexed by [`Hand::index`].
    pub hands: [ClimbHandStatus; 2],
    /// The velocity the last step gave the body, world space.
    pub velocity: Vec3,
    /// How far the body was from where the hands would have it, metres.
    pub gap_m: f32,
    /// Whether the game judged the player in the air on the last aim update
    /// ([`jc3gi::character::character::Character::IsInAir`]).
    pub in_air: Option<bool>,
    pub grips: u64,
    /// Grips on moving bodies.
    pub moving_grips: u64,
    pub releases: u64,
    pub mantles: u64,
    /// Grips ended against the player's wish.
    pub drops: u64,
    pub last_drop: Option<&'static str>,
    /// The speed of the last fling, metres per second.
    pub last_fling_mps: Option<f32>,
    pub last_error: Option<String>,
}

/// A mantle that landed on a moving body.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Landing {
    /// The body whose ledge the mantle carried the player over.
    pub body: u32,
    /// Where the feet landed, game space.
    pub feet: Vec3,
}

/// What the palms' tick found for the climb, indexed by [`Hand::index`].
pub(crate) struct Observed {
    /// Each driven hand's palm in the rig: its tracking-space place turned by the body's facing
    /// and scaled into world metres.
    pub palms: [Option<Vec3>; 2],
    /// The static surface in each driven hand's reach.
    pub reaches: [Option<Hold>; 2],
    /// Whether each driven hand's body presses into something that holds it back.
    pub pressing: [bool; 2],
    /// The body's facing the palms were turned by.
    pub body_rotation: Quat,
}
