//! Hiding the game's own HUD reticles while the hands aim
//! ([`OverlayConfig::hide_game_reticle`](crate::controllers::config::OverlayConfig::hide_game_reticle)).
//!
//! The HUD movie draws a weapon reticle at the centre of the screen and a grapple reticle at the
//! grapple slot's projected aim point, pinned to the centre when the grapple aims at a direct
//! position (`docs/engine/gameplay/aim-pipeline.md`, "The reticles"). Under the controller scheme
//! the guns and the grapple follow the hands, so a reticle at the gaze marks a point nothing aims
//! at. Each reticle is hidden wherever its aim belongs to the hands rather than the gaze:
//!
//! - on foot and in the air, the weapon reticle whenever the guns aim by hand, armed or not, and
//!   the grapple reticle whenever the grapple aims by hand, so neither shows in the gaps between
//!   aim updates (drawing, holstering, a weapon switch, an empty hand);
//! - in the driver's seat, the weapon reticle while the vehicle's built-in weapons aim by hand;
//! - at a mounted gun, the weapon reticle while the gun's point comes from the hands;
//! - in a passenger seat, each reticle while the aim's latest results aimed it by hand.
//!
//! A gun the gaze still aims keeps the reticle: the driver's weapons with
//! [`VehiclesConfig::driver_weapons_by_hand`](crate::controllers::vehicles::config::VehiclesConfig::driver_weapons_by_hand)
//! off, and a mounted gun with its hand aim off. The world-space overlay's grapple highlight and
//! gun readout stand in for the hidden reticles.
//!
//! [`decide`] is the pure rule; [`hide_game_reticles`] reads the mode, the settings, and the aim's
//! latest results, and holds the reticle clips hidden through [`crate::hud::scaleform`].

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim,
        mode::{self, Mode},
        scheme, vehicles,
    },
};

/// Which of the game's reticles to hide.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HiddenReticles {
    /// The weapon reticle group (every weapon's crosshair shape).
    pub weapon: bool,
    /// The grapple reticle group (the aim cursor, the attached-wire marker, and its ghost).
    pub grapple: bool,
}

/// Where the player is, as far as who aims is concerned.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum AimPlace {
    /// On foot, swimming, falling, in the wingsuit, or under the parachute.
    #[default]
    OnFoot,
    /// In a vehicle's driver's seat.
    Driver,
    /// In a vehicle's other seats.
    Passenger,
    /// A menu is up.
    Menu,
}

impl AimPlace {
    /// The place for a detected mode.
    pub fn of(mode: Mode) -> Self {
        match mode {
            Mode::OnFoot(_) | Mode::Airborne(_) => Self::OnFoot,
            Mode::Vehicle(seat) if seat.driver => Self::Driver,
            Mode::Vehicle(_) => Self::Passenger,
            Mode::Ui => Self::Menu,
        }
    }
}

/// What the rule reads: the toggle, the scheme, the place, the hand-aim settings, and the aim's
/// latest results.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReticleInputs {
    /// The toggle, with the controllers' master switch.
    pub enabled: bool,
    /// Whether the controller scheme is in effect.
    pub controllers_scheme: bool,
    pub place: AimPlace,
    /// Whether the guns aim by hand ([`crate::controllers::config::AimConfig::gun`]).
    pub guns_by_hand: bool,
    /// Whether the grapple aims by hand ([`crate::controllers::config::AimConfig::grapple`]).
    pub grapple_by_hand: bool,
    /// Whether the driver's built-in weapons aim by hand.
    pub driver_weapons_by_hand: bool,
    /// Whether a manned mounted gun is aimed by hand now.
    pub mounted_by_hand: bool,
    /// Whether the aim's results are recent. Stale results belong to an aim update that no longer
    /// runs, and say nothing about what aims now.
    pub aim_fresh: bool,
    /// Whether the last aim update aimed the wielded weapon by hand.
    pub weapon_aimed: bool,
    /// Whether the last aim update aimed the grapple by hand.
    pub grapple_aimed: bool,
}

/// Which reticles to hide for `inputs`.
pub fn decide(inputs: ReticleInputs) -> HiddenReticles {
    if !inputs.enabled || !inputs.controllers_scheme {
        return HiddenReticles::default();
    }
    let weapon_aimed = inputs.aim_fresh && inputs.weapon_aimed;
    let grapple_aimed = inputs.aim_fresh && inputs.grapple_aimed;
    // The mounted gun's point has its own freshness, so it does not wait on the aim update.
    let mounted = inputs.mounted_by_hand;
    match inputs.place {
        AimPlace::OnFoot => HiddenReticles {
            weapon: inputs.guns_by_hand || weapon_aimed || mounted,
            grapple: inputs.grapple_by_hand || grapple_aimed,
        },
        AimPlace::Driver => HiddenReticles {
            weapon: inputs.driver_weapons_by_hand || weapon_aimed || mounted,
            grapple: grapple_aimed,
        },
        AimPlace::Passenger | AimPlace::Menu => HiddenReticles {
            weapon: weapon_aimed || mounted,
            grapple: grapple_aimed,
        },
    }
}

/// Hold the game's reticles hidden while the hands aim, and restore them otherwise.
///
/// # Safety
///
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour, as
/// [`crate::hud::scaleform::apply_reticle_suppression`] requires.
pub(crate) unsafe fn hide_game_reticles() {
    let (enabled, guns, grapple, driver) = Config::lock_query(|c| {
        let controllers = &c.controllers;
        (
            controllers.enabled && controllers.overlay.hide_game_reticle,
            controllers.aim.gun,
            controllers.aim.grapple,
            controllers.vehicles.driver_weapons_by_hand,
        )
    });
    let hidden = if enabled {
        let status = aim::status();
        decide(ReticleInputs {
            enabled,
            controllers_scheme: scheme::is_controllers(),
            place: AimPlace::of(mode::current().mode),
            guns_by_hand: guns,
            grapple_by_hand: grapple,
            driver_weapons_by_hand: driver,
            mounted_by_hand: vehicles::mounted_aim_point().is_some(),
            aim_fresh: aim_fresh(status.updates),
            weapon_aimed: status.weapon.is_some(),
            grapple_aimed: status.grapple.is_some(),
        })
    } else {
        HiddenReticles::default()
    };
    // SAFETY: forwarded from the caller.
    unsafe { crate::hud::scaleform::apply_reticle_suppression(hidden.weapon, hidden.grapple) };
}

/// How long the aim's results count as recent after its update count last moved.
const AIM_MAX_AGE: Duration = Duration::from_millis(250);

/// The aim's update count when it was last seen to change, and when.
static LAST_AIM_UPDATE: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// Whether the aim's update count `updates` has moved within [`AIM_MAX_AGE`].
fn aim_fresh(updates: u64) -> bool {
    let now = Instant::now();
    let mut last = LAST_AIM_UPDATE.lock();
    match *last {
        Some((seen, at)) if seen == updates => now.duration_since(at) < AIM_MAX_AGE,
        _ => {
            *last = Some((updates, now));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{AirborneVariant, OnFootVariant, VehicleClass, VehicleSeat};

    /// On foot under the controller scheme with every hand aim on, and nothing aimed.
    const ON_FOOT: ReticleInputs = ReticleInputs {
        enabled: true,
        controllers_scheme: true,
        place: AimPlace::OnFoot,
        guns_by_hand: true,
        grapple_by_hand: true,
        driver_weapons_by_hand: true,
        mounted_by_hand: false,
        aim_fresh: false,
        weapon_aimed: false,
        grapple_aimed: false,
    };

    const BOTH: HiddenReticles = HiddenReticles {
        weapon: true,
        grapple: true,
    };

    #[test]
    fn on_foot_the_hand_aim_hides_both_reticles_armed_or_not() {
        assert_eq!(decide(ON_FOOT), BOTH);
        let aiming = ReticleInputs {
            aim_fresh: true,
            weapon_aimed: true,
            grapple_aimed: true,
            ..ON_FOOT
        };
        assert_eq!(decide(aiming), BOTH);
    }

    #[test]
    fn on_foot_a_gaze_aimed_gun_or_grapple_keeps_its_reticle() {
        let gaze_guns = ReticleInputs {
            guns_by_hand: false,
            ..ON_FOOT
        };
        assert_eq!(
            decide(gaze_guns),
            HiddenReticles {
                weapon: false,
                grapple: true
            }
        );
        let gaze_grapple = ReticleInputs {
            grapple_by_hand: false,
            ..ON_FOOT
        };
        assert_eq!(
            decide(gaze_grapple),
            HiddenReticles {
                weapon: true,
                grapple: false
            }
        );
    }

    #[test]
    fn the_toggle_and_the_gamepad_scheme_keep_both_reticles() {
        let off = ReticleInputs {
            enabled: false,
            ..ON_FOOT
        };
        let gamepad = ReticleInputs {
            controllers_scheme: false,
            mounted_by_hand: true,
            ..ON_FOOT
        };
        assert_eq!(decide(off), HiddenReticles::default());
        assert_eq!(decide(gamepad), HiddenReticles::default());
    }

    #[test]
    fn the_driver_keeps_the_reticle_only_for_gaze_aimed_weapons() {
        let driver = ReticleInputs {
            place: AimPlace::Driver,
            ..ON_FOOT
        };
        assert_eq!(
            decide(driver),
            HiddenReticles {
                weapon: true,
                grapple: false
            }
        );
        let gaze = ReticleInputs {
            driver_weapons_by_hand: false,
            ..driver
        };
        assert_eq!(decide(gaze), HiddenReticles::default());
    }

    #[test]
    fn a_passenger_follows_the_aim_results_and_the_mounted_gun() {
        let passenger = ReticleInputs {
            place: AimPlace::Passenger,
            ..ON_FOOT
        };
        assert_eq!(decide(passenger), HiddenReticles::default());
        let mounted = ReticleInputs {
            mounted_by_hand: true,
            ..passenger
        };
        assert_eq!(
            decide(mounted),
            HiddenReticles {
                weapon: true,
                grapple: false
            }
        );
        let stale = ReticleInputs {
            weapon_aimed: true,
            grapple_aimed: true,
            ..passenger
        };
        assert_eq!(decide(stale), HiddenReticles::default());
        let fresh = ReticleInputs {
            aim_fresh: true,
            ..stale
        };
        assert_eq!(decide(fresh), BOTH);
    }

    #[test]
    fn the_modes_map_to_places() {
        assert_eq!(
            AimPlace::of(Mode::OnFoot(OnFootVariant::Swimming)),
            AimPlace::OnFoot
        );
        assert_eq!(
            AimPlace::of(Mode::Airborne(AirborneVariant::Wingsuit)),
            AimPlace::OnFoot
        );
        let seat = |driver| {
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver,
            })
        };
        assert_eq!(AimPlace::of(seat(true)), AimPlace::Driver);
        assert_eq!(AimPlace::of(seat(false)), AimPlace::Passenger);
        assert_eq!(AimPlace::of(Mode::Ui), AimPlace::Menu);
    }
}
