//! The post-simulation half of the hand climbing's tick: find the surface a reaching hand's palm
//! touches, and publish what the palms' tick found, pulsing the hands for the last step's grips
//! and releases.

use std::time::Instant;

use glam::Vec3;
use jc3vr_body::controllers::{
    haptics, mode,
    mode::{Mode, OnFootVariant},
};
use jc3vr_input::{
    frame::Hand,
    haptics::cues::{self, Sustain},
};
use jc3vr_view::vr::recenter_count;

use crate::controllers::physical_hands::{
    climb::{
        engine,
        solve::tension,
        state::{Event, STATE},
        tuning::CLIMB,
        types::{Hold, Observed},
    },
    contact::{Surface, SurfaceHit},
    palm::PalmBox,
    physics::drive::rig_jumped,
    pickup::REACH_DIRECTIONS,
};

/// The surface `palm` reaches: the nearest hit, on a body a hand climbs ([`engine::climbable`]), of
/// the rays a hand reaches for an object along ([`REACH_DIRECTIONS`]), at most `reach_m` past the
/// palm. Called from the palms' tick for a hand its body drives.
///
/// # Safety
///
/// The game thread, outside the physics step; `cast` must cast against the live world.
pub unsafe fn reach(
    palm: &PalmBox,
    reach_m: f32,
    cast: &mut impl FnMut(Vec3, Vec3) -> Option<SurfaceHit>,
) -> Option<Hold> {
    let min_moving_mass_kg = CLIMB.get().climb.min_moving_mass_kg;
    let origin = palm.point(Vec3::new(0.0, 1.0, 0.0));
    let length = reach_m.max(0.0) + 2.0 * palm.half_extents.y;
    REACH_DIRECTIONS
        .iter()
        .filter_map(|&local| {
            let direction = (palm.orientation * local).normalize_or_zero();
            let hit = cast(origin, origin + direction * length)?;
            let Surface::Solid { body, .. } = hit.surface else {
                return None;
            };
            // SAFETY: forwarded from the caller; the hit was found this tick.
            let frame = unsafe { engine::body_frame(body) }?;
            engine::climbable(&frame, min_moving_mass_kg).then_some(Hold {
                point: hit.position,
                normal: hit.normal,
                body,
                moving: !frame.is_static,
                identity: unsafe { engine::body_identity(body) }?,
            })
        })
        .min_by(|a, b| {
            a.point
                .distance_squared(palm.centre)
                .total_cmp(&b.point.distance_squared(palm.centre))
        })
}

/// Publish what the palms' tick found, with where the player's body is, and pulse the hands for
/// the last step's grips and releases. Called from the palms' tick, on the game thread outside the
/// physics step.
pub fn observe(observed: Observed) {
    let Observed {
        palms,
        reaches,
        pressing,
        body_rotation,
    } = observed;
    // SAFETY: the game thread outside the physics step (the palms' tick).
    let player = unsafe { engine::player_place() }.ok();
    // SAFETY: as above.
    let in_air = unsafe { engine::in_air() };
    let mode = mode::current().mode;
    let tuning = &CLIMB.get().climb;
    let events = {
        let mut state = STATE.lock();
        // A recenter or a snap turn moves every palm in the rig at once without the hands moving;
        // the grips are taken afresh from where the body is then, rather than pulling it.
        let recenters = recenter_count();
        if rig_jumped(state.rig, body_rotation, recenters) {
            state.rig_epoch = state.rig_epoch.wrapping_add(1);
        }
        state.rig = Some((body_rotation, recenters));
        state.palms = palms;
        state.pressing = pressing;
        state.published = player.is_some().then(Instant::now);
        state.publishes = state.publishes.wrapping_add(1);
        for hand in Hand::BOTH {
            state.status.hands[hand.index()].reach = reaches[hand.index()];
        }
        state.status.in_air = in_air;
        let climbing = state.grips.iter().any(Option::is_some);
        if climbing
            && !state.held_by_game
            && mode != Mode::OnFoot(OnFootVariant::Ground)
            && !state.warned_mode
        {
            state.warned_mode = true;
            tracing::warn!(
                ?mode,
                in_air = ?in_air,
                "hand climbing: the game left on foot on the ground while a hand gripped the world"
            );
        }
        if !climbing {
            state.warned_mode = false;
        }
        // The gripping hands feel the pull while the hands move the body.
        let strain = (climbing && !state.held_by_game).then(|| {
            tension(
                state.status.velocity.length(),
                tuning.tension_full_speed_mps,
            )
        });
        let gripping = state.grips.map(|g| g.is_some());
        (std::mem::take(&mut state.events), strain, gripping)
    };
    let (events, strain, gripping) = events;
    for (hand, event) in events {
        match event {
            Event::Gripped => haptics::play(hand, cues::Event::ClimbGrip),
            Event::Released => haptics::play(hand, cues::Event::ClimbRelease),
            Event::Flung(share) => haptics::play_scaled(hand, cues::Event::ClimbFling, share),
        }
    }
    if let Some(strain) = strain.filter(|&s| s >= tuning.min_tension) {
        for hand in Hand::BOTH.into_iter().filter(|h| gripping[h.index()]) {
            haptics::sustain(hand, Sustain::ClimbStrain, strain);
        }
    }
}
