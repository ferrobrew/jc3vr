//! What the palms' contacts do to the world: a strike on a character, a knock and a shove of a
//! loose object, and the pulses of a hand that presses or pulls hard.
//!
//! Every function here changes engine state that is the main thread's, and runs from the palms'
//! tick ([`crate::controllers::physical_hands::after_aim`]) only on that thread.

use glam::Vec3;
use jc3gi::character::character::Character;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        HandStatus, PhysicalHandsConfig, ShoveReport, State, StrikeReport, bodies,
        contact::{Surface, SurfaceHit},
        engine, pulse,
        shove::{self, ShoveTuning},
        strike::{self, StrikeKind, impact_speed, strength},
    },
    weapons::haptics::PulseSource,
};

/// Something a tick's contacts do to the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum Event {
    /// A palm hit a character.
    Strike {
        hand: Hand,
        hit: SurfaceHit,
        velocity: Vec3,
        kind: StrikeKind,
    },
    /// A palm met a solid surface.
    Touch {
        hand: Hand,
        hit: SurfaceHit,
        velocity: Vec3,
    },
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
    /// Act on one event.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::physical_hands::after_aim`], with the event's objects found by
    /// this tick's casts.
    pub(super) unsafe fn act(
        &mut self,
        event: Event,
        owner: &Character,
        melee: Option<(i32, f32)>,
        config: &PhysicalHandsConfig,
    ) {
        match event {
            Event::Strike {
                hand,
                hit,
                velocity,
                kind,
            } => {
                let Surface::Character(victim) = hit.surface else {
                    return;
                };
                let impact = impact_speed(velocity, hit.normal);
                let Some(strength) = strength(
                    impact,
                    config.min_strike_speed_mps,
                    config.full_strike_speed_mps,
                ) else {
                    return;
                };
                let Some((damage, impulse)) = melee else {
                    return;
                };
                if !self.gates[hand.index()].take(config) {
                    return;
                }
                let plan = strike::plan(kind, strength, damage, impulse, config);
                let direction = velocity.try_normalize().unwrap_or(-hit.normal);
                // SAFETY: forwarded from the caller.
                let dealt = unsafe {
                    engine::strike_character(owner, victim, hit.position, direction, &plan)
                };
                tracing::debug!(target: "controllers", ?hand, kind = kind.label(), strength, damage = plan.damage, dealt, "physical hands: strike");
                self.status.strikes += 1;
                self.status.last_strike = Some(StrikeReport {
                    hand,
                    plan,
                    impact_mps: impact,
                    dealt,
                });
                pulse(
                    hand,
                    config.strike_pulse,
                    0.3 + 0.7 * strength,
                    config,
                    PulseSource::MeleeStrike,
                );
            }
            Event::Touch {
                hand,
                hit,
                velocity,
            } => {
                let impact = impact_speed(velocity, hit.normal);
                let share = (impact / config.full_strike_speed_mps.max(1e-3)).clamp(0.0, 1.0);
                pulse(
                    hand,
                    config.contact_pulse,
                    0.25 + 0.75 * share,
                    config,
                    PulseSource::Collision,
                );
                let Surface::Solid {
                    object: Some(_),
                    body,
                } = hit.surface
                else {
                    return;
                };
                if !config.object_impulses {
                    return;
                }
                let (Some(strength), Some((_, melee_impulse))) = (
                    strength(
                        impact,
                        config.min_object_speed_mps,
                        config.full_strike_speed_mps,
                    ),
                    melee,
                ) else {
                    return;
                };
                let direction = velocity.try_normalize().unwrap_or(-hit.normal);
                let impulse =
                    direction * (melee_impulse * config.object_impulse * strength.max(0.1));
                // SAFETY: forwarded from the caller.
                match unsafe { engine::push_body(body, impulse, hit.position) } {
                    Ok(()) => self.status.object_pushes += 1,
                    Err(e) => self.status.last_error = Some(e.to_string()),
                }
            }
        }
    }

    /// Shove the object a palm presses into, and return the shove, or `None` when the surface is
    /// not a loose object: world geometry, or a body that is not dynamic, gives nothing.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::physical_hands::after_aim`], with the hit found by this tick's
    /// casts.
    pub(super) unsafe fn shove(
        &mut self,
        hand: Hand,
        input: ShoveInput,
        tuning: &ShoveTuning,
        config: &PhysicalHandsConfig,
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
        let resistance =
            self.gives[i].update(body, &shove, input.dt, config.shove.give_smoothing_s);
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

/// Pulse each hand that presses or pulls hard, in proportion to its load: the shove of what its
/// palm presses into, or the pull and twist of what it holds.
pub(super) fn pressure_pulses(hands: &[HandStatus; 2], config: &PhysicalHandsConfig) {
    for hand in Hand::BOTH {
        let status = &hands[hand.index()];
        let (load, source) = match (status.held, status.shove) {
            (Some(held), _) => (held.load.max(held.twist), PulseSource::Pickup),
            (None, Some(shove)) => (
                shove.load.max(shove.resistance * 0.5),
                PulseSource::Collision,
            ),
            (None, None) => continue,
        };
        if load >= MIN_PRESSURE_LOAD {
            pulse(hand, config.pressure_pulse, load, config, source);
        }
    }
}

/// The least load that pulses a hand: a tenth of its strength.
const MIN_PRESSURE_LOAD: f32 = 0.1;
