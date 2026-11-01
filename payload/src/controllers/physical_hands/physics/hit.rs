//! A hand body's hits, pure: how hard a contact struck, what a struck character's own hit reaction
//! is given and what damage the blow deals, how hard the hand pulses, and the gate that makes one
//! blow one hit.
//!
//! A contact is a blow only when the hand swung into the character: its speed toward the character
//! before the contact ([`approach`]), measured against the player's own motion ([`swing`]) so that
//! a hand carried into a character by the player running is not a swing, must reach
//! [`HandHitsTuning::min_approach_speed_mps`], so a hand resting on a character, or a character
//! walking into a still hand, strikes nothing. A hand reaching for the gun in a character's hands
//! ([`Intent`]) strikes that character only with a deliberate swing, and a hand closing on it never
//! does: the grab takes the gun instead. The
//! blow's impulse is the momentum of an arm of [`HandHitsTuning::blow_mass_kg`] at the hand's own
//! share of that closing speed ([`blow_impulse`]), not the solver's contact impulse, which grows
//! with the hand body's own mass and with how fast the character moves into the hand. The character
//! reacts through its own hit reaction, which scales the impulse it is given by the blow type's
//! factor and compares the result against the character's stagger and knockdown thresholds. The
//! blow is calibrated against the character's own thresholds ([`blow`]): a blow of
//! [`HandHitsTuning::stagger_impulse_ns`] (a solid punch) reaches its stagger threshold, a harder
//! one rises toward its knockdown threshold, and a blow of
//! [`HandHitsTuning::knockdown_impulse_ns`] (a hard swing) reaches the knockdown and ragdolls it;
//! a weaker blow than a solid punch flinches it in proportion. Every character is struck alike,
//! whatever its faction: the game's own reactions to being hit (alarm, heat) follow.

use glam::Vec3;

use crate::controllers::{
    clock::TimeScale,
    grab::{GrabVolume, latch::GripLatch},
    physical_hands::physics::config::HandHitsTuning,
};

/// How a hand and a character closed on each other before a contact: the hand's speed toward the
/// character, and the character's toward the hand, metres per second, neither below zero.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Approach {
    pub hand_mps: f32,
    pub victim_mps: f32,
}

/// How a hand moving at `hand_velocity` and a character moving at `victim_velocity` closed on
/// each other along `toward_victim`, the way from the hand to the character.
pub fn approach(hand_velocity: Vec3, victim_velocity: Vec3, toward_victim: Vec3) -> Approach {
    let Some(direction) = toward_victim.try_normalize() else {
        return Approach {
            hand_mps: 0.0,
            victim_mps: 0.0,
        };
    };
    let speed = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    Approach {
        hand_mps: speed(hand_velocity.dot(direction)),
        victim_mps: speed(-victim_velocity.dot(direction)),
    }
}

/// The velocity a hand moving at `hand_velocity` swings at, for a player moving at
/// `player_velocity`: the hand's own motion, without the player's that carries it. A player
/// velocity that is not finite counts as none.
pub fn swing(hand_velocity: Vec3, player_velocity: Vec3) -> Vec3 {
    if player_velocity.is_finite() {
        hand_velocity - player_velocity
    } else {
        hand_velocity
    }
}

/// What a hand that met a character meant to do to it, from its grip and whether the gun in that
/// character's hands is within the hand's reach to take.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Nothing of the character's within reach to take, or a fist: a swing strikes.
    Swing,
    /// An open hand within reach of the gun in the character's hands, on its way to take it: only a
    /// deliberate swing strikes.
    Reach,
    /// A hand closing on, or holding, the gun in the character's hands: it strikes nothing, and the
    /// grab takes the gun.
    Take,
}

impl Intent {
    /// The intent of a hand whose grip is `latch`, with `gun_in_reach` whether the gun in the
    /// struck character's hands is offered to it. A grip that took a weapon in the world takes,
    /// whatever was offered after.
    pub fn of(latch: GripLatch, gun_in_reach: bool) -> Self {
        match (latch, gun_in_reach) {
            (GripLatch::Grab(GrabVolume::WorldWeapon), _) => Intent::Take,
            (_, false) | (GripLatch::Fist { .. }, true) => Intent::Swing,
            (GripLatch::Open, true) => Intent::Reach,
            (GripLatch::Grab(_) | GripLatch::Default, true) => Intent::Take,
        }
    }
}

/// The impulse of a blow closing as `approach`, by a hand meaning `intent`, in a world running at
/// `time_scale`, newton seconds: an arm of `config.blow_mass_kg` at the hand's own speed into the
/// character, which is the hand's share of the closing speed. The speeds are the world's, per game
/// second. Whether the hand swung fast enough to strike is the player's arm, so it is judged per
/// real second: [`HandHitsTuning::min_approach_speed_mps`], or for a hand reaching for the
/// character's gun, the speed of a solid punch ([`HandHitsTuning::stagger_impulse_ns`]). The blow
/// itself is the hand's speed through the world, so in slow motion the player's arm, as fast as
/// ever in real time, strikes as hard as it moves through the slowed world, as a throw leaves the
/// hand. `None` for a hand too slow to strike, and for a hand taking the gun.
pub fn blow_impulse(
    approach: Approach,
    intent: Intent,
    time_scale: TimeScale,
    config: &HandHitsTuning,
) -> Option<f32> {
    let swing = config.min_approach_speed_mps.max(0.0);
    let min_mps = match intent {
        Intent::Swing => swing,
        Intent::Reach => {
            let punch = config.stagger_impulse_ns / config.blow_mass_kg;
            if punch.is_finite() {
                swing.max(punch)
            } else {
                swing
            }
        }
        Intent::Take => return None,
    };
    let real_mps = approach.hand_mps * time_scale.get();
    (real_mps >= min_mps && approach.hand_mps > 0.0)
        .then(|| config.blow_mass_kg.max(0.0) * approach.hand_mps)
}

/// The hits' settings for a blow struck with a held gun of `gun_mass_kg`: the gun's mass swung with
/// the arm's, so a rifle swung into a character strikes harder than a fist at the same speed.
/// `config` unchanged for a hand's own blow, or a mass that is not finite and positive.
pub fn striking_with(config: &HandHitsTuning, gun_mass_kg: Option<f32>) -> HandHitsTuning {
    match gun_mass_kg.filter(|m| m.is_finite() && *m > 0.0) {
        Some(mass) => HandHitsTuning {
            blow_mass_kg: config.blow_mass_kg.max(0.0) + mass,
            ..*config
        },
        None => *config,
    }
}

/// A struck character's own hit-reaction tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct VictimTuning {
    /// The factor the character's hit reaction scales a blow of this type by.
    pub type_scale: f32,
    /// The scaled impulse at or above which the character staggers.
    pub stagger_threshold: f32,
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
    config: &HandHitsTuning,
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
    let reaction_impulse = scaled_reaction(impulse_ns, victim, config) / victim.type_scale;
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

/// The impulse the character's hit reaction compares against its thresholds, after its type
/// factor, for a blow of `impulse_ns`: a solid punch (the stagger impulse) just past the stagger
/// threshold, rising to just short of the knockdown threshold toward the knockdown impulse, and the
/// knockdown threshold and past it from there; below the stagger impulse, the same share of the
/// stagger threshold, a flinch. A character whose stagger threshold is not below its knockdown
/// threshold, or a stagger impulse not below the knockdown impulse, takes the knockdown's share
/// alone.
fn scaled_reaction(impulse_ns: f32, victim: VictimTuning, config: &HandHitsTuning) -> f32 {
    let knockdown = config.knockdown_impulse_ns;
    let stagger = config.stagger_impulse_ns;
    let linear = impulse_ns / knockdown * victim.knockdown_threshold;
    let calibrated = victim.stagger_threshold.is_finite()
        && victim.stagger_threshold > 0.0
        && victim.stagger_threshold < victim.knockdown_threshold
        && stagger.is_finite()
        && stagger > 0.0
        && stagger < knockdown;
    if !calibrated || impulse_ns >= knockdown {
        return linear;
    }
    if impulse_ns < stagger {
        return impulse_ns / stagger * victim.stagger_threshold * UNDER_STAGGER;
    }
    let t = (impulse_ns - stagger) / (knockdown - stagger);
    let from = victim.stagger_threshold * OVER_STAGGER;
    let to = victim.knockdown_threshold * UNDER_KNOCKDOWN;
    from + (to - from.min(to)) * t
}

/// How far past the stagger threshold a solid punch lands, as a factor of it.
const OVER_STAGGER: f32 = 1.1;
/// How far short of the stagger threshold a blow just under a solid punch stays, as a factor.
const UNDER_STAGGER: f32 = 0.9;
/// How far short of the knockdown threshold a blow just under a hard swing stays, as a factor.
const UNDER_KNOCKDOWN: f32 = 0.95;

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

/// The hand index [`HitGate::admit`] is given to gate a character's damage from either hand.
pub const ANY_HAND: usize = usize::MAX;

/// The most hits the gate remembers.
const MAX_RECENT: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::grab::PickupKind;

    const CONFIG: HandHitsTuning = HandHitsTuning::new();
    const VICTIM: VictimTuning = VictimTuning {
        type_scale: 2.0,
        stagger_threshold: 40.0,
        knockdown_threshold: 100.0,
    };

    fn plan(impulse: f32) -> BlowPlan {
        match blow(impulse, VICTIM, 100, &CONFIG) {
            Blow::Strikes(plan) => plan,
            other => panic!("{impulse}: {other:?}"),
        }
    }

    #[test]
    fn a_gun_swings_its_mass_with_the_arm() {
        let with_rifle = striking_with(&CONFIG, Some(4.0));
        assert_eq!(with_rifle.blow_mass_kg, CONFIG.blow_mass_kg + 4.0);
        assert_eq!(striking_with(&CONFIG, None), CONFIG);
        assert_eq!(striking_with(&CONFIG, Some(f32::NAN)), CONFIG);
        assert_eq!(striking_with(&CONFIG, Some(-1.0)), CONFIG);
        // The same swing strikes harder with the gun.
        let swing = Approach {
            hand_mps: 4.0,
            victim_mps: 0.0,
        };
        assert!(
            blow_impulse(swing, Intent::Swing, TimeScale::REAL, &with_rifle).unwrap()
                > blow_impulse(swing, Intent::Swing, TimeScale::REAL, &CONFIG).unwrap()
        );
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
        let off = HandHitsTuning {
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
    fn only_a_swing_into_a_character_is_a_blow() {
        let toward = Vec3::X;
        // A hand swung at a standing character.
        let swing = approach(Vec3::X * 5.0, Vec3::ZERO, toward);
        assert_eq!(swing.hand_mps, 5.0);
        assert_eq!(
            blow_impulse(swing, Intent::Swing, TimeScale::REAL, &CONFIG),
            Some(CONFIG.blow_mass_kg * 5.0)
        );
        // A hand at rest, which a character walks into.
        let walked_into = approach(Vec3::ZERO, Vec3::NEG_X * 3.0, toward);
        assert_eq!(walked_into.victim_mps, 3.0);
        assert_eq!(
            blow_impulse(walked_into, Intent::Swing, TimeScale::REAL, &CONFIG),
            None
        );
        // A hand moving away, or resting on the character.
        assert_eq!(
            blow_impulse(
                approach(Vec3::NEG_X, Vec3::ZERO, toward),
                Intent::Swing,
                TimeScale::REAL,
                &CONFIG
            ),
            None
        );
        let resting = approach(Vec3::X * 0.3, Vec3::ZERO, toward);
        assert_eq!(
            blow_impulse(resting, Intent::Swing, TimeScale::REAL, &CONFIG),
            None
        );
        // Only the hand's own speed counts: a character running into a swing adds nothing.
        let both = approach(Vec3::X * 5.0, Vec3::NEG_X * 4.0, toward);
        assert_eq!(
            blow_impulse(both, Intent::Swing, TimeScale::REAL, &CONFIG),
            blow_impulse(swing, Intent::Swing, TimeScale::REAL, &CONFIG)
        );
        assert_eq!(approach(Vec3::X, Vec3::ZERO, Vec3::ZERO).hand_mps, 0.0);
    }

    #[test]
    fn a_hand_carried_by_the_running_player_is_no_swing() {
        let toward = Vec3::X;
        // The player runs at the character at 6 m/s with the arm held out still: the hand body
        // moves at the player's speed, and swings at none of it.
        let running = Vec3::X * 6.0;
        let carried = approach(swing(running, running), Vec3::ZERO, toward);
        assert_eq!(
            blow_impulse(carried, Intent::Swing, TimeScale::REAL, &CONFIG),
            None
        );
        // A punch thrown while running swings at the arm's own speed.
        let punch = approach(swing(running + Vec3::X * 5.0, running), Vec3::ZERO, toward);
        assert_eq!(
            blow_impulse(punch, Intent::Swing, TimeScale::REAL, &CONFIG),
            Some(CONFIG.blow_mass_kg * 5.0)
        );
        // A player velocity that is not finite counts as none.
        assert_eq!(swing(Vec3::X, Vec3::NAN), Vec3::X);
    }

    #[test]
    fn a_hand_after_the_gun_strikes_only_with_a_deliberate_swing() {
        let toward = Vec3::X;
        let at = |mps: f32| approach(Vec3::X * mps, Vec3::ZERO, toward);
        let punch_mps = CONFIG.stagger_impulse_ns / CONFIG.blow_mass_kg;
        // A brisk reach strikes a character the hand is not after the gun of.
        assert!(blow_impulse(at(2.5), Intent::Swing, TimeScale::REAL, &CONFIG).is_some());
        // Reaching for the gun, it does not; a solid punch still does.
        assert_eq!(
            blow_impulse(at(2.5), Intent::Reach, TimeScale::REAL, &CONFIG),
            None
        );
        assert!(
            blow_impulse(at(punch_mps + 0.1), Intent::Reach, TimeScale::REAL, &CONFIG).is_some()
        );
        // A hand closing on the gun never strikes, however fast.
        assert_eq!(
            blow_impulse(at(20.0), Intent::Take, TimeScale::REAL, &CONFIG),
            None
        );
    }

    #[test]
    fn a_slowed_world_judges_the_swing_in_real_time_and_strikes_with_its_world_speed() {
        let slow = TimeScale::new(0.25);
        let toward = Vec3::X;
        // A hand body following a real 1 m/s reach moves 4 m/s through the world at a quarter:
        // a reach, not a swing.
        let reach = approach(Vec3::X * 4.0, Vec3::ZERO, toward);
        assert!(blow_impulse(reach, Intent::Swing, TimeScale::REAL, &CONFIG).is_some());
        assert_eq!(blow_impulse(reach, Intent::Swing, slow, &CONFIG), None);
        // A real 2 m/s swing strikes, with the world's 8 m/s.
        let swing = approach(Vec3::X * 8.0, Vec3::ZERO, toward);
        assert_eq!(
            blow_impulse(swing, Intent::Swing, slow, &CONFIG),
            Some(CONFIG.blow_mass_kg * 8.0)
        );
    }

    #[test]
    fn the_grip_and_the_gun_in_reach_say_what_the_hand_means() {
        let weapon = GripLatch::Grab(GrabVolume::WorldWeapon);
        let other = GripLatch::Grab(GrabVolume::Pickup(PickupKind::Character));
        let fist = GripLatch::Fist { closed_s: 0.5 };
        assert_eq!(Intent::of(GripLatch::Open, false), Intent::Swing);
        assert_eq!(Intent::of(fist, false), Intent::Swing);
        assert_eq!(Intent::of(other, false), Intent::Swing);
        assert_eq!(Intent::of(GripLatch::Open, true), Intent::Reach);
        // A fist closed on nothing and swung by the gun is a punch.
        assert_eq!(Intent::of(fist, true), Intent::Swing);
        // A closing grip by the gun takes it.
        assert_eq!(Intent::of(other, true), Intent::Take);
        assert_eq!(Intent::of(GripLatch::Default, true), Intent::Take);
        // A grip that took the gun takes, whatever is offered after.
        assert_eq!(Intent::of(weapon, false), Intent::Take);
        assert_eq!(Intent::of(weapon, true), Intent::Take);
    }

    #[test]
    fn a_solid_punch_staggers_and_a_hard_swing_knocks_down() {
        let reacts = |impulse: f32| plan(impulse).reaction_impulse * VICTIM.type_scale;
        // A solid punch, about three metres a second: past the stagger, short of a knockdown.
        let punch = reacts(CONFIG.stagger_impulse_ns);
        assert!(punch > VICTIM.stagger_threshold, "{punch}");
        assert!(punch < VICTIM.knockdown_threshold, "{punch}");
        // A blow a little short of it flinches.
        assert!(reacts(CONFIG.stagger_impulse_ns * 0.8) < VICTIM.stagger_threshold);
        // A hard swing knocks down, and harder still more.
        assert!(reacts(CONFIG.knockdown_impulse_ns) >= VICTIM.knockdown_threshold - 1e-3);
        assert!(reacts(CONFIG.knockdown_impulse_ns * 1.5) > VICTIM.knockdown_threshold);
        // Between the two the reaction rises, and never reaches the knockdown early.
        let mut last = punch;
        let steps = 10;
        for k in 1..steps {
            let impulse = CONFIG.stagger_impulse_ns
                + (CONFIG.knockdown_impulse_ns - CONFIG.stagger_impulse_ns) * k as f32
                    / steps as f32;
            let r = reacts(impulse);
            assert!(
                r >= last && r < VICTIM.knockdown_threshold,
                "{impulse}: {r}"
            );
            last = r;
        }
        // Damage scales with the blow.
        let damage = |impulse: f32| plan(impulse).damage.unwrap_or(0);
        assert!(damage(CONFIG.knockdown_impulse_ns * 1.5) > damage(CONFIG.stagger_impulse_ns));
    }

    #[test]
    fn a_character_with_tuning_out_of_order_takes_the_knockdowns_share() {
        let odd = VictimTuning {
            stagger_threshold: 150.0,
            ..VICTIM
        };
        let Blow::Strikes(p) = blow(CONFIG.stagger_impulse_ns, odd, 100, &CONFIG) else {
            panic!("a blow");
        };
        let share = CONFIG.stagger_impulse_ns / CONFIG.knockdown_impulse_ns;
        assert!(
            (p.reaction_impulse * odd.type_scale - share * odd.knockdown_threshold).abs() < 1e-3
        );
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
