//! Haptics routing: the plan of record's "Recoil and haptics"
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! The game's feedback reaches the controllers through the one call every one-off rumble goes
//! through, [`RumbleManager::SpawnRumble`]: the weapon fire rumble, the player's damage rumbles
//! (impacts, bullets, and drowning), vehicle collisions, the rumbles on animation track messages
//! and melee hits, the reel-in kick, the force pulse, and the event-driven rumbles. A pre-call
//! detour turns each into a pulse on the hand that caused it, and always lets the game's own rumble
//! run:
//!
//! - a weapon fire rumble inside a shot of a gun held in a hand goes to that hand
//!   ([`crate::controllers::weapons::fire`] marks the shot), with its amplitude raised by the shot's
//!   recoil kick;
//! - any other rumble goes by motor: the gamepad's left motors to the left hand and its right
//!   motors to the right hand, so a rumble on both motors (the damage rumbles) plays on both hands.
//!   A spatial rumble is attenuated by its distance from the head as the game attenuates it.
//!
//! The grapple hand's pulses (the hook leaving the gauntlet, the hook attaching, and a yank) come
//! from the input tick ([`pulse`]). Every pulse lands in the haptics queue
//! ([`crate::vr::input::request_haptic`]), which the VR frame loop flushes.

use std::{
    ffi::{CStr, c_char},
    time::Duration,
};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
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
    headpose,
    vr::input::{HapticPulse, request_haptic},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SPAWN_RUMBLE_BINDER)
}

/// Where a pulse came from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PulseSource {
    /// A weapon fire rumble, on the hand whose gun fired.
    Shot,
    /// A shot of a weapon with no fire rumble of its own: the recoil kick alone.
    Kick,
    /// A damage rumble: an impact, a bullet, or drowning.
    Damage,
    /// A collision rumble.
    Collision,
    /// A rumble on an animation's track message, or a melee hit.
    Animation,
    /// Any other game rumble.
    OtherRumble,
    GrappleFire,
    GrappleAttach,
    Yank,
    /// A grenade primed in the hand, and the warning as its fuse runs down.
    GrenadeFuse,
    /// A physical strike that started a melee attack.
    MeleeStrike,
    /// A magazine taken from the pouch or inserted.
    Magazine,
    /// A controller chord the mod acts on: the recenter or the debug panel.
    Chord,
    /// A trigger pull of an empty gun that cannot reload.
    DryFire,
    /// A C4 charge primed in the hand, and a press of the wrist detonator.
    C4,
    /// An object taken hold of, a weapon picked up, or an object let go of by a physical hand.
    Pickup,
}

impl PulseSource {
    pub const fn label(self) -> &'static str {
        match self {
            PulseSource::Shot => "shot",
            PulseSource::Kick => "recoil kick",
            PulseSource::Damage => "damage",
            PulseSource::Collision => "collision",
            PulseSource::Animation => "animation",
            PulseSource::OtherRumble => "game rumble",
            PulseSource::GrappleFire => "grapple fire",
            PulseSource::GrappleAttach => "grapple attach",
            PulseSource::Yank => "yank",
            PulseSource::GrenadeFuse => "grenade fuse",
            PulseSource::MeleeStrike => "melee strike",
            PulseSource::Magazine => "magazine",
            PulseSource::Chord => "chord",
            PulseSource::DryFire => "dry fire",
            PulseSource::C4 => "C4",
            PulseSource::Pickup => "pickup",
        }
    }

    /// Classify a game rumble by its debug name.
    pub fn of_rumble(name: &str) -> PulseSource {
        if name == WEAPON_FIRE {
            PulseSource::Shot
        } else if name.contains("OnDamage") {
            PulseSource::Damage
        } else if name.contains("OnCollision") {
            PulseSource::Collision
        } else if name.contains("HandleTrackMessage") {
            PulseSource::Animation
        } else {
            PulseSource::OtherRumble
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

    /// Each hand's share by motor side, indexed by [`Hand::index`]: `None` for a hand whose motors
    /// are still.
    pub fn by_hand(&self) -> [Option<f32>; 2] {
        let side = |body: f32, trigger: f32| Some(body.max(trigger)).filter(|v| *v > MIN_AMPLITUDE);
        [
            side(self.left, self.left_trigger),
            side(self.right, self.right_trigger),
        ]
    }
}

/// The game's spatial falloff: one minus the squared distance over the squared maximum distance,
/// clamped to `0..=1`; no falloff without a maximum distance.
pub fn spatial_gain(distance_sq: f32, max_distance: f32) -> f32 {
    if max_distance <= 1e-6 {
        return 1.0;
    }
    let gain = 1.0 - distance_sq / (max_distance * max_distance);
    if gain.is_finite() {
        gain.clamp(0.0, 1.0)
    } else {
        0.0
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

fn acting(config: &ControllersConfig) -> bool {
    config.enabled && config.weapons.haptics.enabled && scheme::is_controllers()
}

fn send(hand: Hand, amplitude: f32, duration_s: f32, source: PulseSource) {
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
    route(
        effect_type,
        position,
        motors,
        max_distance,
        time,
        debug_name,
    );
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

fn route(
    effect_type: RumbleEffectType,
    position: *const Vector3,
    motors: Motors,
    max_distance: f32,
    time: f32,
    debug_name: *const u8,
) {
    let config = Config::lock_query(|c| c.controllers);
    if !(acting(&config) && config.weapons.haptics.game_rumble) {
        return;
    }
    let haptics = &config.weapons.haptics;
    let name = (!debug_name.is_null())
        .then(|| unsafe { CStr::from_ptr(debug_name.cast::<c_char>()) })
        .and_then(|name| name.to_str().ok())
        .unwrap_or("");
    let source = PulseSource::of_rumble(name);
    let duration_s = rumble_duration(time, haptics);

    if source == PulseSource::Shot
        && let Some((hand, kick_rad)) = fire::rumble_in_shot()
    {
        let amplitude = shot_amplitude(motors.strongest(), kick_rad, haptics);
        send(hand, amplitude, duration_s, source);
        return;
    }

    let gain = match effect_type {
        RumbleEffectType::EType_Absolute => 1.0,
        RumbleEffectType::EType_Disabled => 0.0,
        RumbleEffectType::EType_Spatial => {
            let at = unsafe { position.as_ref() }.map(|p| Vec3::from_array(p.data));
            let head = headpose::query().position;
            at.map_or(0.0, |at| {
                spatial_gain(at.distance_squared(head), max_distance)
            })
        }
    };
    let amplitudes = motors.by_hand().map(|a| {
        a.map(|a| (a * gain).clamp(0.0, 1.0))
            .filter(|a| *a > MIN_AMPLITUDE)
    });
    send_each(amplitudes, duration_s, source);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rumbles_are_classified_by_their_names() {
        assert_eq!(PulseSource::of_rumble("WeaponFire"), PulseSource::Shot);
        assert_eq!(
            PulseSource::of_rumble("CPlayer::OnDamage BULLET"),
            PulseSource::Damage
        );
        assert_eq!(
            PulseSource::of_rumble("CCharacter::OnCollision Vehicle impact"),
            PulseSource::Collision
        );
        assert_eq!(
            PulseSource::of_rumble("CCharacter::HandleTrackMessage PlayRumble"),
            PulseSource::Animation
        );
        assert_eq!(
            PulseSource::of_rumble("Force Pulse"),
            PulseSource::OtherRumble
        );
    }

    #[test]
    fn the_motors_route_by_side() {
        let both = Motors {
            left: 0.4,
            right: 0.4,
            ..Motors::default()
        };
        assert_eq!(both.by_hand(), [Some(0.4), Some(0.4)]);
        let right = Motors {
            right: 0.6,
            left_trigger: 0.0,
            ..Motors::default()
        };
        assert_eq!(right.by_hand(), [None, Some(0.6)]);
        let trigger = Motors {
            left_trigger: 0.3,
            ..Motors::default()
        };
        assert_eq!(trigger.by_hand(), [Some(0.3), None]);
        assert_eq!(trigger.strongest(), 0.3);
    }

    #[test]
    fn a_spatial_rumble_fades_with_distance_as_the_game_fades_it() {
        assert_eq!(spatial_gain(0.0, 10.0), 1.0);
        assert!((spatial_gain(25.0, 10.0) - 0.75).abs() < 1e-6);
        assert_eq!(spatial_gain(200.0, 10.0), 0.0);
        assert_eq!(spatial_gain(200.0, 0.0), 1.0);
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
