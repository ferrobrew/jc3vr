//! Controller input through OpenXR: the action sets, suggested bindings, the controller spaces,
//! haptics output, `XR_EXT_hand_tracking` (with `XR_EXT_hand_joints_motion_range` when offered),
//! and the persistence of the action handles across
//! reinjects. Once per rendered frame the VR frame loop syncs the actions, locates the controllers
//! at the frame's predicted display time, and publishes a
//! [`ControllerFrame`](crate::controllers::frame::ControllerFrame) for the rest of the mod.
//!
//! # Action sets
//!
//! OpenXR gives every action exactly one owning set. The layout is:
//!
//! - `hands`, always active: the grip and aim poses, the haptic output, and the raw inputs, named
//!   for the physical inputs of the Valve Index controller (see [`layout`]): triggers, grips,
//!   sticks, trackpads, face buttons, the system button, and their touches. The raw inputs feed
//!   the published [`ControllerFrame`](crate::controllers::frame::ControllerFrame), whatever the
//!   mode, and everything that reads it: the gestures, the grabs, the finger curls, the chords,
//!   input recovery, scheme detection, and the pointers.
//! - `global`, always active, and one mode set at a time (`on_foot`, `wingsuit`, `land_vehicle`,
//!   `helicopter`, `plane`, or `menu`): the semantic actions, named for what they do in the game
//!   ([`semantic`]). The mode detector picks the mode set through [`set_active_action_set`]. Their
//!   per-hand states are published each frame as a
//!   [`SemanticFrame`](semantic::frame::SemanticFrame), which the binding tables read
//!   ([`crate::controllers::bindings::intents`]).
//!
//! Every set has priority 0, so an input bound in `hands` and in a semantic set reaches both. Every
//! action is created with both hands as subaction paths and is read per hand. Suggested bindings
//! are shipped for `/interaction_profiles/valve/index_controller` and
//! `/interaction_profiles/oculus/touch_controller`; the raw inputs bind by position (the left Touch
//! controller's X, Y, and menu button stand for A, B, and the system button), and the semantic
//! actions follow [`semantic::defaults`], whose table is chosen from the dominant hand when the
//! actions are created.
//!
//! [`self_check`] probes, once per session, how the runtime treats this layout, and [`sources`]
//! caches the runtime's bound sources for the debug UI, the bindings' masks, and the controls
//! summary's glyphs.
//!
//! # Handedness
//!
//! OpenXR has no per-user binding presets: suggestions are per interaction profile and per
//! application. The left-handed layout is the right-handed suggestion table mirrored
//! ([`semantic::defaults::suggestions`]), chosen from `controllers.dominant_hand` when the actions
//! are created. Monado follows it; SteamVR follows it while the player has no custom binding for
//! the game. Since suggestions are fixed once the sets are attached, and the instance persists, a
//! change of hand takes effect when the game restarts; until then [`restart`] tells the player so.
//!
//! # Persistence
//!
//! The instance and session persist across inject cycles, and action sets can be attached to a
//! session only once. On uninject the action handles are stashed in the process environment
//! ([`stash`]) and the wrappers released without destroying anything; on reinject they are
//! re-wrapped. Suggesting bindings and attaching happen only for actions created fresh, and
//! attaching also for re-wrapped actions meeting a session that was created fresh on the persisted
//! instance. The action spaces and hand trackers are session children that are cheap to recreate,
//! so, like the LOCAL reference space, they are destroyed on uninject and recreated on reinject
//! rather than stashed.
//!
//! The actions are created and attached whether or not the controllers are enabled, because
//! attachment is possible only once per session; the master toggle gates the per-frame sync and the
//! publication.

pub(crate) mod actions;
mod haptics;
pub(crate) mod layout;
pub mod restart;
mod sample;
pub(crate) mod self_check;
pub(crate) mod semantic;
pub(crate) mod sources;
mod stash;
mod state;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub use haptics::{HapticPulse, request as request_haptic};
use parking_lot::Mutex;
pub(crate) use stash::clear as clear_stash;
pub(crate) use state::InputState;

use crate::{
    controllers::frame::{ActionSetKind, Hand, InteractionProfile},
    vr::input::restart::RestartReason,
};

/// Select the mode action set read from the next frame on. The mode detector calls this once per
/// sim tick.
pub fn set_active_action_set(kind: ActionSetKind) {
    ACTIVE_SET.store(kind.index() as u8, Ordering::Relaxed);
}

/// The mode action set currently selected.
pub fn active_action_set() -> ActionSetKind {
    let index = ACTIVE_SET.load(Ordering::Relaxed) as usize;
    ActionSetKind::ALL
        .get(index)
        .copied()
        .unwrap_or(ActionSetKind::OnFoot)
}

/// The controller input's state, for the debug UI.
#[derive(Debug, Clone, Default)]
pub struct InputStatus {
    /// Whether the actions are created and attached to the running session.
    pub attached: bool,
    /// How the actions were obtained at bring-up.
    pub origin: Option<ActionsOrigin>,
    /// Why the input could not be set up, when it could not.
    pub error: Option<String>,
    /// Why the game must restart for the input to work, when bring-up failed for a reason a
    /// restart fixes.
    pub restart: Option<RestartReason>,
    /// The dominant hand the attached actions' bindings were suggested for.
    pub suggested_for: Option<Hand>,
    /// The interaction profile of each hand.
    pub profiles: [InteractionProfile; 2],
    pub hand_tracking: HandTrackingStatus,
}

/// How the actions were obtained at bring-up.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActionsOrigin {
    /// Created, suggested, and attached.
    Created,
    /// Re-wrapped from the stash and attached to a fresh session on the persisted instance.
    RewrappedAndAttached,
    /// Re-wrapped from the stash, already attached to the persisted session.
    Rewrapped,
}

/// `XR_EXT_hand_tracking` availability.
#[derive(Debug, Copy, Clone, Default)]
pub struct HandTrackingStatus {
    /// Whether the instance was created with the extension enabled.
    pub extension_enabled: bool,
    /// Whether the system reports hand tracking support.
    pub system_supported: bool,
    /// Whether hand trackers currently exist.
    pub trackers_created: bool,
    /// Whether the instance was created with `XR_EXT_hand_joints_motion_range`, so the joints are
    /// located with the motion range that conforms to a held controller.
    pub conforming_motion_range: bool,
}

/// A snapshot of the controller input's state. Never touches the runtime lock.
pub fn status() -> InputStatus {
    STATUS.lock().clone()
}

/// The dominant hand the attached actions' bindings were suggested for, `None` while none are
/// attached. Lock-free, so that any thread, holding any lock, may ask.
pub fn bound_hand() -> Option<Hand> {
    match BOUND_HAND.load(Ordering::Relaxed) {
        BOUND_LEFT => Some(Hand::Left),
        BOUND_RIGHT => Some(Hand::Right),
        _ => None,
    }
}

/// Publish the hand the attached bindings were suggested for ([`bound_hand`]).
fn set_bound_hand(hand: Option<Hand>) {
    let value = match hand {
        None => BOUND_NONE,
        Some(Hand::Left) => BOUND_LEFT,
        Some(Hand::Right) => BOUND_RIGHT,
    };
    BOUND_HAND.store(value, Ordering::Relaxed);
}

/// Note that the runtime changed a hand's interaction profile, so the next frame re-queries it.
/// Called from the event pump.
pub(crate) fn mark_profiles_dirty() {
    PROFILES_DIRTY.store(true, Ordering::Relaxed);
}

static ACTIVE_SET: AtomicU8 = AtomicU8::new(0);

/// [`bound_hand`]'s value: one of the `BOUND_` constants.
static BOUND_HAND: AtomicU8 = AtomicU8::new(BOUND_NONE);
const BOUND_NONE: u8 = 0;
const BOUND_LEFT: u8 = 1;
const BOUND_RIGHT: u8 = 2;

/// Whether the profiles need re-querying. Starts set so the first frame queries them.
static PROFILES_DIRTY: AtomicBool = AtomicBool::new(true);

static STATUS: Mutex<InputStatus> = Mutex::new(InputStatus {
    attached: false,
    origin: None,
    error: None,
    restart: None,
    suggested_for: None,
    profiles: [InteractionProfile::None; 2],
    hand_tracking: HandTrackingStatus {
        extension_enabled: false,
        system_supported: false,
        trackers_created: false,
        conforming_motion_range: false,
    },
});
