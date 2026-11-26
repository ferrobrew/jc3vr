//! The procedural body's tuning (`docs/mod/body/procedural-body.md`).

/// The procedural body's tuning: the hand-over's blends, the torso's limits and clearances, the
/// arms' keep-out, the elbow hints, the clavicles, and the hit reaction's hold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ProceduralTuning {
    /// How long a region of the body takes to hand over from the animation to the procedural pose,
    /// seconds: the smoothing time of its spring.
    pub blend_s: f32,
    /// How long a region takes to hand back to the animation, seconds; shorter, since the game's
    /// authored motion (a vehicle entry, a climb, a fall) is under way when it does.
    pub blend_out_s: f32,
    /// How far the chest turns toward the player's torso, at most, degrees.
    pub chest_turn_limit_deg: f32,
    /// How far the spine leans from upright, at most, degrees.
    pub lean_limit_deg: f32,
    /// A point of the chest higher than this below the camera's eyes is kept out of the near
    /// field, metres.
    pub chest_clear_below_m: f32,
    /// How far behind the eyes, along the view's level forward, the chest is kept, metres.
    pub chest_clear_back_m: f32,
    /// The chest capsule's radius, metres.
    pub chest_radius_m: f32,
    /// The abdomen capsule's radius, metres.
    pub abdomen_radius_m: f32,
    /// How far outside the torso a pushed wrist or elbow lands, metres: the hand's half-thickness.
    pub hand_margin_m: f32,
    /// The elbow hint's reach: how strongly the solve pulls the elbow toward the hint.
    pub elbow_hint_reach: f32,
    /// How far a clavicle lifts for a hand overhead, at most, degrees.
    pub clavicle_elevation_deg: f32,
    /// How far a clavicle drops for a hand reaching far down, at most, degrees.
    pub clavicle_depression_deg: f32,
    /// How far a clavicle swings forward for a hand reaching far ahead or across, at most,
    /// degrees.
    pub clavicle_protraction_deg: f32,
    /// The share of the arm's length at which a reach ahead or down starts to take the clavicle;
    /// it takes it fully at the arm's full length.
    pub clavicle_reach_start: f32,
    /// How quickly the clavicles follow the hands, seconds: the time constant of their follower.
    pub clavicle_follow_s: f32,
}

/// The procedural body's tuning.
pub const TUNING: ProceduralTuning = ProceduralTuning {
    blend_s: 0.35,
    blend_out_s: 0.2,
    chest_turn_limit_deg: 70.0,
    lean_limit_deg: 60.0,
    chest_clear_below_m: 0.25,
    chest_clear_back_m: 0.08,
    chest_radius_m: 0.14,
    abdomen_radius_m: 0.13,
    hand_margin_m: 0.03,
    elbow_hint_reach: 0.3,
    clavicle_elevation_deg: 30.0,
    clavicle_depression_deg: 10.0,
    clavicle_protraction_deg: 25.0,
    clavicle_reach_start: 0.8,
    clavicle_follow_s: 0.1,
};
