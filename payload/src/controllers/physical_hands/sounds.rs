//! Impact sounds: a hand that meets something hard enough makes the sound the game's own melee hit
//! makes there (`docs/mod/input/impact-sounds.md`).
//!
//! The player character's melee impact usage
//! ([`AttachedEffectContainer::MELEE_IMPACT_USAGE`]) resolves, for the struck surface's material,
//! to the effect and sound the game plays where a punch or a kick lands
//! ([`AttachedEffectContainer::GetDynamicAttachedEffects`]); the mod plays only its sound
//! ([`EffectAndSoundPair::PlayOneOffs`] with the effect culled), at the contact, facing back along
//! the hand's travel. A palm's touch plays against the material its cast read, a hand body's contact
//! against flesh when it struck a character and against the default material otherwise.
//!
//! The game's sound events take no loudness, so a meeting's strength only gates whether it sounds:
//! a palm must meet the surface at [`ImpactSoundsConfig::min_touch_speed_mps`] or faster, and a
//! hand body's contact must carry [`ImpactSoundsConfig::min_strike_share`] of the strongest hit
//! pulse. A hand sounds at most once per [`ImpactSoundsConfig::hand_cooldown_s`], and both hands
//! together at most once per [`ImpactSoundsConfig::min_interval_s`], so that one slap, which the
//! palm's cast and the hand body may both report, and a hand resting against a wall make one
//! sound.
//!
//! Both callers run on the game thread outside the physics step: the palms' tick and the hand
//! bodies' frame tick.

use std::{ptr, time::Instant};

use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::attached_effects::{AttachedEffectContainer, CreateCollisionMatrix, EffectCull},
    physics::GameMaterialId,
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::controllers::frame::Hand;

/// The impact sounds' settings, `physical_hands.sounds`.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpactSoundsConfig {
    /// Play the game's melee impact sound where a hand meets something hard enough.
    pub enabled: bool,
    /// The slowest a palm may meet a surface and still sound, metres per second.
    pub min_touch_speed_mps: f32,
    /// The least share of the strongest hit pulse a hand body's contact must carry to sound.
    pub min_strike_share: f32,
    /// How long a hand stays silent after it sounds, seconds.
    pub hand_cooldown_s: f32,
    /// The least time between two sounds of either hand, seconds.
    pub min_interval_s: f32,
}

impl ImpactSoundsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            min_touch_speed_mps: 1.5,
            min_strike_share: 0.35,
            hand_cooldown_s: 0.25,
            min_interval_s: 0.06,
        }
    }
}

impl Default for ImpactSoundsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What a sound is for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ImpactKind {
    /// A palm's cast met a surface.
    Touch,
    /// A hand body's contact.
    Strike,
}

impl ImpactKind {
    pub const fn label(self) -> &'static str {
        match self {
            ImpactKind::Touch => "touch",
            ImpactKind::Strike => "strike",
        }
    }
}

/// One impact that may sound.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Impact {
    pub hand: Hand,
    pub kind: ImpactKind,
    /// Where the hand met it, world space.
    pub at: Vec3,
    /// The hand's velocity, world space.
    pub velocity: Vec3,
    /// The struck surface's game material.
    pub material: u32,
    /// How hard the hand met it, in the units its kind's threshold is in: metres per second for a
    /// touch, the share of the strongest pulse for a strike.
    pub strength: f32,
}

/// The impact sounds' readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct SoundsStatus {
    /// The sounds played.
    pub played: u64,
    /// Impacts the cooldowns held back.
    pub limited: u64,
    /// Impacts whose material has no melee impact sound.
    pub silent: u64,
    /// The last sound's material and kind.
    pub last: Option<(u32, ImpactKind)>,
}

pub fn status() -> SoundsStatus {
    STATE.lock().status
}

/// Whether `impact` is strong enough to sound under `config`.
pub fn strong_enough(impact: &Impact, config: &ImpactSoundsConfig) -> bool {
    let min = match impact.kind {
        ImpactKind::Touch => config.min_touch_speed_mps,
        ImpactKind::Strike => config.min_strike_share,
    };
    impact.strength.is_finite() && impact.strength >= min
}

/// The cooldowns that keep one slap from sounding twice.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Limiter {
    last_hand: [Option<f64>; 2],
    last_any: Option<f64>,
}

impl Limiter {
    pub const fn new() -> Self {
        Self {
            last_hand: [None; 2],
            last_any: None,
        }
    }

    /// Whether `hand` may sound at `now_s`, and if so, note that it does.
    pub fn admit(&mut self, hand: Hand, now_s: f64, config: &ImpactSoundsConfig) -> bool {
        let since = |last: Option<f64>| last.map_or(f64::INFINITY, |t| now_s - t);
        if since(self.last_hand[hand.index()]) < f64::from(config.hand_cooldown_s.max(0.0))
            || since(self.last_any) < f64::from(config.min_interval_s.max(0.0))
        {
            return false;
        }
        self.last_hand[hand.index()] = Some(now_s);
        self.last_any = Some(now_s);
        true
    }
}

/// The material a hand body's contact plays against: flesh for a character, the default for
/// anything else, since the contact does not carry the struck shape.
pub const fn strike_material(character: bool) -> u32 {
    if character {
        GameMaterialId::EGameMaterialId_Flesh as u32
    } else {
        GameMaterialId::EGameMaterialId_Default as u32
    }
}

/// Play `impact`'s sound when it is strong enough and the cooldowns allow.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(super) unsafe fn play(impact: Impact, config: &ImpactSoundsConfig) {
    if !config.enabled || !strong_enough(&impact, config) {
        return;
    }
    let mut state = STATE.lock();
    let now_s = state
        .epoch
        .get_or_insert_with(Instant::now)
        .elapsed()
        .as_secs_f64();
    if !state.limiter.admit(impact.hand, now_s, config) {
        state.status.limited += 1;
        return;
    }
    // SAFETY: forwarded from the caller.
    match unsafe { sound(&impact) } {
        Outcome::Played => {
            state.status.played += 1;
            state.status.last = Some((impact.material, impact.kind));
            tracing::info!(
                hand = impact.hand.label(),
                kind = impact.kind.label(),
                material = impact.material,
                strength = impact.strength,
                "impact sounds: a hand made the melee impact sound"
            );
        }
        Outcome::Silent => {
            state.status.silent += 1;
            if !state.warned_silent {
                state.warned_silent = true;
                tracing::info!(
                    material = impact.material,
                    "impact sounds: the player's melee impact has no sound for a struck material"
                );
            }
        }
        Outcome::NoPlayer => {}
    }
}

struct State {
    limiter: Limiter,
    status: SoundsStatus,
    epoch: Option<Instant>,
    warned_silent: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    limiter: Limiter::new(),
    status: SoundsStatus {
        played: 0,
        limited: 0,
        silent: 0,
        last: None,
    },
    epoch: None,
    warned_silent: false,
});

enum Outcome {
    Played,
    Silent,
    NoPlayer,
}

/// The cull that plays a pair's sound and not its effect.
const SOUND_ONLY: EffectCull = EffectCull {
    m_PlayEffect: false,
    m_PlaySound: true,
};

/// Play the player's melee impact sound for `impact`'s material at the impact.
///
/// # Safety
///
/// As [`play`].
unsafe fn sound(impact: &Impact) -> Outcome {
    // SAFETY: the game thread, where the local player character is live.
    let Some(player) = (unsafe { Character::GetLocalPlayerCharacter().as_mut() }) else {
        return Outcome::NoPlayer;
    };
    // SAFETY: the player's own container; the lookup only reads its loaded usages.
    let pair = unsafe {
        player.m_AttachedEffects.GetDynamicAttachedEffects(
            AttachedEffectContainer::MELEE_IMPACT_USAGE,
            GameMaterialId::EGameMaterialId_Default,
            impact.material,
            ptr::null_mut(),
        )
    };
    // SAFETY: the lookup returns a pair it owns, or its shared empty pair, never null.
    let Some(pair) = (unsafe { pair.as_ref() }).filter(|p| !p.m_Sound.px.is_null()) else {
        return Outcome::Silent;
    };
    let facing = (-impact.velocity).try_normalize().unwrap_or(Vec3::Y);
    let mut transform = Matrix4::ZERO;
    // SAFETY: plain values on the stack; the pair stays loaded while the player lives, and the
    // sound holds its own reference once started.
    unsafe {
        CreateCollisionMatrix(&mut transform, &vector(impact.at), &vector(facing));
        pair.PlayOneOffs(
            &transform,
            &vector(Vec3::ZERO),
            &SOUND_ONLY,
            ptr::null(),
            ptr::null(),
        );
    }
    Outcome::Played
}

fn vector(v: Vec3) -> Vector3 {
    Vector3 { data: v.to_array() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ImpactSoundsConfig = ImpactSoundsConfig::new();

    fn impact(kind: ImpactKind, strength: f32) -> Impact {
        Impact {
            hand: Hand::Left,
            kind,
            at: Vec3::ZERO,
            velocity: Vec3::X,
            material: strike_material(false),
            strength,
        }
    }

    #[test]
    fn a_gentle_meeting_makes_no_sound() {
        assert!(!strong_enough(&impact(ImpactKind::Touch, 0.5), &CONFIG));
        assert!(strong_enough(&impact(ImpactKind::Touch, 3.0), &CONFIG));
        assert!(!strong_enough(&impact(ImpactKind::Strike, 0.1), &CONFIG));
        assert!(strong_enough(&impact(ImpactKind::Strike, 0.8), &CONFIG));
        assert!(!strong_enough(
            &impact(ImpactKind::Touch, f32::NAN),
            &CONFIG
        ));
    }

    #[test]
    fn a_hand_sounds_once_per_cooldown() {
        let mut limiter = Limiter::new();
        assert!(limiter.admit(Hand::Left, 0.0, &CONFIG));
        assert!(!limiter.admit(Hand::Left, 0.1, &CONFIG));
        assert!(limiter.admit(Hand::Left, 0.3, &CONFIG));
    }

    #[test]
    fn the_other_hand_waits_only_the_shared_interval() {
        let mut limiter = Limiter::new();
        assert!(limiter.admit(Hand::Left, 0.0, &CONFIG));
        assert!(!limiter.admit(Hand::Right, 0.02, &CONFIG));
        assert!(limiter.admit(Hand::Right, 0.1, &CONFIG));
    }

    #[test]
    fn a_held_back_impact_does_not_restart_the_cooldown() {
        let mut limiter = Limiter::new();
        assert!(limiter.admit(Hand::Left, 0.0, &CONFIG));
        assert!(!limiter.admit(Hand::Left, 0.2, &CONFIG));
        assert!(limiter.admit(Hand::Left, 0.26, &CONFIG));
    }

    #[test]
    fn a_strike_on_a_character_is_flesh() {
        assert_ne!(strike_material(true), strike_material(false));
        assert_eq!(
            strike_material(true),
            GameMaterialId::EGameMaterialId_Flesh as u32
        );
    }
}
