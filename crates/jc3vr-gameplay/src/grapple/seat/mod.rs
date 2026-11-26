//! The seat grapple.

mod engine;
#[cfg(test)]
mod tests;

use std::num::NonZeroUsize;

pub(crate) use engine::{Target, WireReading};
use glam::Vec3;
use jc3vr_body::controllers::{
    facts::seat::stands_in,
    haptics,
    intent::grapple_button::{SeatRequest, TAP_S},
    physics::grapple_hook::local,
};
use jc3vr_input::{frame::Hand, haptics::cues::Event};
use parking_lot::Mutex;

/// What the bindings hand the seat grapple's tick.
pub(crate) struct SeatTickInput {
    /// The seat grapple is switched on and stands in for the game's ([`stands_in`]).
    pub seated: bool,
    /// The grapple button fired this tick.
    pub fire: Option<SeatRequest>,
    pub held: bool,
    pub release_all: bool,
    /// The seated aim's point, while it holds something in the grapple's range.
    pub target: Option<Target>,
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
    // Clearing tethers also applies when the native hook handles fire, outside `stands_in`.
    if input.release_all
        && let Some((_, hook)) = unsafe { local() }
    {
        unsafe { hook.ReleaseAllDualTethers() };
    }
    let phase = STATE.lock().phase;
    let mut reading = phase.wire().map(|wire| unsafe { engine::read(wire) });
    if let Phase::Flying {
        wire,
        released: false,
        ..
    } = phase
        && input.held
        && input.seated
        && !input.release_all
        && matches!(reading, Some(WireReading::Attached | WireReading::Flying))
        && !unsafe { engine::track_hand(wire) }
    {
        reading = Some(WireReading::Gone);
    }
    let phase = match phase {
        Phase::Flying {
            wire,
            flown_s,
            held_s,
            first,
            released,
            second,
        } if reading == Some(WireReading::Attached) => Phase::Flying {
            wire,
            flown_s,
            held_s,
            first: unsafe { engine::attachment_point(wire) }.unwrap_or(first),
            released,
            second,
        },
        phase => phase,
    };
    let (phase, act) = step(phase, input, reading);
    let (phase, record) = match act {
        Act::None => (phase, Record::None),
        Act::Fire(pending) => match unsafe { engine::fire(pending.target) } {
            Ok(wire) => {
                tracing::info!(target: "grapple", request = ?pending.request, "seat grapple: fired");
                (pending.phase(wire), Record::Fired)
            }
            Err(why) => (phase, Record::Refused(why)),
        },
        Act::Refuse(why) => (phase, Record::Refused(why)),
        Act::Tether(wire, second) => {
            if unsafe { engine::tether(wire, second) } {
                tracing::info!(target: "grapple", dual = second.is_some(), "seat grapple: tether handed off");
                haptics::play(input.hand, Event::LineLock);
                (phase, Record::Tethered)
            } else {
                unsafe { engine::cancel(wire) };
                tracing::info!(target: "grapple", "seat grapple: the tether was refused");
                (Phase::Cancelling { wire, target: None }, Record::Missed)
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
    status.flying = !matches!(phase, Phase::Idle);
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
    Flying {
        wire: NonZeroUsize,
        flown_s: f32,
        held_s: f32,
        first: Vec3,
        released: bool,
        second: Option<Target>,
    },
    Cancelling {
        wire: NonZeroUsize,
        target: Option<Pending>,
    },
}

impl Phase {
    fn wire(self) -> Option<NonZeroUsize> {
        match self {
            Phase::Idle => None,
            Phase::Flying { wire, .. } | Phase::Cancelling { wire, .. } => Some(wire),
        }
    }
}

/// What a tick asks of the game.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Act {
    None,
    /// Fire the hook at this point.
    Fire(Pending),
    /// The press found nothing to fire at.
    Refuse(Refusal),
    /// The hook holds: tether the wire to the vehicle.
    Tether(NonZeroUsize, Option<Target>),
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
    let dt = if input.dt.is_finite() {
        input.dt.max(0.0)
    } else {
        0.0
    };
    let target = input.target.filter(|at| at.point.is_finite());
    match phase {
        Phase::Idle if input.seated && input.fire.is_some() && !input.release_all => (
            Phase::Idle,
            Pending::capture(input).map_or(Act::Refuse(Refusal::NoTarget), Act::Fire),
        ),
        Phase::Idle => (Phase::Idle, Act::None),
        Phase::Cancelling {
            wire,
            target: pending,
        } => {
            let pending = if !input.seated || input.release_all {
                None
            } else if input.fire.is_some() {
                Pending::capture(input)
            } else {
                pending.and_then(|pending| pending.observe(input, dt))
            };
            if reading == Some(WireReading::Ready) {
                (Phase::Idle, pending.map_or(Act::None, Act::Fire))
            } else {
                (
                    Phase::Cancelling {
                        wire,
                        target: pending,
                    },
                    Act::None,
                )
            }
        }
        Phase::Flying {
            wire,
            flown_s,
            held_s,
            first,
            released,
            second,
        } => {
            let cancel = |target| (Phase::Cancelling { wire, target }, Act::Cancel(wire));
            if !input.seated || input.release_all {
                return cancel(None);
            }
            if input.fire.is_some() {
                return cancel(Pending::capture(input));
            }
            let held_s = held_s + if input.held && !released { dt } else { 0.0 };
            let second = if !released && !input.held {
                target
            } else {
                second
            };
            let released = released || !input.held;
            let long_hold = held_s >= TAP_S;
            if released && long_hold && second.is_none() {
                return cancel(None);
            }
            match reading {
                None | Some(WireReading::Ready) => (Phase::Idle, Act::Missed),
                Some(WireReading::Gone) => cancel(None),
                Some(WireReading::Attached) if released => (
                    Phase::Idle,
                    Act::Tether(wire, if long_hold { second } else { None }),
                ),
                Some(WireReading::Attached) => (
                    Phase::Flying {
                        wire,
                        flown_s: 0.0,
                        held_s,
                        first,
                        released,
                        second,
                    },
                    Act::None,
                ),
                Some(WireReading::Flying) => {
                    let flown_s = flown_s + dt;
                    if flown_s >= FLIGHT_S {
                        cancel(None)
                    } else {
                        (
                            Phase::Flying {
                                wire,
                                flown_s,
                                held_s,
                                first,
                                released,
                                second,
                            },
                            Act::None,
                        )
                    }
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Pending {
    pub target: Target,
    pub request: SeatRequest,
    pub held_s: f32,
    pub released: bool,
    pub second: Option<Target>,
}

impl Pending {
    fn phase(self, wire: NonZeroUsize) -> Phase {
        Phase::Flying {
            wire,
            flown_s: 0.0,
            held_s: self.held_s,
            first: self.target.point,
            released: self.released,
            second: self.second,
        }
    }

    fn capture(input: &SeatTickInput) -> Option<Self> {
        let request = input.fire?;
        let pending = Self {
            target: input.target.filter(|at| at.point.is_finite())?,
            request,
            held_s: if request == SeatRequest::Hold {
                TAP_S
            } else {
                0.0
            },
            released: request == SeatRequest::Tap,
            second: None,
        };
        pending.observe(input, 0.0)
    }

    fn observe(mut self, input: &SeatTickInput, dt: f32) -> Option<Self> {
        if !self.released {
            if input.held {
                self.held_s += dt;
            } else {
                self.released = true;
                self.second = input.target.filter(|at| at.point.is_finite());
            }
        }
        (!(self.released && self.held_s >= TAP_S && self.second.is_none())).then_some(self)
    }
}

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
