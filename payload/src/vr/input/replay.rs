//! Publishing a replay's recorded controller and semantic frames in place of the sampled ones.
//!
//! The recorded frames are what the input update published live, so they are published the same
//! way: one after another, each folding its button edges into the latch, with the scheme tick
//! seeing each. The publish stamps the sequence and the publish time; the display time is the
//! recording's own clock, which the hands' motion history spaces the frames by.

use std::time::Instant;

use crate::{
    controllers::{
        config::ControllersConfig,
        frame::{self, ControllerFrame},
        scheme,
    },
    testing::replay,
    vr::input::semantic::frame::{self as semantic_frame, SemanticFrame},
};

/// Publish the recorded frames that came due this frame, if a replay plays and the input update
/// has not published them yet. Returns whether a replay supplied the frame, in which case the
/// controllers are not to be sampled. With the controllers switched off, publishes nothing, as
/// the live update does.
pub(crate) fn publish_replayed(cfg: &ControllersConfig) -> bool {
    if !replay::is_driving() {
        return false;
    }
    let Some(frames) = replay::take_frames() else {
        // Nothing came due since the last frame; the last frame published still stands.
        return true;
    };
    if !cfg.enabled {
        frame::clear();
        semantic_frame::clear();
        return true;
    }
    for recorded in frames {
        let Some(controllers) = recorded.controllers else {
            // The controllers were not sampled on this recorded frame.
            frame::clear();
            semantic_frame::clear();
            continue;
        };
        let now = Instant::now();
        let published = ControllerFrame {
            sequence: 0,
            display_time_ns: recorded.display_time_ns,
            published_at: now,
            focused: controllers.focused,
            action_set: controllers.action_set,
            hands: controllers.hands,
        };
        scheme::tick(&published, cfg);
        frame::publish(published);
        match recorded.semantic {
            Some(semantic) => semantic_frame::publish(SemanticFrame {
                published_at: now,
                action_set: semantic.action_set,
                global: semantic.global,
                mode: semantic.mode,
            }),
            None => semantic_frame::clear(),
        }
    }
    true
}
