//! What the palms' contacts do to the world: a knock and a shove of a loose object, and the
//! vibration of a hand that presses or pulls hard.
//!
//! Every function here changes engine state that is the main thread's, and runs from the palms'
//! tick ([`crate::controllers::physical_hands::main_thread_tick`]) only on that thread.

use glam::Vec3;
use jc3vr_body::controllers::{haptics, physics::bodies};
use jc3vr_input::{
    frame::Hand,
    haptics::cues::{Event, Sustain},
};

use crate::controllers::physical_hands::{
    HandStatus, ShoveReport, State,
    contact::{Surface, SurfaceHit},
    engine, shove,
    shove::ShoveTuning,
    sounds,
    sounds::{Impact, ImpactKind},
    tuning::{PHYSICAL_HANDS, PalmTuning, PressureHapticsTuning},
};

/// A palm meeting a solid surface on a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Touch {
    pub(super) hand: Hand,
    pub(super) hit: SurfaceHit,
    /// The hand's velocity, world space.
    pub(super) velocity: Vec3,
}

/// A palm's press into an object, for its shove.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct ShoveInput {
    pub(super) hit: SurfaceHit,
    pub(super) depth: f32,
    /// The palm's velocity, world space.
    pub(super) hand_velocity: Vec3,
    pub(super) dt: f32,
}

impl State {
    /// Pulse the hand of a palm that met a surface, and knock the object it met when the hand was
    /// fast enough.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::physical_hands::main_thread_tick`], with the touch's object found by this
    /// tick's casts.
    pub(super) unsafe fn touch(&mut self, touch: Touch, melee_impulse: Option<f32>) {
        let Touch {
            hand,
            hit,
            velocity,
        } = touch;
        let tuning = PHYSICAL_HANDS.get();
        let palms = &tuning.palms;
        let impact = impact_speed(velocity, hit.normal);
        let pressure_owned = self.door_press[hand.index()].engaged();
        if let Some(scale) = touch_haptic_scale(impact, pressure_owned, palms, &tuning.haptics) {
            haptics::play_scaled(hand, Event::PalmTouch, scale);
        }
        let material = match hit.surface {
            Surface::Character { .. } => sounds::strike_material(true),
            Surface::Solid { .. } => hit
                .material
                .unwrap_or_else(|| sounds::strike_material(false)),
        };
        // SAFETY: forwarded from the caller: the palms' tick on the game thread.
        unsafe {
            sounds::play(Impact {
                hand,
                kind: ImpactKind::Touch,
                at: hit.position,
                velocity,
                material,
                strength: impact,
            })
        };
        let Surface::Solid {
            object: Some(_),
            body,
        } = hit.surface
        else {
            return;
        };
        let (Some(strength), Some(melee_impulse)) = (
            strength(impact, palms.min_object_speed_mps, palms.full_hit_speed_mps),
            melee_impulse,
        ) else {
            return;
        };
        let direction = velocity.try_normalize().unwrap_or(-hit.normal);
        let impulse = direction
            * (melee_impulse * palms.object_impulse * strength.max(palms.min_object_strength));
        // SAFETY: forwarded from the caller.
        match unsafe { engine::push_body(body, impulse, hit.position) } {
            Ok(()) => self.status.object_pushes += 1,
            Err(e) => self.status.last_error = Some(e.to_string()),
        }
    }

    /// Shove the object a palm presses into, and return the shove, or `None` when the surface is
    /// not a loose object: world geometry, or a body that is not dynamic, gives nothing.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::physical_hands::main_thread_tick`], with the hit found by this tick's
    /// casts.
    pub(super) unsafe fn shove(
        &mut self,
        hand: Hand,
        input: ShoveInput,
        tuning: &ShoveTuning,
    ) -> Option<ShoveReport> {
        let i = hand.index();
        let Surface::Solid {
            object: Some(object),
            body,
        } = input.hit.surface
        else {
            self.gives[i].reset();
            return None;
        };
        // SAFETY: forwarded from the caller.
        let state = match unsafe { bodies::read(body, object) } {
            Ok(state) => state,
            Err(bodies::BodyError::NotDynamic) => {
                self.gives[i].reset();
                return None;
            }
            Err(e) => {
                self.status.last_error = Some(e.to_string());
                self.gives[i].reset();
                return None;
            }
        };
        let shove = shove::shove(
            &state.rigid(),
            input.hit.position,
            input.hit.normal,
            input.depth,
            input.hand_velocity,
            input.dt,
            tuning,
        );
        let resistance = self.gives[i].update(body, &shove, input.dt, tuning);
        if shove.impulse != Vec3::ZERO {
            // SAFETY: forwarded from the caller.
            match unsafe { bodies::shove(body, object, shove.impulse, input.hit.position) } {
                Ok(()) => self.status.shoves += 1,
                Err(e) => self.status.last_error = Some(e.to_string()),
            }
        }
        Some(ShoveReport {
            at: input.hit.position,
            impulse: shove.impulse,
            load: shove.load,
            resistance,
        })
    }
}

/// Vibrate each hand that presses or pulls hard, in proportion to its load: the shove of what its
/// palm presses into, or the pull and twist of what it holds.
pub(super) fn pressure_haptics(hands: &[HandStatus; 2]) {
    let tuning = &PHYSICAL_HANDS.get().haptics;
    for hand in Hand::BOTH {
        let status = &hands[hand.index()];
        let (load, source) = match (status.held, status.shove) {
            (Some(held), _) => (held.load.max(held.twist), Sustain::HeldLoad),
            (None, Some(shove)) => (
                shove
                    .load
                    .max(shove.resistance * tuning.pressure_resistance_share),
                Sustain::PalmPressure,
            ),
            (None, None) => continue,
        };
        if load >= tuning.min_pressure_load {
            haptics::sustain(hand, source, load);
        }
    }
}

/// The strength of a hit at `speed` metres per second, `0..=1`: zero at `min_mps`, one from
/// `full_mps`. `None` below `min_mps`.
pub fn strength(speed: f32, min_mps: f32, full_mps: f32) -> Option<f32> {
    if !speed.is_finite() || speed < min_mps {
        return None;
    }
    let span = full_mps - min_mps;
    if span <= f32::EPSILON {
        return Some(1.0);
    }
    Some(((speed - min_mps) / span).clamp(0.0, 1.0))
}

/// The speed of a hand moving at `velocity` into a surface whose normal is `normal` (pointing back
/// toward the hand): the share of the velocity that drives into the surface, never negative.
pub fn impact_speed(velocity: Vec3, normal: Vec3) -> f32 {
    let into = velocity.dot(-normal.normalize_or_zero());
    if into.is_finite() { into.max(0.0) } else { 0.0 }
}

/// The strength of the pulse of a palm touch at `impact` metres per second, from
/// [`PressureHapticsTuning::min_touch_scale`] still to one at
/// [`PalmTuning::full_hit_speed_mps`]; `None` while the door pressure owns the hand's cues
/// (`pressure_owned`).
fn touch_haptic_scale(
    impact: f32,
    pressure_owned: bool,
    palms: &PalmTuning,
    haptics: &PressureHapticsTuning,
) -> Option<f32> {
    if pressure_owned {
        return None;
    }
    let share = (impact / palms.full_hit_speed_mps.max(1e-3)).clamp(0.0, 1.0);
    let min = haptics.min_touch_scale;
    Some(min + (1.0 - min) * share)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_ownership_suppresses_only_that_hands_touch_cue() {
        let palms = &PHYSICAL_HANDS.get().palms;
        let haptics = &PHYSICAL_HANDS.get().haptics;
        let scale = |speed: f32, owned: bool| touch_haptic_scale(speed, owned, palms, haptics);
        for owned in Hand::BOTH {
            for speed in [
                0.0,
                palms.full_hit_speed_mps * 0.5,
                palms.full_hit_speed_mps * 2.0,
            ] {
                let cues = Hand::BOTH.map(|hand| scale(speed, hand == owned));
                assert_eq!(cues[owned.index()], None);
                assert_eq!(cues[owned.other().index()], scale(speed, false));
                assert!(cues[owned.other().index()].is_some());
            }
        }
        let min = haptics.min_touch_scale;
        assert_eq!(scale(0.0, false), Some(min));
        let half = scale(palms.full_hit_speed_mps * 0.5, false).unwrap();
        assert!((half - (min + 1.0) * 0.5).abs() < 1e-6);
        assert_eq!(scale(palms.full_hit_speed_mps * 2.0, false), Some(1.0));
    }

    #[test]
    fn strength_ramps_between_the_speeds() {
        let cases = [
            (0.5, None),
            (1.0, Some(0.0)),
            (4.0, Some(0.5)),
            (9.0, Some(1.0)),
            (f32::NAN, None),
        ];
        for (speed, expected) in cases {
            match (strength(speed, 1.0, 7.0), expected) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 1e-5, "{speed}: {a} != {b}"),
                (a, b) => assert_eq!(a, b, "{speed}"),
            }
        }
    }

    #[test]
    fn only_the_share_into_the_surface_is_impact() {
        let normal = Vec3::NEG_Z;
        assert!((impact_speed(Vec3::new(0.0, 0.0, 4.0), normal) - 4.0).abs() < 1e-6);
        assert!((impact_speed(Vec3::new(3.0, 0.0, 4.0), normal) - 4.0).abs() < 1e-6);
        assert_eq!(impact_speed(Vec3::new(0.0, 0.0, -4.0), normal), 0.0);
    }
}
