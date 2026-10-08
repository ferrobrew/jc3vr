//! The post-simulation half of the hand bodies' tick: publish each acting hand's palm pose and
//! velocity for the next step, and take the last step's results to draw the hands and pulse a
//! pressed one.

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use glam::Vec3;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        hands::CockpitHand,
        haptics::{self, Sustain},
        physical_hands::physics::{
            blows, callback,
            drive::rig_jumped,
            lifecycle::HANDS,
            render::RenderOffset,
            state::{
                CLEANUP_REGISTERED, MIN_PRESSURE_LOAD, SHARED, TRACKERS_AT_REST, Target,
                VELOCITY_SMOOTHING_S, acting_palms, enabled,
            },
        },
        roomscale::teleport,
    },
    headpose, hooks,
    vr::recenter_count,
};

/// Register the pre-simulation callback when the hand bodies are wanted, and the cleanup that
/// removes it. Called once per frame on the game thread, ahead of the game's update.
pub(crate) fn frame_tick() {
    if crate::is_shutting_down() {
        blows::discard();
        return;
    }
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    if hooks::in_gameplay() {
        // The shoves come from the pickups, which hold characters with or without the bodies.
        // SAFETY: the game thread, ahead of the game's update.
        let shoves = unsafe { blows::deliver_shoves(&config) };
        if shoves.shoves > 0 {
            let mut shared = SHARED.lock();
            shared.status.shoves += shoves.shoves;
            shared.status.knockdowns += shoves.knockdowns;
            shared.status.last_shove = shoves.last;
        }
    }
    if !enabled(&config) || !hooks::in_gameplay() {
        blows::discard();
        return;
    }
    // SAFETY: the game thread, ahead of the game's update.
    let delivered = unsafe { blows::deliver(&config) };
    // SAFETY: the game thread, ahead of the game's update, which fires the callbacks.
    let registered = unsafe { callback::ensure_registered() };
    if registered && !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { shutdown() };
        });
    }
    let mut shared = SHARED.lock();
    let now = Instant::now();
    let follow_through = Duration::from_secs_f32(config.bodies.hits.follow_through_s.max(0.0));
    for (until, struck) in shared
        .follow_through
        .iter_mut()
        .zip(delivered.follow_through)
    {
        if struck {
            *until = Some(now + follow_through);
        }
    }
    let status = &mut shared.status;
    status.registered = registered;
    status.contacts += delivered.contacts;
    status.dropped_contacts += delivered.dropped;
    status.blows += delivered.blows;
    if delivered.last.is_some() {
        status.last_blow = delivered.last;
    }
}

/// Publish each acting hand's palm pose and velocity for the next step, and take the last step's
/// results. Called from the player aim control's post-simulation update, `dt` seconds after the
/// last.
pub(crate) fn observe(dt: f32) {
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let now = Instant::now();
    let acting = enabled(&config).then(|| acting_palms(&config)).flatten();
    let park_now = acting.as_ref().map(|a| (now, a.park_now));
    let palms = acting
        .filter(|a| a.palms.iter().any(Option::is_some))
        .map(|a| (a.palms, a.world_scale));
    let body_rotation = headpose::xr::body_rotation();
    let recenters = recenter_count();
    let mut loads = [0.0; 2];
    {
        let mut shared = SHARED.lock();
        if teleport::epoch() != shared.epoch {
            shared.epoch = teleport::epoch();
            shared.trackers = TRACKERS_AT_REST;
        }
        // A snap turn or a recenter moves every palm at once without the hands moving: the
        // palms' motion starts afresh, and the next step carries the bodies with the rig.
        let jumped = rig_jumped(shared.rig, body_rotation, recenters);
        shared.rig = Some((body_rotation, recenters));
        if jumped {
            shared.trackers = TRACKERS_AT_REST;
        }
        let previous = shared.targets;
        let world_scale = palms.as_ref().map_or(1.0, |(_, scale)| *scale);
        let mut targets = [None; 2];
        for hand in Hand::BOTH {
            let i = hand.index();
            let palm = palms.as_ref().and_then(|(palms, _)| palms[i]);
            let Some(palm) = palm else {
                shared.trackers[i].0.reset();
                shared.trackers[i].1.reset();
                continue;
            };
            let linear = shared.trackers[i]
                .0
                .update(palm.centre, dt, VELOCITY_SMOOTHING_S);
            let angular = shared.trackers[i]
                .1
                .update(palm.orientation, dt, VELOCITY_SMOOTHING_S);
            targets[i] = Some(Target {
                position: palm.centre,
                orientation: palm.orientation,
                linear,
                angular,
            });
            if jumped && let Some(from) = previous[i] {
                shared.rig_moves[i] =
                    Some((from.goal(), targets[i].map_or(from.goal(), |t| t.goal())));
            }
        }
        shared.targets = targets;
        shared.park_now = park_now;
        shared.world_scale = world_scale;
        shared.published = palms.is_some().then_some(now);
        let stepped = shared.stepped_lately(now);
        let show = stepped && config.bodies.render_at_body;
        let scale = world_scale.max(1e-3);
        let offsets = shared.world_offsets;
        for hand in Hand::BOTH {
            let i = hand.index();
            shared.cockpit_offsets[i] = if show && targets[i].is_some() {
                offsets[i].to_cockpit(body_rotation, scale)
            } else {
                RenderOffset::NONE
            };
            if stepped && targets[i].is_some() {
                loads[i] = shared.loads[i];
            }
        }
    }
    for hand in Hand::BOTH {
        let load = loads[hand.index()];
        if load >= MIN_PRESSURE_LOAD {
            haptics::sustain(hand, Sustain::BodyPressure, load);
        }
    }
}

/// Draw each driven hand where its body is, in the cockpit frame, turning it about the palm's
/// centre at `palm_offset` in the wrist frame. Called by
/// [`crate::controllers::physical_hands::apply`].
pub(crate) fn apply(cockpit: &mut [Option<CockpitHand>; 2], palm_offset: Vec3) {
    let offsets = {
        let shared = SHARED.lock();
        if !shared.fresh(Instant::now()) {
            return;
        }
        shared.cockpit_offsets
    };
    for (hand, offset) in cockpit.iter_mut().zip(offsets) {
        if let Some(hand) = hand.as_mut()
            && !offset.is_none()
        {
            offset.apply(hand, palm_offset);
        }
    }
}

/// Remove the callback and destroy the bodies.
///
/// # Safety
///
/// The game thread, outside the game's update.
unsafe fn shutdown() {
    unsafe { callback::unregister() };
    let destroyed = unsafe { HANDS.lock().destroy_all("the payload is unloading") };
    let mut shared = SHARED.lock();
    shared.status.registered = false;
    shared.status.destroyed += destroyed;
}
