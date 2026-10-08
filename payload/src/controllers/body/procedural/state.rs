//! What the character is doing, read for the hand-over table at the solve gate
//! ([`StateTracker::read`]), and whether the animation moves the body this tick
//! ([`animation_moves_body`]), which the stick locomotion asks before it replaces the movement
//! task's velocity.
//!
//! The predicates read here walk the character's own rule system and interaction graph, which the
//! engine itself reads from the pose-finalization pass (`Character::IsInStuntPosition` and
//! `Character::IsGrappleHanging` among them).

use std::time::Instant;

use jc3gi::{character::character::Character, state::StateBitFlag};

use crate::{
    config::Config,
    controllers::{
        body::procedural::{
            authority::CharacterState,
            config::ProceduralBodyConfig,
            signals::{in_hit_reaction, in_ledge_climb},
        },
        hands::tiers::{self, TierState},
        locomotion::LocomotionStatus,
        mode::{AirborneVariant, Mode, OnFootVariant, current as current_mode},
        physical_hands::climb,
    },
    headpose::{Source, source},
    hooks::in_gameplay,
};

/// Whether the game's animation moves the body this tick, rather than the movement task's
/// locomotion: an authored motion (a one-off, a hard landing, a context action, a pickup, cover)
/// or a hit reaction's hold. The root motion of these is the motion: a stumble that the stick's
/// velocity replaced would go nowhere.
///
/// # Safety
///
/// `character` must be the live local character, on the thread running its update.
pub(crate) unsafe fn animation_moves_body(character: &Character, now: Instant) -> bool {
    let hold_s = Config::lock_query(|c| c.controllers.body.procedural.hit_reaction_hold_s);
    let bits = unsafe { StateBits::of(character) };
    bits.authored_motion() || in_hit_reaction(now, hold_s)
}

/// The history the hand-over table's reads keep between gates.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StateTracker {
    /// When Rico was last on the ground with the movement task running, for the jump's grace.
    grounded_at: Option<Instant>,
    /// When the movement task had last run as of the previous gate, so this gate can tell whether
    /// it ran since.
    task_seen_at: Option<Instant>,
    /// How long the proxy has been off its support without a pause, seconds.
    in_air_s: f32,
}

impl StateTracker {
    pub const fn new() -> Self {
        Self {
            grounded_at: None,
            task_seen_at: None,
            in_air_s: 0.0,
        }
    }

    /// What the character is doing, for the hand-over table, read at the gate.
    ///
    /// # Safety
    ///
    /// `character` must be the live local character, inside its pose-finalization pass, on the
    /// thread running it.
    pub unsafe fn read(
        &mut self,
        character: &Character,
        locomotion: &LocomotionStatus,
        now: Instant,
        dt: f32,
        jump_grace_s: f32,
        config: &ProceduralBodyConfig,
    ) -> CharacterState {
        let detection = current_mode();
        let bits = unsafe { StateBits::of(character) };
        let has = |flag| bits.has(flag);
        let motion = character.m_CurrentMotionState;
        // The movement task runs in the state machines' pass, before this one, on every tick Rico
        // is on foot on the ground: it ran this tick when its stamp moved since the last gate.
        let movement_task = locomotion.at.is_some() && locomotion.at != self.task_seen_at;
        self.task_seen_at = locomotion.at;
        // The proxy's own support, read where the engine reads it in this pass, catches walking
        // off a ledge before the mode detection sees the fall.
        let off_support = unsafe { character.IsInAir() };
        self.in_air_s = if off_support { self.in_air_s + dt } else { 0.0 };
        let falling = detection.mode == Mode::Airborne(AirborneVariant::Falling);
        let airborne = has(StateBitFlag::SB_IS_JUMPING)
            || falling
            || (off_support && self.in_air_s >= IN_AIR_DEBOUNCE_S);
        let on_ground = detection.mode == Mode::OnFoot(OnFootVariant::Ground);
        let stunt_position = unsafe { character.IsInStuntPosition() };
        let stunt_traversing = unsafe { character.IsStuntTraversing() };
        // A roof, a deck, or a wing: not the helicopter's hang, nor a step up or down.
        let stunt_standing = stunt_position
            && !stunt_traversing
            && unsafe { character.IsInStuntArea() }
            && !unsafe { character.IsStuntHanging() };
        if movement_task && on_ground && !airborne {
            self.grounded_at = Some(now);
        }
        let in_grace = self
            .grounded_at
            .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() <= jump_grace_s);
        CharacterState {
            in_headset: source() == Source::Vr,
            in_gameplay: in_gameplay(),
            sequence: character.m_InSequence,
            dead: unsafe { character.GetHealth() } <= 0 || has(StateBitFlag::SB_IS_IN_DEAD_STATE),
            ragdoll: motion == Character::MOTION_STATE_RAGDOLL || has(StateBitFlag::SB_IN_RAGDOLL),
            getting_up: has(StateBitFlag::SB_IS_GETTING_UP_FROM_RAGDOLL),
            vehicle: matches!(detection.mode, Mode::Vehicle(_))
                || has(StateBitFlag::SB_INSIDE_VEHICLE)
                || stunt_position,
            // The hand tiers' classification, from the input tick: seated and riding, which an
            // entry, an exit, a seat switch, and a mounted gun are not. It lags by up to a frame,
            // as the mode detection does.
            seated: tiers::status().state == Some(TierState::VehicleSeated) && !stunt_position,
            stunt_standing,
            traversal: detection.traversal.is_some()
                || stunt_traversing
                || unsafe { character.IsGrappleHanging() }
                || has(StateBitFlag::SB_IN_GRAPPLE_STRUGGLE),
            ledge_climb: in_ledge_climb(now),
            hit_reaction: in_hit_reaction(now, config.hit_reaction_hold_s),
            hand_climb: climb::climbing(),
            swimming: detection.mode == Mode::OnFoot(OnFootVariant::Swimming)
                || motion == Character::MOTION_STATE_SWIMMING
                || has(StateBitFlag::SB_IS_SWIMMING),
            wingsuit: detection.mode == Mode::Airborne(AirborneVariant::Wingsuit)
                || has(StateBitFlag::SB_IS_WINGSUIT)
                || has(StateBitFlag::SB_IN_TRANSITION_TO_WINGSUIT),
            parachute: detection.mode == Mode::Airborne(AirborneVariant::Parachute),
            flight: airborne && !in_grace,
            // In the air wins over the movement task's stamp: the task can run on the tick a jump
            // or a step off a ledge leaves the ground.
            jump: airborne && in_grace,
            authored_motion: bits.authored_motion(),
            movement_task,
        }
    }
}

/// How long the proxy must be off its support, with no jump and no fall detected, before the
/// body counts as in the air, seconds: a stair or a bump lifts it for a tick without a fall.
const IN_AIR_DEBOUNCE_S: f32 = 0.06;

/// The state bits of an authored motion: the animation carries the body through it.
const AUTHORED_MOTION: [StateBitFlag; 7] = [
    StateBitFlag::SB_IS_LOCO_ONE_OFF,
    StateBitFlag::SB_IS_LANDING_HARD,
    StateBitFlag::SB_IN_LOOP_CONTEXT_ACTION,
    StateBitFlag::SB_IN_FULLBODY_PICKUP,
    StateBitFlag::SB_IN_COVER_ENTRY,
    StateBitFlag::SB_IN_COVER,
    StateBitFlag::SB_IN_COVER_EXIT,
];

/// The character's state-bit words, read once.
#[derive(Debug, Copy, Clone, PartialEq)]
struct StateBits {
    words: Option<[u64; 3]>,
}

impl StateBits {
    /// # Safety
    ///
    /// `character` must be live.
    unsafe fn of(character: &Character) -> Self {
        let words = unsafe { character.m_ControlParameters.as_ref() }.map(|control| {
            let vm = &control.m_UserVM;
            [
                vm.m_CurrentStateBitFlags0,
                vm.m_CurrentStateBitFlags1,
                vm.m_CurrentStateBitFlags2,
            ]
        });
        Self { words }
    }

    fn has(&self, flag: StateBitFlag) -> bool {
        let bit = flag as u32 as usize;
        self.words
            .and_then(|words| words.get(bit / 64).copied())
            .is_some_and(|word| (word >> (bit % 64)) & 1 != 0)
    }

    fn authored_motion(&self) -> bool {
        AUTHORED_MOTION.into_iter().any(|flag| self.has(flag))
    }
}
