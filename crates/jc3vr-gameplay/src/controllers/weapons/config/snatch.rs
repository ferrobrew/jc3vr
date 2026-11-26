//! The grapple snatch's settings, held as `weapons.snatch` beside the weapon-handling settings.
//! Its fixed tuning is in `controllers::snatch::config`.

use jc3vr_body::controllers::intent::yank::YankConfig;
use serde::{Deserialize, Serialize};

/// The grapple snatch (see `controllers::snatch`). Acts only under the controller scheme.
/// Its tuning is fixed at `SNATCH`, apart from the
/// yank that fires it.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnatchConfig {
    /// Snatch a weapon out of reach into the empty grapple hand: grip at it under the grapple's
    /// aim, then yank the hand back. Also lets the grapple's aim see weapons lying in the world,
    /// which a tap of the grapple button then tosses to the player's feet, and a hold tethers. A
    /// gun holstered on a character, a loose object one hand can carry, and a vehicle's closed door
    /// are snatched too.
    pub enabled: bool,
    /// Swing a target on the held line: an object too heavy for a hand to carry is always swung
    /// rather than pulled in, and holding B after gripping a weapon or a light object swings it too.
    /// Off, heavy objects are not offered.
    pub sling: bool,
    /// The held line's tuning ([`SwingConfig`]).
    pub swing: SwingConfig,
    /// The yank that fires the snatch, measured along the grapple hand's aim at the grip edge
    /// ([`reach_along`](jc3vr_body::controllers::intent::yank::reach_along)).
    pub yank: YankConfig,
}

impl SnatchConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            sling: true,
            swing: SwingConfig::new(),
            yank: YankConfig {
                enabled: true,
                min_distance_m: 0.1,
                min_speed_mps: 0.7,
                window_s: 0.25,
                cooldown_s: 0.5,
            },
        }
    }
}

impl Default for SnatchConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The held line's tuning: the swing of a heavy object, and of a weapon or a light object while B
/// is held (`controllers::snatch::sling`).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwingConfig {
    /// The heaviest loose object the snatch offers, kilograms. One a hand can carry is pulled into
    /// the hand; one heavier, up to this, is swung on the line and never comes to the hand.
    pub ceiling_kg: f32,
    /// The line's stiffness: the pull per metre it is stretched past its held length, per second
    /// squared, so the same stretch asks the same acceleration of any mass, up to its strength.
    pub stiffness_per_s2: f32,
    /// The line's damping of the target's speed away from the hand, per second.
    pub damping_per_s: f32,
    /// The most tension the line holds on a target of
    /// `REFERENCE_MASS_KG`, newtons.
    pub strength_n: f32,
    /// How the line's strength grows with the target's mass, `0..=1`: the strength is
    /// [`strength_n`](Self::strength_n) times the mass over
    /// `REFERENCE_MASS_KG` to this power
    /// (`line_strength_n`). At 1 every mass
    /// swings alike; at 0 the line pulls every mass equally hard, so a heavy one barely moves.
    pub mass_scaling: f32,
    /// The line's strength while the other hand braces the grapple hand's wrist, as a multiple of
    /// one hand's.
    pub brace_factor: f32,
    /// The length the line reels in to once the hook bites, metres: a target bitten farther off
    /// is drawn in to it and then swung around the hand. A target bitten nearer keeps its length.
    pub swing_length_m: f32,
    /// How fast the line reels in to [`swing_length_m`](Self::swing_length_m), metres per second.
    /// The line cannot push, so a target reeled in keeps its speed once the reel stops and coasts
    /// on toward the hand until friction, gravity, or a swing takes it; zero keeps the length the
    /// hook bit at.
    pub reel_mps: f32,
}

impl SwingConfig {
    pub const fn new() -> Self {
        Self {
            ceiling_kg: 300.0,
            stiffness_per_s2: 400.0,
            damping_per_s: 30.0,
            strength_n: 1500.0,
            mass_scaling: 0.5,
            brace_factor: 1.75,
            swing_length_m: 3.0,
            reel_mps: 4.0,
        }
    }
}

impl Default for SwingConfig {
    fn default() -> Self {
        Self::new()
    }
}
