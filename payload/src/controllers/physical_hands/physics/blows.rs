//! The hand bodies' blows: the contacts the physics step reported ([`contact`]), dealt on the game
//! thread's frame tick, outside the game's update.
//!
//! Each contact pulses its hand by its impulse. A contact with a character, other than the player,
//! is a blow ([`hit`]): the character reacts through its own hit reaction
//! ([`Character::HitReaction`]) as to a melee blow ([`DamageFlags::DMG_TYPE_MELEE`]) of the
//! contact's impulse along the hand's travel, which staggers it or knocks it down against its own
//! thresholds and carries the blow's momentum into the ragdoll it falls as. A blow hard enough to
//! deal damage is a melee damage message ([`DamageMsg`]) to the character's damage virtual
//! ([`PhysicsGameObject::OnDamage`]) instead, which takes the health and runs the same hit reaction
//! from the message's impulse.
//!
//! The character is found again from the contact's body when the blow is dealt: the body's engine
//! instance must still be the one the event named, and its owner the same object, so a character
//! freed since the step is never touched.
//!
//! [`contact`]: crate::controllers::physical_hands::physics::contact
//! [`hit`]: crate::controllers::physical_hands::physics::hit

use std::{mem::MaybeUninit, num::NonZeroUsize, time::Instant};

use glam::{Mat4, Vec3};
use jc3gi::{
    animation::symbol_table::RagdollRBGroupId,
    character::character::Character,
    damage::{DamageFlags, DamageMsg},
    physics::{PfxInstance, PhysicsGameObject},
    types::math::Vector3,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        PhysicalHandsConfig, engine,
        physics::{
            body,
            contact::{self, Contact},
            hit::{Blow, BlowPlan, HitGate, VictimTuning, blow, pulse_share},
        },
        pulse,
    },
    weapons::haptics::PulseSource,
};

/// A blow a hand dealt, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BlowReport {
    pub hand: Hand,
    pub impulse_ns: f32,
    /// The blow as a share of the knockdown impulse.
    pub strength: f32,
    /// The damage the blow carried, if it carried any.
    pub damage: Option<i16>,
    /// The health the character lost.
    pub dealt: i16,
}

/// What a frame tick's delivery did.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Delivered {
    pub contacts: u64,
    pub dropped: u64,
    pub blows: u64,
    pub last: Option<BlowReport>,
}

/// Deal the blows of the contacts recorded since the last frame tick, and pulse their hands.
///
/// # Safety
///
/// The game thread, outside the game's update.
pub(super) unsafe fn deliver(config: &PhysicalHandsConfig) -> Delivered {
    let (contacts, dropped) = contact::take();
    let mut delivered = Delivered {
        contacts: contacts.len() as u64,
        dropped,
        ..Delivered::default()
    };
    if contacts.is_empty() {
        return delivered;
    }
    let hits = &config.bodies.hits;
    let attacker = unsafe { Character::GetLocalPlayerCharacter().as_mut() };
    let mut gate = GATE.lock();
    let now_s = gate
        .1
        .get_or_insert_with(Instant::now)
        .elapsed()
        .as_secs_f64();
    for contact in contacts {
        if hits.haptics {
            let share = pulse_share(contact.impulse_ns, hits.full_pulse_impulse_ns);
            if share >= MIN_PULSE_SHARE {
                pulse(
                    contact.hand,
                    config.contact_pulse,
                    share,
                    config,
                    PulseSource::Collision,
                );
            }
        }
        if !hits.reactions {
            continue;
        }
        let Some(attacker) = attacker.as_deref() else {
            continue;
        };
        // SAFETY: forwarded from the caller.
        let Some(victim) = (unsafe { victim(&contact, attacker) }) else {
            continue;
        };
        if !gate.0.admit(
            contact.hand.index(),
            victim as usize,
            now_s,
            hits.cooldown_s,
        ) {
            continue;
        }
        // SAFETY: a live character, found again above.
        let victim = unsafe { &mut *victim };
        let tuning = VictimTuning {
            type_scale: victim
                .m_HitReaction_DamageImpulseScale
                .get(melee_type_index())
                .copied()
                .unwrap_or(0.0),
            knockdown_threshold: victim.m_HitReaction_ImpulseRagdollThreshold,
        };
        let melee_damage = unsafe { engine::melee_damage(attacker) }.unwrap_or(0);
        let plan = match blow(contact.impulse_ns, tuning, melee_damage, hits) {
            Blow::Strikes(plan) => plan,
            Blow::TooWeak => continue,
            Blow::Uncalibrated => {
                tracing::debug!(
                    type_scale = tuning.type_scale,
                    knockdown_threshold = tuning.knockdown_threshold,
                    "hand bodies: a struck character has no hit-reaction tuning to scale a blow by"
                );
                continue;
            }
        };
        let victim_at = Mat4::from(victim.m_WorldMatrixT1).w_axis.truncate();
        let direction = blow_direction(contact.hand_velocity, victim_at - contact.hand_position);
        // SAFETY: forwarded from the caller; both characters are live.
        let dealt = unsafe { strike(attacker, victim, contact.hand_position, direction, &plan) };
        tracing::info!(
            hand = contact.hand.label(),
            impulse_ns = contact.impulse_ns,
            strength = plan.strength,
            reaction_impulse = plan.reaction_impulse,
            damage = plan.damage,
            dealt,
            stagger_threshold = victim.m_HitReaction_ImpulseStumbleThreshold,
            knockdown_threshold = tuning.knockdown_threshold,
            type_scale = tuning.type_scale,
            "hand bodies: a hand struck a character"
        );
        delivered.blows += 1;
        delivered.last = Some(BlowReport {
            hand: contact.hand,
            impulse_ns: contact.impulse_ns,
            strength: plan.strength,
            damage: plan.damage,
            dealt,
        });
    }
    delivered
}

/// Forget the contacts recorded since the last frame tick and the characters struck lately.
pub(super) fn discard() {
    let _ = contact::take();
    GATE.lock().0.clear();
}

/// The least share of the strongest hit pulse that pulses a hand.
const MIN_PULSE_SHARE: f32 = 0.05;

/// The characters each hand struck lately, and the time base of the gate.
static GATE: Mutex<(HitGate, Option<Instant>)> = Mutex::new((HitGate::new(), None));

/// The character the contact `contact` met, found again from its body: `None` when the body's
/// instance or its owner changed since the step, when the owner is not a character, or when it is
/// the player `attacker`.
///
/// # Safety
///
/// The game thread, outside the game's update.
unsafe fn victim(contact: &Contact, attacker: &Character) -> Option<*mut Character> {
    let object = NonZeroUsize::new(contact.other_object)?;
    let (world, _) = unsafe { body::world() }.ok()?;
    let instance = unsafe { body::instance_of(world, contact.other_body) }?;
    if instance != contact.other_instance {
        return None;
    }
    let owner = unsafe { (*(instance as *const PfxInstance)).m_GameObject } as usize;
    if owner != object.get() || object.get() == attacker as *const Character as usize {
        return None;
    }
    // SAFETY: the owner of a live instance is a live physics game object.
    if !unsafe { engine::is_character(object) } {
        return None;
    }
    Some(object.get() as *mut Character)
}

/// The index of the melee type bit, the entry of a character's per-type impulse scales its hit
/// reaction picks for a melee blow.
fn melee_type_index() -> usize {
    DamageFlags::DMG_TYPE_MELEE.bits().trailing_zeros() as usize
}

/// The direction a blow travels: the hand's own, or, for a hand that was not moving, from the hand
/// toward the character, level.
fn blow_direction(hand_velocity: Vec3, toward_victim: Vec3) -> Vec3 {
    let level = Vec3::new(toward_victim.x, 0.0, toward_victim.z);
    hand_velocity
        .try_normalize()
        .or_else(|| level.try_normalize())
        .unwrap_or(Vec3::NEG_Z)
}

/// Deal `plan` to `victim`, struck at `at` along `direction` by `attacker`, and return the health
/// it lost.
///
/// # Safety
///
/// The game thread, outside the game's update; both characters live.
unsafe fn strike(
    attacker: &Character,
    victim: &mut Character,
    at: Vec3,
    direction: Vec3,
    plan: &BlowPlan,
) -> i16 {
    let Some(damage) = plan.damage else {
        let mut zone = RagdollRBGroupId {
            m_id: RagdollRBGroupId::NONE,
        };
        // The reaction destroys the zone it is given, so the copy takes its own use first.
        unsafe {
            zone.init_ptr();
            victim.HitReaction(
                &vector(at),
                &vector(direction),
                plan.reaction_impulse,
                false,
                &mut zone.m_id,
                DamageFlags::DMG_TYPE_MELEE.bits(),
                false,
            );
        }
        return 0;
    };
    // Every field is a pointer, an integer, or a float, so all-zero is a valid value to construct
    // over: the smart pointers start null, as the engine clears them before `Clear`.
    let mut msg = unsafe { MaybeUninit::<DamageMsg>::zeroed().assume_init() };
    msg.__vftable = DamageMsg::VFTABLE;
    unsafe { msg.Clear() };
    msg.m_Damage = damage;
    msg.m_OriginalDamage = damage;
    msg.m_DamageFlags = DamageFlags::DMG_TYPE_MELEE;
    unsafe { msg.m_Damager.Assign(&attacker.m_WeakThis) };
    msg.m_Position = vector(at);
    msg.m_Direction = vector(direction);
    msg.m_DamagerPosition = vector(Mat4::from(attacker.m_WorldMatrixT1).w_axis.truncate());
    msg.m_Impulse = plan.reaction_impulse;
    msg.m_Faction = unsafe { attacker.GetFaction() };
    msg.m_DamagerSourceStateInfo = unsafe { attacker.GetCurrentDamageSourceStateInfo() };
    // SAFETY: a character is a physics game object, with the damage virtual in its primary table.
    let dealt =
        unsafe { (*(victim as *mut Character).cast::<PhysicsGameObject>()).OnDamage(&mut msg) };
    unsafe { msg.Destruct() };
    dealt
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blow_travels_with_the_hand() {
        let d = blow_direction(Vec3::new(3.0, 0.0, 0.0), Vec3::Z);
        assert!((d - Vec3::X).length() < 1e-6);
        // A still hand strikes toward the character, level.
        let d = blow_direction(Vec3::ZERO, Vec3::new(0.0, 5.0, 2.0));
        assert!((d - Vec3::Z).length() < 1e-6);
        assert_eq!(blow_direction(Vec3::ZERO, Vec3::ZERO), Vec3::NEG_Z);
    }
}
