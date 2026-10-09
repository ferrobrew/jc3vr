//! The hand bodies' blows: the contacts the physics step reported ([`contact`]), dealt on the game
//! thread's frame tick, outside the game's update.
//!
//! Each contact pulses its hand by how fast the hand met it. A contact with a character, other than
//! the player, that the hand swung into is a blow ([`hit`]): the character reacts through its own
//! hit reaction
//! ([`Character::HitReaction`]) as to a melee blow ([`DamageFlags::DMG_TYPE_MELEE`]) of the
//! contact's impulse along the hand's travel, which staggers it or knocks it down against its own
//! thresholds and carries the blow's momentum into the ragdoll it falls as. A blow hard enough to
//! deal damage is a melee damage message ([`DamageMsg`]) to the character's damage virtual
//! ([`PhysicsGameObject::OnDamage`]) instead, which takes the health and runs the same hit reaction
//! from the message's impulse. Only a character the player's faction is hostile to
//! ([`GetRelation`]) takes damage or a knockdown: an ally, an escort, or a bystander is staggered
//! at most, so that a careless swing cannot kill a mission's own characters.
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
    aim::target::FactionRelation,
    animation::symbol_table::RagdollRBGroupId,
    character::character::Character,
    damage::{DamageFlags, DamageMsg},
    faction::GetRelation,
    physics::{PfxInstance, PhysicsGameObject},
    types::math::Vector3,
};
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    haptics::{self, Event},
    physical_hands::{
        PhysicalHandsConfig,
        bodies::owner_of,
        engine,
        physics::{
            HandHitsConfig, body,
            contact::{self, Contact},
            guns,
            hit::{
                ANY_HAND, Blow, BlowPlan, HitGate, VictimTuning, approach, blow, blow_impulse,
                pulse_share, restrain, striking_with,
            },
        },
        sounds::{self, Impact, ImpactKind},
        world::World,
    },
};

/// A blow a hand dealt, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BlowReport {
    pub hand: Hand,
    /// The blow's impulse: the arm's momentum at the hand's speed into the character.
    pub impulse_ns: f32,
    /// Whether the player's faction is hostile to the character.
    pub hostile: bool,
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
    /// The hands that struck hard enough to follow through.
    pub follow_through: [bool; 2],
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
    let own_hits = &config.bodies.hits;
    let attacker = unsafe { Character::GetLocalPlayerCharacter().as_mut() };
    let mut gate = GATE.lock();
    let now_s = gate
        .1
        .get_or_insert_with(Instant::now)
        .elapsed()
        .as_secs_f64();
    let pulse_interval_s = f64::from(config.bodies.guns.contact_pulse_interval_s.max(0.0));
    for contact in contacts {
        // The player's own body, the hands, and the other gun are in the player's group, which
        // the gun body never collides with; a contact with one is none.
        // SAFETY: forwarded from the caller.
        if contact.gun && unsafe { body::in_player_group(contact.other_body) } {
            continue;
        }
        // A held gun strikes with its mass on the arm's.
        let gun_mass = contact.gun.then(|| guns::mass(contact.hand)).flatten();
        if contact.gun {
            guns::count_contact(contact.hand);
        }
        let hits = &striking_with(own_hits, gun_mass);
        // Harder the faster the hand met it: a punch, not a shove. A gun's pulse is the arm's
        // alone, and a gun rubbing along what it touches pulses at most once an interval, so a
        // held gun does not buzz the hand.
        let pulse_mass = if contact.gun {
            own_hits.blow_mass_kg
        } else {
            hits.blow_mass_kg
        };
        let met = pulse_mass.max(0.0) * contact.hand_velocity.length();
        let share = pulse_share(met, hits.full_pulse_impulse_ns);
        let pulse = share >= MIN_PULSE_SHARE
            && (!contact.gun
                || gun_pulse_due(
                    &mut GUN_PULSES.lock(),
                    contact.hand,
                    now_s,
                    pulse_interval_s,
                ));
        if pulse {
            haptics::play_scaled(contact.hand, Event::HandStrike, share);
        }
        // SAFETY: forwarded from the caller.
        let victim = attacker
            .as_deref()
            .and_then(|attacker| unsafe { victim(&contact, attacker) });
        // SAFETY: forwarded from the caller: the game thread, outside the game's update.
        unsafe {
            sounds::play(
                Impact {
                    hand: contact.hand,
                    kind: ImpactKind::Strike,
                    at: contact.hand_position,
                    velocity: contact.hand_velocity,
                    material: sounds::strike_material(victim.is_some()),
                    strength: share,
                },
                &config.sounds,
            )
        };
        if !hits.reactions {
            continue;
        }
        let (Some(attacker), Some(victim)) = (attacker.as_deref(), victim) else {
            continue;
        };
        // SAFETY: a live character, found again above.
        let victim = unsafe { &mut *victim };
        let victim_at = Mat4::from(victim.m_WorldMatrixT1).w_axis.truncate();
        let toward = victim_at + Vec3::Y * CHEST_HEIGHT_M - contact.hand_position;
        let closing = approach(
            contact.hand_velocity,
            Vec3::from_array(victim.m_MeasuredWorldVelocity.data),
            toward,
        );
        // A hand that did not swing into the character, resting on it or met by it, strikes
        // nothing.
        let Some(impulse_ns) = blow_impulse(closing, hits) else {
            continue;
        };
        if !gate.0.admit(
            contact.hand.index(),
            victim as *mut Character as usize,
            now_s,
            hits.cooldown_s,
        ) {
            continue;
        }
        let tuning = VictimTuning {
            type_scale: victim
                .m_HitReaction_DamageImpulseScale
                .get(melee_type_index())
                .copied()
                .unwrap_or(0.0),
            knockdown_threshold: victim.m_HitReaction_ImpulseRagdollThreshold,
        };
        let melee_damage = unsafe { engine::melee_damage(attacker) }.unwrap_or(0);
        let mut plan = match blow(impulse_ns, tuning, melee_damage, hits) {
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
        // SAFETY: both characters are live.
        let relation = unsafe { GetRelation(attacker.GetFaction(), victim.GetFaction()) };
        let hostile = relation == FactionRelation::E_RELATION_HOSTILE;
        if !hostile {
            plan = restrain(plan, NOT_HOSTILE_MAX_STRENGTH);
        }
        // One damaging blow per character a while, from either hand.
        if plan.damage.is_some()
            && !gate.0.admit(
                ANY_HAND,
                victim as *mut Character as usize,
                now_s,
                hits.damage_cooldown_s,
            )
        {
            plan.damage = None;
        }
        let direction = blow_direction(contact.hand_velocity, victim_at - contact.hand_position);
        // Read before the strike, which may free the character's tuning with its ragdoll.
        let stagger_threshold = victim.m_HitReaction_ImpulseStumbleThreshold;
        // SAFETY: forwarded from the caller; both characters are live.
        let dealt = unsafe { strike(attacker, victim, contact.hand_position, direction, &plan) };
        tracing::info!(
            hand = contact.hand.label(),
            with_gun = contact.gun,
            impulse_ns,
            hand_mps = closing.hand_mps,
            victim_mps = closing.victim_mps,
            contact_impulse_ns = contact.impulse_ns,
            hostile,
            strength = plan.strength,
            reaction_impulse = plan.reaction_impulse,
            damage = plan.damage,
            dealt,
            stagger_threshold,
            knockdown_threshold = tuning.knockdown_threshold,
            type_scale = tuning.type_scale,
            "hand bodies: a hand struck a character"
        );
        delivered.blows += 1;
        delivered.follow_through[contact.hand.index()] |= plan.strength >= FOLLOW_THROUGH_STRENGTH;
        delivered.last = Some(BlowReport {
            hand: contact.hand,
            impulse_ns,
            hostile,
            strength: plan.strength,
            damage: plan.damage,
            dealt,
        });
    }
    delivered
}

/// Whether a gun contact in `hand` at `now_s` may pulse: none has pulsed in that hand for
/// `interval_s`. Records the pulse when it may.
fn gun_pulse_due(last: &mut [Option<f64>; 2], hand: Hand, now_s: f64, interval_s: f64) -> bool {
    let due = last[hand.index()].is_none_or(|at| now_s - at >= interval_s || now_s < at);
    if due {
        last[hand.index()] = Some(now_s);
    }
    due
}

/// When each hand's gun last pulsed, seconds on the blows' clock.
static GUN_PULSES: Mutex<[Option<f64>; 2]> = Mutex::new([None; 2]);

/// Forget the contacts recorded since the last frame tick, the characters struck lately, and the
/// shoves queued.
pub(super) fn discard() {
    let _ = contact::take();
    GATE.lock().0.clear();
    *GUN_PULSES.lock() = [None; 2];
    SHOVES.lock().clear();
}

/// A push a hand gives a character it holds: the yank that throws the character off its feet, or
/// the shove of a hand that lets go of it moving.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Shove {
    pub hand: Hand,
    pub character: NonZeroUsize,
    /// The character's body the hand held, which must still belong to it.
    pub body: u32,
    /// Where the hand pushes, world space.
    pub at: Vec3,
    /// The push's direction, a unit vector.
    pub direction: Vec3,
    pub force: ShoveForce,
}

/// How hard a [`Shove`] pushes.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ShoveForce {
    /// A yank, at this share of the knockdown blow.
    Yank { strength: f32 },
    /// A shove from a hand moving at this speed, as a blow of the arm's momentum.
    Push { speed_mps: f32 },
}

/// A shove dealt, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShoveReport {
    pub hand: Hand,
    /// The shove as a share of the knockdown impulse.
    pub strength: f32,
    pub hostile: bool,
    pub yank: bool,
}

/// What a frame tick's shoves did.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ShovesDelivered {
    pub shoves: u64,
    /// Shoves that reached a knockdown.
    pub knockdowns: u64,
    pub last: Option<ShoveReport>,
}

/// Queue `shove` for the next frame tick. Called from the palms' tick.
pub(crate) fn queue_shove(shove: Shove) {
    let mut shoves = SHOVES.lock();
    if shoves.len() < MAX_SHOVES {
        shoves.push(shove);
    }
}

/// Deal the shoves queued since the last frame tick through each character's own hit reaction, as
/// a blow without damage. A character the player is not hostile to is staggered at most.
///
/// # Safety
///
/// The game thread, outside the game's update.
pub(super) unsafe fn deliver_shoves(config: &PhysicalHandsConfig) -> ShovesDelivered {
    let shoves = std::mem::take(&mut *SHOVES.lock());
    let mut delivered = ShovesDelivered::default();
    let Some(attacker) = (unsafe { Character::GetLocalPlayerCharacter().as_ref() }) else {
        return delivered;
    };
    let hits = HandHitsConfig {
        damage: false,
        min_impulse_ns: 0.0,
        ..config.bodies.hits
    };
    for shove in shoves {
        // SAFETY: forwarded from the caller; the body's owner proves the character alive.
        let alive = unsafe { owner_of(shove.body) } == Some(shove.character)
            && unsafe { engine::is_character(shove.character) };
        if !alive || shove.character.get() == attacker as *const Character as usize {
            continue;
        }
        // SAFETY: a live character, checked above.
        let victim = unsafe { &mut *(shove.character.get() as *mut Character) };
        let impulse_ns = match shove.force {
            ShoveForce::Yank { strength } => strength.max(0.0) * hits.knockdown_impulse_ns,
            ShoveForce::Push { speed_mps } => hits.blow_mass_kg.max(0.0) * speed_mps.max(0.0),
        };
        let tuning = VictimTuning {
            type_scale: victim
                .m_HitReaction_DamageImpulseScale
                .get(melee_type_index())
                .copied()
                .unwrap_or(0.0),
            knockdown_threshold: victim.m_HitReaction_ImpulseRagdollThreshold,
        };
        let Blow::Strikes(mut plan) = blow(impulse_ns, tuning, 0, &hits) else {
            continue;
        };
        // SAFETY: both characters are live.
        let relation = unsafe { GetRelation(attacker.GetFaction(), victim.GetFaction()) };
        let hostile = relation == FactionRelation::E_RELATION_HOSTILE;
        if !hostile {
            plan = restrain(plan, NOT_HOSTILE_MAX_STRENGTH);
        }
        plan.damage = None;
        let yank = matches!(shove.force, ShoveForce::Yank { .. });
        // SAFETY: forwarded from the caller; both characters are live.
        unsafe { strike(attacker, victim, shove.at, shove.direction, &plan) };
        tracing::info!(
            hand = shove.hand.label(),
            yank,
            hostile,
            strength = plan.strength,
            reaction_impulse = plan.reaction_impulse,
            "physical hands: characters: a hand threw a character"
        );
        delivered.shoves += 1;
        delivered.knockdowns += u64::from(plan.strength >= 1.0);
        delivered.last = Some(ShoveReport {
            hand: shove.hand,
            strength: plan.strength,
            hostile,
            yank,
        });
    }
    delivered
}

/// The most shoves queued between two frame ticks.
const MAX_SHOVES: usize = 8;

/// The shoves queued since the last frame tick.
static SHOVES: Mutex<Vec<Shove>> = Mutex::new(Vec::new());

/// How far above a character's root its chest is, metres: where a blow aims.
const CHEST_HEIGHT_M: f32 = 1.2;

/// The strongest blow a character the player is not hostile to takes, as a share of a knockdown.
const NOT_HOSTILE_MAX_STRENGTH: f32 = 0.9;

/// The share of a knockdown from which a blow follows through.
const FOLLOW_THROUGH_STRENGTH: f32 = 0.5;

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
    let world = unsafe { World::get() }?;
    let instance = unsafe { body::instance_of(world.world(), contact.other_body) }?;
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
    fn a_guns_contacts_pulse_at_most_once_an_interval_per_hand() {
        let mut last = [None; 2];
        assert!(gun_pulse_due(&mut last, Hand::Right, 10.0, 0.2));
        assert!(!gun_pulse_due(&mut last, Hand::Right, 10.1, 0.2));
        // The other hand keeps its own interval.
        assert!(gun_pulse_due(&mut last, Hand::Left, 10.1, 0.2));
        assert!(gun_pulse_due(&mut last, Hand::Right, 10.25, 0.2));
        // A clock that went back starts again.
        assert!(gun_pulse_due(&mut last, Hand::Right, 1.0, 0.2));
    }

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
