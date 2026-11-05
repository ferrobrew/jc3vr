//! Why a snatch did not happen, or ended before the target reached the hand.

use std::fmt;

use crate::controllers::{
    physical_hands::BodyError,
    weapons::hold::{pickup, rack::RackRefusal, steal::StealRefusal},
};

/// Why a snatch did not happen, or ended before the target reached the hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player or hook.
    NoPlayer,
    /// The grapple has a line out.
    LineOut,
    /// The hook is firing or reeling.
    HookBusy,
    /// The target is gone: despawned, picked up, or no longer where it was.
    Gone,
    /// The weapon no longer lies in the world.
    NotDropped,
    /// The weapon has no body in the world.
    NoBody,
    /// The hook did not take the custom fire.
    FireRefused,
    /// The hook missed, ran out of line, or the game ended its line before it held the target.
    Missed,
    /// The hook flew for too long without holding the target.
    TimedOut,
    /// The target stopped coming: snagged on the world.
    Stalled,
    /// The target is farther from the hand than the line reaches.
    OutOfLine,
    /// The line came away while it pulled.
    LineLost,
    /// The target's body could not be read or driven.
    Body(BodyError),
    /// The character would not let go of its gun.
    Free(StealRefusal),
    /// The rack or locker would not give its weapon up.
    Rack(RackRefusal),
    /// The target flew past the hand, or stopped short of it, without being caught.
    CatchMissed,
    /// The take into the hand refused the weapon.
    Take(pickup::Refusal),
    /// The door would not come open.
    Door(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refusal::NoPlayer => "there is no local player or hook",
            Refusal::LineOut => "the grapple has a line out",
            Refusal::HookBusy => "the hook is firing or reeling",
            Refusal::Gone => "the target is gone",
            Refusal::NotDropped => "the weapon no longer lies in the world",
            Refusal::NoBody => "the weapon has no body in the world",
            Refusal::FireRefused => "the hook did not fire",
            Refusal::Missed => "the hook missed",
            Refusal::TimedOut => "the hook flew too long",
            Refusal::Stalled => "the target snagged",
            Refusal::OutOfLine => "the target is beyond the line",
            Refusal::LineLost => "the line came away",
            Refusal::CatchMissed => "the hand missed the catch",
            Refusal::Body(e) => return write!(f, "snatch: {e}"),
            Refusal::Free(e) => return write!(f, "snatch: {e}"),
            Refusal::Rack(e) => return write!(f, "snatch: {e}"),
            Refusal::Take(e) => return write!(f, "snatch: {e}"),
            Refusal::Door(e) => return write!(f, "snatch: the door: {e}"),
        };
        write!(f, "snatch: {why}")
    }
}

impl std::error::Error for Refusal {}
