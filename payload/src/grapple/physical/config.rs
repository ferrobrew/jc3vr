//! The physical grapple's settings, held in
//! [`ControllersConfig`](crate::controllers::config::ControllersConfig) as `grapple`. Every part
//! acts only under the controller scheme with a tracked grapple hand, so each defaults on.

use serde::{Deserialize, Serialize};

use crate::controllers::weapons::config::PulseSpec;

/// All of the physical grapple's settings.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalGrappleConfig {
    /// How the grapple hand's pull is measured, for the reel and the tether retract.
    pub pull: PullConfig,
    /// The reel driven by the pull.
    pub reel: ReelConfig,
    /// The tether retract driven by the pull.
    pub tether: TetherConfig,
    /// The hang swing pumped by the grapple hand's motion.
    pub swing: SwingConfig,
    /// The grapple hand's continuous vibration and the tether retract's pulse.
    pub haptics: GrappleHapticsConfig,
}

impl PhysicalGrappleConfig {
    pub const fn new() -> Self {
        Self {
            pull: PullConfig::new(),
            reel: ReelConfig::new(),
            tether: TetherConfig::new(),
            swing: SwingConfig::new(),
            haptics: GrappleHapticsConfig::new(),
        }
    }
}

impl Default for PhysicalGrappleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How the grapple hand's pull toward the body is measured
/// ([`crate::grapple::physical::pull`]). The hand's distance from the body point is the yank's
/// ([`YankConfig::body_drop_m`](crate::controllers::bindings::yank::YankConfig::body_drop_m)).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PullConfig {
    /// How long a pull's speed is remembered after the hand stops, seconds: the time constant of
    /// the peak-held speed's decay.
    pub speed_hold_s: f32,
    /// How fast the pull's reference point follows a hand held still, metres per second: a hand
    /// held back stops pulling once the reference catches up with it.
    pub relax_mps: f32,
}

impl PullConfig {
    pub const fn new() -> Self {
        Self {
            speed_hold_s: 0.8,
            relax_mps: 0.25,
        }
    }
}

impl Default for PullConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The reel by pulling ([`crate::grapple::physical::reel`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReelConfig {
    /// Scale the reel's speed and acceleration by the grapple hand's pull, and boost the reel on
    /// a yank during it.
    pub enabled: bool,
    /// The reel's scale with no pull.
    pub min_scale: f32,
    /// The reel's scale at a pull of [`full_speed_mps`](Self::full_speed_mps) or faster.
    pub max_scale: f32,
    /// The pull speed at which the scale reaches [`max_scale`](Self::max_scale), metres per second.
    pub full_speed_mps: f32,
    /// How long a yank during the reel boosts it, seconds.
    pub boost_s: f32,
    /// Boost only when the game's own retract boost would apply: the retract reel-in boost upgrade
    /// is purchased.
    pub boost_needs_upgrade: bool,
}

impl ReelConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            min_scale: 1.0,
            max_scale: 1.5,
            full_speed_mps: 2.5,
            boost_s: 0.6,
            boost_needs_upgrade: true,
        }
    }
}

impl Default for ReelConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The tether retract by pulling ([`crate::grapple::physical::tether`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TetherConfig {
    /// Retract the tethers while the grapple hand pulls back, on foot or falling, with tethers
    /// attached and no line of the grapple's own.
    pub enabled: bool,
    /// The pull below which nothing retracts, metres.
    pub deadzone_m: f32,
    /// The pull that retracts at full strength, metres.
    pub full_m: f32,
}

impl TetherConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            deadzone_m: 0.08,
            full_m: 0.35,
        }
    }
}

impl Default for TetherConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The hang swing's pump ([`crate::grapple::physical::swing`]).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwingConfig {
    /// Pump a hanging character's swing by the grapple hand's motion, on top of the hold of
    /// [`AnchorConfig::hang_swing`](crate::controllers::hands::anchor::AnchorConfig::hang_swing).
    pub pump: bool,
    /// The hand speed below which the hand does not pump, metres per second.
    pub deadzone_mps: f32,
    /// The hand speed that pumps at full strength, metres per second.
    pub full_mps: f32,
    /// The full-strength pump per hang tick, as a multiple of the game's own continuous stick
    /// swing at full deflection. Negative pumps the body the way the hand moves.
    pub scale: f32,
    /// The most the hold and the pump together push per hang tick, as a multiple of the game's
    /// own continuous stick swing at full deflection.
    pub max_scale: f32,
    /// The time constant of the hand's smoothed velocity, seconds.
    pub smoothing_s: f32,
}

impl SwingConfig {
    pub const fn new() -> Self {
        Self {
            pump: true,
            deadzone_mps: 0.3,
            full_mps: 1.5,
            scale: 1.5,
            max_scale: 2.0,
            smoothing_s: 0.05,
        }
    }
}

impl Default for SwingConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The grapple hand's vibration ([`crate::grapple::physical::haptics`]), on top of the fire,
/// attach, and yank pulses of
/// [`HapticsConfig`](crate::controllers::weapons::config::HapticsConfig).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrappleHapticsConfig {
    /// Vibrate the grapple hand with the line's tension and the reel's speed, and pulse it when the
    /// tethers start to retract.
    pub enabled: bool,
    /// The amplitude at full tether tension.
    pub tension_amplitude: f32,
    /// The amplitude at the reel's top speed.
    pub reel_amplitude: f32,
    /// How long each continuous pulse plays before the next input tick refreshes it, seconds.
    pub pulse_s: f32,
    /// The pulse as the tethers start to retract.
    pub retract: PulseSpec,
}

impl GrappleHapticsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            tension_amplitude: 0.45,
            reel_amplitude: 0.25,
            pulse_s: 0.06,
            retract: PulseSpec {
                amplitude: 0.6,
                duration_s: 0.05,
            },
        }
    }
}

impl Default for GrappleHapticsConfig {
    fn default() -> Self {
        Self::new()
    }
}
