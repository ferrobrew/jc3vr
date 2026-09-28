//! The OpenXR action sets and actions: creating them from the [`layout`], suggesting their
//! bindings, attaching them to a session, reading their states, and releasing, re-wrapping, and
//! destroying their handles.
//!
//! The actions are held as raw handles and driven through the instance's function table rather
//! than through the `openxr` crate's `ActionSet` and `Action` wrappers. The wrappers destroy their
//! handle on drop and each holds a reference to the instance, and the crate offers no way to share
//! one set wrapper between actions re-wrapped from raw handles. Re-wrapped actions would each need
//! their own set wrapper, and dropping them would destroy the set once per action; leaking them
//! instead would leak the instance references and stop a genuine teardown from ever destroying the
//! instance. With raw handles the ownership is explicit: [`Actions::destroy`] destroys the sets
//! (and with them their actions), and dropping [`Actions`] without it releases the handles to the
//! stash.

use std::{ffi::c_char, ptr};

use anyhow::Context as _;
use glam::Vec2;
use openxr as xr;
use openxr::sys::{self, Handle as _};

use crate::{
    controllers::frame::{ActionSetKind, Hand},
    vr::input::layout::{
        self, BoolInput, FloatInput, HandsAction, Input, Profile, Vec2Input, binding, hand_path,
    },
};

/// Every action set and action of the layout, as raw handles.
pub(crate) struct Actions {
    hands_set: sys::ActionSet,
    /// Indexed like [`HandsAction::ALL`].
    hands: [sys::Action; 3],
    /// Indexed by [`ActionSetKind::index`].
    modes: Vec<ModeActions>,
    /// The subaction path of each hand, indexed by [`Hand::index`].
    hand_paths: [xr::Path; 2],
}

/// One mode's set: every raw input, indexed like [`BoolInput::ALL`], [`FloatInput::ALL`], and
/// [`Vec2Input::ALL`].
struct ModeActions {
    set: sys::ActionSet,
    bools: Vec<sys::Action>,
    floats: Vec<sys::Action>,
    vec2s: Vec<sys::Action>,
}

/// An input's value and whether the runtime bound it to an active source this sync.
#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct State<T> {
    pub(crate) value: T,
    pub(crate) active: bool,
}

impl Actions {
    /// Create every action set and action on `instance`. A failure destroys whatever was created.
    pub(crate) fn create(instance: &xr::Instance) -> anyhow::Result<Actions> {
        let hand_paths = hand_paths(instance)?;
        let mut created = Vec::new();
        let result = Self::create_into(instance, hand_paths, &mut created);
        if result.is_err() {
            for set in created {
                destroy_set(instance, set);
            }
        }
        result
    }

    /// Re-wrap persisted handles, given in [`layout::stash_order`].
    ///
    /// # Safety
    /// `handles` must be live action set and action handles created on `instance` by a payload with
    /// this layout, in stash order, and nothing else may destroy them.
    pub(crate) unsafe fn rewrap(
        instance: &xr::Instance,
        handles: &[u64],
    ) -> anyhow::Result<Actions> {
        anyhow::ensure!(
            handles.len() == layout::stash_len(),
            "the stash holds {} handles, the layout {}",
            handles.len(),
            layout::stash_len()
        );
        let hand_paths = hand_paths(instance)?;
        let mut rest = handles.iter().copied();
        let hands_set = sys::ActionSet::from_raw(rest.next().unwrap_or(0));
        let hands = [
            action_from(&mut rest),
            action_from(&mut rest),
            action_from(&mut rest),
        ];
        let mut modes = Vec::with_capacity(ActionSetKind::ALL.len());
        for _ in ActionSetKind::ALL {
            let set = sys::ActionSet::from_raw(rest.next().unwrap_or(0));
            let bools = BoolInput::ALL
                .iter()
                .map(|_| action_from(&mut rest))
                .collect();
            let floats = FloatInput::ALL
                .iter()
                .map(|_| action_from(&mut rest))
                .collect();
            let vec2s = Vec2Input::ALL
                .iter()
                .map(|_| action_from(&mut rest))
                .collect();
            modes.push(ModeActions {
                set,
                bools,
                floats,
                vec2s,
            });
        }
        Ok(Actions {
            hands_set,
            hands,
            modes,
            hand_paths,
        })
    }

    /// Suggest the bindings for every profile in [`Profile::ALL`]. A profile the runtime rejects is
    /// logged and skipped, so that one unsupported profile does not cost the others.
    pub(crate) fn suggest_bindings(&self, instance: &xr::Instance) {
        for profile in Profile::ALL {
            match self.suggest_profile(instance, profile) {
                Ok(count) => {
                    tracing::info!(target: "vr", profile = profile.path(), count, "input: suggested bindings");
                }
                Err(e) => {
                    tracing::warn!(target: "vr", "input: {} bindings not suggested: {e:#}", profile.path());
                }
            }
        }
    }

    /// Attach every set to `session`. Possible once per session.
    pub(crate) fn attach(&self, session: &xr::Session<xr::D3D11>) -> anyhow::Result<()> {
        let mut sets = vec![self.hands_set];
        sets.extend(self.modes.iter().map(|m| m.set));
        let info = sys::SessionActionSetsAttachInfo {
            ty: sys::SessionActionSetsAttachInfo::TYPE,
            next: ptr::null(),
            count_action_sets: sets.len() as u32,
            action_sets: sets.as_ptr(),
        };
        // SAFETY: `info` and the array it points to outlive the call.
        let result = unsafe {
            (session.instance().fp().attach_session_action_sets)(session.as_raw(), &info)
        };
        check(result).context("attaching the action sets")?;
        Ok(())
    }

    /// Sync the hands set and the `mode` set. Returns whether the session has input focus; an
    /// unfocused session reports every action inactive.
    pub(crate) fn sync(
        &self,
        session: &xr::Session<xr::D3D11>,
        mode: ActionSetKind,
    ) -> anyhow::Result<bool> {
        let active = [
            sys::ActiveActionSet {
                action_set: self.hands_set,
                subaction_path: xr::Path::NULL,
            },
            sys::ActiveActionSet {
                action_set: self.modes[mode.index()].set,
                subaction_path: xr::Path::NULL,
            },
        ];
        let info = sys::ActionsSyncInfo {
            ty: sys::ActionsSyncInfo::TYPE,
            next: ptr::null(),
            count_active_action_sets: active.len() as u32,
            active_action_sets: active.as_ptr(),
        };
        // SAFETY: `info` and the array it points to outlive the call.
        let result = unsafe { (session.instance().fp().sync_actions)(session.as_raw(), &info) };
        let code = check(result).context("syncing the actions")?;
        Ok(code != sys::Result::SESSION_NOT_FOCUSED)
    }

    pub(crate) fn bool_state(
        &self,
        session: &xr::Session<xr::D3D11>,
        mode: ActionSetKind,
        input: BoolInput,
        hand: Hand,
    ) -> anyhow::Result<State<bool>> {
        let action = self.modes[mode.index()].bools[position(&BoolInput::ALL, input)];
        let info = self.state_info(action, hand);
        let mut out = sys::ActionStateBoolean {
            ty: sys::ActionStateBoolean::TYPE,
            next: ptr::null_mut(),
            current_state: false.into(),
            changed_since_last_sync: false.into(),
            last_change_time: xr::Time::from_nanos(0),
            is_active: false.into(),
        };
        // SAFETY: `info` and `out` are valid for the duration of the call.
        let result = unsafe {
            (session.instance().fp().get_action_state_boolean)(session.as_raw(), &info, &mut out)
        };
        check(result).with_context(|| format!("reading {}", input.name()))?;
        Ok(State {
            value: out.current_state.into(),
            active: out.is_active.into(),
        })
    }

    pub(crate) fn float_state(
        &self,
        session: &xr::Session<xr::D3D11>,
        mode: ActionSetKind,
        input: FloatInput,
        hand: Hand,
    ) -> anyhow::Result<State<f32>> {
        let action = self.modes[mode.index()].floats[position(&FloatInput::ALL, input)];
        let info = self.state_info(action, hand);
        let mut out = sys::ActionStateFloat {
            ty: sys::ActionStateFloat::TYPE,
            next: ptr::null_mut(),
            current_state: 0.0,
            changed_since_last_sync: false.into(),
            last_change_time: xr::Time::from_nanos(0),
            is_active: false.into(),
        };
        // SAFETY: `info` and `out` are valid for the duration of the call.
        let result = unsafe {
            (session.instance().fp().get_action_state_float)(session.as_raw(), &info, &mut out)
        };
        check(result).with_context(|| format!("reading {}", input.name()))?;
        Ok(State {
            value: out.current_state,
            active: out.is_active.into(),
        })
    }

    pub(crate) fn vec2_state(
        &self,
        session: &xr::Session<xr::D3D11>,
        mode: ActionSetKind,
        input: Vec2Input,
        hand: Hand,
    ) -> anyhow::Result<State<Vec2>> {
        let action = self.modes[mode.index()].vec2s[position(&Vec2Input::ALL, input)];
        let info = self.state_info(action, hand);
        let mut out = sys::ActionStateVector2f {
            ty: sys::ActionStateVector2f::TYPE,
            next: ptr::null_mut(),
            current_state: xr::Vector2f::default(),
            changed_since_last_sync: false.into(),
            last_change_time: xr::Time::from_nanos(0),
            is_active: false.into(),
        };
        // SAFETY: `info` and `out` are valid for the duration of the call.
        let result = unsafe {
            (session.instance().fp().get_action_state_vector2f)(session.as_raw(), &info, &mut out)
        };
        check(result).with_context(|| format!("reading {}", input.name()))?;
        Ok(State {
            value: Vec2::new(out.current_state.x, out.current_state.y),
            active: out.is_active.into(),
        })
    }

    /// Whether a pose action of `hand` is bound to an active source.
    pub(crate) fn pose_active(
        &self,
        session: &xr::Session<xr::D3D11>,
        pose: HandsAction,
        hand: Hand,
    ) -> anyhow::Result<bool> {
        let info = self.state_info(self.hands[position(&HandsAction::ALL, pose)], hand);
        let mut out = sys::ActionStatePose {
            ty: sys::ActionStatePose::TYPE,
            next: ptr::null_mut(),
            is_active: false.into(),
        };
        // SAFETY: `info` and `out` are valid for the duration of the call.
        let result = unsafe {
            (session.instance().fp().get_action_state_pose)(session.as_raw(), &info, &mut out)
        };
        check(result).with_context(|| format!("reading {}", pose.name()))?;
        Ok(out.is_active.into())
    }

    /// Create the action space of a pose action for `hand`, at the pose's origin.
    pub(crate) fn create_space(
        &self,
        session: &xr::Session<xr::D3D11>,
        pose: HandsAction,
        hand: Hand,
    ) -> anyhow::Result<xr::Space> {
        let info = sys::ActionSpaceCreateInfo {
            ty: sys::ActionSpaceCreateInfo::TYPE,
            next: ptr::null(),
            action: self.hands[position(&HandsAction::ALL, pose)],
            subaction_path: self.hand_paths[hand.index()],
            pose_in_action_space: xr::Posef::IDENTITY,
        };
        let mut out = sys::Space::NULL;
        // SAFETY: `info` and `out` are valid for the duration of the call.
        let result = unsafe {
            (session.instance().fp().create_action_space)(session.as_raw(), &info, &mut out)
        };
        check(result)
            .with_context(|| format!("creating the {} {} space", hand.label(), pose.name()))?;
        // SAFETY: `out` is a space just created on `session`, owned by nothing else. The wrapper
        // destroys it on drop, and the action it refers to outlives it (see `InputState`).
        Ok(unsafe { xr::Space::reference_from_raw(session.clone(), out) })
    }

    /// Start a vibration on `hand`.
    pub(crate) fn apply_haptic(
        &self,
        session: &xr::Session<xr::D3D11>,
        hand: Hand,
        vibration: &sys::HapticVibration,
    ) -> anyhow::Result<()> {
        let info = sys::HapticActionInfo {
            ty: sys::HapticActionInfo::TYPE,
            next: ptr::null(),
            action: self.hands[position(&HandsAction::ALL, HandsAction::Haptic)],
            subaction_path: self.hand_paths[hand.index()],
        };
        // SAFETY: `info` and `vibration` are valid for the duration of the call; a vibration
        // begins with the haptic base header.
        let result = unsafe {
            (session.instance().fp().apply_haptic_feedback)(
                session.as_raw(),
                &info,
                (vibration as *const sys::HapticVibration).cast(),
            )
        };
        check(result).context("applying haptic feedback")?;
        Ok(())
    }

    /// The subaction path of `hand`.
    pub(crate) fn hand_path(&self, hand: Hand) -> xr::Path {
        self.hand_paths[hand.index()]
    }

    /// The raw handles in [`layout::stash_order`].
    pub(crate) fn raw_handles(&self) -> Vec<u64> {
        let mut handles = vec![self.hands_set.into_raw()];
        handles.extend(self.hands.iter().map(|a| a.into_raw()));
        for mode in &self.modes {
            handles.push(mode.set.into_raw());
            handles.extend(mode.bools.iter().map(|a| a.into_raw()));
            handles.extend(mode.floats.iter().map(|a| a.into_raw()));
            handles.extend(mode.vec2s.iter().map(|a| a.into_raw()));
        }
        handles
    }

    /// Destroy every set, and with it its actions. Every action space must be dropped first.
    pub(crate) fn destroy(self, instance: &xr::Instance) {
        destroy_set(instance, self.hands_set);
        for mode in self.modes {
            destroy_set(instance, mode.set);
        }
    }

    fn create_into(
        instance: &xr::Instance,
        hand_paths: [xr::Path; 2],
        created: &mut Vec<sys::ActionSet>,
    ) -> anyhow::Result<Actions> {
        let hands_set = create_set(
            instance,
            layout::HANDS_SET_NAME,
            layout::HANDS_SET_LOCALIZED_NAME,
        )?;
        created.push(hands_set);
        let pose = |action: HandsAction, ty| {
            create_action(
                instance,
                hands_set,
                action.name(),
                action.localized_name(),
                ty,
                &hand_paths,
            )
        };
        let hands = [
            pose(HandsAction::GripPose, sys::ActionType::POSE_INPUT)?,
            pose(HandsAction::AimPose, sys::ActionType::POSE_INPUT)?,
            pose(HandsAction::Haptic, sys::ActionType::VIBRATION_OUTPUT)?,
        ];

        let mut modes = Vec::with_capacity(ActionSetKind::ALL.len());
        for kind in ActionSetKind::ALL {
            let set = create_set(instance, kind.name(), kind.localized_name())?;
            created.push(set);
            let make = |name: &str, localized: &str, ty| {
                create_action(instance, set, name, localized, ty, &hand_paths)
                    .with_context(|| format!("in the {} set", kind.name()))
            };
            let bools = BoolInput::ALL
                .iter()
                .map(|i| make(i.name(), i.localized_name(), sys::ActionType::BOOLEAN_INPUT))
                .collect::<anyhow::Result<_>>()?;
            let floats = FloatInput::ALL
                .iter()
                .map(|i| make(i.name(), i.localized_name(), sys::ActionType::FLOAT_INPUT))
                .collect::<anyhow::Result<_>>()?;
            let vec2s = Vec2Input::ALL
                .iter()
                .map(|i| {
                    make(
                        i.name(),
                        i.localized_name(),
                        sys::ActionType::VECTOR2F_INPUT,
                    )
                })
                .collect::<anyhow::Result<_>>()?;
            modes.push(ModeActions {
                set,
                bools,
                floats,
                vec2s,
            });
        }

        Ok(Actions {
            hands_set,
            hands,
            modes,
            hand_paths,
        })
    }

    fn state_info(&self, action: sys::Action, hand: Hand) -> sys::ActionStateGetInfo {
        sys::ActionStateGetInfo {
            ty: sys::ActionStateGetInfo::TYPE,
            next: ptr::null(),
            action,
            subaction_path: self.hand_paths[hand.index()],
        }
    }

    /// Suggest one profile's bindings; returns how many were suggested.
    fn suggest_profile(&self, instance: &xr::Instance, profile: Profile) -> anyhow::Result<usize> {
        let mut bindings = Vec::new();
        for hand in Hand::BOTH {
            for (i, &action) in HandsAction::ALL.iter().enumerate() {
                push(
                    &mut bindings,
                    instance,
                    profile,
                    hand,
                    Input::Hands(action),
                    self.hands[i],
                )?;
            }
            for mode in &self.modes {
                for (i, &input) in BoolInput::ALL.iter().enumerate() {
                    push(
                        &mut bindings,
                        instance,
                        profile,
                        hand,
                        Input::Bool(input),
                        mode.bools[i],
                    )?;
                }
                for (i, &input) in FloatInput::ALL.iter().enumerate() {
                    push(
                        &mut bindings,
                        instance,
                        profile,
                        hand,
                        Input::Float(input),
                        mode.floats[i],
                    )?;
                }
                for (i, &input) in Vec2Input::ALL.iter().enumerate() {
                    push(
                        &mut bindings,
                        instance,
                        profile,
                        hand,
                        Input::Vec2(input),
                        mode.vec2s[i],
                    )?;
                }
            }
        }
        let info = sys::InteractionProfileSuggestedBinding {
            ty: sys::InteractionProfileSuggestedBinding::TYPE,
            next: ptr::null(),
            interaction_profile: instance
                .string_to_path(profile.path())
                .context("resolving the profile path")?,
            count_suggested_bindings: bindings.len() as u32,
            suggested_bindings: bindings.as_ptr(),
        };
        // SAFETY: `info` and the array it points to outlive the call.
        let result = unsafe {
            (instance.fp().suggest_interaction_profile_bindings)(instance.as_raw(), &info)
        };
        check(result).context("suggesting the bindings")?;
        Ok(bindings.len())
    }
}

/// Map an OpenXR result to `Ok` for every success code, including the qualified ones.
fn check(result: sys::Result) -> Result<sys::Result, sys::Result> {
    if result.into_raw() >= 0 {
        Ok(result)
    } else {
        Err(result)
    }
}

fn position<T: PartialEq>(all: &[T], item: T) -> usize {
    all.iter()
        .position(|i| *i == item)
        .expect("every item of a layout enum is in its ALL table")
}

fn action_from(handles: &mut impl Iterator<Item = u64>) -> sys::Action {
    sys::Action::from_raw(handles.next().unwrap_or(0))
}

fn hand_paths(instance: &xr::Instance) -> anyhow::Result<[xr::Path; 2]> {
    Ok([
        instance
            .string_to_path(hand_path(Hand::Left))
            .context("resolving the left hand path")?,
        instance
            .string_to_path(hand_path(Hand::Right))
            .context("resolving the right hand path")?,
    ])
}

fn push(
    bindings: &mut Vec<sys::ActionSuggestedBinding>,
    instance: &xr::Instance,
    profile: Profile,
    hand: Hand,
    input: Input,
    action: sys::Action,
) -> anyhow::Result<()> {
    let Some(component) = binding(profile, hand, input) else {
        return Ok(());
    };
    let full = format!("{}/{component}", hand_path(hand));
    let path = instance
        .string_to_path(&full)
        .with_context(|| format!("resolving {full}"))?;
    bindings.push(sys::ActionSuggestedBinding {
        action,
        binding: path,
    });
    Ok(())
}

fn create_set(
    instance: &xr::Instance,
    name: &str,
    localized: &str,
) -> anyhow::Result<sys::ActionSet> {
    let mut info = sys::ActionSetCreateInfo {
        ty: sys::ActionSetCreateInfo::TYPE,
        next: ptr::null(),
        action_set_name: [0; sys::MAX_ACTION_SET_NAME_SIZE],
        localized_action_set_name: [0; sys::MAX_LOCALIZED_ACTION_SET_NAME_SIZE],
        priority: 0,
    };
    fill_cstr(&mut info.action_set_name, name)?;
    fill_cstr(&mut info.localized_action_set_name, localized)?;
    let mut out = sys::ActionSet::NULL;
    // SAFETY: `info` and `out` are valid for the duration of the call.
    let result = unsafe { (instance.fp().create_action_set)(instance.as_raw(), &info, &mut out) };
    check(result).with_context(|| format!("creating the {name} action set"))?;
    Ok(out)
}

fn create_action(
    instance: &xr::Instance,
    set: sys::ActionSet,
    name: &str,
    localized: &str,
    action_type: sys::ActionType,
    subaction_paths: &[xr::Path; 2],
) -> anyhow::Result<sys::Action> {
    let mut info = sys::ActionCreateInfo {
        ty: sys::ActionCreateInfo::TYPE,
        next: ptr::null(),
        action_name: [0; sys::MAX_ACTION_NAME_SIZE],
        action_type,
        count_subaction_paths: subaction_paths.len() as u32,
        subaction_paths: subaction_paths.as_ptr(),
        localized_action_name: [0; sys::MAX_LOCALIZED_ACTION_NAME_SIZE],
    };
    fill_cstr(&mut info.action_name, name)?;
    fill_cstr(&mut info.localized_action_name, localized)?;
    let mut out = sys::Action::NULL;
    // SAFETY: `info`, the path array it points to, and `out` are valid for the duration of the
    // call.
    let result = unsafe { (instance.fp().create_action)(set, &info, &mut out) };
    check(result).with_context(|| format!("creating the {name} action"))?;
    Ok(out)
}

fn destroy_set(instance: &xr::Instance, set: sys::ActionSet) {
    // SAFETY: `set` is a live action set on `instance`, destroyed exactly once.
    let result = unsafe { (instance.fp().destroy_action_set)(set) };
    if let Err(e) = check(result) {
        tracing::warn!(target: "vr", "input: destroying an action set failed: {e}");
    }
}

/// Copy `text` into a fixed-size, null-terminated OpenXR name field.
fn fill_cstr(dst: &mut [c_char], text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        text.len() < dst.len() && !text.bytes().any(|b| b == 0),
        "the name {text:?} does not fit an OpenXR name field of {} bytes",
        dst.len()
    );
    for (d, b) in dst.iter_mut().zip(text.bytes()) {
        *d = b as c_char;
    }
    dst[text.len()] = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_fits_its_openxr_field() {
        let mut name = [0 as c_char; sys::MAX_ACTION_NAME_SIZE];
        let mut localized = [0 as c_char; sys::MAX_LOCALIZED_ACTION_NAME_SIZE];
        for i in BoolInput::ALL {
            fill_cstr(&mut name, i.name()).unwrap();
            fill_cstr(&mut localized, i.localized_name()).unwrap();
        }
        for i in FloatInput::ALL {
            fill_cstr(&mut name, i.name()).unwrap();
            fill_cstr(&mut localized, i.localized_name()).unwrap();
        }
        for i in Vec2Input::ALL {
            fill_cstr(&mut name, i.name()).unwrap();
            fill_cstr(&mut localized, i.localized_name()).unwrap();
        }
        for a in HandsAction::ALL {
            fill_cstr(&mut name, a.name()).unwrap();
            fill_cstr(&mut localized, a.localized_name()).unwrap();
        }
        let mut set = [0 as c_char; sys::MAX_ACTION_SET_NAME_SIZE];
        for k in ActionSetKind::ALL {
            fill_cstr(&mut set, k.name()).unwrap();
        }
    }

    #[test]
    fn an_overlong_name_is_rejected() {
        let mut field = [0 as c_char; 4];
        assert!(fill_cstr(&mut field, "abc").is_ok());
        assert_eq!(field, [b'a' as c_char, b'b' as c_char, b'c' as c_char, 0]);
        assert!(fill_cstr(&mut field, "abcd").is_err());
    }
}
