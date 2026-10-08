//! The game's rumble, forwarded to the controllers: the plan of record's "Recoil and haptics"
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! The game's rumble reaches the controllers through two detours, so that a vehicle's continuous
//! engine and road rumble is covered as well as every one-off effect:
//!
//! - a pre-call detour on [`RumbleManager::SpawnRumble`], which every one-off rumble goes through
//!   (the weapon fire rumble, the player's damage rumbles, collisions, animation track messages and
//!   melee hits, the reel-in kick, the force pulse, and the event-driven rumbles), picks out a
//!   weapon's own fire rumble inside a shot of the gun held in a hand
//!   ([`crate::controllers::weapons::fire`] marks the shot) and plays it on that hand as
//!   [`Event::Shot`], raised by the shot's recoil kick. It always lets the game's own call
//!   through, since the effect it starts still needs to exist for the game's own per-frame sum
//!   below;
//! - a pre-call detour on [`Gamepad::SetMotorValue`] sees the result of that sum: the manager's
//!   per-frame, already-decayed and distance-attenuated left and right (low- and high-frequency)
//!   motor strength, covering every active rumble at once, one-off or continuous, including a
//!   vehicle's own engine and road rumble, which never calls `SpawnRumble` at all. It sends the
//!   low motor to the left hand and the high motor to the right hand as [`Sustain::GameRumble`],
//!   which the mixer holds off a hand while one of the mod's one-off cues (the shot among them)
//!   plays on it, and, unless [`HapticsConfig::gamepad_rumble`] says otherwise, replaces the value
//!   it passes through with zero so the physical gamepad stays still while the controllers take
//!   over.

use std::ffi::{CStr, c_char};

use detours_macro::detour;
use jc3gi::{
    input::gamepad::Gamepad,
    rumble_manager::{RumbleEffectType, RumbleManager},
    types::math::Vector3,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        haptics::{self, Event, HapticsConfig, Sustain},
        scheme,
        weapons::fire,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SPAWN_RUMBLE_BINDER)
        .with_static_binder(&GAMEPAD_SET_MOTOR_VALUE_BINDER)
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

/// A shot's amplitude: its rumble's strength raised by its recoil kick (radians).
pub fn shot_amplitude(strength: f32, kick_rad: f32, config: &HapticsConfig) -> f32 {
    (strength + kick_rad.abs() * config.kick_amplitude_per_rad).clamp(0.0, 1.0)
}

/// The length a weapon's fire rumble plays for on a controller.
pub fn shot_duration(time: f32, config: &HapticsConfig) -> f32 {
    if time.is_finite() {
        time.clamp(0.0, config.max_shot_s.max(0.0))
    } else {
        0.0
    }
}

/// Play a shot of a weapon with no fire rumble on `hand`: the kick alone, as [`Event::Kick`].
pub(crate) fn kick(hand: Hand, kick_rad: f32) {
    let config = Config::lock_query(|c| c.controllers.haptics);
    if !config.game_rumble {
        return;
    }
    let scale = shot_amplitude(0.0, kick_rad, &config);
    if scale > MIN_AMPLITUDE {
        haptics::play_scaled(hand, Event::Kick, scale);
    }
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

/// What the motor forward does with the rumble manager's per-frame `value` for `motor`: the hand
/// and level to send, if any, and the value to actually pass through to the physical gamepad.
/// `forward` is false while forwarding is off; `suppress_gamepad` silences a motor the game uses
/// whether or not it is forwarded.
pub fn continuous_motor(
    motor: i32,
    value: f32,
    forward: bool,
    suppress_gamepad: bool,
) -> ContinuousMotor {
    let hand = motor_hand(motor);
    let level = hand
        .filter(|_| forward)
        .map(|hand| (hand, value.clamp(0.0, 1.0)));
    let passthrough = if hand.is_some() && suppress_gamepad {
        0.0
    } else {
        value
    };
    ContinuousMotor { level, passthrough }
}

/// The result of [`continuous_motor`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ContinuousMotor {
    /// The hand and its level, if the motor's value is forwarded this call. A level of zero ends
    /// the hand's game rumble at once.
    pub level: Option<(Hand, f32)>,
    /// The value to actually pass to [`Gamepad::SetMotorValue`]'s real implementation.
    pub passthrough: f32,
}

const WEAPON_FIRE: &str = "WeaponFire";
/// Below this, a kick plays nothing.
const MIN_AMPLITUDE: f32 = 0.01;

fn acting() -> Option<HapticsConfig> {
    let config = Config::lock_query(|c| c.controllers);
    (config.enabled && config.haptics.enabled && scheme::is_controllers()).then_some(config.haptics)
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

/// Picks a weapon fire rumble out of every one-off the game spawns, and plays it on the hand whose
/// gun fired it, raised by the shot's recoil kick. Every other one-off rumble is left for the motor
/// forward below, once the game's own per-frame sum (which this one still needs to run through
/// unaffected) folds it in.
fn route(motors: Motors, time: f32, debug_name: *const u8) {
    let Some(config) = acting().filter(|c| c.game_rumble) else {
        return;
    };
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
    let amplitude = shot_amplitude(motors.strongest(), kick_rad, &config);
    haptics::play_shot(hand, amplitude, shot_duration(time, &config));
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
    let Some(config) = acting() else {
        return value;
    };
    let result = continuous_motor(motor, value, config.game_rumble, !config.gamepad_rumble);
    if let Some((hand, level)) = result.level {
        haptics::sustain(hand, Sustain::GameRumble, level);
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
    fn a_forwarded_motor_sends_its_hand_and_silences_the_pad() {
        let result = continuous_motor(0, 0.6, true, true);
        assert_eq!(result.level, Some((Hand::Left, 0.6)));
        assert_eq!(result.passthrough, 0.0);
    }

    #[test]
    fn letting_the_gamepad_rumble_keeps_its_own_value() {
        let result = continuous_motor(1, 0.6, true, false);
        assert_eq!(result.level, Some((Hand::Right, 0.6)));
        assert_eq!(result.passthrough, 0.6);
    }

    #[test]
    fn an_unforwarded_motor_still_silences_the_pad() {
        let result = continuous_motor(0, 0.6, false, true);
        assert_eq!(result.level, None);
        assert_eq!(result.passthrough, 0.0);
    }

    #[test]
    fn a_still_motor_ends_its_hands_rumble() {
        let result = continuous_motor(0, 0.0, true, true);
        assert_eq!(result.level, Some((Hand::Left, 0.0)));
    }

    #[test]
    fn an_unsupported_motor_is_left_untouched() {
        let result = continuous_motor(2, 0.9, true, true);
        assert_eq!(result.level, None);
        assert_eq!(result.passthrough, 0.9);
    }

    #[test]
    fn a_motor_value_past_one_clamps_its_level() {
        let result = continuous_motor(0, 1.5, true, false);
        assert_eq!(result.level, Some((Hand::Left, 1.0)));
    }

    #[test]
    fn a_bigger_kick_plays_harder() {
        let config = HapticsConfig::new();
        let small = shot_amplitude(0.2, 0.01, &config);
        let big = shot_amplitude(0.2, 0.05, &config);
        assert!(big > small && small > 0.2);
        assert_eq!(shot_amplitude(0.9, 1.0, &config), 1.0);
    }

    #[test]
    fn long_rumbles_are_capped() {
        let config = HapticsConfig::new();
        assert_eq!(shot_duration(5.0, &config), config.max_shot_s);
        assert_eq!(shot_duration(0.1, &config), 0.1);
        assert_eq!(shot_duration(f32::NAN, &config), 0.0);
    }
}
