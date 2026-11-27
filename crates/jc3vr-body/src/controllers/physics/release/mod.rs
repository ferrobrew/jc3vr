//! The one release of everything a hand holds and lets go of: props, ragdolls, guns, grenades, and
//! charges leave the hand the same way for the same swing.
//!
//! The hand's share is the controller's timestamped history in the cockpit frame, read at its
//! fastest moment, at the released point carried rigidly with the grip, so the hand's turn adds its
//! lever; the spin is the hand's. The player's own movement is the rig's: the character's measured
//! velocity, with the rig's turn at the point, sampled on each input tick ([`sample`]) and added
//! once. Only the hand's share is capped, per real second, and in slow motion it becomes the same
//! motion per game second ([`ReleaseMotion::in_game_time`]); the carried share passes through, cut
//! only past any real speed.
//!
//! A snap turn, a recenter, and the game's own sudden turns are the rig's jumps
//! ([`crate::controllers::physics::rig`]): a re-base or a game's turn cuts the history, and a turn
//! of the mod's leaves the rig at rest for that tick, so a throw on it takes no spin from it.

mod estimate;
#[cfg(test)]
mod tests;

use std::time::Instant;

use glam::Vec3;
use jc3gi::character::character::Character;
use jc3vr_input::{
    frame::{self, Hand},
    motion,
};
use jc3vr_view::{headpose, headpose::teleport, vr::VR_CONFIG};
use parking_lot::Mutex;

pub use crate::controllers::physics::release::estimate::{
    ControllerContext, ReleaseFrame, ReleaseInput, ReleaseMotion, ReleaseSource, ReleaseTransport,
    RigMotion, estimate,
};
use crate::controllers::{
    clock::TimeScale,
    physics::{rig::RigSample, tuning::PHYSICS},
};

/// Sample the rig for the releases, once per input tick on the game thread before the world
/// update: the cockpit frame's place in the world, and the character's measured velocity as the
/// rig's linear motion.
pub fn sample() {
    let scale = VR_CONFIG.lock().world_scale;
    let frame = headpose::xr::cockpit_pose().map(|head_cockpit| {
        let head_world = jc3vr_view::hooks::camera::eye_point();
        ReleaseFrame {
            origin: head_world,
            rig: RigSample::now(),
            scale,
            teleport_epoch: teleport::epoch(),
        }
        .with_composed_head(head_world, head_cockpit.position)
    });
    let time_s = motion::latest_time();
    // The rig's origin moves only on sim ticks, which a frame may run none or two of, so its
    // difference over a frame read the platform's speed as zero or about double, so a charge
    // thrown forward off a moving jeep lands behind. The character's own
    // measured velocity is the tick's, and the platform carries the rig with it.
    // SAFETY: the local player, read on the game thread before the world update.
    let measured = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .map(|character| Vec3::from(character.m_MeasuredWorldVelocity))
        .filter(|velocity| velocity.is_finite());
    let now = Instant::now();
    let mut state = STATE.lock();
    let dt = state.last.map_or(0.0, |last| {
        now.saturating_duration_since(last).as_secs_f32()
    });
    state.last = Some(now);
    state.rig = state.transport.update(frame, dt, time_s).map(|mut rig| {
        if let Some(measured) = measured {
            rig.motion.linear = measured;
        }
        (now, rig)
    });
}

/// The motion of the point at `point_world` that `hand` lets go of, for an item the hand has held
/// since `since_s` (display time; earlier samples belong to whatever the hand did before), per game
/// second. Request it with [`ReleaseMotion::thrown`].
pub fn at(hand: Hand, point_world: Vec3, since_s: f64) -> ReleaseMotion {
    let controller = frame::with_latest(|frame| ControllerContext::from_frame(frame, hand));
    at_with(hand, point_world, since_s, controller.as_ref())
}

/// [`at`], checked against `controller`, the context a caller already copied for its record.
pub fn at_with(
    hand: Hand,
    point_world: Vec3,
    since_s: f64,
    controller: Option<&ControllerContext>,
) -> ReleaseMotion {
    let (rig, history_after_s) = {
        let state = STATE.lock();
        let rig = state
            .rig
            .filter(|(at, _)| at.elapsed().as_secs_f32() <= PHYSICS.get().release.max_age_s);
        (rig.map(|(_, rig)| rig), state.transport.history_after_s())
    };
    let samples = motion::history(hand);
    estimate(ReleaseInput {
        rig,
        controller,
        samples: &samples,
        since_s,
        history_after_s,
        point_world,
    })
    // SAFETY: a release happens while the game runs.
    .in_game_time(unsafe { TimeScale::current() })
}

/// The rig's sampling across input ticks.
struct State {
    transport: ReleaseTransport,
    /// The latest rig motion, and when it was sampled.
    rig: Option<(Instant, RigMotion)>,
    /// When the last input tick sampled.
    last: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    transport: ReleaseTransport::new(),
    rig: None,
    last: None,
});
