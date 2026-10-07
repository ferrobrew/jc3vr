//! A hand body's hits, pure: how hard a contact struck, what a struck character's own hit reaction
//! is given and what damage the blow deals, how hard the hand pulses, and the gate that makes one
//! blow one hit.
//!
//! A blow is measured by the impulse the physics solver applied at the contact over the step it
//! started, which is the momentum the hand gave up to what it struck. The character reacts through
//! its own hit reaction, which scales the impulse it is given by the blow type's factor and
//! compares the result against the character's stagger and knockdown thresholds. The blow is
//! calibrated against the character's own knockdown threshold: a blow of
//! [`HandHitsConfig::knockdown_impulse_ns`] reaches it, and a weaker blow the same share of it, so
//! the character's own tuning decides where between a flinch and a stagger a weaker blow lands.

use crate::controllers::physical_hands::physics::config::HandHitsConfig;

/// A struck character's own hit-reaction tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct VictimTuning {
    /// The factor the character's hit reaction scales a blow of this type by.
    pub type_scale: f32,
    /// The scaled impulse at or above which the character is knocked down.
    pub knockdown_threshold: f32,
}

/// What a blow does to a character.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Blow {
    /// Too weak to react to.
    TooWeak,
    /// The character's tuning cannot scale a blow: a type factor or knockdown threshold that is not
    /// positive.
    Uncalibrated,
    /// A blow the character reacts to.
    Strikes(BlowPlan),
}

/// A blow a character reacts to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BlowPlan {
    /// The blow as a share of the knockdown impulse; one and above knocks the character down.
    pub strength: f32,
    /// The impulse the character's hit reaction is given, before it scales it by the type factor.
    pub reaction_impulse: f32,
    /// The damage dealt, in health points, for a blow hard enough to deal any.
    pub damage: Option<i16>,
}

/// The blow of a contact of `impulse_ns` on a character tuned as `victim`, from a player whose
/// melee damage is `melee_damage`.
pub fn blow(
    impulse_ns: f32,
    victim: VictimTuning,
    melee_damage: i32,
    config: &HandHitsConfig,
) -> Blow {
    if !impulse_ns.is_finite() || impulse_ns < config.min_impulse_ns.max(0.0) || impulse_ns <= 0.0 {
        return Blow::TooWeak;
    }
    if !(victim.type_scale.is_finite()
        && victim.type_scale > 0.0
        && victim.knockdown_threshold.is_finite()
        && victim.knockdown_threshold > 0.0)
    {
        return Blow::Uncalibrated;
    }
    let knockdown = config.knockdown_impulse_ns;
    if !(knockdown.is_finite() && knockdown > 0.0) {
        return Blow::Uncalibrated;
    }
    let strength = impulse_ns / knockdown;
    let reaction_impulse = strength * victim.knockdown_threshold / victim.type_scale;
    let damage = (config.damage && impulse_ns >= config.damage_impulse_ns).then(|| {
        let damage = melee_damage.max(0) as f32 * config.damage_scale.max(0.0) * strength;
        if damage.is_finite() {
            damage.round().clamp(1.0, f32::from(i16::MAX)) as i16
        } else {
            1
        }
    });
    Blow::Strikes(BlowPlan {
        strength,
        reaction_impulse: if reaction_impulse.is_finite() {
            reaction_impulse
        } else {
            0.0
        },
        damage,
    })
}

/// The share of the strongest hit pulse a contact of `impulse_ns` pulses its hand with, `0..=1`.
pub fn pulse_share(impulse_ns: f32, full_impulse_ns: f32) -> f32 {
    if !(impulse_ns.is_finite() && full_impulse_ns.is_finite() && full_impulse_ns > 0.0) {
        return 0.0;
    }
    (impulse_ns / full_impulse_ns).clamp(0.0, 1.0)
}

/// The total of a contact's point impulses: the first `points` of `impulses`, the positive and
/// finite ones.
pub fn contact_impulse(impulses: [f32; 4], points: i32) -> f32 {
    let points = points.clamp(0, 4) as usize;
    impulses[..points]
        .iter()
        .filter(|i| i.is_finite() && **i > 0.0)
        .sum()
}

/// Which characters each hand struck lately: a character a hand struck ignores that hand's further
/// contacts for a while, so that one blow, whose contact may start and end several times as the
/// hand passes through, is one hit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HitGate {
    /// The hand, the character's address, and when it was struck, seconds.
    recent: Vec<(usize, usize, f64)>,
}

impl HitGate {
    pub const fn new() -> Self {
        Self { recent: Vec::new() }
    }

    /// Whether hand `hand` may strike the character `victim` at `now_s` seconds; if it may, the hit
    /// is recorded. Hits older than `cooldown_s` are forgotten.
    pub fn admit(&mut self, hand: usize, victim: usize, now_s: f64, cooldown_s: f32) -> bool {
        let cooldown = f64::from(cooldown_s.max(0.0));
        self.recent.retain(|&(_, _, at)| now_s - at < cooldown);
        if self
            .recent
            .iter()
            .any(|&(h, v, _)| h == hand && v == victim)
        {
            return false;
        }
        if self.recent.len() >= MAX_RECENT {
            self.recent.remove(0);
        }
        self.recent.push((hand, victim, now_s));
        true
    }

    pub fn clear(&mut self) {
        self.recent.clear();
    }
}

/// The most hits the gate remembers.
const MAX_RECENT: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: HandHitsConfig = HandHitsConfig::new();
    const VICTIM: VictimTuning = VictimTuning {
        type_scale: 2.0,
        knockdown_threshold: 100.0,
    };

    fn plan(impulse: f32) -> BlowPlan {
        match blow(impulse, VICTIM, 100, &CONFIG) {
            Blow::Strikes(plan) => plan,
            other => panic!("{impulse}: {other:?}"),
        }
    }

    #[test]
    fn a_light_touch_is_no_blow() {
        assert_eq!(blow(1.0, VICTIM, 100, &CONFIG), Blow::TooWeak);
        assert_eq!(blow(f32::NAN, VICTIM, 100, &CONFIG), Blow::TooWeak);
        assert_eq!(blow(-5.0, VICTIM, 100, &CONFIG), Blow::TooWeak);
    }

    #[test]
    fn the_knockdown_impulse_reaches_the_knockdown_threshold() {
        let p = plan(CONFIG.knockdown_impulse_ns);
        assert!((p.strength - 1.0).abs() < 1e-6);
        // The character scales the reaction impulse by its type factor.
        assert!((p.reaction_impulse * VICTIM.type_scale - VICTIM.knockdown_threshold).abs() < 1e-3);
    }

    #[test]
    fn the_reaction_grows_with_the_impulse() {
        let weak = plan(CONFIG.min_impulse_ns + 1.0);
        let strong = plan(CONFIG.knockdown_impulse_ns * 1.5);
        assert!(weak.reaction_impulse < strong.reaction_impulse);
        assert!(weak.strength < 1.0 && strong.strength > 1.0);
    }

    #[test]
    fn only_a_hard_blow_deals_damage() {
        assert_eq!(plan(CONFIG.damage_impulse_ns - 1.0).damage, None);
        let hard = plan(CONFIG.knockdown_impulse_ns);
        assert_eq!(hard.damage, Some(100));
        let off = HandHitsConfig {
            damage: false,
            ..CONFIG
        };
        let Blow::Strikes(p) = blow(CONFIG.knockdown_impulse_ns, VICTIM, 100, &off) else {
            panic!("a blow");
        };
        assert_eq!(p.damage, None);
        // A blow that deals damage deals at least a point.
        let Blow::Strikes(p) = blow(CONFIG.damage_impulse_ns, VICTIM, 0, &CONFIG) else {
            panic!("a blow");
        };
        assert_eq!(p.damage, Some(1));
    }

    #[test]
    fn a_character_without_tuning_is_not_struck() {
        for victim in [
            VictimTuning {
                type_scale: 0.0,
                ..VICTIM
            },
            VictimTuning {
                knockdown_threshold: f32::NAN,
                ..VICTIM
            },
        ] {
            assert_eq!(blow(60.0, victim, 100, &CONFIG), Blow::Uncalibrated);
        }
    }

    #[test]
    fn a_contact_counts_its_own_points() {
        assert_eq!(contact_impulse([1.0, 2.0, 4.0, 8.0], 2), 3.0);
        assert_eq!(contact_impulse([1.0, -2.0, f32::NAN, 8.0], 4), 9.0);
        assert_eq!(contact_impulse([1.0; 4], 9), 4.0);
        assert_eq!(contact_impulse([1.0; 4], -1), 0.0);
    }

    #[test]
    fn the_pulse_saturates() {
        assert_eq!(pulse_share(25.0, 50.0), 0.5);
        assert_eq!(pulse_share(500.0, 50.0), 1.0);
        assert_eq!(pulse_share(5.0, 0.0), 0.0);
    }

    #[test]
    fn one_blow_is_one_hit() {
        let mut gate = HitGate::new();
        assert!(gate.admit(0, 7, 0.0, 0.5));
        assert!(!gate.admit(0, 7, 0.1, 0.5));
        // The other hand, or another character, is another hit.
        assert!(gate.admit(1, 7, 0.1, 0.5));
        assert!(gate.admit(0, 8, 0.1, 0.5));
        // Past the cooldown, the hand strikes again.
        assert!(gate.admit(0, 7, 0.6, 0.5));
    }
}
