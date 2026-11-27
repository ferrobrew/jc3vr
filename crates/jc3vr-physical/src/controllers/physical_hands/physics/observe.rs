//! The post-simulation half of the hand bodies' tick: publish each acting hand's palm pose and
//! velocity for the next step, and take the last step's results to draw the hands and pulse a
//! pressed one.

use std::{sync::atomic::Ordering, time::Duration};

use glam::Vec3;
use jc3vr_body::controllers::{
    clock::TimeScale, hands::CockpitHand, haptics, held, held::guns, physics::rig::RigSample,
};
use jc3vr_core::{
    game_tick::GameTick,
    published::{Published, Stage, Stamp, Step},
};
use jc3vr_input::{
    frame::{Hand, Pose},
    haptics::cues::Sustain,
};
use jc3vr_view::{headpose, headpose::teleport};

use crate::controllers::physical_hands::{
    physics::{
        PHYSICS, blows, callback, drawn_body,
        lifecycle::HANDS,
        render::{RenderOffset, apply_offsets},
        state::{CLEANUP_REGISTERED, SHARED, TRACKERS_AT_REST, Target, acting_palms},
    },
    tuning::PHYSICAL_HANDS,
};

/// Register the pre-simulation callback when the hand bodies are wanted, and the cleanup that
/// removes it. Called once per frame in a game tick ahead of the game's update.
pub fn frame_tick(game: &GameTick<'_>) {
    if jc3vr_core::is_shutting_down() {
        blows::discard();
        return;
    }
    crate::controllers::physical_hands::impacts::deliver(game);
    if jc3vr_core::hooks::in_gameplay() {
        // The shoves come from the pickups, which hold characters with or without the bodies.
        let shoves = blows::deliver_shoves(game);
        if shoves.shoves > 0 {
            let mut shared = SHARED.lock();
            shared.status.shoves += shoves.shoves;
            shared.status.knockdowns += shoves.knockdowns;
        }
    }
    // SAFETY: the game thread, ahead of the game's update, which fires the callbacks.
    let registered = unsafe { callback::ensure_registered() };
    if registered && !CLEANUP_REGISTERED.swap(true, Ordering::AcqRel) {
        jc3vr_core::lifecycle::on_cleanup(|_renderer| {
            // SAFETY: the shutdown cleanups run on the game thread, outside the game's update.
            unsafe { shutdown() };
        });
    }
    if !jc3vr_core::hooks::in_gameplay() {
        blows::discard();
        return;
    }
    let delivered = blows::deliver(game);
    let mut shared = SHARED.lock();
    let now = Stamp::now();
    let follow_through =
        Duration::from_secs_f32(PHYSICS.get().hand_bodies.hits.follow_through_s.max(0.0));
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
pub fn observe(dt: f32) {
    let now = Stamp::now();
    let acting = acting_palms();
    let step = Step::now();
    let park_now = acting.as_ref().map(|a| (step, a.park_now));
    let tracked = acting.as_ref().map_or([None; 2], |a| a.tracked);
    let palms = acting
        .filter(|a| a.palms.iter().any(Option::is_some))
        .map(|a| (a.palms, a.world_scale, a.origin));
    let rig = RigSample::now();
    let mut loads = [0.0; 2];
    {
        let mut shared = SHARED.lock();
        if teleport::epoch() != shared.epoch {
            shared.epoch = teleport::epoch();
            shared.trackers = TRACKERS_AT_REST;
            shared.carrier.reset();
        }
        // A snap turn or a recenter moves every palm at once without the hands moving: the
        // palms' motion starts afresh, and the next step carries the bodies with the rig.
        let jumped = shared.rig.observe(rig).is_some();
        if jumped {
            shared.trackers = TRACKERS_AT_REST;
            shared.carrier.reset();
        }
        let previous = shared.targets;
        let world_scale = palms.as_ref().map_or(1.0, |(_, scale, _)| *scale);
        // The palms move in real time, so their velocities, per game second, smooth over a real
        // stretch of time.
        // SAFETY: the observation runs while the game runs.
        let smoothing_s =
            unsafe { TimeScale::current() }.game_s(PHYSICAL_HANDS.get().state.velocity_smoothing_s);
        let carrier = match &palms {
            Some((_, _, origin)) => shared.carrier.update(*origin, dt, smoothing_s),
            None => {
                shared.carrier.reset();
                Vec3::ZERO
            }
        };
        let mut targets = [None; 2];
        for hand in Hand::BOTH {
            let i = hand.index();
            let palm = palms.as_ref().and_then(|(palms, _, _)| palms[i]);
            let Some(palm) = palm else {
                shared.trackers[i].0.reset();
                shared.trackers[i].1.reset();
                continue;
            };
            let linear = shared.trackers[i].0.update(palm.centre, dt, smoothing_s);
            let angular = shared.trackers[i]
                .1
                .update(palm.orientation, dt, smoothing_s);
            targets[i] = Some(Target {
                position: palm.centre,
                orientation: palm.orientation,
                linear,
                angular,
                carrier,
            });
            if jumped && let Some(from) = previous[i] {
                shared.rig_moves[i] =
                    Some((from.goal(), targets[i].map_or(from.goal(), |t| t.goal())));
            }
        }
        shared.targets = targets;
        shared.tracked = tracked;
        shared.park_now = park_now;
        shared.world_scale = world_scale;
        shared.published = palms.is_some().then_some(now);
        shared.published_step = palms.is_some().then_some(step);
        let stepped = shared.stepped_lately(now);
        let show = stepped;
        let scale = world_scale.max(1e-3);
        let offsets = shared.world_offsets;
        for hand in Hand::BOTH {
            let i = hand.index();
            shared.shown[i] = show && targets[i].is_some();
            shared.cockpit_offsets[i] = if shared.shown[i] {
                offsets[i].to_cockpit(rig.rotation, scale)
            } else {
                RenderOffset::NONE
            };
            if stepped && targets[i].is_some() {
                loads[i] = shared.loads[i];
            }
        }
    }
    let min_pressure_load = PHYSICS.get().step.min_pressure_load;
    for hand in Hand::BOTH {
        let load = loads[hand.index()];
        if load >= min_pressure_load {
            haptics::sustain(hand, Sustain::BodyPressure, load);
        }
    }
}

/// Draw each driven hand where its body is, in the cockpit frame, turning it about the palm's
/// centre at `palm_offset` in the wrist frame. Called by
/// [`crate::controllers::physical_hands::apply`].
pub fn apply(cockpit: &mut [Option<CockpitHand>; 2], palm_offset: Vec3) {
    let offsets = {
        let shared = SHARED.lock();
        if !shared.fresh(Stamp::now()) {
            return;
        }
        shared.cockpit_offsets
    };
    apply_offsets(
        cockpit,
        offsets.map(|offset| (!offset.is_none()).then_some(offset)),
        palm_offset,
        braced_by_hold(),
    );
}

/// The holding hand of the engaged two-handed hold, whose bracing hand rides its offset.
fn braced_by_hold() -> Option<Hand> {
    let contents = held::contents();
    Hand::BOTH
        .into_iter()
        .find(|hand| contents[hand.other().index()].braces.is_some())
}

/// Freeze post-step presentation offsets against the published palms, in their transport frame.
/// This reads bodies but never drives or places them.
///
/// # Safety
///
/// The game thread, after simulation and outside the physics step.
pub unsafe fn sample_render_offsets() {
    let now = Stamp::now();
    let (targets, rotation, scale, shown, fallback) = {
        let shared = SHARED.lock();
        if !shared.fresh(now) || !shared.stepped_lately(now) {
            RENDER_OFFSETS.clear();
            return;
        }
        (
            shared.targets,
            shared
                .rig
                .last()
                .map_or_else(headpose::xr::body_rotation, |rig| rig.rotation),
            shared.world_scale.max(1e-3),
            shared.shown,
            shared.offset_timeline.at(now),
        )
    };
    let physics = PHYSICS.get();
    let on_gun = guns::physical_hands();
    let hands = HANDS.lock();
    let offsets = Hand::BOTH.map(|hand| {
        let i = hand.index();
        targets[i].filter(|_| shown[i])?;
        let body = hands.bodies[i].as_ref()?;
        // SAFETY: the caller excludes the physics step; the owning body validates its slot.
        let motion = match unsafe { body.motion() } {
            Ok(motion) => motion,
            Err(_) => return fallback.map(|offsets| offsets[i].to_cockpit(rotation, scale)),
        };
        // The step drove the body toward the published palm led by the palm's world velocity
        // (`Target::led`), so the body leads the published palm by that motion even with nothing
        // in its way: a hand moving fast, or a player running, would be drawn ahead of its
        // controller by it. Measured against the led palm, the offset is only what holds the body
        // back. The led palm comes from `acting_palms`' support/weight-adjusted target, so the
        // residual does not cancel that target when it is applied.
        let palm = hands.sent_palm(hand)?;
        let offset = RenderOffset::between(
            Pose {
                position: palm.position,
                orientation: palm.orientation,
            },
            Pose {
                position: motion.position,
                orientation: motion.orientation,
            },
            physics.hand_bodies.break_distance_m * scale,
            if on_gun[i] {
                0.0
            } else {
                physics.render.turn_deadband_rad
            },
            physics.render.max_turn_rad,
        );
        Some(offset.to_cockpit(rotation, scale))
    });
    drop(hands);
    RENDER_OFFSETS.publish_at(offsets, now);
}

/// Apply the frame's post-step offsets without adding another physics-step interpolation delay.
pub fn apply_at_render(cockpit: &mut [Option<CockpitHand>; 2], palm_offset: Vec3) {
    match RENDER_OFFSETS.fresh(PHYSICS.get().step.stale_after()) {
        Some(offsets) => apply_offsets(cockpit, offsets, palm_offset, braced_by_hold()),
        None => apply(cockpit, palm_offset),
    }
}

static RENDER_OFFSETS: Published<[Option<RenderOffset>; 2]> = Published::new(Stage::RenderUpdate);

/// Remove the callback and destroy the bodies.
///
/// # Safety
///
/// The game thread, outside the game's update.
unsafe fn shutdown() {
    crate::controllers::physical_hands::climb::native::clear();
    // SAFETY: shutdown runs outside the world update before hooks are removed.
    unsafe { crate::controllers::physical_hands::impacts::clear() };
    unsafe { callback::unregister() };
    // SAFETY: as above.
    unsafe { drawn_body::clear() };
    let destroyed = unsafe { HANDS.lock().destroy_all("the payload is unloading") };
    let mut shared = SHARED.lock();
    shared.status.registered = false;
    shared.status.destroyed += destroyed;
}
