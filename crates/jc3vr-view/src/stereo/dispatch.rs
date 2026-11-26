//! Render dispatch selection and ownership, independent of physical eye identity.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::stereo::STEREO_STATE;

/// A frame's render dispatch policy. Explicit modes are diagnostic overrides of the native plan.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    /// Use the configured stereo, single-pass, and far-field behavior.
    #[default]
    Native,
    /// Render only the physical left eye.
    LeftOnly,
    /// Render only the physical right eye.
    RightOnly,
    /// Render the physical left eye, then the physical right eye.
    Multipass,
    /// Render the physical right eye, then the physical left eye.
    ReverseMultipass,
    /// Render both eyes in one single-pass dispatch.
    SinglePass,
}

impl RenderMode {
    /// The captured physical eye duplicated into both XR slices, if only one eye renders.
    #[inline]
    pub const fn selected_eye(self) -> Option<usize> {
        match self {
            Self::LeftOnly => Some(0),
            Self::RightOnly => Some(1),
            _ => None,
        }
    }

    /// The physical capture supplying an XR slice.
    pub(crate) const fn source_eye(self, slice: usize) -> usize {
        match self.selected_eye() {
            Some(eye) => eye,
            None => slice,
        }
    }

    #[inline]
    pub(crate) const fn first_eye(self) -> usize {
        match self {
            Self::RightOnly | Self::ReverseMultipass => 1,
            _ => 0,
        }
    }
}

/// The current dispatch policy.
#[inline]
pub fn mode() -> RenderMode {
    *MODE.lock()
}

/// The physical eye rendered in isolation, or `None` when the mode renders both eyes.
#[inline]
pub fn selected_eye() -> Option<usize> {
    mode().selected_eye()
}

/// Set the dispatch policy after draining the previous frame's draw work and deferred tail.
/// The caller applies the matching single-pass settings and shader reload before this call.
pub fn set_mode(mode: RenderMode) {
    *MODE.lock() = mode;
}

/// True for the physical eye that prepares shared UI before the other eye draws it.
/// Single-pass callers pass the physical eye of each half, not the dispatch's eye tag.
#[inline]
pub fn first_ui_eye(eye: usize) -> bool {
    eye == mode().first_eye()
}

/// The ordered physical-eye dispatches and their once-per-frame ownership.
#[derive(Clone, Copy)]
pub struct DispatchPlan {
    /// Each dispatch's physical eye and far-only flag.
    pub dispatches: &'static [(usize, bool)],
}

impl DispatchPlan {
    pub fn new(mode: RenderMode, collapsed: bool, share: bool) -> Self {
        let dispatches: &'static [(usize, bool)] = match mode {
            RenderMode::Native if collapsed => &[(0, false)],
            RenderMode::Native if share => &[(0, true), (0, false), (1, false)],
            RenderMode::Native | RenderMode::Multipass => &[(0, false), (1, false)],
            RenderMode::ReverseMultipass => &[(1, false), (0, false)],
            RenderMode::LeftOnly | RenderMode::SinglePass => &[(0, false)],
            RenderMode::RightOnly => &[(1, false)],
        };
        Self { dispatches }
    }

    pub fn share_frame(self) -> bool {
        self.dispatches.iter().any(|&(_, far)| far)
    }

    pub fn last_near(self, ordinal: usize) -> bool {
        ordinal + 1 == self.dispatches.len()
            && self.dispatches.get(ordinal).is_some_and(|&(_, far)| !far)
    }
}

/// Whether a scene dispatch follows another scene dispatch in the same stereo frame.
#[inline]
pub(crate) fn later_scene_dispatch(
    active: bool,
    far_phase: bool,
    share_frame: bool,
    ordinal: usize,
) -> bool {
    active && !far_phase && ordinal > usize::from(share_frame)
}

/// Reset the per-dispatch stereo state ([`STEREO_STATE`]) to the frame's baseline: eye 0, no far
/// phase, no share frame, ordinal 0 — so state from a share frame's last dispatch never leaks into
/// the next frame's first dispatch (or into a non-stereo frame). Called on the main thread inline,
/// or from the tail worker after it drains the final dispatch (see [`crate::vr::tail`]).
pub fn reset_dispatch_state() {
    let mut state = STEREO_STATE.lock();
    state.draw_index = 0;
    state.far_phase = false;
    state.share_frame = false;
    state.dispatch_ordinal = 0;
    state.last_near_dispatch = true;
}

static MODE: Mutex<RenderMode> = Mutex::new(RenderMode::Native);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_preserves_collapsed_and_far_share_plans() {
        for (collapsed, share, expected) in [
            (false, false, &[(0, false), (1, false)][..]),
            (false, true, &[(0, true), (0, false), (1, false)][..]),
            (true, false, &[(0, false)][..]),
            (true, true, &[(0, false)][..]),
        ] {
            assert_eq!(
                DispatchPlan::new(RenderMode::Native, collapsed, share).dispatches,
                expected
            );
        }
    }

    #[test]
    fn explicit_modes_ignore_native_plan_flags() {
        for (mode, expected) in [
            (RenderMode::LeftOnly, &[(0, false)][..]),
            (RenderMode::RightOnly, &[(1, false)][..]),
            (RenderMode::Multipass, &[(0, false), (1, false)][..]),
            (RenderMode::ReverseMultipass, &[(1, false), (0, false)][..]),
            (RenderMode::SinglePass, &[(0, false)][..]),
        ] {
            for collapsed in [false, true] {
                for share in [false, true] {
                    let plan = DispatchPlan::new(mode, collapsed, share);
                    assert_eq!(plan.dispatches, expected);
                    assert!(!plan.share_frame());
                }
            }
        }
    }

    #[test]
    fn ownership_follows_order_not_physical_eye() {
        for mode in [
            RenderMode::Native,
            RenderMode::LeftOnly,
            RenderMode::RightOnly,
            RenderMode::Multipass,
            RenderMode::ReverseMultipass,
            RenderMode::SinglePass,
        ] {
            for collapsed in [false, true] {
                for share in [false, true] {
                    let plan = DispatchPlan::new(mode, collapsed, share);
                    let mut scene_count = 0;
                    let mut last_count = 0;
                    for (ordinal, &(eye, far)) in plan.dispatches.iter().enumerate() {
                        assert_eq!(
                            later_scene_dispatch(true, far, plan.share_frame(), ordinal),
                            !far && scene_count > 0
                        );
                        if !far {
                            if scene_count == 0 {
                                assert_eq!(eye, mode.first_eye());
                            }
                            scene_count += 1;
                        }
                        if plan.last_near(ordinal) {
                            assert!(!far);
                            last_count += 1;
                        }
                        assert!(!later_scene_dispatch(
                            false,
                            far,
                            plan.share_frame(),
                            ordinal
                        ));
                    }
                    assert_eq!(last_count, 1);
                    assert!(!plan.last_near(plan.dispatches.len()));
                }
            }
        }
    }

    #[test]
    fn isolated_eye_is_the_only_dispatch_and_both_slice_sources() {
        for (mode, eye) in [(RenderMode::LeftOnly, 0), (RenderMode::RightOnly, 1)] {
            assert_eq!(mode.selected_eye(), Some(eye));
            assert_eq!([mode.source_eye(0), mode.source_eye(1)], [eye; 2]);
            assert_eq!(
                DispatchPlan::new(mode, false, false).dispatches,
                &[(eye, false)]
            );
        }
        for mode in [
            RenderMode::Native,
            RenderMode::Multipass,
            RenderMode::ReverseMultipass,
            RenderMode::SinglePass,
        ] {
            assert_eq!(mode.selected_eye(), None);
            assert_eq!([mode.source_eye(0), mode.source_eye(1)], [0, 1]);
        }
    }
}
