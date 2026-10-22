//! The physical hands' readout: why the palms did nothing on the last tick, and what each one did
//! when they acted.

use std::time::Instant;

use glam::{Vec2, Vec3};

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        free_hands,
        grasp::Reach,
        palm::PalmBox,
        physics,
        pickup::{HeldReport, PickupStatus},
        state::STATE,
    },
};

/// Why the palms did nothing on their last tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Idle {
    /// The physical hands are off, or the controller scheme is not in effect.
    Disabled,
    /// Not on foot on the ground or falling.
    Mode,
    /// No tracked hands.
    NoHands,
    /// The palms' tick ran on another thread than the input tick.
    OffThread,
}

impl Idle {
    pub const fn label(self) -> &'static str {
        match self {
            Idle::Disabled => "off",
            Idle::Mode => "not on foot on the ground or falling",
            Idle::NoHands => "no tracked hands",
            Idle::OffThread => "the tick ran off the main thread",
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
    /// Palms' ticks that ran off the main thread.
    pub off_thread: u64,
    /// The last cast or engine failure.
    pub last_error: Option<String>,
    pub pickup: PickupStatus,
}

pub fn status() -> PhysicalHandsStatus {
    STATE.lock().status.clone()
}

/// What the palms' last tick did for one hand: the inputs of the grip press log's summary
/// ([`press_summary`]) and of the grab view's palm marker.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PalmReadout {
    /// Why the palms stood down, if they did.
    pub idle: Option<Idle>,
    /// How long ago the palms last ticked, or `None` before the first tick.
    pub age_ms: Option<u128>,
    /// Whether the hand holds nothing else: no gun, not the foregrip, no grenade or charge, and the
    /// controller drives it.
    pub free: bool,
    /// Whether the hand's body drives it.
    pub driven: bool,
    /// Whether the hand's body has a fresh pose.
    pub body_pose: bool,
    /// Whether the palm was placed on the last tick.
    pub palm: bool,
    /// Whether the reach found something the grip would take.
    pub reach: bool,
    /// Whether the hand holds what its grip took from the world.
    pub holding: bool,
    /// The casts the last tick made.
    pub casts: u32,
    pub last_error: Option<String>,
}

/// What the palms' last tick did for `hand`.
pub(crate) fn palm_readout(hand: Hand) -> PalmReadout {
    let (idle, age_ms, hand_status, casts, last_error) = {
        let state = STATE.lock();
        let age_ms = state
            .at
            .map(|at| Instant::now().saturating_duration_since(at).as_millis());
        let status = &state.status;
        (
            status.idle,
            age_ms,
            status.hands[hand.index()],
            status.casts,
            status.last_error.clone(),
        )
    };
    PalmReadout {
        idle,
        age_ms,
        free: free_hands()[hand.index()],
        driven: physics::drives(hand),
        body_pose: physics::body_pose(hand).is_some(),
        palm: hand_status.palm.is_some(),
        reach: hand_status.reach.is_some(),
        holding: hand_status.held.is_some(),
        casts,
        last_error,
    }
}

/// What the palms' last tick did for `hand`, for the grip's press log: why the palms stood down,
/// or whether the hand was free, driven by its body (and whether that body's pose was fresh), had
/// a palm, and reached anything, with the tick's age, casts, and last failure. A grab the reach
/// should have offered and did not is explained by these.
pub(crate) fn press_summary(hand: Hand) -> String {
    let r = palm_readout(hand);
    let summary = match (r.idle, r.age_ms) {
        (Some(idle), _) => format!("idle: {}", idle.label()),
        (None, None) => "never ticked".to_owned(),
        (None, Some(age_ms)) => format!(
            "ticked {age_ms} ms ago, free={}, driven={}, body pose={}, palm={}, reach={}, casts={}",
            r.free,
            r.driven,
            if r.body_pose { "fresh" } else { "none" },
            r.palm,
            r.reach,
            r.casts,
        ),
    };
    match &r.last_error {
        Some(error) => format!("{summary}, last error: {error}"),
        None => summary,
    }
}
