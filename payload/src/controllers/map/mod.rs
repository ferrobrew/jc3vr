//! Controller pointing for the game's native map UI (`CommMapUI`).
//!
//! The map keeps its own cursor, picking, panning, and zoom-to-cursor behaviour, all driven from
//! its mouse branch. The UI input boundary ([`crate::hooks::ui`]) prepares one controller sample
//! per frame ([`prepare_dispatch`]); when the map handles its input it commits that sample and,
//! for the duration of the call, switches the engine to mouse mode and answers the map's mouse and
//! zoom action lookups with effectors owned here ([`lookup`]), leaving every physical action-map
//! slot untouched. The map's cursor comes from [`cursor_uv`] through the UI manager's coordinate
//! hook.
//!
//! - [`controller`] is the pure owner, button-edge, and zoom-step state machine;
//! - [`lookup`] is the scoped effector shim.

mod controller;
mod lookup;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use detours_macro::detour;
use glam::Vec2;
use jc3gi::{
    input::{
        input_action_map::{InputActionMap, InputDeviceEffector},
        steering::Steering,
    },
    ui::{comm_map_ui::CommMapUI, ui_manager::UIManager},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        frame::{self, MAX_FRAME_AGE},
        map::{
            controller::{ControllerSample, MapController, MapDelivery, prepare_delivery},
            lookup::{SyntheticLookupScope, synthetic_effector},
        },
    },
    hud::pointer::{ControllerPointer, RoutedPointer},
};

/// The map hooks: the map's input handler and per-frame update, and the action lookup the shim
/// answers. Unmatched maps, actions, and devices continue through the engine trampoline.
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&ON_MANAGE_INPUT_BINDER)
        .with_static_binder(&MAP_UPDATE_BINDER)
        .with_static_binder(&GET_ACTION_EFFECTOR_BINDER)
}

/// Prepare the map's controller delivery for this frame's UI input dispatch, from the pointer that
/// owns the UI and the route peeked for the frame (`None` while the mod does not route the mouse).
/// Nothing advances until the map actually handles its input.
pub(crate) fn prepare_dispatch(
    controller: Option<ControllerPointer>,
    routed: Option<RoutedPointer>,
) {
    let sample = controller.map(controller_sample);
    let now = Instant::now();
    let mut state = MAP_STATE.lock();
    let mut next = state.controller;
    let delivery = prepare_delivery(&mut next, sample, routed, now);
    let cursor = delivery.map(|delivery| delivery.uv).or_else(|| {
        (routed == Some(RoutedPointer::Parked))
            .then(|| next.resting_uv())
            .flatten()
    });
    state.prepared = Some(Prepared {
        delivery,
        next,
        cursor,
    });
    CONTROLLER_DRIVEN.store(false, Ordering::Relaxed);
}

/// Drop this frame's prepared delivery once the UI input dispatch has returned.
pub(crate) fn finish_dispatch() {
    MAP_STATE.lock().prepared = None;
}

/// The map cursor for this dispatch: the controller's UV while it drives the map, or where it left
/// the cursor while the pointer is parked. `None` leaves the cursor to the desktop mouse.
pub(crate) fn cursor_uv() -> Option<Vec2> {
    MAP_STATE
        .lock()
        .prepared
        .and_then(|prepared| prepared.cursor)
}

/// Drop native map ownership at shutdown. The map receives no further dispatch from the mod, so
/// nothing is delivered.
pub(crate) fn cancel() {
    let mut state = MAP_STATE.lock();
    state.controller = MapController::new();
    state.prepared = None;
    CONTROLLER_DRIVEN.store(false, Ordering::Relaxed);
}

#[derive(Debug, Copy, Clone)]
struct Prepared {
    delivery: Option<MapDelivery>,
    /// The controller state after `delivery`, committed when the map consumes it.
    next: MapController,
    cursor: Option<Vec2>,
}

struct MapState {
    controller: MapController,
    prepared: Option<Prepared>,
}

static MAP_STATE: Mutex<MapState> = Mutex::new(MapState {
    controller: MapController::new(),
    prepared: None,
});

/// Whether the map handled a controller delivery in the latest dispatch. `UIManager::PreRender`
/// runs the map's update after the dispatch, and it reads the engine's gamepad flag to choose its
/// cursor display.
static CONTROLLER_DRIVEN: AtomicBool = AtomicBool::new(false);

fn controller_sample(pointer: ControllerPointer) -> ControllerSample {
    let zoom_axis = frame::with_latest(|frame| {
        if frame.focused && frame.age() <= MAX_FRAME_AGE {
            frame.hand(pointer.hand.other()).inputs.thumbstick.y
        } else {
            0.0
        }
    })
    .unwrap_or(0.0);
    ControllerSample {
        hand: pointer.hand,
        uv: Vec2::new(pointer.uv.0, pointer.uv.1),
        primary: pointer.buttons & 1 != 0,
        secondary: pointer.buttons & 2 != 0,
        zoom_axis,
    }
}

/// Commit this frame's prepared map state and return its delivery.
fn take_delivery() -> Option<MapDelivery> {
    let mut state = MAP_STATE.lock();
    let prepared = state.prepared?;
    state.controller = prepared.next;
    prepared.delivery
}

#[detour(address = InputActionMap::GetActionEffector_ADDRESS)]
fn get_action_effector(
    this: *mut InputActionMap,
    action_id: i32,
    device_index: i32,
) -> *mut InputDeviceEffector {
    if let Some(slot) = synthetic_effector(this, action_id, device_index) {
        return slot;
    }
    GET_ACTION_EFFECTOR
        .get()
        .unwrap()
        .call(this, action_id, device_index)
}

#[detour(address = CommMapUI::OnManageInput_ADDRESS)]
fn on_manage_input(this: *mut CommMapUI, steering: *mut Steering, dt: f32, check_for_freeze: bool) {
    // SAFETY: the engine passes its live map and steering for the duration of the call.
    let (map, steering_ref) = unsafe { (this.as_ref(), steering.as_ref()) };
    let delivery = take_delivery()
        .zip(map)
        .map(|(delivery, map)| delivery.for_map(map.m_MouseDragging));
    let lookup = delivery
        .zip(steering_ref)
        .and_then(|(delivery, steering)| SyntheticLookupScope::enter(steering, delivery));
    let mode = lookup.as_ref().and_then(|_| MapInputMode::enter());
    CONTROLLER_DRIVEN.store(
        mode.is_some() && delivery.is_some_and(|delivery| !delivery.cancelled),
        Ordering::Relaxed,
    );
    ON_MANAGE_INPUT
        .get()
        .unwrap()
        .call(this, steering, dt, check_for_freeze);
}

#[detour(address = CommMapUI::Update_ADDRESS)]
fn map_update(this: *mut CommMapUI, dt: f32, dtf: f32) {
    let _mode = CONTROLLER_DRIVEN
        .load(Ordering::Relaxed)
        .then(MapInputMode::enter)
        .flatten();
    MAP_UPDATE.get().unwrap().call(this, dt, dtf);
}

/// Clears `UIManager::m_IsUsingGamepad` while alive, so the map takes its mouse branch, and restores
/// it when the last live scope drops. The input handler and the update can run on different
/// threads, so the saved value is shared and the flag is restored only once.
struct MapInputMode;

impl MapInputMode {
    fn enter() -> Option<Self> {
        // SAFETY: the UI manager is a live singleton while the map runs.
        let manager = unsafe { UIManager::get()? };
        INPUT_MODE.lock().enter(&mut manager.m_IsUsingGamepad);
        Some(Self)
    }
}

impl Drop for MapInputMode {
    fn drop(&mut self) {
        let mut mode = INPUT_MODE.lock();
        // SAFETY: as in `enter`; the manager outlives the map callbacks.
        match unsafe { UIManager::get() } {
            Some(manager) => mode.exit(&mut manager.m_IsUsingGamepad),
            None => mode.depth = mode.depth.saturating_sub(1),
        }
    }
}

/// The nesting count of live [`MapInputMode`] scopes and the flag value the first one saved.
#[derive(Debug, Default, PartialEq, Eq)]
struct ModeDepth {
    depth: u32,
    saved: bool,
}

static INPUT_MODE: Mutex<ModeDepth> = Mutex::new(ModeDepth {
    depth: 0,
    saved: false,
});

impl ModeDepth {
    fn enter(&mut self, flag: &mut bool) {
        if self.depth == 0 {
            self.saved = *flag;
        }
        *flag = false;
        self.depth += 1;
    }

    fn exit(&mut self, flag: &mut bool) {
        self.depth = self.depth.saturating_sub(1);
        if self.depth == 0 {
            *flag = self.saved;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepad_mode_is_restored_once_after_nested_scopes() {
        for original in [false, true] {
            let mut flag = original;
            let mut mode = ModeDepth::default();
            mode.enter(&mut flag);
            assert!(!flag);
            mode.enter(&mut flag);
            mode.exit(&mut flag);
            assert!(!flag, "the outer scope is still live");
            mode.exit(&mut flag);
            assert_eq!(flag, original);
        }
    }
}
