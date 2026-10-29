//! The physical grapple's settings, held in
//! [`ControllersConfig`](crate::controllers::config::ControllersConfig) as `grapple`, and its fixed
//! tuning. Every part acts only under the controller scheme with a tracked grapple hand, so each
//! defaults on.

use serde::{Deserialize, Serialize};

/// The physical grapple's switches.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicalGrappleConfig {
    /// Scale the reel's speed and acceleration by the grapple hand's pull, and boost the reel on a
    /// yank during it; and retract the tethers while the grapple hand pulls back, on foot or
    /// falling, with tethers attached and no line of the grapple's own.
    pub pull_to_reel: bool,
    /// Pump a hanging character's swing by the grapple hand's motion, on top of the hold of
    /// [`AnchorConfig::enabled`](crate::controllers::hands::anchor::AnchorConfig::enabled).
    pub pump: bool,
}

impl PhysicalGrappleConfig {
    pub const fn new() -> Self {
        Self {
            pull_to_reel: true,
            pump: true,
        }
    }
}

impl Default for PhysicalGrappleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The physical grapple's tuning with the default switches.
pub const GRAPPLE: PhysicalGrappleTuning = PhysicalGrappleTuning::new();

/// The physical grapple's fixed tuning, with the switches it takes from [`PhysicalGrappleConfig`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PhysicalGrappleTuning {
    /// How the grapple hand's pull is measured, for the reel and the tether retract.
    pub pull: PullTuning,
    /// The reel driven by the pull.
    pub reel: ReelPullTuning,
    /// The tether retract driven by the pull.
    pub tether: TetherTuning,
    /// The hang swing pumped by the grapple hand's motion.
    pub swing: SwingTuning,
}

impl PhysicalGrappleTuning {
    const fn new() -> Self {
        Self {
            pull: PullTuning::new(),
            reel: ReelPullTuning::new(),
            tether: TetherTuning::new(),
            swing: SwingTuning::new(),
        }
    }

    /// The fixed tuning with the switches `config` sets.
    pub const fn of(config: &PhysicalGrappleConfig) -> Self {
        Self {
            reel: ReelPullTuning {
                enabled: config.pull_to_reel,
                ..GRAPPLE.reel
            },
            tether: TetherTuning {
                enabled: config.pull_to_reel,
                ..GRAPPLE.tether
            },
            swing: SwingTuning {
                pump: config.pump,
                ..GRAPPLE.swing
            },
            ..GRAPPLE
        }
    }
}

/// How the grapple hand's pull toward the body is measured
/// ([`crate::grapple::physical::pull`]). The hand's distance from the body point is the yank's
/// ([`GRAPPLE_YANK`](crate::controllers::bindings::yank::GRAPPLE_YANK)).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PullTuning {
    /// How long a pull's speed is remembered after the hand stops, seconds: the time constant of
    /// the peak-held speed's decay.
    pub speed_hold_s: f32,
    /// How fast the pull's reference point follows a hand held still, metres per second: a hand
    /// held back stops pulling once the reference catches up with it.
    pub relax_mps: f32,
}

impl PullTuning {
    const fn new() -> Self {
        Self {
            speed_hold_s: 0.8,
            relax_mps: 0.25,
        }
    }
}

/// The reel by pulling ([`crate::grapple::physical::reel`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ReelPullTuning {
    /// [`PhysicalGrappleConfig::pull_to_reel`].
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

impl ReelPullTuning {
    const fn new() -> Self {
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

/// The tether retract by pulling ([`crate::grapple::physical::tether`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TetherTuning {
    /// [`PhysicalGrappleConfig::pull_to_reel`].
    pub enabled: bool,
    /// The pull below which nothing retracts, metres.
    pub deadzone_m: f32,
    /// The pull that retracts at full strength, metres.
    pub full_m: f32,
}

impl TetherTuning {
    const fn new() -> Self {
        Self {
            enabled: true,
            deadzone_m: 0.08,
            full_m: 0.35,
        }
    }
}

/// The hang swing's pump ([`crate::grapple::physical::swing`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SwingTuning {
    /// [`PhysicalGrappleConfig::pump`].
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

impl SwingTuning {
    const fn new() -> Self {
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
