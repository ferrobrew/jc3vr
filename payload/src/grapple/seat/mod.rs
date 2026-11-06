//! The seat grapple: the grapple fired from a vehicle seat, tethering what it hits to the vehicle.
//! The mod side is in `docs/mod/input/seat-grapple.md`.
//!
//! The game takes no grapple fire in a seat: every seat's animation state clears the hook's fire
//! input ([`GrapplingHook::m_GrappleFireInputEnabled`]). It ships a vehicle grapple of its own
//! ([`VehicleGrappleHook`]) that nothing calls, which fires the hook as a scripted wire at the aim's
//! target and, once the hook holds, turns the wire into a tether between the target and the
//! vehicle. This module runs the same steps from the input tick: the grapple button's fire in a
//! seat ([`crate::controllers::bindings::grapple_button`]) fires the hook at the seated aim's point
//! (the off-hand's ray, cast by the aim update since the game's targeting does not run), and the
//! hook's hold tethers it ([`engine::tether`]). From there it is an ordinary tether: the button's
//! double tap releases it and its hold tightens it.
//!
//! [`GrapplingHook::m_GrappleFireInputEnabled`]: jc3gi::equipment::grappling_hook::GrapplingHook::m_GrappleFireInputEnabled
//! [`VehicleGrappleHook`]: jc3gi::equipment::grappling_hook::VehicleGrappleHook

mod engine;
#[cfg(test)]
mod tests;

use std::num::NonZeroUsize;

pub(crate) use engine::WireReading;
use glam::Vec3;
use jc3gi::{character::character::Character, equipment::grappling_hook::GrapplingHook};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::Hand,
        haptics::{self, Event},
    },
    grapple::hook::local,
};

/// What the bindings hand the seat grapple's tick.
pub(crate) struct SeatTickInput {
    /// The seat grapple is switched on and stands in for the game's ([`stands_in`]).
    pub seated: bool,
    /// The grapple button fired this tick.
    pub fire: bool,
    /// The seated aim's point, while it holds something in the grapple's range.
    pub target: Option<Vec3>,
    /// The tick's length, game seconds.
    pub dt: f32,
    pub hand: Hand,
}

/// The seat grapple's readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SeatStatus {
    /// The hook is flying from the seat.
    pub flying: bool,
    /// Fires, tethers made, and misses since the payload loaded.
    pub fires: u64,
    pub tethers: u64,
    pub misses: u64,
    /// Why the last press did not fire, if it did not.
    pub refused: Option<Refusal>,
}

/// Why a press did not fire the seat grapple.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The seated aim holds nothing in range.
    NoTarget,
    /// No local player, hook, or vehicle.
    NoPlayer,
    /// A line is out, or a fire or a reel is under way.
    Busy,
    /// The hook did not take the fire.
    FireRefused,
}

impl Refusal {
    pub const fn label(self) -> &'static str {
        match self {
            Refusal::NoTarget => "nothing in range",
            Refusal::NoPlayer => "no player",
            Refusal::Busy => "hook busy",
            Refusal::FireRefused => "fire refused",
        }
    }
}

/// Whether `character` rides in a vehicle seat, as the game's own vehicle grapple tests it: it has
/// a vehicle, and is in one of the riding states ([`Character::IsInRidingInVehicleState`]). The one
/// test of being in a seat for the seat grapple's button, aim, and hook, and for the passenger's
/// body aim.
///
/// # Safety
///
/// `character` must be live, on the game thread.
///
/// [`Character::IsInRidingInVehicleState`]: jc3gi::character::character::Character::IsInRidingInVehicleState
pub(crate) unsafe fn rides_in_seat(character: &Character) -> bool {
    !unsafe { character.GetVehiclePtr() }.is_null()
        && unsafe { character.IsInRidingInVehicleState() }
}

/// Whether the seat grapple stands in for the game's grapple for `character` with its hook `hook`:
/// it rides in a seat ([`rides_in_seat`]) and the game refuses the hook's fire there.
///
/// # Safety
///
/// `character` and `hook` must be live, on the game thread.
pub(crate) unsafe fn stands_in(character: &Character, hook: &GrapplingHook) -> bool {
    unsafe { !hook.GetFireInputEnabled() && rides_in_seat(character) }
}

/// [`stands_in`] for the local player.
///
/// # Safety
///
/// The game thread.
pub(crate) unsafe fn stands_in_for_local() -> bool {
    unsafe { local() }.is_some_and(|(character, hook)| unsafe { stands_in(character, hook) })
}

/// The seat grapple's readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> SeatStatus {
    STATE.lock().status
}

/// Run the seat grapple's part of the input tick.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn tick(input: &SeatTickInput) {
    let phase = STATE.lock().phase;
    let reading = phase.wire().map(|wire| unsafe { engine::read(wire) });
    let (phase, act) = step(phase, input, reading);
    let (phase, record) = match act {
        Act::None => (phase, Record::None),
        Act::Fire(at) => match unsafe { engine::fire(at) } {
            Ok(wire) => {
                tracing::info!(target: "grapple", ?at, "seat grapple: fired");
                (Phase::Flying { wire, flown_s: 0.0 }, Record::Fired)
            }
            Err(why) => (phase, Record::Refused(why)),
        },
        Act::Refuse(why) => (phase, Record::Refused(why)),
        Act::Tether(wire) => {
            if unsafe { engine::tether(wire) } {
                tracing::info!(target: "grapple", "seat grapple: tethered to the vehicle");
                haptics::play(input.hand, Event::LineLock);
                (phase, Record::Tethered)
            } else {
                unsafe { engine::cancel(wire) };
                tracing::info!(target: "grapple", "seat grapple: the tether was refused");
                (phase, Record::Missed)
            }
        }
        Act::Missed => {
            tracing::info!(target: "grapple", "seat grapple: missed");
            (phase, Record::Missed)
        }
        Act::Cancel(wire) => {
            unsafe { engine::cancel(wire) };
            tracing::info!(target: "grapple", "seat grapple: the hook was called back");
            (phase, Record::Missed)
        }
    };
    let mut state = STATE.lock();
    state.phase = phase;
    let status = &mut state.status;
    status.flying = matches!(phase, Phase::Flying { .. });
    match record {
        Record::None => {}
        Record::Fired => {
            status.fires += 1;
            status.refused = None;
        }
        Record::Refused(why) => {
            status.refused = Some(why);
            tracing::info!(target: "grapple", why = why.label(), "seat grapple: refused");
        }
        Record::Tethered => status.tethers += 1,
        Record::Missed => status.misses += 1,
    }
}

/// Call back a hook still flying from the seat, and forget it: the controllers are left, so no
/// later tick would tether or cancel it, and a wire left flying keeps the hook busy for every
/// later fire. A pause of the controllers (the debug panel, a lost frame) does not call this: the
/// phase is kept, and the next tick tethers the hook if it holds and the character is still
/// seated, or calls it back otherwise.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn stand_down() {
    let phase = {
        let mut state = STATE.lock();
        state.status.flying = false;
        std::mem::take(&mut state.phase)
    };
    if let Some(wire) = phase.wire() {
        unsafe { engine::cancel(wire) };
        tracing::info!(target: "grapple", "seat grapple: the hook was called back on leaving the controllers");
    }
}

/// How long the hook may fly from the seat before it is called back, game seconds. The hook flies
/// the grapple's range in well under a second.
pub(crate) const FLIGHT_S: f32 = 2.0;

/// The seat grapple between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    /// The hook flies along `wire`, for `flown_s`.
    Flying { wire: NonZeroUsize, flown_s: f32 },
}

impl Phase {
    fn wire(self) -> Option<NonZeroUsize> {
        match self {
            Phase::Idle => None,
            Phase::Flying { wire, .. } => Some(wire),
        }
    }
}

/// What a tick asks of the game.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Act {
    None,
    /// Fire the hook at this point.
    Fire(Vec3),
    /// The press found nothing to fire at.
    Refuse(Refusal),
    /// The hook holds: tether the wire to the vehicle.
    Tether(NonZeroUsize),
    /// The wire ended before the hook held.
    Missed,
    /// Call the hook back: it flew too long, or the seat was left.
    Cancel(NonZeroUsize),
}

/// One tick of the seat grapple: the phase it moves to, and what it asks of the game. `reading`
/// is the flying wire's, when one flies. Pure.
pub(crate) fn step(
    phase: Phase,
    input: &SeatTickInput,
    reading: Option<WireReading>,
) -> (Phase, Act) {
    match phase {
        Phase::Idle if input.seated && input.fire => match input.target {
            Some(at) => (Phase::Idle, Act::Fire(at)),
            None => (Phase::Idle, Act::Refuse(Refusal::NoTarget)),
        },
        Phase::Idle => (Phase::Idle, Act::None),
        Phase::Flying { wire, flown_s } => match reading {
            None | Some(WireReading::Gone) => (Phase::Idle, Act::Missed),
            Some(WireReading::Attached) if input.seated => (Phase::Idle, Act::Tether(wire)),
            Some(_) if !input.seated => (Phase::Idle, Act::Cancel(wire)),
            Some(_) => {
                let dt = if input.dt.is_finite() {
                    input.dt.max(0.0)
                } else {
                    0.0
                };
                let flown_s = flown_s + dt;
                if flown_s >= FLIGHT_S {
                    (Phase::Idle, Act::Cancel(wire))
                } else {
                    (Phase::Flying { wire, flown_s }, Act::None)
                }
            }
        },
    }
}

/// What a tick's act came to, for the readout.
enum Record {
    None,
    Fired,
    Refused(Refusal),
    Tethered,
    Missed,
}

struct State {
    phase: Phase,
    status: SeatStatus,
}

/// A leaf lock: nothing else is taken while it is held, and it is never held across a game call.
/// The input tick is its one writer.
static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    status: SeatStatus {
        flying: false,
        fires: 0,
        tethers: 0,
        misses: 0,
        refused: None,
    },
});
