//! Haptics routing: the plan of record's "Recoil and haptics"
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! The game's rumble reaches the controllers through two detours, so that a vehicle's continuous
//! engine and road rumble is covered as well as every one-off effect:
//!
//! - a pre-call detour on [`RumbleManager::SpawnRumble`], which every one-off rumble goes through
//!   (the weapon fire rumble, the player's damage rumbles, collisions, animation track messages and
//!   melee hits, the reel-in kick, the force pulse, and the event-driven rumbles), picks out a
//!   weapon's own fire rumble inside a shot of the gun held in a hand
//!   ([`crate::controllers::weapons::fire`] marks the shot) and pulses that hand, raised by the
//!   shot's recoil kick. It always lets the game's own call through, since the effect it starts
//!   still needs to exist for the game's own per-frame sum below;
//! - a pre-call detour on [`Gamepad::SetMotorValue`] sees the result of that sum: the manager's
//!   per-frame, already-decayed and distance-attenuated left and right (low- and high-frequency)
//!   motor strength, covering every active rumble at once, one-off or continuous, including a
//!   vehicle's own engine and road rumble, which never calls `SpawnRumble` at all. It forwards the
//!   low motor to the left hand and the high motor to the right hand as a continuous pulse, skips a
//!   hand that a shot just pulsed precisely (so the two paths do not double up on the firing hand),
//!   and, unless [`HapticsConfig::gamepad_rumble`] says otherwise, replaces the value it passes
//!   through with zero so the physical gamepad stays still while the controllers take over.
//!
//! The grapple hand's pulses (the hook leaving the gauntlet, the hook attaching, and a yank) come
//! from the input tick ([`pulse`]). Every pulse lands in the haptics queue
//! ([`crate::vr::input::request_haptic`]), which the VR frame loop flushes.

use std::{
    ffi::{CStr, c_char},
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::{
    input::gamepad::Gamepad,
    rumble_manager::{RumbleEffectType, RumbleManager},
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        config::ControllersConfig,
        frame::Hand,
        scheme,
        weapons::{
            config::{HapticsConfig, PulseSpec},
            fire,
        },
    },
    vr::input::{HapticPulse, request_haptic},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SPAWN_RUMBLE_BINDER)
        .with_static_binder(&GAMEPAD_SET_MOTOR_VALUE_BINDER)
}

/// Where a pulse came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PulseSource {
    /// A weapon fire rumble, on the hand whose gun fired.
    Shot,
    /// A shot of a weapon with no fire rumble of its own: the recoil kick alone.
    Kick,
    /// A collision rumble.
    Collision,
    /// The game's continuous per-frame rumble, forwarded as it decays: damage, impacts, landings,
    /// animation track messages, a vehicle's engine and road rumble, and anything else that is not
    /// a weapon's own fire rumble.
    Continuous,
    GrappleFire,
    GrappleAttach,
    Yank,
    /// A grenade primed in the hand, and the warning as its fuse runs down.
    GrenadeFuse,
    /// A grab at a pouch that gave nothing: the game refused a grenade or a charge.
    PouchRefused,
    /// A step of the true reload: an eject, a round taken from the pouch, caught by the well, or
    /// seated, a belt reload, or the action worked.
    Reload,
    /// A controller chord the mod acts on: the recenter or the debug panel.
    Chord,
    /// A trigger pull of an empty gun that cannot reload.
    DryFire,
    /// A C4 charge primed in the hand, and a press of the wrist detonator.
    C4,
    /// An object taken hold of, a weapon picked up, or an object let go of by a physical hand.
    Pickup,
    /// The weapon wheel opened, its choice changed, or its release committed a weapon.
    WeaponWheel,
    /// The held steering wheel reached its lock or passed centre.
    SteeringWheel,
    /// A hand entering a holster or a pouch, a draw, a holster, or a foregrip grab or release.
    GrabVolume,
}

impl PulseSource {
    pub const fn label(self) -> &'static str {
        match self {
            PulseSource::Shot => "shot",
            PulseSource::Kick => "recoil kick",
            PulseSource::Collision => "collision",
            PulseSource::Continuous => "game rumble",
            PulseSource::GrappleFire => "grapple fire",
            PulseSource::GrappleAttach => "grapple attach",
            PulseSource::Yank => "yank",
            PulseSource::GrenadeFuse => "grenade fuse",
            PulseSource::PouchRefused => "pouch refused",
            PulseSource::Reload => "reload",
            PulseSource::Chord => "chord",
            PulseSource::DryFire => "dry fire",
            PulseSource::C4 => "C4",
            PulseSource::Pickup => "pickup",
            PulseSource::WeaponWheel => "weapon wheel",
            PulseSource::SteeringWheel => "steering wheel",
            PulseSource::GrabVolume => "grab volume",
        }
    }
}

/// One routed pulse, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HapticEvent {
    pub source: PulseSource,
    /// The amplitude each hand was sent, indexed by [`Hand::index`].
    pub amplitudes: [Option<f32>; 2],
    pub duration_s: f32,
}

/// The haptics readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HapticsStatus {
    /// The most recent pulses, newest last.
    pub events: Vec<HapticEvent>,
    pub routed: u64,
}

pub fn status() -> HapticsStatus {
    STATUS.lock().clone()
}

/// A game rumble's motor strengths.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Motors {
    pub left: f32,
    pub right: f32,
    pub left_trigger: f32,
    pub right_trigger: f32,
}

impl Motors {
    /// The strongest motor.
    pub fn strongest(&self) -> f32 {
        self.left
            .max(self.right)
            .max(self.left_trigger)
            .max(self.right_trigger)
    }
}

/// A shot's pulse amplitude: its rumble's strength raised by its recoil kick (radians).
pub fn shot_amplitude(strength: f32, kick_rad: f32, config: &HapticsConfig) -> f32 {
    (strength + kick_rad.abs() * config.kick_amplitude_per_rad).clamp(0.0, 1.0)
}

/// The duration a game rumble plays for on a controller.
pub fn rumble_duration(time: f32, config: &HapticsConfig) -> f32 {
    if time.is_finite() {
        time.clamp(0.0, config.max_pulse_s.max(0.0))
    } else {
        0.0
    }
}

/// Queue a fixed pulse on `hand`, when the haptics are on under the controller scheme.
pub fn pulse(hand: Hand, spec: PulseSpec, source: PulseSource) {
    let config = Config::lock_query(|c| c.controllers);
    if !acting(&config) {
        return;
    }
    send(hand, spec.amplitude, spec.duration_s, source);
}

/// Queue a shot's pulse on `hand` for a shot whose weapon has no fire rumble: the kick alone.
pub(super) fn kick_pulse(hand: Hand, kick_rad: f32) {
    let config = Config::lock_query(|c| c.controllers);
    if !(acting(&config) && config.weapons.haptics.game_rumble) {
        return;
    }
    let amplitude = shot_amplitude(0.0, kick_rad, &config.weapons.haptics);
    if amplitude > MIN_AMPLITUDE {
        send(hand, amplitude, KICK_PULSE_S, PulseSource::Kick);
    }
}

const WEAPON_FIRE: &str = "WeaponFire";
/// Below this, a motor counts as still.
const MIN_AMPLITUDE: f32 = 0.01;
/// How long a kick-only pulse lasts, seconds.
const KICK_PULSE_S: f32 = 0.03;
/// How many pulses the readout keeps.
const MAX_EVENTS: usize = 8;

static STATUS: Mutex<HapticsStatus> = Mutex::new(HapticsStatus {
    events: Vec::new(),
    routed: 0,
});

/// Until when each hand's shot or kick pulse holds off the continuous motor forward on that hand,
/// indexed by [`Hand::index`], so the two paths do not double up on the firing hand.
static SHOT_HOLD_UNTIL: Mutex<[Option<Instant>; 2]> = Mutex::new([None, None]);

fn acting(config: &ControllersConfig) -> bool {
    config.enabled && config.weapons.haptics.enabled && scheme::is_controllers()
}

fn send(hand: Hand, amplitude: f32, duration_s: f32, source: PulseSource) {
    if matches!(source, PulseSource::Shot | PulseSource::Kick) {
        hold_against_continuous(hand, duration_s);
    }
    let mut amplitudes = [None; 2];
    amplitudes[hand.index()] = Some(amplitude);
    send_each(amplitudes, duration_s, source);
}

fn send_each(amplitudes: [Option<f32>; 2], duration_s: f32, source: PulseSource) {
    let duration = Duration::try_from_secs_f32(duration_s).unwrap_or(Duration::ZERO);
    if duration.is_zero() || amplitudes.iter().all(Option::is_none) {
        return;
    }
    for hand in Hand::BOTH {
        if let Some(amplitude) = amplitudes[hand.index()] {
            request_haptic(
                hand,
                HapticPulse {
                    duration,
                    frequency_hz: None,
                    amplitude: amplitude.clamp(0.0, 1.0),
                },
            );
        }
    }
    let mut status = STATUS.lock();
    status.routed += 1;
    status.events.push(HapticEvent {
        source,
        amplitudes,
        duration_s,
    });
    let excess = status.events.len().saturating_sub(MAX_EVENTS);
    status.events.drain(..excess);
}

/// Hold the continuous motor forward off `hand` for `duration_s`: the length of the shot or kick
/// pulse just sent, so it owns the hand until it has fully decayed.
fn hold_against_continuous(hand: Hand, duration_s: f32) {
    let until = Duration::try_from_secs_f32(duration_s.max(0.0))
        .ok()
        .filter(|d| !d.is_zero())
        .map(|d| Instant::now() + d);
    SHOT_HOLD_UNTIL.lock()[hand.index()] = until;
}

/// Whether a shot or kick pulse still holds `hand` off the continuous motor forward.
fn held_against_continuous(hand: Hand) -> bool {
    SHOT_HOLD_UNTIL.lock()[hand.index()].is_some_and(|until| Instant::now() < until)
}

/// Every one-off rumble, turned into a controller pulse before the game spawns it.
#[detour(address = jc3gi::rumble_manager::RumbleManager::SpawnRumble_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn spawn_rumble(
    manager: *mut RumbleManager,
    effect_type: RumbleEffectType,
    position: *const Vector3,
    left_motor_value: f32,
    right_motor_value: f32,
    max_distance: f32,
    time: f32,
    debug_name: *const u8,
    left_trigger_motor_value: f32,
    right_trigger_motor_value: f32,
) {
    let motors = Motors {
        left: left_motor_value,
        right: right_motor_value,
        left_trigger: left_trigger_motor_value,
        right_trigger: right_trigger_motor_value,
    };
    route(motors, time, debug_name);
    SPAWN_RUMBLE.get().unwrap().call(
        manager,
        effect_type,
        position,
        left_motor_value,
        right_motor_value,
        max_distance,
        time,
        debug_name,
        left_trigger_motor_value,
        right_trigger_motor_value,
    )
}

/// Picks a weapon fire rumble out of every one-off the game spawns, and pulses the hand whose gun
/// fired it, raised by the shot's recoil kick. Every other one-off rumble is left for the
/// continuous motor forward below, once the game's own per-frame sum (which this one still needs
/// to run through unaffected) folds it in.
fn route(motors: Motors, time: f32, debug_name: *const u8) {
    let config = Config::lock_query(|c| c.controllers);
    if !(acting(&config) && config.weapons.haptics.game_rumble) {
        return;
    }
    let haptics = &config.weapons.haptics;
    let name = (!debug_name.is_null())
        .then(|| unsafe { CStr::from_ptr(debug_name.cast::<c_char>()) })
        .and_then(|name| name.to_str().ok())
        .unwrap_or("");
    if name != WEAPON_FIRE {
        return;
    }
    let Some((hand, kick_rad)) = fire::rumble_in_shot() else {
        return;
    };
    let duration_s = rumble_duration(time, haptics);
    let amplitude = shot_amplitude(motors.strongest(), kick_rad, haptics);
    send(hand, amplitude, duration_s, PulseSource::Shot);
}

/// Which hand a gamepad motor index drives: motor 0 (low frequency) the left hand, motor 1 (high
/// frequency, which also carries the trigger motors) the right hand, matching how a one-off
/// rumble's own left and right motors are already understood to sit on each hand. Any other index
/// is not one the game uses.
pub fn motor_hand(motor: i32) -> Option<Hand> {
    match motor {
        0 => Some(Hand::Left),
        1 => Some(Hand::Right),
        _ => None,
    }
}

/// What the continuous motor forward does with the rumble manager's per-frame `value` for `motor`:
/// the pulse to queue, if any, and the value to actually pass through to the physical gamepad.
/// `send_pulse` is false while forwarding is off or the hand is held by a shot's own pulse (see
/// [`held_against_continuous`]); `suppress_gamepad` silences a motor the game uses regardless of
/// whether a pulse was queued for it, so the pad stays quiet even under the amplitude floor.
pub fn continuous_motor(
    motor: i32,
    value: f32,
    send_pulse: bool,
    suppress_gamepad: bool,
) -> ContinuousMotor {
    let hand = motor_hand(motor);
    let pulse = hand.filter(|_| send_pulse).and_then(|hand| {
        let amplitude = value.clamp(0.0, 1.0);
        (amplitude > MIN_AMPLITUDE).then_some((hand, amplitude))
    });
    let passthrough = if hand.is_some() && suppress_gamepad {
        0.0
    } else {
        value
    };
    ContinuousMotor { pulse, passthrough }
}

/// The result of [`continuous_motor`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ContinuousMotor {
    /// The hand to pulse and its amplitude, if the motor's value is forwarded this call.
    pub pulse: Option<(Hand, f32)>,
    /// The value to actually pass to [`Gamepad::SetMotorValue`]'s real implementation.
    pub passthrough: f32,
}

/// The rumble manager's per-frame push of its summed motor strengths to the gamepad: every active
/// rumble at once, one-off and continuous alike, already decayed and distance-attenuated by the
/// game itself.
#[detour(address = jc3gi::input::gamepad::Gamepad::SetMotorValue_ADDRESS)]
fn gamepad_set_motor_value(gamepad: *mut Gamepad, motor: i32, value: f32) {
    let passthrough = route_continuous(motor, value);
    GAMEPAD_SET_MOTOR_VALUE
        .get()
        .unwrap()
        .call(gamepad, motor, passthrough);
}

fn route_continuous(motor: i32, value: f32) -> f32 {
    let config = Config::lock_query(|c| c.controllers);
    if !acting(&config) {
        return value;
    }
    let haptics = config.weapons.haptics;
    let send_pulse =
        haptics.game_rumble && motor_hand(motor).is_some_and(|hand| !held_against_continuous(hand));
    let result = continuous_motor(motor, value, send_pulse, !haptics.gamepad_rumble);
    if let Some((hand, amplitude)) = result.pulse {
        send(
            hand,
            amplitude,
            haptics.continuous_pulse_s,
            PulseSource::Continuous,
        );
    }
    result.passthrough
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strongest_motor_wins() {
        let motors = Motors {
            left: 0.4,
            right: 0.6,
            left_trigger: 0.1,
            right_trigger: 0.3,
        };
        assert_eq!(motors.strongest(), 0.6);
    }

    #[test]
    fn motor_0_is_the_left_hand_and_motor_1_the_right() {
        assert_eq!(motor_hand(0), Some(Hand::Left));
        assert_eq!(motor_hand(1), Some(Hand::Right));
        assert_eq!(motor_hand(2), None);
        assert_eq!(motor_hand(-1), None);
    }

    #[test]
    fn a_forwarded_motor_pulses_its_hand_and_silences_the_pad() {
        let result = continuous_motor(0, 0.6, true, true);
        assert_eq!(result.pulse, Some((Hand::Left, 0.6)));
        assert_eq!(result.passthrough, 0.0);
    }

    #[test]
    fn letting_the_gamepad_rumble_keeps_its_own_value() {
        let result = continuous_motor(1, 0.6, true, false);
        assert_eq!(result.pulse, Some((Hand::Right, 0.6)));
        assert_eq!(result.passthrough, 0.6);
    }

    #[test]
    fn a_held_hand_is_not_pulsed_but_the_pad_is_still_silenced() {
        let result = continuous_motor(0, 0.6, false, true);
        assert_eq!(result.pulse, None);
        assert_eq!(result.passthrough, 0.0);
    }

    #[test]
    fn a_motor_below_the_floor_is_not_pulsed() {
        let result = continuous_motor(0, MIN_AMPLITUDE / 2.0, true, true);
        assert_eq!(result.pulse, None);
    }

    #[test]
    fn an_unsupported_motor_is_left_untouched() {
        let result = continuous_motor(2, 0.9, true, true);
        assert_eq!(result.pulse, None);
        assert_eq!(result.passthrough, 0.9);
    }

    #[test]
    fn a_motor_value_past_one_clamps_its_pulse_amplitude() {
        let result = continuous_motor(0, 1.5, true, false);
        assert_eq!(result.pulse, Some((Hand::Left, 1.0)));
    }

    #[test]
    fn a_bigger_kick_pulses_harder() {
        let config = HapticsConfig::new();
        let small = shot_amplitude(0.2, 0.01, &config);
        let big = shot_amplitude(0.2, 0.05, &config);
        assert!(big > small && small > 0.2);
        assert_eq!(shot_amplitude(0.9, 1.0, &config), 1.0);
    }

    #[test]
    fn long_rumbles_are_capped() {
        let config = HapticsConfig::new();
        assert_eq!(rumble_duration(5.0, &config), config.max_pulse_s);
        assert_eq!(rumble_duration(0.1, &config), 0.1);
        assert_eq!(rumble_duration(f32::NAN, &config), 0.0);
    }
}
