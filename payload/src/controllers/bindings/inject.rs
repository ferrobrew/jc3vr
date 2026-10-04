//! Applying effector writes to the game's action maps, on the input tick after the device poll.
//!
//! Each write resolves its action through the same lookup the game's readers use (device `-1`),
//! so it lands in the effector the next reader picks: the one with the largest value among the
//! devices that bind the action. Values are only ever raised, so a physical device's reading in the
//! same effector is never reduced. An action no device binds resolves to the null effector, and
//! the write is skipped and reported. The writes that undo earlier ones (releases, undone clicks, and
//! the return to idle) resolve through the lookup that ignores the player map's blockers, so that a
//! map blocked since the write was made cannot hide the effector it landed on.

use jc3gi::input::input_action_map::{
    EffectorState, InputActionMap, InputDeviceEffector, LocalPlayerActionMap, get_null_effector,
    get_ui_action_map,
};

use crate::controllers::bindings::hold::{ActionWrite, EffectorId, MapKind, Phase};

/// What became of one write.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    /// Skipped: the effector shows a physical press that a release would cut short.
    KeptPhysicalPress,
    /// Skipped: the device poll already moved the clicked effector on, or the lookup now picks
    /// another effector.
    AlreadyMovedOn,
    /// The map is absent, blocked, or binds the action to no device.
    Unresolved,
}

/// What became of one write, and the effector it resolved to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Landing {
    pub outcome: Outcome,
    /// The effector the write resolved to, or `None` when it was unresolved.
    pub effector: Option<EffectorId>,
}

/// Apply `writes` in order.
///
/// # Safety
///
/// Must run on the main thread, on the input tick after `InputDeviceManager::Update`, while the
/// action maps exist.
pub unsafe fn apply(writes: &[ActionWrite]) -> Vec<Landing> {
    // SAFETY: the null effector is a static the lookups return by address.
    let null: *mut InputDeviceEffector = unsafe { get_null_effector() };
    writes
        .iter()
        .map(|write| {
            let effector = unsafe { resolve(write) };
            match unsafe { effector.as_mut() } {
                Some(effector) if !std::ptr::eq(effector, null) => {
                    let id = EffectorId::from_address(std::ptr::from_mut(effector).addr());
                    Landing {
                        outcome: apply_one(effector, id, write),
                        effector: Some(id),
                    }
                }
                _ => Landing {
                    outcome: Outcome::Unresolved,
                    effector: None,
                },
            }
        })
        .collect()
}

/// Resolve a write's effector: null when the map is absent.
unsafe fn resolve(write: &ActionWrite) -> *mut InputDeviceEffector {
    let action = write.target.action;
    let undoes = matches!(
        write.phase,
        Phase::Released { .. } | Phase::Unclick { .. } | Phase::Neutral
    );
    unsafe {
        match write.target.map {
            MapKind::Player if !undoes => LocalPlayerActionMap::get()
                .map_or(std::ptr::null_mut(), |map| map.GetActionEffector(action)),
            MapKind::Player | MapKind::PlayerThroughBlockers => {
                LocalPlayerActionMap::get().map_or(std::ptr::null_mut(), |map| {
                    map.as_input_action_map()
                        .GetActionEffectorIgnoringBlockers(action, ANY_DEVICE, true)
                })
            }
            MapKind::Ui => get_ui_action_map()
                .as_mut()
                .map_or(std::ptr::null_mut(), |map: &mut InputActionMap| {
                    map.GetActionEffectorForAction(action)
                }),
        }
    }
}

/// The device index that makes the lookup pick among every device's effector instead of
/// allocating one for a specific device.
const ANY_DEVICE: i32 = -1;

fn apply_one(effector: &mut InputDeviceEffector, id: EffectorId, write: &ActionWrite) -> Outcome {
    let raise = |effector: &mut InputDeviceEffector| {
        effector.m_Value = effector.m_Value.max(write.value);
    };
    match write.phase {
        Phase::Value => raise(effector),
        Phase::Clicked => {
            raise(effector);
            effector.m_PrevValue = 0.0;
            effector.m_State = EffectorState::Clicked;
            effector.m_StateTime = 0.0;
        }
        Phase::Held { seconds } => {
            raise(effector);
            effector.m_State = EffectorState::Pressed;
            effector.m_StateTime = seconds;
        }
        Phase::Released { seconds } => {
            // The poll leaves an effector pressed only when its device reads a press. Both states
            // are pressed states; a physical click must not be cut short by our release write.
            if matches!(
                effector.m_State,
                EffectorState::Clicked | EffectorState::Pressed
            ) && effector.m_Value > 0.0
            {
                return Outcome::KeptPhysicalPress;
            }
            effector.m_Value = 0.0;
            effector.m_State = EffectorState::Released;
            effector.m_StateTime = seconds;
        }
        Phase::Unclick { effector: clicked } => {
            // The poll moves a clicked effector to pressed or released on every run, so one still
            // clicked since our click is one the poll skipped: left alone, it reads as a fresh click
            // again. It is released as the poll would have released it. The physical-press guard
            // does not apply: the value in it is the one our click wrote.
            if id != clicked || effector.m_State != EffectorState::Clicked {
                return Outcome::AlreadyMovedOn;
            }
            effector.m_Value = 0.0;
            effector.m_PrevValue = 0.0;
            effector.m_State = EffectorState::Released;
            effector.m_StateTime = 0.0;
        }
        Phase::Neutral => {
            // With the poll disabled nothing moves an effector on, so a released state would read
            // as a release on every frame; idle reads as no input. No physical press is cut short:
            // the poll is not reading the devices.
            effector.m_Value = 0.0;
            effector.m_PrevValue = 0.0;
            effector.m_State = EffectorState::Idle;
            effector.m_StateTime = 0.0;
        }
    }
    Outcome::Applied
}

#[cfg(test)]
mod tests {
    use jc3gi::input::input_action_map::Action;

    use super::*;
    use crate::controllers::bindings::hold::Target;

    fn effector(value: f32, state: EffectorState, time: f32) -> InputDeviceEffector {
        // SAFETY: the effector is plain data; all-zero is a valid idle effector.
        let mut e: InputDeviceEffector = unsafe { std::mem::zeroed() };
        e.m_Value = value;
        e.m_State = state;
        e.m_StateTime = time;
        e
    }

    const ID: EffectorId = EffectorId::from_address(0x1000);

    fn write(value: f32, phase: Phase) -> ActionWrite {
        ActionWrite {
            target: Target::player(Action::FIRE_GRAPPLE),
            value,
            phase,
        }
    }

    #[test]
    fn a_held_write_sets_the_hold_time() {
        let mut e = effector(0.0, EffectorState::Released, 0.9);
        apply_one(&mut e, ID, &write(1.0, Phase::Held { seconds: 0.4 }));
        assert_eq!(e.m_State, EffectorState::Pressed);
        assert_eq!(e.m_StateTime, 0.4);
        assert_eq!(e.m_Value, 1.0);
    }

    #[test]
    fn writes_never_lower_a_physical_value() {
        let mut e = effector(0.9, EffectorState::Pressed, 0.1);
        apply_one(&mut e, ID, &write(0.3, Phase::Value));
        assert_eq!(e.m_Value, 0.9);
    }

    #[test]
    fn a_click_zeroes_the_time() {
        let mut e = effector(0.0, EffectorState::Idle2, 5.0);
        apply_one(&mut e, ID, &write(1.0, Phase::Clicked));
        assert_eq!(e.m_State, EffectorState::Clicked);
        assert_eq!(e.m_StateTime, 0.0);
    }

    #[test]
    fn a_release_keeps_a_physical_press() {
        for state in [EffectorState::Clicked, EffectorState::Pressed] {
            let mut e = effector(1.0, state, 0.2);
            let outcome = apply_one(&mut e, ID, &write(0.0, Phase::Released { seconds: 0.5 }));
            assert_eq!(outcome, Outcome::KeptPhysicalPress);
            assert_eq!(e.m_State, state);
        }

        let mut e = effector(0.0, EffectorState::Released, 0.2);
        apply_one(&mut e, ID, &write(0.0, Phase::Released { seconds: 0.5 }));
        assert_eq!(e.m_State, EffectorState::Released);
        assert_eq!(e.m_StateTime, 0.5);
    }

    #[test]
    fn an_unclick_releases_a_click_the_poll_skipped() {
        let mut e = effector(1.0, EffectorState::Clicked, 0.0);
        let outcome = apply_one(&mut e, ID, &write(0.0, Phase::Unclick { effector: ID }));
        assert_eq!(outcome, Outcome::Applied);
        assert_eq!(e.m_State, EffectorState::Released);
        assert_eq!(e.m_Value, 0.0);
        assert_eq!(e.m_StateTime, 0.0);
    }

    #[test]
    fn an_unclick_leaves_an_effector_the_poll_moved_on() {
        for state in [
            EffectorState::Pressed,
            EffectorState::Released,
            EffectorState::Idle,
        ] {
            let mut e = effector(1.0, state, 0.3);
            let outcome = apply_one(&mut e, ID, &write(0.0, Phase::Unclick { effector: ID }));
            assert_eq!(outcome, Outcome::AlreadyMovedOn);
            assert_eq!(e.m_State, state);
            assert_eq!(e.m_Value, 1.0);
        }
    }

    #[test]
    fn an_unclick_leaves_another_effector_clicked() {
        let mut e = effector(1.0, EffectorState::Clicked, 0.4);
        let other = EffectorId::from_address(0x2000);
        let outcome = apply_one(&mut e, other, &write(0.0, Phase::Unclick { effector: ID }));
        assert_eq!(outcome, Outcome::AlreadyMovedOn);
        assert_eq!(e.m_State, EffectorState::Clicked);
    }

    #[test]
    fn neutral_idles_even_a_pressed_effector() {
        for state in [EffectorState::Clicked, EffectorState::Pressed] {
            let mut e = effector(0.8, state, 2.0);
            let outcome = apply_one(&mut e, ID, &write(0.0, Phase::Neutral));
            assert_eq!(outcome, Outcome::Applied);
            assert_eq!(e.m_State, EffectorState::Idle);
            assert_eq!(e.m_Value, 0.0);
            assert_eq!(e.m_PrevValue, 0.0);
            assert_eq!(e.m_StateTime, 0.0);
        }
    }
}
