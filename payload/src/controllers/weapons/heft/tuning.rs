//! The weight's fixed tuning, by weapon class, and the one feature switch each weight has from the
//! configuration ([`HeftConfig`]).

use crate::controllers::weapons::config::HeftConfig;

/// The weight's tuning with both weights switched on.
pub const HEFT: Heft = Heft {
    enabled: true,
    reach_near_m: 0.3,
    reach_far_m: 0.65,
    sway_hz: 0.35,
    max_angle_deg: 35.0,
    sidearm: HeftTuning::NONE,
    rifle: HeftTuning {
        sag_deg: 5.0,
        reach_sag_deg: 7.0,
        lag_stiffness: 260.0,
        lag_damping_ratio: 0.75,
        sway_deg: 0.6,
        kick_scale: 1.3,
        spread_scale: 1.6,
    },
    heavy: HeftTuning {
        sag_deg: 14.0,
        reach_sag_deg: 14.0,
        lag_stiffness: 70.0,
        lag_damping_ratio: 0.65,
        sway_deg: 1.5,
        kick_scale: 1.8,
        spread_scale: 2.5,
    },
    braced: BracedHeft {
        enabled: true,
        strength: 1.0,
        max_angle_deg: 8.0,
        smg: BracedTuning {
            sag_deg: 0.5,
            stiffness: 5000.0,
        },
        rifle: BracedTuning {
            sag_deg: 1.5,
            stiffness: 1600.0,
        },
        heavy: BracedTuning {
            sag_deg: 3.0,
            stiffness: 500.0,
        },
    },
};

/// The weight of a long weapon: held in one hand, a long gun sags, trails the hand on a spring,
/// and sways, its kick grows, and its spread widens, all scaled by the weapon's class; braced on
/// the foregrip, it keeps a lighter weight of its own ([`BracedHeft`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Heft {
    /// The one-handed weight.
    pub enabled: bool,
    /// The hand's distance from the head at which the reach sag starts, metres.
    pub reach_near_m: f32,
    /// The hand's distance from the head at which the reach sag is full, metres.
    pub reach_far_m: f32,
    /// The sway's frequency, hertz; the two axes run at incommensurate multiples of it.
    pub sway_hz: f32,
    /// The furthest the weapon may trail or sag away from the controller, degrees.
    pub max_angle_deg: f32,
    /// Pistols, revolvers, and SMGs.
    pub sidearm: HeftTuning,
    /// Assault rifles, shotguns, and sniper rifles.
    pub rifle: HeftTuning,
    /// Rocket and grenade launchers, miniguns, and the other heavy and special weapons.
    pub heavy: HeftTuning,
    /// The weight of a gun braced in both hands.
    pub braced: BracedHeft,
}

impl Heft {
    /// The fixed tuning with the switches `config` sets.
    pub const fn of(config: &HeftConfig) -> Self {
        Self {
            enabled: config.enabled,
            braced: BracedHeft {
                enabled: config.braced.enabled,
                ..HEFT.braced
            },
            ..HEFT
        }
    }
}

/// The weight of a long gun braced in both hands, on the foregrip: the gun follows where the hands
/// turn it on a critically damped spring, so it lags a quick swing a little and settles on the aim
/// without overshooting, and it sags a little. The hands are carried with the gun, so they stay on
/// it. Pistols and revolvers carry none.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BracedHeft {
    pub enabled: bool,
    /// How heavy a braced gun feels: one is the classes' tuning, zero carries no weight, and two
    /// lags twice as long and sags twice as far.
    pub strength: f32,
    /// The furthest a braced gun may lag or sag from where the hands turn it, degrees, so that the
    /// hands, carried with it, never stray far from the controllers.
    pub max_angle_deg: f32,
    /// SMGs, the lightest.
    pub smg: BracedTuning,
    /// Assault rifles, shotguns, and sniper rifles.
    pub rifle: BracedTuning,
    /// Launchers, miniguns, and the other heavy and special weapons, the heaviest.
    pub heavy: BracedTuning,
}

/// One weapon class's braced weight.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BracedTuning {
    /// How far the barrel pitches down, degrees.
    pub sag_deg: f32,
    /// The stiffness of the critically damped spring the gun follows the hands on, per second
    /// squared. The higher, the less it lags: a swing at a steady speed trails by about twice the
    /// speed over the stiffness's square root. Zero follows rigidly.
    pub stiffness: f32,
}

impl BracedTuning {
    /// No weight at all: the braced gun follows the hands as they are.
    pub const NONE: Self = Self {
        sag_deg: 0.0,
        stiffness: 0.0,
    };

    /// This tuning at `strength`: the sag scaled by it, and the stiffness by its inverse square,
    /// so that the lag scales with it. Zero or less carries no weight.
    pub fn at_strength(self, strength: f32) -> Self {
        if !(strength.is_finite() && strength > 0.0) {
            return Self::NONE;
        }
        Self {
            sag_deg: self.sag_deg * strength,
            stiffness: self.stiffness / (strength * strength),
        }
    }
}

impl Default for BracedTuning {
    fn default() -> Self {
        Self::NONE
    }
}

/// One weapon class's one-handed weight.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HeftTuning {
    /// How far the weapon pitches down with the hand close to the body, degrees.
    pub sag_deg: f32,
    /// How much further it pitches down with the arm extended, degrees.
    pub reach_sag_deg: f32,
    /// The stiffness of the spring the weapon follows the hand's rotation on, per second squared.
    /// Zero follows the hand rigidly.
    pub lag_stiffness: f32,
    /// The spring's damping ratio: `1` is critically damped, below `1` overshoots.
    pub lag_damping_ratio: f32,
    /// The sway's amplitude, degrees.
    pub sway_deg: f32,
    /// Multiplies the recoil kick while the weapon is held in one hand.
    pub kick_scale: f32,
    /// Multiplies the game's shot scatter while the weapon is held in one hand.
    pub spread_scale: f32,
}

impl HeftTuning {
    /// No weight at all: the weapon follows the hand as it is.
    pub const NONE: Self = Self {
        sag_deg: 0.0,
        reach_sag_deg: 0.0,
        lag_stiffness: 0.0,
        lag_damping_ratio: 1.0,
        sway_deg: 0.0,
        kick_scale: 1.0,
        spread_scale: 1.0,
    };
}

impl Default for HeftTuning {
    fn default() -> Self {
        Self::NONE
    }
}
