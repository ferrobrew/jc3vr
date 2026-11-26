//! The scoped action-lookup shim for the native map: while the map handles a controller dispatch,
//! the effector lookups it makes on its steering's action map return effectors owned here, so the
//! map reads the controller's buttons and zoom without any physical action-map slot being written.
//!
//! The shim also watches the map's own read of the confirm action. The map's mouse branch opens
//! the context menu for a clicked confirm at the last selection point, which only a mouse release
//! moves, so a confirm alone would open the menu wherever the last click was (at first, the top
//! left of the stage). When the map reads a clicked confirm, the shim reports the secondary mouse
//! button released in the same dispatch, which is the mouse's own context action: it selects at the
//! cursor and opens the menu there, as the gamepad's confirm does at its own cursor.

use std::{
    cell::{Cell, RefCell, UnsafeCell},
    ptr,
};

use jc3gi::input::{
    input_action_map::{Action, EffectorState, InputActionMap, InputDeviceEffector},
    input_device_manager::{DeviceOrder, DeviceType, InputDeviceManager},
    steering::Steering,
};

use crate::controllers::map::controller::{MapDelivery, ZoomStep};

/// Installs the shim for the current thread until dropped, restoring whatever was installed
/// before.
pub(crate) struct SyntheticLookupScope {
    previous: Option<Box<SyntheticLookup>>,
}

impl SyntheticLookupScope {
    /// Install the shim for `steering`'s action map. The mouse-scoped actions answer only for the
    /// first mouse device, which is the device the map reads them from, and the zoom actions answer
    /// only for the steering's own device selector, which is how its readers look them up. `None`
    /// without a mouse device.
    pub(crate) fn enter(steering: &Steering, delivery: MapDelivery) -> Option<Self> {
        // SAFETY: the device manager is a live singleton while the UI dispatches input.
        let mouse_device_index = unsafe {
            let manager = InputDeviceManager::get()?;
            let device = manager.GetDevice(DeviceType::Mouse, 0, DeviceOrder::Appearance);
            device.as_ref()?.ID.action_device_index()
        };
        Some(Self::install(SyntheticLookup::new(
            steering.ActionMap,
            DeviceSelectors {
                mouse: mouse_device_index,
                steering: steering.m_DeviceIndex,
            },
            delivery,
        )))
    }

    /// Whether the map read a clicked confirm under this scope, which the shim answered with the
    /// context action at the cursor.
    pub(crate) fn context_from_confirm(&self) -> bool {
        SYNTHETIC_LOOKUP.with(|scope| {
            scope
                .borrow()
                .as_ref()
                .is_some_and(|lookup| lookup.context_from_confirm.get())
        })
    }

    fn install(lookup: SyntheticLookup) -> Self {
        let previous = SYNTHETIC_LOOKUP.with(|scope| scope.borrow_mut().replace(Box::new(lookup)));
        Self { previous }
    }
}

impl Drop for SyntheticLookupScope {
    fn drop(&mut self) {
        SYNTHETIC_LOOKUP.with(|scope| {
            *scope.borrow_mut() = self.previous.take();
        });
    }
}

/// The shim's effector for `action_id` on `device_index` of `action_map`, when a scope is
/// installed on this thread and covers that lookup.
pub(crate) fn synthetic_effector(
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

/// Watch an effector the engine's lookup returned for `action_id` on `device_index` of
/// `action_map`. A clicked confirm read by the map through its steering, under a scope that
/// continues the controller's ownership, turns the shim's secondary button into a release (see the
/// module documentation).
///
/// # Safety
///
/// `effector` must be null or the engine's live effector for this lookup.
pub(crate) unsafe fn observe_effector(
    action_map: *mut InputActionMap,
    action_id: i32,
    device_index: i32,
    effector: *const InputDeviceEffector,
) {
    // Every input read in the game passes through here; most return before the thread-local.
    if effector.is_null() || action_id != Action::GUI_OK as i32 {
        return;
    }
    SYNTHETIC_LOOKUP.with(|scope| {
        let scope = scope.borrow();
        let Some(lookup) = scope
            .as_ref()
            .filter(|lookup| lookup.action_map == action_map)
        else {
            return;
        };
        // SAFETY: the caller guarantees a live effector. The state is read as its raw integer so
        // that a value outside the enum is not a Rust enum read.
        let state = unsafe { ptr::addr_of!((*effector).m_State).cast::<i32>().read() };
        lookup.observe(action_id, device_index, state);
    });
}

/// The device selectors the shim answers for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct DeviceSelectors {
    /// The first mouse device's selector, for the mouse-scoped actions.
    mouse: i32,
    /// The steering's own selector, for the actions its readers resolve.
    steering: i32,
}

/// The actions the map reads from the mouse device in its mouse branch: the buttons and the delta
/// axes it uses to start a drag.
const MOUSE_ACTIONS: [Action; 6] = [
    Action::MOUSE1,
    Action::MOUSE2,
    Action::GUI_RSTICK_UP,
    Action::GUI_RSTICK_DOWN,
    Action::GUI_RSTICK_LEFT,
    Action::GUI_RSTICK_RIGHT,
];

/// The actions the map reads through its steering in its mouse branch.
const STEERING_ACTIONS: [Action; 2] = [Action::GUI_ZOOM_IN, Action::GUI_ZOOM_OUT];

const SLOT_COUNT: usize = MOUSE_ACTIONS.len() + STEERING_ACTIONS.len();

struct SyntheticLookup {
    action_map: *mut InputActionMap,
    devices: DeviceSelectors,
    slots: Box<[UnsafeCell<InputDeviceEffector>; SLOT_COUNT]>,
    /// Whether a clicked confirm may become the context action: the delivery continues the
    /// controller's ownership rather than cancelling it.
    confirm_selects: bool,
    /// Whether it did, in this scope.
    context_from_confirm: Cell<bool>,
}

impl SyntheticLookup {
    fn new(
        action_map: *mut InputActionMap,
        devices: DeviceSelectors,
        delivery: MapDelivery,
    ) -> Self {
        let slots = std::array::from_fn(|index| {
            let (value, state) = match slot_action(index) {
                Action::MOUSE1 => button(
                    delivery.primary,
                    delivery.primary_pressed,
                    delivery.primary_released,
                ),
                Action::MOUSE2 => button(
                    delivery.secondary,
                    delivery.secondary_pressed,
                    delivery.secondary_released,
                ),
                Action::GUI_ZOOM_IN => click(delivery.zoom == ZoomStep::In),
                Action::GUI_ZOOM_OUT => click(delivery.zoom == ZoomStep::Out),
                // The delta axes stay at rest: the map starts a drag once the cursor has moved far
                // enough from the press, and holding them at rest keeps the physical mouse from
                // starting one while the controller drives the map.
                _ => (0.0, EffectorState::Idle),
            };
            UnsafeCell::new(InputDeviceEffector {
                m_Value: value,
                m_PrevValue: value,
                m_State: state,
                m_IsAnalogue: false,
                m_IsDeltaBased: false,
                m_IsUpdated: true,
                m_ForceClick: false,
                m_StateTime: 0.0,
            })
        });
        Self {
            action_map,
            devices,
            slots: Box::new(slots),
            confirm_selects: !delivery.cancelled,
            context_from_confirm: Cell::new(false),
        }
    }

    /// See [`observe_effector`]. Returns whether the secondary button became a release.
    fn observe(&self, action_id: i32, device_index: i32, state: i32) -> bool {
        if !self.confirm_selects
            || action_id != Action::GUI_OK as i32
            || device_index != self.devices.steering
            || state != EffectorState::Clicked as i32
        {
            return false;
        }
        let slot = self.slots[secondary_slot()].get();
        // SAFETY: the slot lives in this lookup; the map holds no reference into it, only the raw
        // pointer an earlier lookup returned, and it runs on this thread.
        let idle = unsafe { matches!((*slot).m_State, EffectorState::Idle) };
        if idle {
            // SAFETY: as above.
            unsafe { (*slot).m_State = EffectorState::Released };
            self.context_from_confirm.set(true);
        }
        idle
    }

    fn slot(&self, action_id: i32, device_index: i32) -> Option<*mut InputDeviceEffector> {
        let (index, selector) = MOUSE_ACTIONS
            .iter()
            .position(|action| *action as i32 == action_id)
            .map(|index| (index, self.devices.mouse))
            .or_else(|| {
                STEERING_ACTIONS
                    .iter()
                    .position(|action| *action as i32 == action_id)
                    .map(|index| (MOUSE_ACTIONS.len() + index, self.devices.steering))
            })?;
        (device_index == selector).then(|| self.slots[index].get())
    }
}

fn secondary_slot() -> usize {
    MOUSE_ACTIONS
        .iter()
        .position(|action| *action == Action::MOUSE2)
        .expect("the secondary button is a mouse action")
}

fn slot_action(index: usize) -> Action {
    MOUSE_ACTIONS
        .get(index)
        .or_else(|| STEERING_ACTIONS.get(index - MOUSE_ACTIONS.len()))
        .copied()
        .expect("every slot has an action")
}

fn button(held: bool, pressed: bool, released: bool) -> (f32, EffectorState) {
    (f32::from(held), button_state(held, pressed, released))
}

fn click(clicked: bool) -> (f32, EffectorState) {
    if clicked {
        (1.0, EffectorState::Clicked)
    } else {
        (0.0, EffectorState::Idle)
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

#[cfg(test)]
mod tests {
    use std::ptr;

    use glam::Vec2;

    use super::*;

    const DEVICES: DeviceSelectors = DeviceSelectors {
        mouse: 0x20002,
        steering: 7,
    };

    fn delivery(zoom: ZoomStep) -> MapDelivery {
        MapDelivery {
            uv: Vec2::new(0.25, 0.5),
            held_at_press: true,
            primary: true,
            secondary: false,
            zoom,
            cancelled: false,
            primary_pressed: true,
            primary_released: false,
            secondary_pressed: false,
            secondary_released: false,
        }
    }

    fn read(lookup: &SyntheticLookup, action: Action, device: i32) -> Option<(f32, EffectorState)> {
        // SAFETY: the slot points into the lookup, which outlives the read.
        lookup
            .slot(action as i32, device)
            .map(|slot| unsafe { ((*slot).m_Value, (*slot).m_State) })
    }

    #[test]
    fn synthetic_slots_select_only_map_actions_and_their_devices() {
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, delivery(ZoomStep::Out));
        assert!(lookup.slot(Action::MOUSE1 as i32, DEVICES.mouse).is_some());
        assert!(
            lookup
                .slot(Action::MOUSE1 as i32, DEVICES.steering)
                .is_none()
        );
        assert!(
            lookup
                .slot(Action::GUI_ZOOM_OUT as i32, DEVICES.steering)
                .is_some()
        );
        assert!(
            lookup
                .slot(Action::GUI_ZOOM_OUT as i32, DEVICES.mouse)
                .is_none()
        );
        assert!(lookup.slot(Action::GUI_ZOOM_OUT as i32, -1).is_none());
        assert!(lookup.slot(Action::GUI_OK as i32, DEVICES.mouse).is_none());
    }

    #[test]
    fn synthetic_button_edges_preserve_native_click_press_release_states() {
        assert_eq!(button_state(true, true, false), EffectorState::Clicked);
        assert_eq!(button_state(true, false, false), EffectorState::Pressed);
        assert_eq!(button_state(false, false, true), EffectorState::Released);
        assert_eq!(button_state(false, false, false), EffectorState::Idle);
    }

    #[test]
    fn zoom_slots_click_only_in_their_direction_and_the_axes_rest() {
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, delivery(ZoomStep::In));
        assert_eq!(
            read(&lookup, Action::GUI_ZOOM_IN, DEVICES.steering),
            Some((1.0, EffectorState::Clicked))
        );
        assert_eq!(
            read(&lookup, Action::GUI_ZOOM_OUT, DEVICES.steering),
            Some((0.0, EffectorState::Idle))
        );
        assert_eq!(
            read(&lookup, Action::MOUSE1, DEVICES.mouse),
            Some((1.0, EffectorState::Clicked))
        );
        for axis in &MOUSE_ACTIONS[2..] {
            assert_eq!(
                read(&lookup, *axis, DEVICES.mouse),
                Some((0.0, EffectorState::Idle))
            );
        }
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, delivery(ZoomStep::None));
        assert_eq!(
            read(&lookup, Action::GUI_ZOOM_IN, DEVICES.steering),
            Some((0.0, EffectorState::Idle))
        );
    }

    const CLICKED: i32 = EffectorState::Clicked as i32;

    #[test]
    fn a_clicked_confirm_releases_the_secondary_button_once() {
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, delivery(ZoomStep::None));
        assert_eq!(
            read(&lookup, Action::MOUSE2, DEVICES.mouse),
            Some((0.0, EffectorState::Idle))
        );
        assert!(lookup.observe(Action::GUI_OK as i32, DEVICES.steering, CLICKED));
        assert!(lookup.context_from_confirm.get());
        assert_eq!(
            read(&lookup, Action::MOUSE2, DEVICES.mouse),
            Some((0.0, EffectorState::Released))
        );
        assert!(!lookup.observe(Action::GUI_OK as i32, DEVICES.steering, CLICKED));
        assert_eq!(
            read(&lookup, Action::MOUSE1, DEVICES.mouse),
            Some((1.0, EffectorState::Clicked)),
            "the primary button is untouched"
        );
    }

    #[test]
    fn only_a_clicked_confirm_on_the_steering_device_is_observed() {
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, delivery(ZoomStep::None));
        for state in [
            EffectorState::Idle,
            EffectorState::Pressed,
            EffectorState::Released,
            EffectorState::Frozen,
        ] {
            assert!(!lookup.observe(Action::GUI_OK as i32, DEVICES.steering, state as i32));
        }
        assert!(!lookup.observe(Action::GUI_OK as i32, DEVICES.mouse, CLICKED));
        assert!(!lookup.observe(Action::GUI_CANCEL as i32, DEVICES.steering, CLICKED));
        assert!(!lookup.observe(Action::GUI_OK as i32, DEVICES.steering, -1));
        assert!(!lookup.context_from_confirm.get());
    }

    #[test]
    fn a_cancelling_dispatch_never_turns_a_confirm_into_a_selection() {
        let cancelled = MapDelivery {
            cancelled: true,
            ..delivery(ZoomStep::None)
        };
        let lookup = SyntheticLookup::new(ptr::dangling_mut(), DEVICES, cancelled);
        assert!(!lookup.observe(Action::GUI_OK as i32, DEVICES.steering, CLICKED));
        assert_eq!(
            read(&lookup, Action::MOUSE2, DEVICES.mouse),
            Some((0.0, EffectorState::Idle))
        );
    }

    #[test]
    fn observing_an_engine_effector_reaches_the_installed_scope() {
        let action_map = ptr::dangling_mut();
        let confirm = InputDeviceEffector {
            m_Value: 1.0,
            m_PrevValue: 0.0,
            m_State: EffectorState::Clicked,
            m_IsAnalogue: false,
            m_IsDeltaBased: false,
            m_IsUpdated: true,
            m_ForceClick: false,
            m_StateTime: 0.0,
        };
        let scope = SyntheticLookupScope::install(SyntheticLookup::new(
            action_map,
            DEVICES,
            delivery(ZoomStep::None),
        ));
        // SAFETY: the effector is a live local.
        unsafe {
            observe_effector(
                ptr::without_provenance_mut(0x1000),
                Action::GUI_OK as i32,
                DEVICES.steering,
                &confirm,
            );
            observe_effector(
                action_map,
                Action::GUI_OK as i32,
                DEVICES.steering,
                ptr::null(),
            );
        }
        assert!(
            !scope.context_from_confirm(),
            "another action map is ignored"
        );
        // SAFETY: as above.
        unsafe {
            observe_effector(
                action_map,
                Action::GUI_OK as i32,
                DEVICES.steering,
                &confirm,
            );
        }
        assert!(scope.context_from_confirm());
    }

    #[test]
    fn lookup_scope_restores_after_drop() {
        let action_map = ptr::dangling_mut();
        let mouse = |device| synthetic_effector(action_map, Action::MOUSE1 as i32, device);
        assert!(mouse(DEVICES.mouse).is_none());
        let outer = SyntheticLookupScope::install(SyntheticLookup::new(
            action_map,
            DEVICES,
            delivery(ZoomStep::None),
        ));
        let outer_slot = mouse(DEVICES.mouse).unwrap();
        let inner_devices = DeviceSelectors {
            mouse: 3,
            ..DEVICES
        };
        let inner = SyntheticLookupScope::install(SyntheticLookup::new(
            action_map,
            inner_devices,
            delivery(ZoomStep::None),
        ));
        assert!(mouse(DEVICES.mouse).is_none());
        assert_ne!(mouse(3).unwrap(), outer_slot);
        drop(inner);
        assert_eq!(mouse(DEVICES.mouse), Some(outer_slot));
        drop(outer);
        assert!(mouse(DEVICES.mouse).is_none());
    }
}
