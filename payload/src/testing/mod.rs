//! The test tools: replaying a tracking recording into the game ([`replay`]) and checking a
//! scenario's expectations while it replays ([`scenario`]), so the mod can be exercised without
//! anyone in the headset. The recording itself is [`crate::recorder::tracking`].
//!
//! Nothing runs until it is started: from the debug UI's Testing tab (on the desktop, or in the
//! headset through the floating panel), the director's key bindings, or the request file a script
//! drops into the configuration directory ([`request`]). `testing.enabled` switches all three off.
//!
//! [`update`] drives the tools from the top of the game's update on the game thread. The replay
//! also has a step at the top of the render update ([`replay::begin_frame`]) and seams in the VR
//! frame loop and the input update. See `docs/mod/testing/recorder.md`.

mod config;
pub(crate) mod log_tap;
pub(crate) mod replay;
pub(crate) mod request;
pub(crate) mod scenario;
pub(crate) mod signal_status;
pub(crate) mod signals;

pub use config::TestingConfig;

/// Drive the test tools for this frame: the request file, the replay's phases, and the running
/// scenario. Game thread, once per frame from the top of the game's update, after the director's
/// pass (which services the replay's teleport).
pub(crate) fn update() {
    request::poll();
    replay::update();
    scenario::update();
    signal_status::publish();
}

/// Replay this session's last tracking recording from its start marker, or stop the replay that
/// runs. For the director's key binding.
pub(crate) fn toggle_replay() {
    if replay::is_active() {
        replay::stop();
        return;
    }
    match crate::recorder::tracking::status().path {
        Some(path) => {
            replay::start(path, replay::ReplayOptions::default());
        }
        None => tracing::warn!(
            "testing: there is no tracking recording from this session to replay; pick one in the \
             Testing tab"
        ),
    }
}

/// Run the last scenario again, or stop the scenario that runs. For the director's key binding.
pub(crate) fn toggle_scenario() {
    if scenario::is_running() {
        scenario::stop();
        return;
    }
    match scenario::last_path() {
        Some(path) => scenario::start(path),
        None => tracing::warn!(
            "testing: no scenario has run in this session; start one from the Testing tab or the \
             request file"
        ),
    }
}

/// Stop the tools and wait for their threads, for eject. Returns whether none is left running.
#[must_use = "a thread that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    scenario::stop();
    log_tap::stop();
    replay::shutdown()
}
