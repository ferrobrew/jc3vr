//! Controller pointing for the game's native map UI.
//!
//! The map keeps its own cursor, picking, panning, and zoom-to-cursor behaviour. This module
//! prepares one coherent controller sample at the UI input boundary and exposes it to the native
//! coordinate and Scaleform paths. During the map's own dispatch it supplies thread-local synthetic
//! effectors for the concrete mouse device, leaving every physical action-map slot untouched.

use std::cell::{RefCell, UnsafeCell};

use detours_macro::detour;
use glam::Vec2;
use jc3gi::{
    input::{
        input_action_map::{Action, EffectorState, InputActionMap, InputDeviceEffector},
        input_device_manager::{DeviceOrder, DeviceType, InputDeviceManager},
        steering::Steering,
    },
    ui::{comm_map_ui::CommMapUI, ui_manager::UIManager},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::frame::{Hand, MAX_FRAME_AGE},
    hud::pointer::{self, ControllerPointer, RoutedPointer},
};

/// A controller sample that is eligible for native map input.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControllerSample {
    /// The controller that owns this map interaction.
    pub hand: Hand,
    /// Absolute panel UV, `(0, 0)` at the top left.
    pub uv: Vec2,
    /// The map's primary button, normally the selected controller's trigger.
    pub primary: bool,
    /// The map's secondary button. The current controller pointer has no secondary binding.
    pub secondary: bool,
    /// A discrete or accumulated zoom request. Positive values zoom in, negative values zoom out.
    pub zoom: f32,
}

/// The values to deliver to the native map for one dispatch.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MapDelivery {
    /// Absolute panel UV for the map cursor. On cancellation this is the last delivered UV, so a
    /// held native button is released at the point where the map saw it.
    pub uv: Vec2,
    /// The primary button level for this dispatch.
    pub primary: bool,
    /// The secondary button level for this dispatch.
    pub secondary: bool,
    /// Absolute movement since the previous eligible controller sample while the primary button
    /// was held. This is derived from absolute UVs, not from a controller velocity or a frame-rate
    /// dependent synthetic mouse delta.
    pub drag_delta: Vec2,
    /// The zoom request for this dispatch. It is zero during source transitions and cancellation.
    pub zoom: f32,
    /// Whether the consumer must treat this dispatch as a neutral release boundary.
    pub cancelled: bool,
    /// Whether the primary button began in this dispatch.
    pub primary_pressed: bool,
    /// Whether the primary button ended in this dispatch.
    pub primary_released: bool,
    /// Whether the secondary button began in this dispatch.
    pub secondary_pressed: bool,
    /// Whether the secondary button ended in this dispatch.
    pub secondary_released: bool,
}

impl MapDelivery {
    const fn neutral(
        uv: Vec2,
        cancelled: bool,
        primary_released: bool,
        secondary_released: bool,
    ) -> Self {
        Self {
            uv,
            primary: false,
            secondary: false,
            drag_delta: Vec2::ZERO,
            zoom: 0.0,
            cancelled,
            primary_pressed: false,
            primary_released,
            secondary_pressed: false,
            secondary_released,
        }
    }
}

/// Cross-dispatch ownership and drag state for a native map controller.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Owner {
    None,
    Controller(Hand),
}

/// The native map's controller-side state machine.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MapController {
    owner: Owner,
    /// A controller press is accepted only after a released sample owned the map. This prevents a
    /// held trigger from becoming a click when tracking or the owning hand changes.
    armed: bool,
    /// Whether the previous delivery held the primary button.
    primary_held: bool,
    /// Whether the previous delivery held the secondary button.
    secondary_held: bool,
    last_uv: Vec2,
}

impl Default for MapController {
    fn default() -> Self {
        Self::new()
    }
}

impl MapController {
    /// Create an inactive controller state.
    pub const fn new() -> Self {
        Self {
            owner: Owner::None,
            armed: false,
            primary_held: false,
            secondary_held: false,
            last_uv: Vec2::ZERO,
        }
    }

    /// Deliver one controller sample, or a neutral cancellation when the sample is absent or
    /// invalid. A cancellation is emitted exactly once for an active owner; the next absent sample
    /// returns `None`.
    pub fn step(&mut self, sample: Option<ControllerSample>) -> Option<MapDelivery> {
        let Some(sample) = sample.filter(|sample| sample.uv.is_finite()) else {
            return self.cancel();
        };
        let uv = sample.uv.clamp(Vec2::ZERO, Vec2::ONE);
        let owner = Owner::Controller(sample.hand);
        if self.owner != owner {
            let was_active = self.owner != Owner::None;
            let release_uv = self.last_uv;
            let primary_released = self.primary_held;
            let secondary_released = self.secondary_held;
            self.owner = owner;
            self.armed = !sample.primary && !sample.secondary;
            self.primary_held = false;
            self.secondary_held = false;
            self.last_uv = uv;
            return Some(MapDelivery::neutral(
                if was_active { release_uv } else { uv },
                was_active,
                primary_released,
                secondary_released,
            ));
        }

        let drag_delta = if self.primary_held {
            uv - self.last_uv
        } else {
            Vec2::ZERO
        };
        self.last_uv = uv;
        if !sample.primary && !sample.secondary {
            self.armed = true;
        }
        let accepted = self.armed;
        let primary = accepted && sample.primary;
        let secondary = accepted && sample.secondary;
        let primary_pressed = primary && !self.primary_held;
        let primary_released = !primary && self.primary_held;
        let secondary_pressed = secondary && !self.secondary_held;
        let secondary_released = !secondary && self.secondary_held;
        self.primary_held = primary;
        self.secondary_held = secondary;
        Some(MapDelivery {
            uv,
            primary,
            secondary,
            drag_delta: if primary { drag_delta } else { Vec2::ZERO },
            zoom: if accepted && sample.zoom.is_finite() {
                sample.zoom
            } else {
                0.0
            },
            cancelled: false,
            primary_pressed,
            primary_released,
            secondary_pressed,
            secondary_released,
        })
    }

    /// Cancel the active controller owner and return the one neutral delivery needed to release a
    /// button in the map. A later absent sample produces no delivery.
    pub fn cancel(&mut self) -> Option<MapDelivery> {
        if self.owner == Owner::None {
            return None;
        }
        let delivery = MapDelivery {
            uv: self.last_uv,
            primary: false,
            secondary: false,
            drag_delta: Vec2::ZERO,
            zoom: 0.0,
            cancelled: true,
            primary_pressed: false,
            primary_released: self.primary_held,
            secondary_pressed: false,
            secondary_released: self.secondary_held,
        };
        self.owner = Owner::None;
        self.armed = false;
        self.primary_held = false;
        self.secondary_held = false;
        Some(delivery)
    }
}

static MAP_STATE: Mutex<MapState> = Mutex::new(MapState::new());

struct MapState {
    controller: MapController,
    prepared: Option<PreparedInput>,
}

impl MapState {
    const fn new() -> Self {
        Self {
            controller: MapController::new(),
            prepared: None,
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct PreparedInput {
    delivery: Option<MapDelivery>,
    routed: RoutedPointer,
}

/// Prepare the one controller and routed-pointer sample consumed during `UIManager::PreUpdate`.
/// The slot is cleared by [`finish_pre_update`] after the original returns. The gates mirror the
/// Scaleform delivery hook: no live movie, redirected geometry, or active native pointer means no
/// controller sample is made eligible for native map input.
fn begin_pre_update(this: *mut UIManager) {
    let egui_captured = crate::egui_impl::EguiState::get()
        .as_ref()
        .is_some_and(|state| state.is_input_captured());
    let redirect_enabled = Config::lock_query(|config| {
        config.hud.redirect && config.hud.quad && config.hud.cursor.enabled
    });
    let dispatch_enabled = !crate::is_shutting_down()
        && redirect_enabled
        && !egui_captured
        && crate::hud::cursor::geometry().is_some()
        && unsafe {
            this.as_ref()
                .is_some_and(|manager| !manager.m_Movie.is_null())
        };
    let controller = dispatch_enabled
        .then(pointer::selected_controller)
        .flatten();
    let sample = controller.and_then(controller_sample);
    let routed = if dispatch_enabled {
        pointer::route_game_pointer(controller)
    } else {
        pointer::reset_route();
        RoutedPointer {
            controller: false,
            uv: None,
            buttons: 0,
        }
    };
    let mut state = MAP_STATE.lock();
    let delivery = prepare_delivery(&mut state.controller, sample, routed);
    state.prepared = Some(PreparedInput { delivery, routed });
}

/// Clear the current UI dispatch sample and cancel a stale map owner when the UI boundary exits.
fn finish_pre_update() {
    MAP_STATE.lock().prepared = None;
}

/// Advance map state after the shared pointer router's decision. A non-controller route cancels
/// stale native ownership and exposes that one neutral delivery so the map sees the release before
/// the prepared slot is cleared. Ordinary desktop routing has no native delivery to expose.
fn prepare_delivery(
    controller: &mut MapController,
    sample: Option<ControllerSample>,
    routed: RoutedPointer,
) -> Option<MapDelivery> {
    if routed.controller {
        controller.step(sample)
    } else {
        controller.cancel()
    }
}

fn prepared_controller_delivery() -> Option<MapDelivery> {
    prepared().and_then(|input| input.delivery)
}

const SYNTHETIC_ACTIONS: [Action; 8] = [
    Action::MOUSE1,
    Action::MOUSE2,
    Action::GUI_RSTICK_UP,
    Action::GUI_RSTICK_DOWN,
    Action::GUI_RSTICK_LEFT,
    Action::GUI_RSTICK_RIGHT,
    Action::GUI_ZOOM_IN,
    Action::GUI_ZOOM_OUT,
];

struct SyntheticLookup {
    action_map: *mut InputActionMap,
    mouse_device_index: i32,
    slots: Box<[UnsafeCell<InputDeviceEffector>; SYNTHETIC_ACTIONS.len()]>,
}

impl SyntheticLookup {
    fn new(
        action_map: *mut InputActionMap,
        mouse_device_index: i32,
        delivery: MapDelivery,
    ) -> Self {
        let mut slots = std::array::from_fn(|_| {
            UnsafeCell::new(InputDeviceEffector {
                m_Value: 0.0,
                m_PrevValue: 0.0,
                m_State: EffectorState::Idle,
                m_IsAnalogue: true,
                m_IsDeltaBased: false,
                m_IsUpdated: true,
                m_ForceClick: false,
                m_StateTime: 0.0,
            })
        });
        for (slot, action) in slots.iter_mut().zip(SYNTHETIC_ACTIONS) {
            let slot = slot.get_mut();
            let (value, state) = match action {
                Action::MOUSE1 => (
                    f32::from(delivery.primary),
                    button_state(
                        delivery.primary,
                        delivery.primary_pressed,
                        delivery.primary_released,
                    ),
                ),
                Action::MOUSE2 => (
                    f32::from(delivery.secondary),
                    button_state(
                        delivery.secondary,
                        delivery.secondary_pressed,
                        delivery.secondary_released,
                    ),
                ),
                Action::GUI_RSTICK_UP => (-delivery.drag_delta.y.min(0.0), EffectorState::Pressed),
                Action::GUI_RSTICK_DOWN => (delivery.drag_delta.y.max(0.0), EffectorState::Pressed),
                Action::GUI_RSTICK_LEFT => {
                    (-delivery.drag_delta.x.min(0.0), EffectorState::Pressed)
                }
                Action::GUI_RSTICK_RIGHT => {
                    (delivery.drag_delta.x.max(0.0), EffectorState::Pressed)
                }
                Action::GUI_ZOOM_IN => (
                    (delivery.zoom > 0.0) as u8 as f32,
                    if delivery.zoom > 0.0 {
                        EffectorState::Clicked
                    } else {
                        EffectorState::Idle
                    },
                ),
                Action::GUI_ZOOM_OUT => (
                    (delivery.zoom < 0.0) as u8 as f32,
                    if delivery.zoom < 0.0 {
                        EffectorState::Clicked
                    } else {
                        EffectorState::Idle
                    },
                ),
                _ => unreachable!(),
            };
            slot.m_Value = value;
            slot.m_PrevValue = value;
            slot.m_State = state;
            slot.m_IsAnalogue = !matches!(
                action,
                Action::MOUSE1 | Action::MOUSE2 | Action::GUI_ZOOM_IN | Action::GUI_ZOOM_OUT
            );
        }
        Self {
            action_map,
            mouse_device_index,
            slots: Box::new(slots),
        }
    }

    fn slot(&self, action_id: i32, device_index: i32) -> Option<*mut InputDeviceEffector> {
        let action = SYNTHETIC_ACTIONS
            .iter()
            .copied()
            .find(|action| *action as i32 == action_id)?;
        let device_scoped = matches!(
            action,
            Action::MOUSE1
                | Action::MOUSE2
                | Action::GUI_RSTICK_UP
                | Action::GUI_RSTICK_DOWN
                | Action::GUI_RSTICK_LEFT
                | Action::GUI_RSTICK_RIGHT
        );
        if device_scoped && device_index != self.mouse_device_index {
            return None;
        }
        if !device_scoped && device_index != -1 && device_index != self.mouse_device_index {
            return None;
        }
        let index = SYNTHETIC_ACTIONS
            .iter()
            .position(|candidate| *candidate == action)
            .expect("synthetic action must have a slot");
        Some(self.slots[index].get())
    }
}

fn button_state(held: bool, pressed: bool, released: bool) -> EffectorState {
    if pressed {
        EffectorState::Clicked
    } else if released {
        EffectorState::Released
    } else if held {
        EffectorState::Pressed
    } else {
        EffectorState::Idle
    }
}

thread_local! {
    static SYNTHETIC_LOOKUP: RefCell<Option<Box<SyntheticLookup>>> = const { RefCell::new(None) };
}

struct SyntheticLookupScope {
    previous: Option<Box<SyntheticLookup>>,
}

impl SyntheticLookupScope {
    fn enter(action_map: *mut InputActionMap, delivery: MapDelivery) -> Option<Self> {
        let mouse_device_index = unsafe {
            let manager = InputDeviceManager::get()?;
            let device = manager.GetDevice(DeviceType::Mouse, 0, DeviceOrder::Appearance);
            device.as_ref()?.ID.action_device_index()
        };
        Self::enter_for_device(action_map, Some(mouse_device_index), delivery)
    }

    fn enter_for_device(
        action_map: *mut InputActionMap,
        mouse_device_index: Option<i32>,
        delivery: MapDelivery,
    ) -> Option<Self> {
        let mouse_device_index = mouse_device_index?;
        let previous = SYNTHETIC_LOOKUP.with(|scope| {
            scope.borrow_mut().replace(Box::new(SyntheticLookup::new(
                action_map,
                mouse_device_index,
                delivery,
            )))
        });
        Some(Self { previous })
    }
}

impl Drop for SyntheticLookupScope {
    fn drop(&mut self) {
        SYNTHETIC_LOOKUP.with(|scope| {
            *scope.borrow_mut() = self.previous.take();
        });
    }
}

fn synthetic_effector(
    action_map: *mut InputActionMap,
    action_id: i32,
    device_index: i32,
) -> Option<*mut InputDeviceEffector> {
    SYNTHETIC_LOOKUP.with(|scope| {
        let scope = scope.borrow();
        let lookup = scope.as_ref()?;
        (lookup.action_map == action_map)
            .then(|| lookup.slot(action_id, device_index))
            .flatten()
    })
}

fn synthetic_any_device_effector(
    action_map: *mut InputActionMap,
    action_id: i32,
) -> Option<*mut InputDeviceEffector> {
    synthetic_effector(action_map, action_id, -1)
}

fn controller_sample(pointer: ControllerPointer) -> Option<ControllerSample> {
    let zoom = crate::controllers::frame::with_latest(|frame| {
        if !frame.focused || frame.age() > MAX_FRAME_AGE {
            return 0.0;
        }
        let y = frame.hand(pointer.hand.other()).inputs.thumbstick.y;
        if y.is_finite() {
            y.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    })
    .unwrap_or(0.0);
    Some(ControllerSample {
        hand: pointer.hand,
        uv: Vec2::new(pointer.uv.0, pointer.uv.1),
        primary: pointer.buttons & 1 != 0,
        secondary: pointer.buttons & 2 != 0,
        zoom,
    })
}

fn prepared() -> Option<PreparedInput> {
    MAP_STATE.lock().prepared
}

/// The prepared pointer route for the current UI dispatch.
pub(crate) fn prepared_routed() -> Option<RoutedPointer> {
    prepared().map(|input| input.routed)
}

/// The prepared native map cursor position for the current UI dispatch.
pub(crate) fn prepared_uv() -> Option<Vec2> {
    prepared().and_then(|input| input.delivery.map(|delivery| delivery.uv))
}

/// Cancel native map ownership during shutdown or an input-system reset.
pub(crate) fn cancel() {
    let mut state = MAP_STATE.lock();
    state.controller.cancel();
    state.prepared = None;
}

/// The map hooks, including the shared UI preparation boundary and the scoped action lookup shim.
/// The shim supplies owned slots only while the map consumes the prepared controller delivery; all
/// unmatched maps, actions, and device indices continue through the engine trampoline.
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&PRE_UPDATE_BINDER)
        .with_static_binder(&ON_MANAGE_INPUT_BINDER)
        .with_static_binder(&MAP_UPDATE_BINDER)
        .with_static_binder(&GET_ACTION_EFFECTOR_BINDER)
        .with_static_binder(&GET_ACTION_EFFECTOR_FOR_ACTION_BINDER)
}

#[detour(address = UIManager::PreUpdate_ADDRESS)]
fn pre_update(this: *mut UIManager, dt: f32) {
    begin_pre_update(this);
    let original = PRE_UPDATE.get().unwrap();
    original.call(this, dt);
    finish_pre_update();
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
    // SAFETY: the detour trampoline has the exact release ABI and is only called by the engine.
    GET_ACTION_EFFECTOR
        .get()
        .unwrap()
        .call(this, action_id, device_index)
}

#[detour(address = InputActionMap::GetActionEffectorForAction_ADDRESS)]
fn get_action_effector_for_action(
    this: *mut InputActionMap,
    action: Action,
) -> *mut InputDeviceEffector {
    if let Some(slot) = synthetic_any_device_effector(this, action as i32) {
        return slot;
    }
    GET_ACTION_EFFECTOR_FOR_ACTION
        .get()
        .unwrap()
        .call(this, action)
}

#[detour(address = CommMapUI::OnManageInput_ADDRESS)]
fn on_manage_input(this: *mut CommMapUI, steering: *mut Steering, dt: f32, check_for_freeze: bool) {
    let delivery = prepared_controller_delivery();
    let action_map = unsafe { steering.as_ref() }.map_or(std::ptr::null_mut(), |s| s.ActionMap);
    let _lookup = delivery.and_then(|delivery| SyntheticLookupScope::enter(action_map, delivery));
    let _mode = delivery.and_then(|_| unsafe { MapInputMode::enter() });
    ON_MANAGE_INPUT
        .get()
        .unwrap()
        .call(this, steering, dt, check_for_freeze);
}

#[detour(address = CommMapUI::Update_ADDRESS)]
fn map_update(this: *mut CommMapUI, dt: f32, dtf: f32) {
    let action_map = unsafe { UIManager::get() }
        .and_then(|manager| unsafe { manager.m_Steering.as_ref() })
        .map_or(std::ptr::null_mut(), |steering| steering.ActionMap);
    let delivery = prepared_controller_delivery();
    let _lookup = delivery.and_then(|delivery| SyntheticLookupScope::enter(action_map, delivery));
    let _mode = delivery.and_then(|_| unsafe { MapInputMode::enter() });
    MAP_UPDATE.get().unwrap().call(this, dt, dtf);
}

struct MapInputMode {
    manager: *mut UIManager,
    previous: bool,
}

impl MapInputMode {
    unsafe fn enter() -> Option<Self> {
        let manager = unsafe { UIManager::get()? };
        let previous = manager.m_IsUsingGamepad;
        manager.m_IsUsingGamepad = false;
        Some(Self { manager, previous })
    }
}

impl Drop for MapInputMode {
    fn drop(&mut self) {
        // SAFETY: the manager remains live for the duration of the map callback.
        unsafe {
            if let Some(manager) = self.manager.as_mut() {
                restore_gamepad_mode(&mut manager.m_IsUsingGamepad, self.previous);
            }
        }
    }
}

fn restore_gamepad_mode(flag: &mut bool, previous: bool) {
    *flag = previous;
}

#[cfg(test)]
mod tests {
    use super::*;

    const UV: Vec2 = Vec2::new(0.25, 0.5);

    fn sample(hand: Hand, uv: Vec2, primary: bool, secondary: bool, zoom: f32) -> ControllerSample {
        ControllerSample {
            hand,
            uv,
            primary,
            secondary,
            zoom,
        }
    }

    #[test]
    fn first_owner_sample_is_neutral_and_requires_release() {
        let mut map = MapController::new();
        assert_eq!(
            map.step(Some(sample(Hand::Right, UV, true, false, 1.0))),
            Some(MapDelivery::neutral(UV, false, false, false))
        );
        let held = map
            .step(Some(sample(Hand::Right, UV, true, false, 1.0)))
            .unwrap();
        assert!(!held.primary);
        let released = map
            .step(Some(sample(Hand::Right, UV, false, false, 0.0)))
            .unwrap();
        assert!(!released.primary);
        let pressed = map
            .step(Some(sample(Hand::Right, UV, true, false, 0.0)))
            .unwrap();
        assert!(pressed.primary);
        assert!(pressed.primary_pressed);
    }

    #[test]
    fn hand_change_cancels_before_accepting_the_new_owner() {
        let mut map = MapController::new();
        map.step(Some(sample(Hand::Right, UV, false, false, 0.0)));
        assert!(
            map.step(Some(sample(Hand::Right, UV, true, false, 0.0)))
                .unwrap()
                .primary
        );
        let cancel = map
            .step(Some(sample(
                Hand::Left,
                Vec2::new(0.8, 0.2),
                true,
                false,
                3.0,
            )))
            .unwrap();
        assert!(cancel.cancelled);
        assert!(cancel.primary_released);
        assert!(!cancel.secondary_released);
        assert_eq!(cancel.uv, UV);
        assert!(!cancel.primary);
        assert!(
            !map.step(Some(sample(
                Hand::Left,
                Vec2::new(0.8, 0.2),
                true,
                false,
                3.0,
            )))
            .unwrap()
            .primary
        );
    }

    #[test]
    fn cancellation_releases_once_at_last_uv() {
        let mut map = MapController::new();
        map.step(Some(sample(Hand::Right, UV, false, false, 0.0)));
        map.step(Some(sample(Hand::Right, UV, true, false, 0.0)));
        let cancel = map.cancel().unwrap();
        assert!(cancel.cancelled);
        assert!(cancel.primary_released);
        assert!(!cancel.secondary_released);
        assert_eq!(cancel.uv, UV);
        assert_eq!(map.cancel(), None);
        assert_eq!(map.step(None), None);
    }

    #[test]
    fn desktop_arbitration_cancels_native_delivery_once() {
        let mut map = MapController::new();
        let controller = sample(Hand::Right, UV, false, false, 0.0);
        let desktop = RoutedPointer {
            controller: false,
            uv: None,
            buttons: 1,
        };
        assert_eq!(prepare_delivery(&mut map, Some(controller), desktop), None);
        assert_eq!(map.step(None), None);

        let controller_route = RoutedPointer {
            controller: true,
            uv: Some((UV.x, UV.y)),
            buttons: 0,
        };
        assert_eq!(
            prepare_delivery(&mut map, Some(controller), controller_route),
            Some(MapDelivery::neutral(UV, false, false, false))
        );

        let pressed = sample(Hand::Right, UV, true, false, 0.0);
        assert!(
            prepare_delivery(&mut map, Some(pressed), controller_route)
                .unwrap()
                .primary
        );
        let cancellation = prepare_delivery(&mut map, Some(pressed), desktop).unwrap();
        assert!(cancellation.cancelled);
        assert!(cancellation.primary_released);
        assert_eq!(cancellation.uv, UV);
        assert_eq!(prepare_delivery(&mut map, Some(pressed), desktop), None);
    }

    #[test]
    fn controller_route_preserves_neutral_handoff_after_desktop_release() {
        let mut map = MapController::new();
        let route = RoutedPointer {
            controller: true,
            uv: Some((UV.x, UV.y)),
            buttons: 0,
        };
        let released = sample(Hand::Right, UV, false, false, 0.0);
        assert!(prepare_delivery(&mut map, Some(released), route).is_some());
        let pressed = sample(Hand::Right, UV, true, false, 0.0);
        assert!(
            prepare_delivery(&mut map, Some(pressed), route)
                .unwrap()
                .primary
        );

        let desktop = RoutedPointer {
            controller: false,
            uv: None,
            buttons: 0,
        };
        let cancellation = prepare_delivery(&mut map, Some(pressed), desktop).unwrap();
        assert!(cancellation.cancelled);
        assert!(cancellation.primary_released);
        assert_eq!(cancellation.uv, UV);
        assert_eq!(prepare_delivery(&mut map, Some(pressed), desktop), None);

        assert!(prepare_delivery(&mut map, Some(released), route).is_some());
        assert!(
            prepare_delivery(&mut map, Some(pressed), route)
                .unwrap()
                .primary
        );
    }

    #[test]
    fn invalid_uv_values_cancel_without_panicking() {
        let mut map = MapController::new();
        assert!(
            map.step(Some(sample(Hand::Right, Vec2::NAN, false, false, 0.0)))
                .is_none()
        );
    }

    #[test]
    fn synthetic_slots_select_only_map_actions_and_the_live_mouse_device() {
        let delivery = MapDelivery {
            uv: UV,
            primary: true,
            secondary: false,
            drag_delta: Vec2::new(-0.25, 0.5),
            zoom: -1.0,
            cancelled: false,
            primary_pressed: true,
            primary_released: false,
            secondary_pressed: false,
            secondary_released: false,
        };
        let lookup = SyntheticLookup::new(std::ptr::dangling_mut(), 0x20002, delivery);
        assert!(lookup.slot(Action::MOUSE1 as i32, 0x20002).is_some());
        assert!(lookup.slot(Action::MOUSE1 as i32, 0x20003).is_none());
        assert!(lookup.slot(Action::GUI_ZOOM_OUT as i32, -1).is_some());
        assert!(lookup.slot(Action::GUI_ZOOM_OUT as i32, 0x20003).is_none());
        assert!(lookup.slot(Action::GUI_OK as i32, 0x20002).is_none());
    }

    #[test]
    fn synthetic_button_edges_preserve_native_click_press_release_states() {
        assert_eq!(button_state(true, true, false), EffectorState::Clicked);
        assert_eq!(button_state(true, false, false), EffectorState::Pressed);
        assert_eq!(button_state(false, false, true), EffectorState::Released);
        assert_eq!(button_state(false, false, false), EffectorState::Idle);
    }

    #[test]
    fn synthetic_zoom_slots_keep_native_direction_and_zero_edges() {
        let delivery = MapDelivery {
            uv: UV,
            primary: false,
            secondary: false,
            drag_delta: Vec2::ZERO,
            zoom: 2.0,
            cancelled: false,
            primary_pressed: false,
            primary_released: false,
            secondary_pressed: false,
            secondary_released: false,
        };
        let lookup = SyntheticLookup::new(std::ptr::dangling_mut(), 3, delivery);
        let zoom_in = unsafe {
            lookup
                .slot(Action::GUI_ZOOM_IN as i32, -1)
                .unwrap()
                .as_ref()
        };
        let zoom_out = unsafe {
            lookup
                .slot(Action::GUI_ZOOM_OUT as i32, -1)
                .unwrap()
                .as_ref()
        };
        assert_eq!(zoom_in.map(|slot| slot.m_Value), Some(1.0));
        assert_eq!(
            zoom_in.map(|slot| slot.m_State),
            Some(EffectorState::Clicked)
        );
        assert_eq!(zoom_out.map(|slot| slot.m_Value), Some(0.0));
        assert_eq!(zoom_out.map(|slot| slot.m_State), Some(EffectorState::Idle));

        let delivery = MapDelivery {
            drag_delta: Vec2::new(-0.25, -0.5),
            zoom: 0.0,
            ..delivery
        };
        let lookup = SyntheticLookup::new(std::ptr::dangling_mut(), 3, delivery);
        let up = unsafe {
            lookup
                .slot(Action::GUI_RSTICK_UP as i32, 3)
                .unwrap()
                .as_ref()
        };
        let down = unsafe {
            lookup
                .slot(Action::GUI_RSTICK_DOWN as i32, 3)
                .unwrap()
                .as_ref()
        };
        assert_eq!(up.map(|slot| slot.m_Value), Some(0.5));
        assert_eq!(down.map(|slot| slot.m_Value), Some(0.0));

        let delivery = MapDelivery {
            drag_delta: Vec2::new(-0.25, 0.5),
            ..delivery
        };
        let lookup = SyntheticLookup::new(std::ptr::dangling_mut(), 3, delivery);
        let up = unsafe {
            lookup
                .slot(Action::GUI_RSTICK_UP as i32, 3)
                .unwrap()
                .as_ref()
        };
        let down = unsafe {
            lookup
                .slot(Action::GUI_RSTICK_DOWN as i32, 3)
                .unwrap()
                .as_ref()
        };
        assert_eq!(up.map(|slot| slot.m_Value), Some(0.0));
        assert_eq!(down.map(|slot| slot.m_Value), Some(0.5));
    }

    #[test]
    fn null_mouse_device_does_not_install_a_synthetic_scope() {
        let action_map = std::ptr::dangling_mut();
        let delivery = MapDelivery::neutral(UV, false, false, false);
        assert!(SyntheticLookupScope::enter_for_device(action_map, None, delivery).is_none());
        assert!(synthetic_effector(action_map, Action::MOUSE1 as i32, 1).is_none());
    }

    #[test]
    fn gamepad_mode_restoration_preserves_both_prior_values() {
        let mut flag = true;
        restore_gamepad_mode(&mut flag, false);
        assert!(!flag);
        restore_gamepad_mode(&mut flag, true);
        assert!(flag);
    }

    #[test]
    fn lookup_scope_restores_after_drop() {
        let action_map = std::ptr::dangling_mut();
        let delivery = MapDelivery::neutral(UV, false, false, false);
        assert!(synthetic_effector(action_map, Action::MOUSE1 as i32, 1).is_none());
        SYNTHETIC_LOOKUP.with(|scope| {
            *scope.borrow_mut() = Some(Box::new(SyntheticLookup::new(action_map, 1, delivery)));
        });
        let outer = synthetic_effector(action_map, Action::MOUSE1 as i32, 1).unwrap();
        let outer_value = unsafe { (*outer).m_Value };
        let nested = SyntheticLookupScope::enter_for_device(action_map, Some(2), delivery).unwrap();
        let inner = synthetic_effector(action_map, Action::MOUSE1 as i32, 2).unwrap();
        assert_ne!(outer, inner);
        assert_eq!(unsafe { (*outer).m_Value }, outer_value);
        assert!(synthetic_effector(action_map, Action::MOUSE1 as i32, 2).is_some());
        drop(nested);
        assert!(synthetic_effector(action_map, Action::MOUSE1 as i32, 1).is_some());
        SYNTHETIC_LOOKUP.with(|scope| *scope.borrow_mut() = None);
        assert!(synthetic_effector(action_map, Action::MOUSE1 as i32, 1).is_none());
    }
}
