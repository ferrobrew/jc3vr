//! The `InputDeviceManager::Update` detour: read and consume the game's own look effectors so the
//! headpose simulation owns the mouse-look deltas.
//!
//! The game polls devices via `InputDeviceManager::Update(dt)`, which populates `InputDeviceEffector`
//! slots including `LOOK_LEFT` / `LOOK_RIGHT` / `LOOK_UP` / `LOOK_DOWN`. These are the mouse-look
//! deltas the game's camera system reads. We intercept them post-call: read the values, clear the
//! effectors so the game's camera doesn't respond, and feed the deltas into the headpose simulation.
//! The same seam is where [`crate::hooks::input::swim`] refuses backward movement in water, and
//! where the motion controllers detect the mode and inject their bindings
//! ([`crate::controllers::bindings::input_tick`]): after the poll, so the injected state survives
//! until the game reads it, and before the swim refusal, so it covers the controllers' input too.
//! The same tick holds the game's active-input-device setting at the gamepad under the controller
//! scheme ([`crate::controllers::gamepad_setting`]).
//! The controllers' right stick turns the body through the same VR turn scheme as the look
//! effectors, and the bindings are told whether this call polled the devices, since they stand down
//! while it does not.

use detours_macro::detour;
use jc3gi::input::{
    input_action_map::{Action, EffectorState, LocalPlayerActionMap},
    input_device_manager::InputDeviceManager,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{self, bindings::hold::DevicePoll},
    headpose,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&INPUT_DEVICE_MANAGER_UPDATE_BINDER)
}

#[detour(address = jc3gi::input::input_device_manager::InputDeviceManager::Update_ADDRESS)]
fn input_device_manager_update(manager: *mut InputDeviceManager, dt: f32) {
    // Read before the call: `InputDeviceManager::m_Enabled` decides whether this call polls the
    // devices, and egui's input capture may flip it from the window thread.
    // SAFETY: the manager is the live singleton the game passes to its own update.
    let poll = match unsafe { manager.as_ref() } {
        Some(manager) if manager.m_Enabled => DevicePoll::Running,
        _ => DevicePoll::Disabled,
    };
    INPUT_DEVICE_MANAGER_UPDATE.get().unwrap().call(manager, dt);

    let controllers = controllers::bindings::input_tick(dt, poll);
    controllers::gamepad_setting::input_tick();
    controllers::physical_hands::input_tick();

    if !headpose::is_active() {
        return;
    }
    // When egui captures input, publish a zero-delta tick — the game's input is already disabled,
    // but the sim's tick cadence (mode detection, pose-pair rotation) must keep running.
    if crate::egui_impl::EguiState::get()
        .as_ref()
        .is_some_and(|s| s.is_input_captured())
    {
        headpose::sim::on_input_tick(0.0, 0.0, false, dt);
        return;
    }

    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return;
    };
    unsafe {
        let (mut look_x, mut look_x_delta) =
            read_look_axis(map, Action::LOOK_RIGHT, Action::LOOK_LEFT);
        // A deflected controller stick takes the turn over from the look effectors: the two are
        // different kinds of input (a stick rate against a possible mouse delta), and summing them
        // would scale neither correctly.
        if let Some(turn) = controllers.body_turn
            && turn != 0.0
        {
            look_x = turn;
            look_x_delta = false;
        }
        let (look_y, _) = read_look_axis(map, Action::LOOK_UP, Action::LOOK_DOWN);

        clear_effector(map, Action::LOOK_LEFT);
        clear_effector(map, Action::LOOK_RIGHT);
        clear_effector(map, Action::LOOK_UP);
        clear_effector(map, Action::LOOK_DOWN);
        // Swimming refuses backward input outright; done here so the game's own input never carries
        // it, rather than unpicking its effects downstream.
        crate::hooks::input::swim::suppress_backward_input(map);

        headpose::sim::on_input_tick(look_x, look_y, look_x_delta, dt);
    }
}

/// Read two opposite-direction effectors and return the net value plus whether it is delta-based (a
/// mouse per-tick displacement) rather than an absolute axis (a stick position). Delta-based if either
/// contributing effector is non-zero and delta-based -- the two device kinds must be turned into body
/// yaw differently (see [`headpose::xr::advance_body_yaw`]).
unsafe fn read_look_axis(
    map: &mut LocalPlayerActionMap,
    positive: Action,
    negative: Action,
) -> (f32, bool) {
    let (pos, pos_delta) = unsafe { read_effector_value(map, positive) };
    let (neg, neg_delta) = unsafe { read_effector_value(map, negative) };
    let delta_based = (pos != 0.0 && pos_delta) || (neg != 0.0 && neg_delta);
    (pos - neg, delta_based)
}

/// Read an effector's analog value and its delta-based flag safely (`(0.0, false)` when absent).
unsafe fn read_effector_value(map: &mut LocalPlayerActionMap, action: Action) -> (f32, bool) {
    unsafe {
        map.GetActionEffector(action)
            .as_ref()
            .map(|effector| (effector.m_Value, effector.m_IsDeltaBased))
            .unwrap_or_default()
    }
}

/// Zero the effector's `m_Value` and set `m_State` to `Idle`, so the game's camera system sees no
/// input.
unsafe fn clear_effector(map: &mut LocalPlayerActionMap, action: Action) {
    let effector_ptr = unsafe { map.GetActionEffector(action) };
    if let Some(effector) = unsafe { effector_ptr.as_mut() } {
        effector.m_Value = 0.0;
        effector.m_PrevValue = 0.0;
        effector.m_State = EffectorState::Idle;
    }
}
