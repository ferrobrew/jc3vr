//! Palm strikes, pure: when a palm's hit on a character strikes, how hard, whether as a punch or a
//! slap, and what it deals.

use std::ffi::CStr;

use glam::Vec3;
use jc3gi::animation::symbol_table::EventIdSymbolTable;

use crate::controllers::physical_hands::config::{PhysicalHandsConfig, StrikeTuning};

/// How a hand strikes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StrikeKind {
    /// A closed fist.
    Punch,
    /// An open hand.
    Slap,
}

impl StrikeKind {
    pub const fn label(self) -> &'static str {
        match self {
            StrikeKind::Punch => "punch",
            StrikeKind::Slap => "slap",
        }
    }

    /// The kind of a hand whose four fingers' mean curl is `curl` (`0..=1`).
    pub fn of_curl(curl: f32, config: &PhysicalHandsConfig) -> Self {
        if curl >= config.fist_curl {
            StrikeKind::Punch
        } else {
            StrikeKind::Slap
        }
    }

    pub fn tuning(self, config: &PhysicalHandsConfig) -> StrikeTuning {
        match self {
            StrikeKind::Punch => config.punch,
            StrikeKind::Slap => config.slap,
        }
    }
}

/// The strength of a hit at `speed` metres per second, `0..=1`: zero at the slowest strike speed,
/// one from the full-strength speed. `None` below the slowest strike speed.
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

/// The per-hand strike gate: a hand strikes once, then must slow below the rearm speed and wait
/// out its cooldown before it strikes again, so that one swing through a body is one strike.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikeGate {
    armed: bool,
    cooldown_s: f32,
}

impl StrikeGate {
    pub const fn new() -> Self {
        Self {
            armed: true,
            cooldown_s: 0.0,
        }
    }

    /// Fold one tick of `dt` seconds with the hand at `speed` metres per second in.
    pub fn advance(&mut self, dt: f32, speed: f32, config: &PhysicalHandsConfig) {
        self.cooldown_s = (self.cooldown_s - dt.max(0.0)).max(0.0);
        if !self.armed && speed < config.rearm_speed_mps {
            self.armed = true;
        }
    }

    /// Whether the hand may strike now; if it may, the strike is taken.
    pub fn take(&mut self, config: &PhysicalHandsConfig) -> bool {
        if !self.armed || self.cooldown_s > 0.0 {
            return false;
        }
        self.armed = false;
        self.cooldown_s = config.strike_cooldown_s.max(0.0);
        true
    }

    pub fn armed(&self) -> bool {
        self.armed && self.cooldown_s <= 0.0
    }
}

impl Default for StrikeGate {
    fn default() -> Self {
        Self::new()
    }
}

/// What one strike deals.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StrikePlan {
    pub kind: StrikeKind,
    /// `0..=1`.
    pub strength: f32,
    /// Health points, never below one.
    pub damage: i16,
    /// The impulse the hit reaction is given.
    pub impulse: f32,
    /// Whether the struck character stumbles.
    pub stumble: bool,
}

/// The strike of `kind` at `strength`, scaled from the player's own melee damage and impulse.
pub fn plan(
    kind: StrikeKind,
    strength: f32,
    melee_damage: i32,
    melee_impulse: f32,
    config: &PhysicalHandsConfig,
) -> StrikePlan {
    let strength = if strength.is_finite() {
        strength.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let tuning = kind.tuning(config);
    let damage = (melee_damage.max(0) as f32 * tuning.damage.max(0.0) * strength).round();
    let impulse = melee_impulse.max(0.0) * tuning.impulse.max(0.0) * strength;
    StrikePlan {
        kind,
        strength,
        damage: damage.clamp(1.0, f32::from(i16::MAX)) as i16,
        impulse: if impulse.is_finite() { impulse } else { 0.0 },
        stumble: strength >= tuning.stumble_strength,
    }
}

/// Which of the game's melee stumbles a struck character plays.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Stumble {
    LeftFront,
    RightFront,
    LeftBehind,
    RightBehind,
}

impl Stumble {
    /// The stumble of a character facing `forward` struck along `direction`. A character facing
    /// into the blow is struck from the front; the side is the side of the character the blow
    /// travels toward.
    pub fn of(forward: Vec3, direction: Vec3) -> Self {
        let forward = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
        let direction = Vec3::new(direction.x, 0.0, direction.z).normalize_or(-forward);
        let right = forward.cross(Vec3::Y);
        let front = forward.dot(direction) < 0.0;
        let toward_right = direction.dot(right) >= 0.0;
        match (front, toward_right) {
            (true, false) => Stumble::LeftFront,
            (true, true) => Stumble::RightFront,
            (false, false) => Stumble::LeftBehind,
            (false, true) => Stumble::RightBehind,
        }
    }

    pub const fn act(self) -> &'static CStr {
        match self {
            Stumble::LeftFront => EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_LEFT_0,
            Stumble::RightFront => EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_RIGHT_0,
            Stumble::LeftBehind => EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_LEFT_180,
            Stumble::RightBehind => EventIdSymbolTable::ACT_HITREACT_MELEE_STUMBLE_RIGHT_180,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: PhysicalHandsConfig = PhysicalHandsConfig::new();
    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn strength_ramps_between_the_speeds() {
        let cases = [
            (1.0, None),
            (2.0, Some(0.0)),
            (4.5, Some(0.5)),
            (9.0, Some(1.0)),
            (f32::NAN, None),
        ];
        for (speed, expected) in cases {
            let s = strength(
                speed,
                CONFIG.min_strike_speed_mps,
                CONFIG.full_strike_speed_mps,
            );
            match (s, expected) {
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

    #[test]
    fn a_fist_punches_and_an_open_hand_slaps() {
        assert_eq!(StrikeKind::of_curl(0.9, &CONFIG), StrikeKind::Punch);
        assert_eq!(StrikeKind::of_curl(0.1, &CONFIG), StrikeKind::Slap);
    }

    #[test]
    fn one_swing_strikes_once() {
        let mut gate = StrikeGate::new();
        assert!(gate.take(&CONFIG));
        // Still fast: the same swing.
        gate.advance(DT, 5.0, &CONFIG);
        assert!(!gate.take(&CONFIG));
        // Slowed, but inside the cooldown.
        gate.advance(DT, 0.2, &CONFIG);
        assert!(!gate.take(&CONFIG));
        let mut t = 0.0;
        while t < CONFIG.strike_cooldown_s {
            gate.advance(DT, 0.2, &CONFIG);
            t += DT;
        }
        assert!(gate.armed());
        assert!(gate.take(&CONFIG));
    }

    #[test]
    fn a_punch_outdeals_a_slap() {
        let punch = plan(StrikeKind::Punch, 1.0, 100, 50.0, &CONFIG);
        let slap = plan(StrikeKind::Slap, 1.0, 100, 50.0, &CONFIG);
        assert_eq!(punch.damage, 100);
        assert!(punch.damage > slap.damage && punch.impulse > slap.impulse);
        assert!(punch.stumble);
        // A weak strike still deals a point of damage and does not stumble.
        let weak = plan(StrikeKind::Slap, 0.01, 100, 50.0, &CONFIG);
        assert_eq!(weak.damage, 1);
        assert!(!weak.stumble);
        // Out-of-range inputs stay in range.
        let odd = plan(StrikeKind::Punch, f32::NAN, -5, f32::INFINITY, &CONFIG);
        assert_eq!(odd.strength, 0.0);
        assert_eq!(odd.damage, 1);
        assert!(odd.impulse.is_finite());
    }

    #[test]
    fn the_stumble_follows_the_blow() {
        let cases = [
            (Vec3::NEG_Z, Vec3::new(0.3, 0.0, 1.0), Stumble::RightFront),
            (Vec3::NEG_Z, Vec3::new(-0.3, 0.0, 1.0), Stumble::LeftFront),
            (Vec3::NEG_Z, Vec3::new(0.3, 0.0, -1.0), Stumble::RightBehind),
            (Vec3::X, Vec3::new(-1.0, 0.0, -0.2), Stumble::LeftFront),
        ];
        for (forward, direction, expected) in cases {
            assert_eq!(
                Stumble::of(forward, direction),
                expected,
                "{forward} {direction}"
            );
        }
    }
}
