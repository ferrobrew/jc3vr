//! The reel by pulling: while the character reels in along the grapple line, the grapple hand's
//! pull scales the reel's speed and acceleration, and a yank boosts it the way the game's own
//! retract boost does. The engine side is in `docs/engine/gameplay/grapple-pipeline.md`.
//!
//! The reel's velocity is advanced once per tick by
//! [`NStateTask_ReelIn::UpdateReelVelocity`], which reads its speeds and accelerations from the
//! [`GrappleTuning`] it is handed rather than from the hook. A detour hands it a copy of the tuning
//! with the reel fields scaled ([`ReelTuning::driven`]) for the local player, and nothing for
//! anyone else, so the shared tuning resource is never written. The copy covers the bound reel
//! fields, which are all the update reads of the tuning. A boost puts the tuning's own retract
//! speed and acceleration in place of the plain ones, as holding the retract does when the game's
//! boost upgrade is purchased; it does not touch the retract input, so the tethers are not
//! squeezed by it.
//!
//! The drive is published by the input tick ([`publish`]) and read by the detour on the same
//! thread's character update; a drive older than [`DRIVE_MAX_AGE`] is ignored, so the reel returns
//! to the game's own as soon as the input tick stops publishing.
//!
//! [`NStateTask_ReelIn::UpdateReelVelocity`]: jc3gi::equipment::grappling_hook::NStateTask_ReelIn::UpdateReelVelocity

use std::time::{Duration, Instant};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    character::character::Character,
    equipment::grappling_hook::{GrappleTuning, GrapplingHook},
    types::{math::Vector3, shared_ptr::SharedPtr},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::grapple::physical::config::ReelConfig;

/// The reel's hooks, aggregated by [`crate::grapple::physical::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_REEL_VELOCITY_BINDER)
}

/// What the hand asks of the reel on this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReelDrive {
    /// The scale of the reel's speeds and accelerations.
    pub scale: f32,
    /// Whether a yank's boost is running.
    pub boost: bool,
    /// Whether the boost needs the game's own retract boost upgrade.
    pub boost_needs_upgrade: bool,
}

/// What the reel did on its last tick, for the readout and the haptics.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ReelReadout {
    /// The scale applied, or `None` when the game's own tuning ran.
    pub scale: Option<f32>,
    /// Whether the boost applied.
    pub boosted: bool,
    /// The reel's speed along the line over its top speed, `0..=1`-ish.
    pub speed_fraction: f32,
}

/// The reel fields of the tuning the hand drives.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReelTuning {
    pub speed_max: f32,
    pub retract_speed_max: f32,
    pub accel: f32,
    pub retract_accel: f32,
}

impl ReelTuning {
    /// The tuning's reel fields.
    pub fn of(tuning: &GrappleTuning) -> Self {
        Self {
            speed_max: tuning.m_ReelInSpeedMax,
            retract_speed_max: tuning.m_RetractReelInSpeedMax,
            accel: tuning.m_ReelInAccel,
            retract_accel: tuning.m_RetractReelInAccel,
        }
    }

    /// The fields scaled by `scale`, with the retract speed and acceleration in place of the plain
    /// ones under a `boost`. A scale that is not finite or not positive leaves the fields alone.
    pub fn driven(self, scale: f32, boost: bool) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let (speed_max, accel) = if boost {
            (
                self.speed_max.max(self.retract_speed_max),
                self.accel.max(self.retract_accel),
            )
        } else {
            (self.speed_max, self.accel)
        };
        Self {
            speed_max: speed_max * scale,
            retract_speed_max: self.retract_speed_max * scale,
            accel: accel * scale,
            retract_accel: self.retract_accel.max(accel) * scale,
        }
    }

    /// Write the fields into `tuning`.
    pub fn apply(self, tuning: &mut GrappleTuning) {
        tuning.m_ReelInSpeedMax = self.speed_max;
        tuning.m_RetractReelInSpeedMax = self.retract_speed_max;
        tuning.m_ReelInAccel = self.accel;
        tuning.m_RetractReelInAccel = self.retract_accel;
    }
}

/// The reel's scale for a pull of `speed_mps`: [`ReelConfig::min_scale`] with no pull, rising
/// linearly to [`ReelConfig::max_scale`] at [`ReelConfig::full_speed_mps`].
pub fn scale_for(speed_mps: f32, config: &ReelConfig) -> f32 {
    let speed = if speed_mps.is_finite() {
        speed_mps.max(0.0)
    } else {
        0.0
    };
    let t = (speed / config.full_speed_mps.max(1e-3)).clamp(0.0, 1.0);
    config.min_scale + (config.max_scale - config.min_scale) * t
}

/// Publish the hand's drive for the reel's next ticks, or `None` to leave the reel to the game.
pub(super) fn publish(drive: Option<ReelDrive>) {
    let mut shared = SHARED.lock();
    shared.drive = drive.map(|d| (d, Instant::now()));
    if drive.is_none() {
        shared.readout = ReelReadout::default();
    }
}

/// The reel's last tick.
pub(super) fn readout() -> ReelReadout {
    SHARED.lock().readout
}

/// How long a published drive stays current.
const DRIVE_MAX_AGE: Duration = Duration::from_millis(250);

struct Shared {
    drive: Option<(ReelDrive, Instant)>,
    readout: ReelReadout,
}

/// A leaf lock: nothing else is taken while it is held.
static SHARED: Mutex<Shared> = Mutex::new(Shared {
    drive: None,
    readout: ReelReadout {
        scale: None,
        boosted: false,
        speed_fraction: 0.0,
    },
});

fn current_drive() -> Option<ReelDrive> {
    SHARED
        .lock()
        .drive
        .filter(|(_, at)| at.elapsed() < DRIVE_MAX_AGE)
        .map(|(drive, _)| drive)
}

// The signature mirrors the engine's, so its arguments cannot be bundled.
#[allow(clippy::too_many_arguments)]
#[detour(address = jc3gi::equipment::grappling_hook::NStateTask_ReelIn::UpdateReelVelocity_ADDRESS)]
fn update_reel_velocity(
    character: *mut Character,
    hook: *const SharedPtr<GrapplingHook>,
    velocity: *mut Vector3,
    instance_properties: *mut std::ffi::c_void,
    current_position: *const Vector3,
    wanted_reel_direction: *const Vector3,
    distance_to_target: f32,
    tuning: *const GrappleTuning,
    dt: f32,
    is_simulating: bool,
) {
    let call = |tuning: *const GrappleTuning| {
        UPDATE_REEL_VELOCITY.get().unwrap().call(
            character,
            hook,
            velocity,
            instance_properties,
            current_position,
            wanted_reel_direction,
            distance_to_target,
            tuning,
            dt,
            is_simulating,
        )
    };
    // SAFETY: the reel task's own arguments, on the character update that owns them.
    let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
    let (Some(drive), Some(base)) = (local.then(current_drive).flatten(), unsafe {
        tuning.as_ref()
    }) else {
        return call(tuning);
    };
    let boost = drive.boost && (!drive.boost_needs_upgrade || unsafe { boost_unlocked(hook) });
    let mut driven = *base;
    let reel = ReelTuning::of(base).driven(drive.scale, boost);
    reel.apply(&mut driven);
    call(&driven);
    if is_simulating {
        return;
    }
    // SAFETY: as above; the update has written the velocity.
    let along = unsafe { speed_along(velocity, wanted_reel_direction) };
    SHARED.lock().readout = ReelReadout {
        scale: Some(drive.scale),
        boosted: boost,
        speed_fraction: along.map_or(0.0, |s| s / reel.speed_max.max(1e-3)),
    };
}

/// Whether the game's own retract boost would apply to the reel: its upgrade is purchased.
///
/// # Safety
///
/// `hook` must be null or the reel task's live hook pointer.
unsafe fn boost_unlocked(hook: *const SharedPtr<GrapplingHook>) -> bool {
    unsafe { hook.as_ref() }
        .and_then(|h| unsafe { h.px.as_ref() })
        .is_some_and(|h| unsafe { h.GetRetractAcceleratesReelIn() })
}

/// The speed of `velocity` along `direction`, metres per second.
///
/// # Safety
///
/// Both pointers must be null or live.
unsafe fn speed_along(velocity: *const Vector3, direction: *const Vector3) -> Option<f32> {
    let velocity = Vec3::from_array(unsafe { velocity.as_ref() }?.data);
    let direction = Vec3::from_array(unsafe { direction.as_ref() }?.data).try_normalize()?;
    let along = velocity.dot(direction);
    along.is_finite().then_some(along.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: ReelTuning = ReelTuning {
        speed_max: 30.0,
        retract_speed_max: 45.0,
        accel: 60.0,
        retract_accel: 90.0,
    };

    #[test]
    fn the_scale_rises_with_the_pull_and_saturates() {
        let config = ReelConfig::new();
        assert_eq!(scale_for(0.0, &config), config.min_scale);
        assert_eq!(scale_for(-3.0, &config), config.min_scale);
        assert_eq!(scale_for(f32::NAN, &config), config.min_scale);
        let half = scale_for(config.full_speed_mps / 2.0, &config);
        assert!((half - (config.min_scale + config.max_scale) / 2.0).abs() < 1e-5);
        assert_eq!(scale_for(100.0, &config), config.max_scale);
    }

    #[test]
    fn the_drive_scales_the_reel_fields() {
        let driven = BASE.driven(1.5, false);
        assert_eq!(driven.speed_max, 45.0);
        assert_eq!(driven.accel, 90.0);
        assert_eq!(driven.retract_speed_max, 67.5);
        assert_eq!(driven.retract_accel, 135.0);
    }

    #[test]
    fn a_boost_takes_the_retract_speed_and_acceleration() {
        let boosted = BASE.driven(1.0, true);
        assert_eq!(boosted.speed_max, 45.0);
        assert_eq!(boosted.accel, 90.0);
        // A tuning whose retract values are below the plain ones is never slowed by a boost.
        let odd = ReelTuning {
            retract_speed_max: 10.0,
            retract_accel: 20.0,
            ..BASE
        };
        let boosted = odd.driven(1.0, true);
        assert_eq!(boosted.speed_max, 30.0);
        assert_eq!(boosted.accel, 60.0);
        assert!(boosted.retract_accel >= boosted.accel);
    }

    #[test]
    fn a_bad_scale_leaves_the_tuning_alone() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let driven = BASE.driven(scale, false);
            assert_eq!(driven.speed_max, BASE.speed_max, "{scale}");
            assert_eq!(driven.accel, BASE.accel, "{scale}");
        }
    }
}
