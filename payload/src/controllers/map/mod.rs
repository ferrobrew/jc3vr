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
//! The trigger is the map's primary button and selects where it was pressed (see
//! [`controller::CLICK_SLOP`]), and the confirm the map reads becomes the mouse's context action at
//! the cursor (see [`lookup`]), so both act at the controller's cursor.
//!
//! - [`controller`] is the pure owner, button-edge, press-point, and zoom-step state machine;
//! - [`lookup`] is the scoped effector shim;
//! - [`report`] is the debug readout.

mod controller;
mod lookup;
mod report;

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

pub(crate) use crate::controllers::map::{
    controller::{CLICK_SLOP, MapDelivery},
    report::{MapReport, report, zoom_label},
};
use crate::{
    controllers::{
        frame::{self, MAX_FRAME_AGE},
        map::{
            controller::{ControllerSample, MapController, prepare_delivery},
            lookup::{SyntheticLookupScope, observe_effector, synthetic_effector},
            report::{DispatchOutcome, DispatchReport, MapFields, record},
        },
    },
    hud::pointer::{ControllerPointer, RoutedPointer},
    vr::input::semantic::{
        self, SemanticSet,
        frame::{ActionValue, SemanticFrame},
        vocabulary::MenuAction,
    },
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
        routed,
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
    /// The route the delivery was prepared from, for the readout.
    routed: Option<RoutedPointer>,
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
    let focused =
        frame::with_latest(|frame| frame.focused && frame.age() <= MAX_FRAME_AGE).unwrap_or(false);
    let zoom_axis = semantic::frame::latest()
        .filter(|_| focused)
        .map_or(0.0, |frame| zoom_axis(&frame));
    ControllerSample {
        hand: pointer.hand,
        uv: Vec2::new(pointer.uv.0, pointer.uv.1),
        primary: pointer.buttons & 1 != 0,
        secondary: pointer.buttons & 2 != 0,
        zoom_axis,
    }
}

/// The map's zoom: the menu set's `map_zoom` action, on whichever hand the player bound it to (the
/// off-hand's stick by default), the hand pushed further winning; zero while the menu set is not
/// active.
fn zoom_axis(frame: &SemanticFrame) -> f32 {
    let Some(menu) = frame.set(SemanticSet::Menu) else {
        return 0.0;
    };
    menu.actions[MenuAction::MapZoom.id().index()]
        .iter()
        .map(|state| match state.value {
            ActionValue::Float(value) if value.is_finite() => value.clamp(-1.0, 1.0),
            ActionValue::Vec2(value) if value.y.is_finite() => value.y.clamp(-1.0, 1.0),
            _ => 0.0,
        })
        .fold(0.0, |a: f32, b: f32| if b.abs() > a.abs() { b } else { a })
}

/// Commit this frame's prepared map state and return it.
fn take_prepared() -> Option<Prepared> {
    let mut state = MAP_STATE.lock();
    let prepared = state.prepared?;
    state.controller = prepared.next;
    Some(prepared)
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
    let effector = GET_ACTION_EFFECTOR
        .get()
        .unwrap()
        .call(this, action_id, device_index);
    // SAFETY: the engine's lookup returned this effector for this call.
    unsafe { observe_effector(this, action_id, device_index, effector) };
    effector
}

#[detour(address = CommMapUI::OnManageInput_ADDRESS)]
fn on_manage_input(this: *mut CommMapUI, steering: *mut Steering, dt: f32, check_for_freeze: bool) {
    // SAFETY: the engine passes its live map and steering for the duration of the call.
    let (map, steering_ref) = unsafe { (this.as_ref(), steering.as_ref()) };
    let prepared = take_prepared();
    let dragging_before = map.is_some_and(|map| map.m_MouseDragging);
    let delivery = prepared
        .and_then(|prepared| prepared.delivery)
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
    let context_from_confirm = lookup
        .as_ref()
        .is_some_and(SyntheticLookupScope::context_from_confirm);
    let outcome = match (prepared, delivery, &lookup, &mode) {
        (None, ..) => DispatchOutcome::NotPrepared,
        (Some(_), None, ..) => DispatchOutcome::NotRouted,
        (Some(_), Some(_), None, _) => DispatchOutcome::NoMouseDevice,
        (Some(_), Some(_), Some(_), None) => DispatchOutcome::NoUiManager,
        (Some(_), Some(delivery), Some(_), Some(_)) if delivery.cancelled => {
            DispatchOutcome::Cancelled
        }
        _ => DispatchOutcome::Driven,
    };
    drop(mode);
    drop(lookup);
    // SAFETY: as above; the map is still live after its own input handler returns.
    let map = unsafe { this.as_ref() }.map(|map| MapFields {
        dragging: map.m_MouseDragging,
        selected: map.m_MouseCursorPositionUpdated,
        context_menu: map.m_ActivateCtxMenu,
        selection: Vec2::from_array(map.m_MouseCursorUiPosition.data),
    });
    record(
        DispatchReport {
            at: Instant::now(),
            outcome,
            route: prepared.and_then(|prepared| prepared.routed),
            delivery,
            cursor_stage: delivery.and_then(|delivery| stage_point(delivery.uv)),
            context_from_confirm,
            map,
        },
        dragging_before,
    );
}

/// A panel UV in the UI movie's stage coordinates, the space the map's cursor and selection live in.
fn stage_point(uv: Vec2) -> Option<Vec2> {
    // SAFETY: reads two fields of the live UI manager singleton.
    let manager = unsafe { UIManager::get()? };
    let stage = Vec2::new(manager.m_CachedStageWidth, manager.m_CachedStageHeight);
    (stage.x > 0.0 && stage.y > 0.0).then_some(uv * stage)
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
    use crate::controllers::frame::{ActionSetKind, Hand};

    fn set(set: SemanticSet) -> semantic::frame::SetState {
        semantic::frame::SetState {
            set,
            actions: set
                .actions()
                .iter()
                .map(|def| [semantic::frame::ActionState::rest(def.kind); 2])
                .collect(),
        }
    }

    fn menu_frame(zoom: [f32; 2]) -> SemanticFrame {
        let mut menu = set(SemanticSet::Menu);
        for hand in Hand::BOTH {
            menu.actions[MenuAction::MapZoom.id().index()][hand.index()].value =
                ActionValue::Float(zoom[hand.index()]);
        }
        SemanticFrame {
            published_at: Instant::now(),
            action_set: ActionSetKind::Ui,
            global: set(SemanticSet::Global),
            mode: menu,
        }
    }

    #[test]
    fn the_zoom_reads_the_map_zoom_action_on_either_hand() {
        assert_eq!(zoom_axis(&menu_frame([0.0, 0.0])), 0.0);
        assert_eq!(zoom_axis(&menu_frame([0.8, 0.0])), 0.8);
        assert_eq!(zoom_axis(&menu_frame([0.2, -0.6])), -0.6);
        assert_eq!(zoom_axis(&menu_frame([f32::NAN, 0.3])), 0.3);
        // Outside the menus the action is not read.
        let mut on_foot = menu_frame([0.9, 0.9]);
        on_foot.mode = set(SemanticSet::OnFoot);
        assert_eq!(zoom_axis(&on_foot), 0.0);
    }

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
