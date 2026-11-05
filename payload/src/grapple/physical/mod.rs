//! The physical grapple: the grapple hand works the line instead of buttons. The mod side is in
//! `docs/mod/input/physical-grapple.md` and the engine side in
//! `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! - [`pull`] measures the grapple hand's pull back along its aim when the reel or the retract
//!   armed: its speed and its depth;
//! - [`reel`] scales the reel by the pull's speed, and boosts it on a yank during the reel;
//! - [`tether`] turns the pull's depth into the game's retract input while tethers are attached;
//! - [`swing`] is the hang swing's pump, which the hang task's detour applies
//!   ([`crate::controllers::hands::anchor`]);
//! - [`lock`] holds the line's length during the reel while the grapple button is held, and the
//!   character swings from it;
//!
//! The yank that starts a reel along an attached line is the bindings' own gesture
//! ([`crate::controllers::bindings::yank`]); the point-and-fire of a tether is the grapple's hand
//! aim ([`crate::controllers::aim::grapple`]) with the grip's held `FIRE_GRAPPLE`. Everything here
//! coexists with the buttons: the retract the pull drives merges with the trigger's, the larger
//! value winning, and a reel the hand does not drive runs on the game's own tuning.
//!
//! [`input_tick`] runs in the bindings' game stage ([`crate::controllers::bindings::stage`]), which
//! hands it the grapple hand's grip and aim and injects the retract it returns. It reads
//! the hook, steps the pull, publishes the reel's drive for the reel task's detour, vibrates the
//! grapple hand, logs the grapple's events, and publishes the readout ([`status`]) for the
//! status rows ([`crate::status`]).

pub mod config;
pub mod lock;
pub mod pull;
pub mod reel;
pub mod swing;
pub mod tether;

use std::time::{Duration, Instant};

pub use config::{PhysicalGrappleConfig, PhysicalGrappleTuning};
use jc3gi::equipment::grappling_hook::GrapplingHookState;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        bindings::yank::{GRAPPLE_YANK, HandLine, LineYank},
        config::ControllersConfig,
        hands::anchor,
        haptics::{self, Event, Sustain},
        mode::{Detection, Traversal},
    },
    grapple::{
        hook::{self, HookRead},
        physical::{
            pull::{LinePull, PullReading},
            reel::ReelDrive,
            tether::TetherTick,
        },
    },
    headpose,
};

/// The physical grapple's hooks, aggregated by [`crate::hooks`].
pub(crate) fn hook_library() -> HookLibrary {
    reel::hook_library().with_hook_library(lock::hook_library())
}

/// What the bindings hand the input tick.
pub(crate) struct TickInput<'a> {
    /// The engine's tick delta, game seconds: the boost's length in the reel it drives.
    pub dt: f32,
    /// The tick's length, real seconds: the hand's pull and yank.
    pub real_dt: f32,
    /// The grapple hand's grip and aim, in the cockpit frame, or `None` while the hand is untracked.
    pub line: Option<HandLine>,
    /// Whether the grapple button is held: the pull retracts the tethers only while it is.
    pub button_held: bool,
    pub detection: &'a Detection,
    pub config: &'a ControllersConfig,
}

/// What the input tick hands back to the bindings.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct TickOutput {
    /// The retract input's value the pull asks for, `0..=1`, while it retracts the tethers.
    pub retract: Option<f32>,
    /// The dual tethers the hook holds.
    pub tethers: u32,
    /// The lock could take hold now: the reel task carries the character
    /// ([`Traversal::ReelingIn`]) along a line [`lock::lockable`] accepts. Or it holds already.
    pub lockable: bool,
    /// The yank fired this tick while the lock held the line: it resumes the reel.
    pub lock_yank: bool,
}

/// Where the grapple is, as the readout names it.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum GrapplePhase {
    /// No line and no fire.
    #[default]
    Inactive,
    /// A fire is committed and the fire animation has not released the hook yet.
    Firing,
    /// The hook is flying toward its target.
    InFlight,
    /// The line is attached and nothing carries the character along it.
    Attached,
    /// The character reels in along the line.
    ReelingIn,
    /// The character hangs from the line.
    Hanging,
    /// The character is reeled onto the attach point: a wall perch, a stunt, or upside down.
    ReeledIn,
    /// A scripted wire is out, fired outside the reel flow (the vehicle and stunt-position
    /// grapples' custom fire).
    ScriptedWire,
}

impl GrapplePhase {
    pub const fn label(self) -> &'static str {
        match self {
            GrapplePhase::Inactive => "INACTIVE",
            GrapplePhase::Firing => "FIRING",
            GrapplePhase::InFlight => "IN FLIGHT",
            GrapplePhase::Attached => "ATTACHED",
            GrapplePhase::ReelingIn => "REELING IN",
            GrapplePhase::Hanging => "HANGING",
            GrapplePhase::ReeledIn => "REELED IN",
            GrapplePhase::ScriptedWire => "SCRIPTED WIRE",
        }
    }
}

/// What the line is attached to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TargetReadout {
    /// Whether the latched target is an object rather than world geometry.
    pub object: bool,
    /// The distance from the head to the attach point, metres.
    pub distance_m: f32,
}

/// The physical grapple's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GrappleStatus {
    /// Whether the input tick ran recently: the controllers inject and the hand is read.
    pub live: bool,
    pub phase: GrapplePhase,
    pub target: Option<TargetReadout>,
    /// The pull on the last tick.
    pub pull: PullReading,
    /// The reel's scale from the hand while the hand drives the reel.
    pub reel_scale: Option<f32>,
    /// Whether a yank's boost applies to the reel.
    pub boosted: bool,
    /// Reels the hand drove, and boosts given, since the payload loaded.
    pub reels: u64,
    pub boosts: u64,
    /// The dual tethers the hook holds.
    pub tethers: u32,
    /// The retract value the pull drives, while it retracts.
    pub retract: Option<f32>,
    /// Retracts the pull started, since the payload loaded.
    pub retracts: u64,
    /// Whether the game squeezes the tethers, from the hand or the trigger.
    pub squeezing: bool,
    /// The highest tether tension fraction.
    pub tension: f32,
    /// The grapple hand's line vibration: the stronger of the tension's and the reel's level,
    /// `0..=1`.
    pub haptic: f32,
}

/// The latest readout. Safe from any thread: it takes only a leaf lock.
pub fn status() -> GrappleStatus {
    let status = STATUS.lock();
    GrappleStatus {
        live: status.1.is_some_and(|at| at.elapsed() < STATUS_MAX_AGE),
        ..status.0
    }
}

/// Run the physical grapple's part of the input tick. Called in the game stage while the bindings
/// inject.
///
/// # Safety
///
/// Game thread, before the world update.
pub(crate) unsafe fn input_tick(input: &TickInput) -> TickOutput {
    let config = &PhysicalGrappleTuning::of(&input.config.grapple);
    let hand = input.config.acting_hand().other();
    let hook = unsafe { read_hook() };
    let anchor = anchor::status().anchor;
    let head = headpose::query().position;
    let now = Instant::now();

    let mut pulses = Vec::new();
    let mut state = STATE.lock();
    // The reel: driven while the line carries the character toward its target.
    let reeling = matches!(
        input.detection.traversal,
        Some(Traversal::PreReel | Traversal::ReelingIn)
    );
    let drive_reel = reeling && config.reel.enabled;
    let tethers = hook.map_or(0, |h| h.tethers);
    let armed = input.button_held
        && tether::armed(
            &TetherTick {
                tethers,
                active_wire: hook.is_some_and(|h| h.read.active_wire),
                detection: *input.detection,
            },
            &config.tether,
        );
    // Each pull is measured along the aim the hand has when the reel or the retract arms, and the
    // boost's yank along the aim it has when the reel starts.
    let pull = state
        .pull
        .step(input.real_dt, input.line, drive_reel || armed, &config.pull);
    let boost = state
        .boost
        .step(input.real_dt, input.line, drive_reel, &GRAPPLE_YANK);
    let locked = lock::holds();
    let lock_yank = locked && boost.is_some();
    if let Some(yank) = boost {
        state.boost_left_s = Some(config.reel.boost_s.max(0.0));
        state.status.boosts += 1;
        tracing::info!(
            target: "grapple",
            pull_m = yank.distance_m,
            speed_mps = yank.speed_mps,
            "physical grapple: a yank boosts the reel"
        );
        pulses.push(Event::Yank);
    }
    let boosted = drive_reel && state.boost_left_s.is_some();
    state.boost_left_s = count_down(state.boost_left_s, input.dt);
    let drive = drive_reel.then(|| ReelDrive {
        scale: reel::scale_for(pull.speed_mps, &config.reel),
        boost: boosted,
        boost_needs_upgrade: config.reel.boost_needs_upgrade,
    });
    reel::publish(drive);
    match (drive, state.reel_peak) {
        (Some(drive), None) => {
            state.reel_peak = Some(drive.scale);
            state.status.reels += 1;
            tracing::info!(
                target: "grapple",
                scale = drive.scale,
                pull_mps = pull.speed_mps,
                "physical grapple: the hand drives the reel"
            );
        }
        (Some(drive), Some(peak)) => state.reel_peak = Some(peak.max(drive.scale)),
        (None, Some(peak)) => {
            state.reel_peak = None;
            state.boost_left_s = None;
            tracing::info!(
                target: "grapple",
                peak_scale = peak,
                boosts = state.status.boosts,
                "physical grapple: the reel ends"
            );
        }
        (None, None) => {}
    }

    // The tether retract: the pull's depth, while tethers are attached and free.
    // A retract that arms with the hand already back starts from that hand, not from a pull.
    let arming = armed && !state.tether_armed;
    if arming {
        state.pull.rebase();
    }
    state.tether_armed = armed;
    let retract = if armed && !arming {
        let value = tether::retract_value(pull.depth_m, &config.tether);
        (value > 0.0).then_some(value)
    } else {
        None
    };
    match (retract, state.retract_peak) {
        (Some(value), None) => {
            state.retract_peak = Some(value);
            state.status.retracts += 1;
            tracing::info!(
                target: "grapple",
                tethers,
                value,
                "physical grapple: the hand retracts the tethers"
            );
            pulses.push(Event::TetherRetract);
        }
        (Some(value), Some(peak)) => state.retract_peak = Some(peak.max(value)),
        (None, Some(peak)) => {
            state.retract_peak = None;
            tracing::info!(
                target: "grapple",
                peak_value = peak,
                "physical grapple: the tether retract ends"
            );
        }
        (None, None) => {}
    }
    if tethers != state.status.tethers {
        tracing::info!(
            target: "grapple",
            from = state.status.tethers,
            to = tethers,
            "physical grapple: the tether count changes"
        );
    }

    // The attach, for the log and the readout.
    let target = anchor.map(|point| TargetReadout {
        object: hook.is_some_and(|h| h.object_target),
        distance_m: point.distance(head),
    });
    if let (Some(target), false) = (target, state.status.target.is_some()) {
        tracing::info!(
            target: "grapple",
            object = target.object,
            distance_m = target.distance_m,
            "physical grapple: the line attaches"
        );
    }

    let squeezing = hook.is_some_and(|h| h.squeezing);
    let tension = hook.map_or(0.0, |h| h.tension);
    let readout = reel::readout();
    // The line's vibration: the tethers' tension while they are squeezed, and the reel's speed
    // while the line carries the character.
    let line = [
        if squeezing { unit(tension) } else { 0.0 },
        if drive.is_some() && !locked {
            unit(readout.speed_fraction)
        } else {
            0.0
        },
    ];
    let haptic = line[0].max(line[1]);

    state.status = GrappleStatus {
        live: true,
        phase: hook.map_or(GrapplePhase::Inactive, |h| {
            phase(
                h.read.state,
                h.read.active_wire,
                anchor.is_some(),
                h.read.firing,
                input.detection,
            )
        }),
        target,
        pull,
        reel_scale: drive.map(|d| d.scale),
        boosted: readout.boosted && drive.is_some(),
        tethers,
        retract,
        squeezing,
        tension,
        haptic,
        ..state.status
    };
    *STATUS.lock() = (state.status, Some(now));
    drop(state);

    for event in pulses {
        haptics::play(hand, event);
    }
    haptics::sustain(hand, Sustain::GrappleTension, line[0]);
    haptics::sustain(hand, Sustain::GrappleReel, line[1]);
    TickOutput {
        retract,
        tethers,
        lockable: locked
            || (input.detection.traversal == Some(Traversal::ReelingIn)
                && unsafe { lock::lockable(&input.config.grapple.lock) }),
        lock_yank,
    }
}

/// Forget the pull and stop driving the reel: the bindings stand down.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.pull = LinePull::new();
    state.boost = LineYank::new();
    state.boost_left_s = None;
    state.tether_armed = false;
    reel::publish(None);
    drop(state);
    lock::stand_down();
}

/// `left_s` after a tick of `dt` seconds: `None` once it runs out, or for a tick that is not finite.
fn count_down(left_s: Option<f32>, dt: f32) -> Option<f32> {
    let left = left_s? - dt;
    (dt.is_finite() && left > 0.0).then_some(left)
}

/// How long the readout counts as live after the last input tick.
const STATUS_MAX_AGE: Duration = Duration::from_millis(500);

/// The input tick's state.
struct State {
    pull: LinePull,
    /// The yank detector armed during a reel, whose yanks boost it.
    boost: LineYank,
    /// The game seconds the boost still lasts: it speeds the reel, which runs on the game's clock.
    boost_left_s: Option<f32>,
    /// The highest scale of the reel the hand drives, while it does.
    reel_peak: Option<f32>,
    /// Whether the tether retract was armed on the last tick.
    tether_armed: bool,
    /// The highest retract value of the retract the pull drives, while it does.
    retract_peak: Option<f32>,
    status: GrappleStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    pull: LinePull::new(),
    boost: LineYank::new(),
    boost_left_s: None,
    reel_peak: None,
    tether_armed: false,
    retract_peak: None,
    status: GrappleStatus {
        live: false,
        phase: GrapplePhase::Inactive,
        target: None,
        pull: PullReading {
            speed_mps: 0.0,
            depth_m: 0.0,
        },
        reel_scale: None,
        boosted: false,
        reels: 0,
        boosts: 0,
        tethers: 0,
        retract: None,
        retracts: 0,
        squeezing: false,
        tension: 0.0,
        haptic: 0.0,
    },
});

/// The published readout and when it was published: a leaf lock, apart from [`STATE`], so that the
/// readout's readers on other threads never wait on the input tick.
static STATUS: Mutex<(GrappleStatus, Option<Instant>)> = Mutex::new((
    GrappleStatus {
        live: false,
        phase: GrapplePhase::Inactive,
        target: None,
        pull: PullReading {
            speed_mps: 0.0,
            depth_m: 0.0,
        },
        reel_scale: None,
        boosted: false,
        reels: 0,
        boosts: 0,
        tethers: 0,
        retract: None,
        retracts: 0,
        squeezing: false,
        tension: 0.0,
        haptic: 0.0,
    },
    None,
));

/// The local player's hook as the input tick reads it: the phase's fields, and the tethers and
/// their tension as the game counts them.
#[derive(Copy, Clone)]
struct HookTick {
    read: HookRead,
    /// Whether the latched fire target is an object.
    object_target: bool,
    tethers: u32,
    tension: f32,
    squeezing: bool,
}

/// Read the local player's grappling hook, or `None` without a local character or hook.
///
/// # Safety
///
/// Game thread, before the world update.
unsafe fn read_hook() -> Option<HookTick> {
    let hook = unsafe { hook::local_hook() }?;
    let tension = unsafe { hook.GetHighestWireTension() };
    Some(HookTick {
        read: HookRead::of(hook),
        object_target: !hook.m_HookFireInfo.m_TargetGameObject.px.is_null(),
        tethers: u32::try_from(unsafe { hook.GetNumDualTethers() }).unwrap_or(0),
        tension: if tension.is_finite() { tension } else { 0.0 },
        squeezing: hook.m_IsSqueezingDualTethers,
    })
}

/// The readout's phase from the hook's fields and the mode detector's traversal.
fn phase(
    state: GrapplingHookState,
    active_wire: bool,
    attached: bool,
    firing: bool,
    detection: &Detection,
) -> GrapplePhase {
    match detection.traversal {
        Some(Traversal::PreReel | Traversal::ReelingIn) => return GrapplePhase::ReelingIn,
        Some(Traversal::PreHang) => return GrapplePhase::Hanging,
        _ => {}
    }
    if !active_wire {
        return if firing {
            GrapplePhase::Firing
        } else {
            GrapplePhase::Inactive
        };
    }
    // The reeled states are the last reel's outcome as much as the current one, so they count
    // only while the mode detector sees the character reeled in.
    let reeled_in = detection.traversal == Some(Traversal::ReeledIn);
    match state {
        GrapplingHookState::GHS_REELED_HANG if reeled_in => GrapplePhase::Hanging,
        GrapplingHookState::GHS_REELED_ATTACHED
        | GrapplingHookState::GHS_REELED_UPSIDEDOWN
        | GrapplingHookState::GHS_REELED_STUNT
            if reeled_in =>
        {
            GrapplePhase::ReeledIn
        }
        GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE => GrapplePhase::ScriptedWire,
        _ if attached => GrapplePhase::Attached,
        _ => GrapplePhase::InFlight,
    }
}

/// `value` as a share, `0..=1`, with a non-finite value as none.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{Mode, OnFootVariant};

    fn on_foot(traversal: Option<Traversal>) -> Detection {
        Detection {
            mode: Mode::OnFoot(OnFootVariant::Ground),
            traversal,
            held: None,
        }
    }

    #[test]
    fn the_phase_follows_the_hook_and_the_traversal() {
        let idle = on_foot(None);
        let inactive = GrapplingHookState::GHS_INACTIVE;
        assert_eq!(
            phase(inactive, false, false, false, &idle),
            GrapplePhase::Inactive
        );
        assert_eq!(
            phase(inactive, false, false, true, &idle),
            GrapplePhase::Firing
        );
        assert_eq!(
            phase(inactive, true, false, false, &idle),
            GrapplePhase::InFlight
        );
        assert_eq!(
            phase(inactive, true, true, false, &idle),
            GrapplePhase::Attached
        );
        assert_eq!(
            phase(
                inactive,
                true,
                true,
                false,
                &on_foot(Some(Traversal::ReelingIn))
            ),
            GrapplePhase::ReelingIn
        );
        assert_eq!(
            phase(
                GrapplingHookState::GHS_REELED_HANG,
                true,
                true,
                false,
                &on_foot(Some(Traversal::ReeledIn))
            ),
            GrapplePhase::Hanging
        );
        assert_eq!(
            phase(
                GrapplingHookState::GHS_REELED_ATTACHED,
                true,
                true,
                false,
                &on_foot(Some(Traversal::ReeledIn))
            ),
            GrapplePhase::ReeledIn
        );
        // A stale reeled state with a fresh line reads as attached.
        assert_eq!(
            phase(
                GrapplingHookState::GHS_REELED_HANG,
                true,
                true,
                false,
                &idle
            ),
            GrapplePhase::Attached
        );
        assert_eq!(
            phase(
                GrapplingHookState::GHS_REELED_ATTACHED,
                true,
                true,
                false,
                &idle
            ),
            GrapplePhase::Attached
        );
        assert_eq!(
            phase(
                GrapplingHookState::GHS_CUSTOM_ACTIVE_WIRE,
                true,
                true,
                false,
                &idle
            ),
            GrapplePhase::ScriptedWire
        );
    }

    #[test]
    fn the_boost_lasts_its_length_in_game_seconds() {
        // In slow motion at a quarter, game ticks of a quarter of a 30 Hz frame: the boost lasts
        // four times as many ticks as at full speed, the same stretch of the reel.
        let ticks = |dt: f32| {
            let mut left = Some(0.6);
            let mut n = 0;
            while left.is_some() {
                left = count_down(left, dt);
                n += 1;
            }
            n
        };
        let (fast, slow) = (ticks(1.0 / 30.0), ticks(0.25 / 30.0));
        assert!((18..=19).contains(&fast), "{fast}");
        assert!((72..=73).contains(&slow), "{slow}");
        assert_eq!(count_down(None, 0.1), None);
        assert_eq!(count_down(Some(0.5), f32::NAN), None);
    }
}
