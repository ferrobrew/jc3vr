//! Deliberate roof-mantle handoff and grip input in the game's existing helicopter hang.
//! Physical grips remain at their actual points through the moving climb. A completed mantle
//! can enter the same vehicle's stunt area, while the normal grapple still enters a hang. Standing
//! on a roof needs no grip. The design is in `docs/mod/input/stunt-positions.md`, the engine side
//! in `docs/engine/gameplay/stunt-positions.md`.
//!
//! - [`config`]: the settings;
//! - [`solve`]: when to enter a stunt position and when to let go, pure;
//! - [`engine`]: the game's stunt position, the vehicle a grip holds, and the entry.
//!
//! It builds on the hand climbing's grips on moving bodies ([`crate::controllers::physical_hands::climb`]),
//! which carry the player on the vehicle until the game takes over. While the game holds the
//! player in a stunt position ([`game_holds`]), the climb's grips hold the hands on the vehicle
//! and move nothing: the game moves the player with the vehicle.
//!
//! Everything runs in the game stage ([`finish_tick`]), on the game thread before the world
//! update, where the game's own entries are safe to call. [`game_holds`] reads a short-lived,
//! lifecycle-matched observation; missing observations conservatively yield movement.

pub mod config;
pub mod engine;
mod observation;
pub mod solve;

use std::time::{Duration, Instant};

pub use config::StuntConfig;
use jc3gi::input::input_action_map::Action;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        bindings::hold::{Desired, MapKind, Target, Want},
        fingers::grip::{self, Grip, GripSource, catalogue::GripPose},
        frame::Hand,
        haptics::{self, Event, Sustain},
        physical_hands::{
            PhysicalHandsConfig, climb,
            stunt::{
                engine::Entry,
                solve::{Decision, StuntKind, Tick, Tracker, Tuning},
            },
        },
    },
};

/// The stunt positions' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StuntStatus {
    /// The stunt position the game holds the player in, as the last input tick read it.
    pub position: Option<StuntKind>,
    /// Whether a grip has held in the current hang, so that letting go of every grip lets go.
    pub armed: bool,
    /// Which hands hold their grip in the hang, indexed by [`Hand::index`].
    pub holding: [bool; 2],
    /// Which hands' climbing grips hold a helicopter, indexed by [`Hand::index`].
    pub on_helicopter: [bool; 2],
    /// Legacy recorder counter. Physical grips never request native hang entry, so it stays zero.
    pub hang_entries: u64,
    /// The stunt areas the mantles entered.
    pub roof_entries: u64,
    /// The entries the game refused.
    pub refusals: u64,
    /// The hangs let go by opening the grips.
    pub releases: u64,
    /// How the last entry was made.
    pub last_entry: Option<Entry>,
    pub last_error: Option<String>,
}

pub fn status() -> StuntStatus {
    STATE.lock().status.clone()
}

/// Whether physical movement must yield to native attachment or an unavailable observation.
/// Only a fresh, lifecycle-matched native observation can admit physical movement.
pub(crate) fn game_holds() -> bool {
    observation::movement_owner().yields()
}

/// Whether the stunt positions are on: their switch, under the hand climbing, which they need.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    config.stunt.enabled && climb::enabled(config)
}

/// Whether the player hangs from a helicopter, so that either hand's grip holds on. Called from the
/// input tick.
pub(crate) fn hanging() -> bool {
    let state = STATE.lock();
    state.status.position == Some(StuntKind::Hanging)
        && state
            .ticked
            .is_some_and(|at| Instant::now().saturating_duration_since(at) <= STALE_AFTER)
}

/// Read the stunt position, enter a roof where a completed mantle offers it, and let go of a hang
/// whose grips opened. `held` is each hand's grip; a let-go asks the game for its jump, which
/// leaves the hang, through `desired`. Called from the input tick after the climb's.
pub(crate) fn finish_tick(held: [bool; 2], desired: &mut Vec<Desired>) {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let now = Instant::now();
    let mut state = STATE.lock();
    let dt = state
        .ticked
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32())
        .min(MAX_TICK_S);
    state.ticked = Some(now);
    // SAFETY: the game stage runs on the game thread before the world update.
    let read = unsafe { engine::position() };
    state.observe_read(read, enabled(&config));
    observation::publish(read, now);
    let Some(read) = read else {
        climb::native::reject_landing();
        state.tracker = Tracker::new();
        state.observe_position(None);
        state.status.holding = [false; 2];
        state.status.on_helicopter = [false; 2];
        return;
    };
    if !enabled(&config) {
        climb::native::reject_landing();
        return;
    }

    let gripped = climb::gripped();
    // SAFETY: as above; the grips were taken on bodies of the live world.
    let helicopters = gripped.map(|grip| unsafe { helicopter_grip(grip) });
    let landing_offer = climb::take_landing();
    let landing = landing_offer.as_ref().map(|offer| {
        let landing = offer.landing;
        // SAFETY: as above.
        let vehicle = unsafe { engine::vehicle_of(landing.body) };
        // SAFETY: as above; the vehicle owns a body of the live world.
        let distance =
            vehicle.and_then(|v| unsafe { engine::stunt_area_distance(v, landing.feet) });
        (vehicle, landing.feet, distance)
    });
    let tick = Tick {
        position: read.kind,
        held,
        roof_landing: landing.map(|(_, _, distance)| distance),
        dt,
    };
    let decision = state.tracker.step(&tick, &TUNING);
    state.status.armed = state.tracker.armed();
    state.status.on_helicopter = helicopters;
    state.status.holding = if read.kind == Some(StuntKind::Hanging) {
        held
    } else {
        [false; 2]
    };
    let mut pulses: Vec<(Hand, Event)> = Vec::new();
    let mut strains: Vec<(Hand, f32)> = Vec::new();
    match decision {
        Decision::Hold => {}
        Decision::EnterRoof => {
            if let Some((Some(vehicle), feet, distance)) = landing {
                // SAFETY: as above.
                let entered = unsafe { engine::enter(vehicle, feet) };
                if state.entered(entered, &TUNING) {
                    tracing::info!(
                        feet = ?feet,
                        distance_m = ?distance,
                        "stunt positions: the mantle took the player into the vehicle's stunt area"
                    );
                    pulses.extend(Hand::BOTH.map(|h| (h, Event::StuntEnter)));
                }
            }
        }
        Decision::LetGo => {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::JUMP,
                },
                want: Want::Click,
            });
            state.status.releases += 1;
            tracing::info!("stunt positions: every grip opened, so the hang lets go");
            pulses.extend(Hand::BOTH.map(|h| (h, Event::StuntRelease)));
        }
        Decision::LetGoIgnored => {
            tracing::warn!(
                "stunt positions: the game kept the player in the hang after the let-go's jump"
            );
        }
    }
    let skid = GripPose::Cylinder {
        diameter_m: SKID_DIAMETER_M,
    };
    let strain = hold_strain(read.vehicle_speed_mps, HOLD_FULL_SPEED_MPS);
    for hand in Hand::BOTH {
        let i = hand.index();
        let in_hang = state.status.holding[i];
        if in_hang || (held[i] && helicopters[i]) {
            grip::declare(hand, Grip::new(GripSource::Stunt).pose(skid));
        }
        if in_hang && strain >= MIN_STRAIN {
            strains.push((hand, strain));
        }
    }
    drop(state);
    for (hand, event) in pulses {
        haptics::play(hand, event);
    }
    for (hand, strain) in strains {
        haptics::sustain(hand, Sustain::StuntHang, strain);
    }
}

/// How long the last input tick's reading stays current.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// The longest tick the tracker counts, seconds, so that a hitch or a pause does not count as a
/// held grip or an open one.
const MAX_TICK_S: f32 = 0.1;

/// The least share of the hold pulse that pulses a hand.
const MIN_STRAIN: f32 = 0.1;

/// Whether a live moving grip holds a helicopter, for the hand pose and readout.
///
/// # Safety
///
/// The game thread, before the world update.
unsafe fn helicopter_grip(grip: Option<climb::Hold>) -> bool {
    let Some(grip) = grip else { return false };
    if !grip.moving || !unsafe { grip.valid() } {
        return false;
    }
    unsafe { engine::vehicle_of(grip.body) }
        .is_some_and(|vehicle| unsafe { engine::is_helicopter(vehicle) })
}

/// The tracker's tuning: enter a vehicle's stunt area when a mantle lands on its roof within
/// [`Tuning::roof_offer_m`] of the nearest one, and let go of the helicopter once both grips have
/// stayed open in its hang for [`Tuning::release_grace_s`] after a grip held there, so that a hand
/// moved from one grip to the other does not. An entry the game refused is tried again after
/// [`Tuning::retry_s`].
const TUNING: Tuning = Tuning {
    enter_roof: true,
    roof_offer_m: 0.6,
    let_go: true,
    release_grace_s: 0.2,
    retry_s: 1.0,
};

/// The diameter of the bar the fingers close round on a helicopter's skid, metres.
const SKID_DIAMETER_M: f32 = 0.05;

/// The helicopter's speed of the strongest hold pulse, metres per second.
const HOLD_FULL_SPEED_MPS: f32 = 40.0;

/// The share of the hold pulse at the vehicle's speed, `0..=1`.
fn hold_strain(speed_mps: f32, full_speed_mps: f32) -> f32 {
    if !(speed_mps.is_finite() && full_speed_mps > 0.0) {
        return 0.0;
    }
    (speed_mps / full_speed_mps).clamp(0.0, 1.0)
}

struct State {
    tracker: Tracker,
    ticked: Option<Instant>,
    status: StuntStatus,
}

impl State {
    fn observe_read(&mut self, read: Option<engine::PositionRead>, assist: bool) {
        self.observe_position(read.and_then(|r| r.kind));
        if !assist {
            self.tracker = Tracker::new();
            self.status.armed = false;
            self.status.holding = [false; 2];
            self.status.on_helicopter = [false; 2];
        }
    }

    /// Log the position as it changes.
    fn observe_position(&mut self, position: Option<StuntKind>) {
        if position == self.status.position {
            return;
        }
        match position {
            Some(kind) => tracing::info!(
                position = kind.label(),
                "stunt positions: the game holds the player in a stunt position"
            ),
            None => tracing::info!(
                from = self.status.position.map(StuntKind::label),
                "stunt positions: the player left the stunt position"
            ),
        }
        self.status.position = position;
    }

    /// Count an entry, or a refusal, which the tracker then waits out; whether it entered.
    fn entered(&mut self, entered: Result<Entry, engine::StuntError>, tuning: &Tuning) -> bool {
        match entered {
            Ok(entry) => {
                self.status.last_entry = Some(entry);
                self.status.roof_entries += 1;
                tracing::debug!(entry = entry.label(), "stunt positions: entered");
                true
            }
            Err(e) => {
                self.tracker.refused(tuning);
                self.status.refusals += 1;
                tracing::info!(error = %e, "stunt positions: the game refused the roof entry");
                self.status.last_error = Some(e.to_string());
                false
            }
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    tracker: Tracker::new(),
    ticked: None,
    status: StuntStatus {
        position: None,
        armed: false,
        holding: [false; 2],
        on_helicopter: [false; 2],
        hang_entries: 0,
        roof_entries: 0,
        refusals: 0,
        releases: 0,
        last_entry: None,
        last_error: None,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_survives_assist_disable_and_a_traversal_without_a_presentation_kind() {
        let mut state = State {
            tracker: Tracker::new(),
            ticked: None,
            status: StuntStatus::default(),
        };
        for assist in [true, false] {
            for kind in [Some(StuntKind::Standing), Some(StuntKind::Hanging), None] {
                state.observe_read(
                    Some(engine::PositionRead {
                        kind,
                        owns_movement: true,
                        vehicle_speed_mps: 0.0,
                    }),
                    assist,
                );
                assert_eq!(state.status.position, kind);
            }
        }
        state.observe_read(None, false);
        assert_eq!(state.status.position, None);
    }

    #[test]
    fn the_hold_pulse_follows_the_vehicles_speed() {
        assert_eq!(hold_strain(0.0, 40.0), 0.0);
        assert_eq!(hold_strain(20.0, 40.0), 0.5);
        assert_eq!(hold_strain(90.0, 40.0), 1.0);
        assert_eq!(hold_strain(f32::NAN, 40.0), 0.0);
        assert_eq!(hold_strain(10.0, 0.0), 0.0);
    }
}
