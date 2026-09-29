//! Controller input through OpenXR: the action sets, suggested bindings, the controller spaces,
//! haptics output, `XR_EXT_hand_tracking` (with `XR_EXT_hand_joints_motion_range` when offered),
//! and the persistence of the action handles across
//! reinjects. Once per rendered frame the VR frame loop syncs the actions, locates the controllers
//! at the frame's predicted display time, and publishes a
//! [`ControllerFrame`](crate::controllers::frame::ControllerFrame) for the rest of the mod.
//!
//! # Action sets
//!
//! The actions carry raw inputs only, named for the physical inputs of the Valve Index controller
//! (see [`layout`]); mapping them to game actions happens in the controllers' bindings layer, per
//! mode. OpenXR gives every action exactly one owning set, so an action cannot be shared between
//! sets. The layout is therefore:
//!
//! - `hands`, always active: the grip and aim poses and the haptic output. The poses must track in
//!   every mode, and one set of action spaces serves them all.
//! - `onfoot`, `vehicle`, `airborne`, and `ui`, one active at a time: each holds the same raw input
//!   actions (triggers, grips, sticks, trackpads, face buttons, and the system button).
//!
//! The mode sets are identical rather than folded into one because the runtime's binding interface
//! (SteamVR's, or xrizer's) is the plan's per-user remapping story, and it remaps per set: a player
//! can move a button in vehicles without moving it on foot. Whichever mode set is active, the
//! published frame names the inputs the same way, so no consumer depends on the active set. Every
//! action is created with both hands as subaction paths and is read per hand.
//!
//! Suggested bindings are shipped for `/interaction_profiles/valve/index_controller` (every input)
//! and `/interaction_profiles/oculus/touch_controller` (the inputs it has, mapped by position).
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

mod actions;
mod haptics;
mod layout;
mod sample;
mod stash;
mod state;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub use haptics::{HapticPulse, request as request_haptic};
use parking_lot::Mutex;
pub(crate) use state::InputState;

use crate::controllers::frame::{ActionSetKind, InteractionProfile};

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

/// Note that the runtime changed a hand's interaction profile, so the next frame re-queries it.
/// Called from the event pump.
pub(crate) fn mark_profiles_dirty() {
    PROFILES_DIRTY.store(true, Ordering::Relaxed);
}

static ACTIVE_SET: AtomicU8 = AtomicU8::new(0);

/// Whether the profiles need re-querying. Starts set so the first frame queries them.
static PROFILES_DIRTY: AtomicBool = AtomicBool::new(true);

static STATUS: Mutex<InputStatus> = Mutex::new(InputStatus {
    attached: false,
    origin: None,
    error: None,
    profiles: [InteractionProfile::None; 2],
    hand_tracking: HandTrackingStatus {
        extension_enabled: false,
        system_supported: false,
        trackers_created: false,
        conforming_motion_range: false,
    },
});
