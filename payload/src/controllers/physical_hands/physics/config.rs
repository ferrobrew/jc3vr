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
    /// A hand body's mass, kilograms. A heavier hand shoves harder against what it meets, but a
    /// light prop between two heavy bodies jitters.
    pub mass_kg: f32,
    /// The strongest pull of one hand at a strength of one, newtons.
    pub max_force_n: f32,
    /// The strongest twist of one hand at a strength of one, newton metres.
    pub max_torque_nm: f32,
    /// The share of a hand's position error the drive closes in one step, `0..=1`.
    pub linear_gain: f32,
    /// The share of a hand's rotation error the drive closes in one step, `0..=1`.
    pub angular_gain: f32,
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
}

impl HandBodiesConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            strength: 1.0,
            mass_kg: 10.0,
            max_force_n: 5000.0,
            max_torque_nm: 400.0,
            linear_gain: 1.0,
            angular_gain: 0.8,
            max_speed_mps: 20.0,
            max_angular_speed_rps: 30.0,
            radius_m: 0.04,
            half_length_m: 0.035,
            break_distance_m: 0.6,
            render_at_body: true,
            push: true,
            body_mass_kg: 90.0,
            push_deadband_m: 0.02,
            max_push_speed_mps: 2.5,
            min_push_horizontal: 0.3,
            push_smoothing_s: 0.1,
            pressure_haptics: true,
            pressure_full_lag_m: 0.1,
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
