//! Pushing up with the hands: a hand body pressed down on a surface, a ledge's top or a low wall,
//! lifts the player, so the hands vault the body up onto what they press on. The design is in
//! `docs/mod/input/physical-hands.md`, "Pushing up".
//!
//! - [`config`]: the settings.
//!
//! A hand that presses down on what does not give falls behind its goal downward: its controller is
//! below the surface its body rests on. Past [`VaultConfig::deadband_m`], that lag lifts the player
//! at [`VaultConfig::stiffness_per_s`] per metre ([`press_lift`]), as the horizontal push moves
//! them away from a wall. The lag alone is not a press: a body that falls behind its goal for any
//! other reason, such as the rig carrying the goals down faster than the bodies follow while the
//! player falls, lags downward too. So a hand's lag counts only while its reach finds a static
//! surface facing up under it ([`supported_press_lift`]), a push starts only while the player
//! stands on the ground, and nothing lifts a player moving down ([`may_lift`]). The lift lifts the rig with the player, the controller with it, so the
//! press shallows as the player rises and settles with the hands resting on the surface: the
//! player is held up for as long as the hands press, and rises no further than the press is deep.
//! It is bounded by [`VaultConfig::max_lift_mps`], and the feet rise at most
//! [`VaultConfig::max_rise_m`] above where the push started.
//!
//! A push is a deliberate act on something above the ground. A surface less than
//! [`VaultConfig::min_height_m`] above the feet takes no press ([`clear_of_the_feet`]): the ground
//! the player stands on, a kerb, a step, and the game's floor wherever the player's real floor sits
//! a little below it, so a hand reaching down for a magazine or a grenade rests on the ground
//! rather than popping the body up. And a press starts to lift only once it has stayed past the
//! deadband for [`VaultConfig::hold_s`] ([`PressTimer`]), so a hand that brushes a table or lands
//! on a car's hood with a moment's overshoot lifts nothing.
//!
//! The lift is given in the hand bodies' pre-simulation step, after the characters' proxy updates,
//! as the climb gives its velocity ([`crate::controllers::physical_hands::climb::engine`]): the
//! proxy keeps its own horizontal velocity and rises at least at the lift. While a pressing hand
//! reaches the static world, the proxy is written as supported on it, as the climb writes it, so
//! the game keeps the player on foot rather than starting a fall.

pub mod config;

pub use config::VaultConfig;
use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::climb::{Hold, engine as proxy},
};

/// The push-up's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct VaultStatus {
    /// Whether the hands lifted the player on the last step.
    pub lifting: bool,
    /// The lift given on the last step, metres per second.
    pub lift_mps: f32,
    /// Pushes up started.
    pub pushes: u64,
    /// How far the last push raised the feet, metres.
    pub last_rise_m: Option<f32>,
    /// Presses on a surface under the hand that lifted nothing because the player was off the
    /// ground or moving down, counted once per press.
    pub refusals: u64,
    /// Presses on a surface too near the feet to push on, counted once per press.
    pub at_the_feet: u64,
}

pub fn status() -> VaultStatus {
    STATE.lock().status
}

/// The lift a hand gives the player while it presses with the lag `lag` (where its body was sent,
/// less where it is, world space), metres per second: past the deadband, the downward press times
/// the stiffness, capped; zero for a press that is not mostly downward.
pub fn press_lift(lag: Vec3, tuning: &LiftTuning) -> f32 {
    let length = lag.length();
    if !(lag.is_finite() && length > f32::EPSILON) {
        return 0.0;
    }
    let down = -lag.y;
    if down < tuning.min_vertical.clamp(0.0, 1.0) * length {
        return 0.0;
    }
    let pressed = down - tuning.deadband_m.max(0.0);
    if pressed <= 0.0 {
        return 0.0;
    }
    (pressed * tuning.stiffness_per_s.max(0.0)).min(tuning.max_lift_mps.max(0.0))
}

/// The lift a hand gives the player while it presses with the lag `lag` on `support`, the static
/// surface its reach found: [`press_lift`] for a hand over a surface that faces up at least as
/// steeply as a press must point down, nothing for a hand that reaches no surface or one on a wall
/// or overhead.
pub fn supported_press_lift(lag: Vec3, support: Option<&Hold>, tuning: &LiftTuning) -> f32 {
    let Some(support) = support else {
        return 0.0;
    };
    let up = support.normal.normalize_or_zero().y;
    if !(up.is_finite() && up >= tuning.min_vertical.clamp(0.0, 1.0)) {
        return 0.0;
    }
    press_lift(lag, tuning)
}

/// Whether `support` is far enough above the feet, at `feet_y` in game space, to push on: at least
/// `min_height_m` above them.
pub fn clear_of_the_feet(support: &Hold, feet_y: f32, min_height_m: f32) -> bool {
    let height = support.point.y - feet_y;
    height.is_finite() && height >= min_height_m.max(0.0)
}

/// How long the current press has stayed past the deadband, before it lifts.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PressTimer {
    held_s: f32,
}

impl PressTimer {
    /// Advance by `dt` seconds with a press past the deadband (`pressing`) or not, and say whether
    /// the press has been held for `hold_s`: a deliberate push. A press that lets up starts over.
    pub fn step(&mut self, pressing: bool, dt: f32, hold_s: f32) -> bool {
        if !pressing {
            self.held_s = 0.0;
            return false;
        }
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.held_s += dt;
        self.held_s >= hold_s.max(0.0)
    }
}

/// Where the player is as the game's character proxy found it on this step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlayerMotion {
    /// Whether the proxy's update found it standing on a surface.
    pub grounded: bool,
    /// The proxy's vertical velocity, metres per second, positive up.
    pub vertical_mps: f32,
}

/// Whether the hands may lift a player moving as `motion` says: never one moving down faster than
/// `fall_tolerance_mps` (a fall, or the drop off a ledge), and one off the ground only while a push
/// that started on the ground goes on (`pushing`).
pub fn may_lift(motion: PlayerMotion, pushing: bool, fall_tolerance_mps: f32) -> bool {
    if !(motion.vertical_mps.is_finite() && motion.vertical_mps >= -fall_tolerance_mps.max(0.0)) {
        return false;
    }
    motion.grounded || pushing
}

/// How a press lifts, in world metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LiftTuning {
    pub deadband_m: f32,
    pub stiffness_per_s: f32,
    pub max_lift_mps: f32,
    pub min_vertical: f32,
}

impl LiftTuning {
    /// The tuning of `config` at the VR world scale `scale`.
    pub fn of(config: &VaultConfig, scale: f32) -> Self {
        Self {
            deadband_m: config.deadband_m * scale,
            stiffness_per_s: config.stiffness_per_s,
            max_lift_mps: config.max_lift_mps,
            min_vertical: config.min_vertical,
        }
    }
}

/// The velocity that keeps the player's own `current` velocity and rises at least at `lift`.
/// Called only for a player [`may_lift`] admits, so `current` never moves down by more than the
/// tolerance, and no fall is turned into a rise.
pub fn lifted(current: Vec3, lift: f32) -> Vec3 {
    Vec3::new(current.x, current.y.max(lift), current.z)
}

/// What the hands press with this step.
pub(crate) struct Press {
    /// Each acting hand's lag, for a hand that neither grips nor holds anything.
    pub lags: [Option<Vec3>; 2],
    /// The static surface each hand reaches: a press counts only on one under the hand, and the
    /// player is written as supported on it.
    pub supports: [Option<Hold>; 2],
    pub scale: f32,
    /// The step's length, seconds.
    pub dt: f32,
}

/// The push-up's part of the hand bodies' pre-simulation step: lift the player while the hands
/// press down. Returns whether it lifted.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step, after the characters' proxy updates
/// (the pre-simulation callback).
pub(crate) unsafe fn step(press: Press, config: &VaultConfig) -> bool {
    let mut state = STATE.lock();
    let tuning = LiftTuning::of(config, press.scale);
    // Each hand that presses on a surface under it, with its lift and the surface.
    let pressing = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let support = press.supports[i]?;
        let lift = supported_press_lift(press.lags[i]?, Some(&support), &tuning);
        Some((lift, support)).filter(|(lift, _)| *lift > 0.0)
    });
    if !config.enabled || pressing.iter().all(Option::is_none) {
        state.release();
        return false;
    }
    // SAFETY: forwarded from the caller.
    let feet = unsafe { proxy::player_place() }.ok().map(|p| p.feet.y);
    // SAFETY: forwarded from the caller.
    let current = unsafe { proxy::proxy_velocity() }.ok();
    let (Some(feet), Some(current)) = (feet, current) else {
        state.release();
        return false;
    };
    let clear =
        pressing.map(|p| p.filter(|(_, s)| clear_of_the_feet(s, feet, config.min_height_m)));
    let lift = clear
        .iter()
        .flatten()
        .map(|(lift, _)| *lift)
        .fold(0.0, f32::max);
    if lift <= 0.0 {
        let height_m = pressing
            .iter()
            .flatten()
            .map(|(_, s)| s.point.y - feet)
            .fold(f32::NEG_INFINITY, f32::max);
        state.refuse(Refusal::AtTheFeet { height_m });
        state.timer = PressTimer::default();
        state.finish();
        return false;
    }
    if !state.lifting && !state.timer.step(true, press.dt, config.hold_s) {
        return false;
    }
    let motion = PlayerMotion {
        // SAFETY: forwarded from the caller; the proxy's update ran before this callback, and its
        // support is read before this step writes it.
        grounded: unsafe { proxy::support() }.is_some(),
        vertical_mps: current.y,
    };
    if !may_lift(motion, state.lifting, config.fall_tolerance_mps) {
        if !state.lifting {
            state.refuse(Refusal::Motion(motion));
        }
        state.finish();
        return false;
    }
    let start = *state.start_feet_y.get_or_insert_with(|| {
        tracing::info!(
            feet_y = feet,
            lift_mps = lift,
            "physical hands: the hands push the body up"
        );
        feet
    });
    if !state.lifting {
        state.lifting = true;
        state.status.pushes += 1;
    }
    let rise = feet - start;
    if rise >= config.max_rise_m.max(0.0) * press.scale {
        state.status.lifting = false;
        state.status.lift_mps = 0.0;
        return false;
    }
    let velocity = lifted(current, lift);
    let support = clear.into_iter().flatten().map(|(_, s)| s.body).next();
    // SAFETY: forwarded from the caller; a support is a static body the hand's reach found.
    let result = match support {
        Some(ground) => unsafe { proxy::hold(velocity, ground, Vec3::ZERO) },
        None => unsafe { proxy::set_velocity(velocity) },
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "physical hands: the push up could not move the body");
        state.finish();
        return false;
    }
    state.status.lifting = true;
    state.status.lift_mps = lift;
    state.status.last_rise_m = Some(rise.max(0.0));
    true
}

/// Stop lifting: the hand bodies stood down.
pub(crate) fn stand_down() {
    STATE.lock().finish();
}

struct State {
    /// The feet's height when the push started.
    start_feet_y: Option<f32>,
    lifting: bool,
    /// How long the current press has been held before it lifts.
    timer: PressTimer,
    /// Whether the current press was refused, so that it is counted and logged once.
    refusing: bool,
    status: VaultStatus,
}

/// Why a press lifted nothing.
#[derive(Debug, Copy, Clone, PartialEq)]
enum Refusal {
    /// The player was off the ground or moving down.
    Motion(PlayerMotion),
    /// Every pressed surface was too near the feet, the highest `height_m` above them.
    AtTheFeet { height_m: f32 },
}

impl State {
    /// A press refused: count it and log it once.
    fn refuse(&mut self, refusal: Refusal) {
        if self.refusing {
            return;
        }
        self.refusing = true;
        match refusal {
            Refusal::Motion(motion) => {
                self.status.refusals += 1;
                tracing::info!(
                    grounded = motion.grounded,
                    vertical_mps = motion.vertical_mps,
                    "physical hands: the hands pressed down, but the player is off the ground or \
                     moving down, so nothing lifts"
                );
            }
            Refusal::AtTheFeet { height_m } => {
                self.status.at_the_feet += 1;
                tracing::info!(
                    height_m,
                    "physical hands: the hands pressed down on a surface at the feet, which is \
                     never pushed on"
                );
            }
        }
    }

    /// No hand presses: the press is over, refused or not.
    fn release(&mut self) {
        self.refusing = false;
        self.timer = PressTimer::default();
        self.finish();
    }

    fn finish(&mut self) {
        if self.lifting
            && let Some(rise) = self.status.last_rise_m
        {
            tracing::info!(rise_m = rise, "physical hands: the push up ended");
        }
        self.lifting = false;
        self.start_feet_y = None;
        self.status.lifting = false;
        self.status.lift_mps = 0.0;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    start_feet_y: None,
    lifting: false,
    timer: PressTimer { held_s: 0.0 },
    refusing: false,
    status: VaultStatus {
        lifting: false,
        lift_mps: 0.0,
        pushes: 0,
        last_rise_m: None,
        refusals: 0,
        at_the_feet: 0,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> LiftTuning {
        LiftTuning::of(&VaultConfig::new(), 1.0)
    }

    #[test]
    fn a_hand_pressed_down_lifts_the_body() {
        let lift = press_lift(Vec3::new(0.0, -0.16, 0.0), &tuning());
        assert!((lift - 1.0).abs() < 1e-4, "{lift}");
        // Within the deadband, resting on the surface, nothing lifts.
        assert_eq!(press_lift(Vec3::new(0.0, -0.03, 0.0), &tuning()), 0.0);
        // A deep press is capped.
        assert_eq!(press_lift(Vec3::new(0.0, -2.0, 0.0), &tuning()), 3.0);
    }

    #[test]
    fn a_press_into_a_wall_or_upward_lifts_nothing() {
        assert_eq!(press_lift(Vec3::new(0.3, -0.1, 0.0), &tuning()), 0.0);
        assert_eq!(press_lift(Vec3::new(0.0, 0.3, 0.0), &tuning()), 0.0);
        assert_eq!(press_lift(Vec3::NAN, &tuning()), 0.0);
        assert_eq!(press_lift(Vec3::ZERO, &tuning()), 0.0);
    }

    #[test]
    fn the_lift_keeps_the_players_own_motion() {
        let v = lifted(Vec3::new(1.0, -0.1, 2.0), 1.5);
        assert_eq!(v, Vec3::new(1.0, 1.5, 2.0));
        // A player already rising faster keeps their speed.
        assert_eq!(lifted(Vec3::new(0.0, 5.0, 0.0), 1.5).y, 5.0);
    }

    fn surface(normal: Vec3) -> Hold {
        Hold {
            point: Vec3::ZERO,
            normal,
            body: 7,
            moving: false,
            identity: Default::default(),
        }
    }

    #[test]
    fn only_a_surface_under_the_hand_takes_a_press() {
        let lag = Vec3::new(0.0, -0.16, 0.0);
        let floor = surface(Vec3::Y);
        let lift = supported_press_lift(lag, Some(&floor), &tuning());
        assert!((lift - 1.0).abs() < 1e-4, "{lift}");
        // A ledge's sloped top still takes a press.
        let ledge = surface(Vec3::new(0.3, 0.95, 0.0));
        assert!(supported_press_lift(lag, Some(&ledge), &tuning()) > 0.0);
        // A hand lagging down beside a wall, under a ceiling, or over nothing presses on nothing:
        // the lag of a falling rig.
        for normal in [Vec3::X, Vec3::NEG_Y, Vec3::NAN] {
            let other = surface(normal);
            assert_eq!(supported_press_lift(lag, Some(&other), &tuning()), 0.0);
        }
        assert_eq!(supported_press_lift(lag, None, &tuning()), 0.0);
    }

    fn motion(grounded: bool, vertical_mps: f32) -> PlayerMotion {
        PlayerMotion {
            grounded,
            vertical_mps,
        }
    }

    #[test]
    fn a_push_starts_only_on_the_ground() {
        let tolerance = VaultConfig::new().fall_tolerance_mps;
        assert!(may_lift(motion(true, 0.0), false, tolerance));
        // Settling onto the ground, or stepping down, is within the tolerance.
        assert!(may_lift(motion(true, -0.1), false, tolerance));
        // In the air, at the top of a jump, nothing starts.
        assert!(!may_lift(motion(false, 0.5), false, tolerance));
        // A push that started on the ground lifts the player off it.
        assert!(may_lift(motion(false, 1.0), true, tolerance));
    }

    #[test]
    fn the_ground_under_the_feet_takes_no_press() {
        let min = VaultConfig::new().min_height_m;
        let at = |y| Hold {
            point: Vec3::new(0.0, y, 0.0),
            ..surface(Vec3::Y)
        };
        // The floor, a kerb, and the game's floor a little above the player's real one.
        for y in [10.0, 10.05, 10.2] {
            assert!(!clear_of_the_feet(&at(y), 10.0, min), "{y}");
        }
        // A table, a car's hood, a ledge.
        for y in [10.75, 11.0, 11.6] {
            assert!(clear_of_the_feet(&at(y), 10.0, min), "{y}");
        }
        assert!(!clear_of_the_feet(&at(f32::NAN), 10.0, min));
        assert!(!clear_of_the_feet(&at(11.0), f32::NAN, min));
    }

    #[test]
    fn only_a_held_press_is_deliberate() {
        let config = VaultConfig::new();
        let dt = 1.0 / 64.0;
        let ticks = (config.hold_s / dt).ceil() as usize;
        let mut timer = PressTimer::default();
        let pushed: Vec<bool> = (0..ticks + 2)
            .map(|_| timer.step(true, dt, config.hold_s))
            .collect();
        assert!(pushed[..ticks - 1].iter().all(|p| !p), "{pushed:?}");
        assert!(pushed[ticks..].iter().all(|p| *p), "{pushed:?}");
        // A brush that lets up starts over.
        let mut timer = PressTimer::default();
        for _ in 0..ticks - 2 {
            assert!(!timer.step(true, dt, config.hold_s));
        }
        assert!(!timer.step(false, dt, config.hold_s));
        assert!(!timer.step(true, dt, config.hold_s));
        assert!(!timer.step(true, f32::NAN, config.hold_s));
    }

    #[test]
    fn a_falling_player_is_never_lifted() {
        let tolerance = VaultConfig::new().fall_tolerance_mps;
        for pushing in [false, true] {
            for grounded in [false, true] {
                assert!(!may_lift(motion(grounded, -4.0), pushing, tolerance));
                assert!(!may_lift(motion(grounded, f32::NAN), pushing, tolerance));
            }
        }
    }
}
