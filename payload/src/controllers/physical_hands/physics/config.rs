//! The hand bodies' settings.

use serde::{Deserialize, Serialize};

/// The hand bodies (see [`crate::controllers::physical_hands::physics`]): each empty hand is a body
/// in the game's physics world, driven toward the controller within the hand's strength.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandBodiesConfig {
    /// Give each empty hand a body in the physics world, in place of the palm's ray probes.
    pub enabled: bool,
    /// The hands' strength, a scale on their force and twist: one is the default "Rico strength".
    pub strength: f32,
    /// A hand body's mass, kilograms. The hand's strength is its force and twist caps, not its mass:
    /// a light hand meets a light prop without throwing it, and still shoves a heavy one within its
    /// strength over a few steps, while a heavy hand makes a prop between it and the world jitter.
    pub mass_kg: f32,
    /// The strongest pull of one hand at a strength of one, newtons.
    pub max_force_n: f32,
    /// The strongest twist of one hand at a strength of one, newton metres.
    pub max_torque_nm: f32,
    /// The share of a hand's position error the drive closes in one step, `0..=1`.
    pub linear_gain: f32,
    /// The share of a hand's rotation error the drive closes in one step, `0..=1`.
    pub angular_gain: f32,
    /// The share of the drive's gain a hand keeps while it presses as hard as it can into what holds
    /// it back, `0..=1`; a lighter press keeps more. It rests on a wall rather than pounding it.
    pub contact_gain: f32,
    /// The fastest a hand body is driven, metres per second, so that a tracking glitch cannot fling
    /// what it touches.
    pub max_speed_mps: f32,
    /// The fastest a hand body is turned, radians per second.
    pub max_angular_speed_rps: f32,
    /// The radius of the capsule a hand body is, metres of the player's hand.
    pub radius_m: f32,
    /// Half the length of the capsule's segment along the fingers, metres of the player's hand.
    pub half_length_m: f32,
    /// How far a hand body may fall behind its controller before it is put back on it, metres.
    pub break_distance_m: f32,
    /// Draw each hand where its body is, so that a hand stops at what stops its body.
    pub render_at_body: bool,
    /// Push the player away from what a hand presses into and cannot move.
    pub push: bool,
    /// The mass the hands' reaction moves, kilograms: the lighter, the further a push carries the
    /// player.
    pub body_mass_kg: f32,
    /// The stiffness of the spring between a held-back hand and its controller that pushes the
    /// player, newtons per metre of lag past the deadband.
    pub push_stiffness_n_per_m: f32,
    /// How far a hand body may lag behind its controller before the lag pushes the player, metres,
    /// so that resting a hand on a wall does not drift the body.
    pub push_deadband_m: f32,
    /// The fastest push of both hands together, metres per second.
    pub max_push_speed_mps: f32,
    /// The least horizontal share of a hand's lag for it to push the player, `0..=1`: pressing down
    /// on a table or the floor does not push.
    pub min_push_horizontal: f32,
    /// How long the push is smoothed over, seconds.
    pub push_smoothing_s: f32,
    /// Pulse a hand while it presses into something that holds it back, harder the further it is
    /// held back.
    pub pressure_haptics: bool,
    /// How far past the push deadband a hand is held back for the strongest pressure pulse, metres.
    pub pressure_full_lag_m: f32,
    /// What a hand body's contacts do: the pulse of a hit, and a character's reaction to it.
    pub hits: HandHitsConfig,
    /// Hold what a hand's grip takes by joining the hand's body to it with a fixed constraint, so
    /// that a held object has mass and the hand's strength carries it, in place of driving the
    /// object toward the hand. Off by default: the world does not end a constraint whose body is
    /// destroyed, so an object broken apart inside a step leaves the joint on a dead body until
    /// the next pre-simulation step ends it (`docs/mod/input/physical-hands.md`).
    pub joint_grabs: bool,
}

/// A hand body's hits (see [`crate::controllers::physical_hands::physics::hit`]): each contact a
/// hand body starts pulses the hand by its impulse, and a character it strikes reacts as to a
/// blow of that impulse, against its own stagger and knockdown thresholds; a hard enough blow deals
/// damage.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HandHitsConfig {
    /// Listen for the hand bodies' contacts at all: the hit pulses and the characters' reactions.
    pub enabled: bool,
    /// A character struck by a hand reacts to the blow: staggers, or is knocked down.
    pub reactions: bool,
    /// A blow at or above [`damage_impulse_ns`](Self::damage_impulse_ns) deals damage.
    pub damage: bool,
    /// The weakest contact impulse a character reacts to, newton seconds.
    pub min_impulse_ns: f32,
    /// The contact impulse that knocks a character down, newton seconds: a blow of this impulse
    /// reaches the character's own knockdown threshold, and a weaker one the same share of it.
    pub knockdown_impulse_ns: f32,
    /// The weakest contact impulse that deals damage, newton seconds.
    pub damage_impulse_ns: f32,
    /// The damage of a blow at the knockdown impulse, as a share of the player's melee damage; a
    /// weaker or stronger blow deals in proportion.
    pub damage_scale: f32,
    /// How long a character struck by a hand ignores that hand's further contacts, seconds, so that
    /// one blow is one hit.
    pub cooldown_s: f32,
    /// How long a character a blow damaged takes no further damage from either hand, seconds.
    pub damage_cooldown_s: f32,
    /// The slowest a hand must move toward a character before the contact for it to be a blow,
    /// metres per second: a hand resting on a character or pushing it slowly strikes nothing.
    pub min_approach_speed_mps: f32,
    /// The mass of the arm a blow carries, kilograms: a blow's impulse is this at the hand's speed
    /// into the character.
    pub blow_mass_kg: f32,
    /// How long a hand's drive is softened after a blow of at least half a knockdown, seconds, so
    /// that the hand follows through rather than stopping dead on the character.
    pub follow_through_s: f32,
    /// The share of the drive's gain a hand keeps while it follows through, `0..=1`.
    pub follow_through_gain: f32,
    /// Pulse a hand when its body starts a contact, harder the faster the hand met it.
    pub haptics: bool,
    /// The impulse of the strongest hit pulse, newton seconds: an arm of
    /// [`blow_mass_kg`](Self::blow_mass_kg) at the hand's speed into what it met.
    pub full_pulse_impulse_ns: f32,
}

impl HandHitsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            reactions: true,
            damage: true,
            min_impulse_ns: 8.0,
            knockdown_impulse_ns: 70.0,
            damage_impulse_ns: 35.0,
            damage_scale: 1.0,
            cooldown_s: 0.5,
            damage_cooldown_s: 1.0,
            min_approach_speed_mps: 1.5,
            blow_mass_kg: 10.0,
            follow_through_s: 0.15,
            follow_through_gain: 0.3,
            haptics: true,
            full_pulse_impulse_ns: 50.0,
        }
    }
}

impl Default for HandHitsConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl HandBodiesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            strength: 1.0,
            mass_kg: 3.0,
            max_force_n: 5000.0,
            max_torque_nm: 400.0,
            linear_gain: 1.0,
            angular_gain: 0.8,
            contact_gain: 0.4,
            max_speed_mps: 20.0,
            max_angular_speed_rps: 30.0,
            radius_m: 0.04,
            half_length_m: 0.035,
            break_distance_m: 0.6,
            render_at_body: true,
            push: true,
            body_mass_kg: 90.0,
            push_stiffness_n_per_m: 9000.0,
            push_deadband_m: 0.02,
            max_push_speed_mps: 2.5,
            min_push_horizontal: 0.3,
            push_smoothing_s: 0.1,
            pressure_haptics: true,
            pressure_full_lag_m: 0.1,
            hits: HandHitsConfig::new(),
            joint_grabs: true,
        }
    }

    /// The strongest pull of one hand at the configured strength, newtons.
    pub fn force_n(&self) -> f32 {
        self.max_force_n * self.strength
    }

    /// The strongest twist of one hand at the configured strength, newton metres.
    pub fn torque_nm(&self) -> f32 {
        self.max_torque_nm * self.strength
    }
}

impl Default for HandBodiesConfig {
    fn default() -> Self {
        Self::new()
    }
}
