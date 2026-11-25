//! The debug panel's placement in the cockpit frame, shared between the threads that move it and
//! the one that draws it.
//!
//! The panel lives in the tracked space the head and the hands are composed in: its pose is in the
//! cockpit frame, so it travels with the player and does not follow the head. Three threads reach
//! the slot, each briefly, under one leaf lock that nothing else is taken under:
//!
//! - the input tick begins, holds, and ends a hand's grab ([`begin_grab`], [`hold_grab`],
//!   [`end_grab`]), keeping the hand-to-panel offset and applying the thumbstick's push to it;
//! - the game thread asks for the panel in front of the head from the debug UI ([`recall`]);
//! - the draw task, on eye 0, settles the placement for the frame ([`resolve`]): it places a panel
//!   that has none, or that was recalled, or that the cockpit frame was re-based under, in front of
//!   the head, and it carries a grabbed panel with the grabbing hand's latest pose.

use parking_lot::Mutex;

use crate::{
    controllers::frame::{Hand, Pose},
    hud::egui_panel::placement::{grab_offset, held_pose, in_front_of, is_behind, push_along_ray},
};

/// Why the panel is being summoned.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Summon {
    /// The panel was shown: it keeps its placement, unless it has none or is behind the player.
    Shown,
    /// The player asked for the panel in front of the head.
    Recall,
}

/// Ask for the panel to be placed for `summon` on the next drawn frame. A recall is not downgraded
/// by a later show.
pub(crate) fn summon(summon: Summon) {
    let mut slot = SLOT.lock();
    if slot.summon != Some(Summon::Recall) {
        slot.summon = Some(summon);
    }
}

/// Ask for the panel in front of the head on the next drawn frame. Ends a grab in progress, whose
/// hand would otherwise carry the panel straight back.
pub(crate) fn recall() {
    let mut slot = SLOT.lock();
    slot.summon = Some(Summon::Recall);
    slot.grab = None;
}

/// Whether the panel has a placement a grab can take hold of.
pub(crate) fn is_placed() -> bool {
    SLOT.lock().pose.is_some()
}

/// Begin `hand`'s grab with the hand at the cockpit-frame `aim` pose, keeping the panel's pose in
/// the hand's frame from now on. Returns whether the grab began: the panel must be placed.
pub(crate) fn begin_grab(hand: Hand, aim: Pose) -> bool {
    let mut slot = SLOT.lock();
    let Some(pose) = slot.pose else {
        return false;
    };
    slot.grab = Some(Grab {
        hand,
        offset: grab_offset(aim, pose),
    });
    true
}

/// Hold `hand`'s grab for a tick: the thumbstick pushes the panel along the aim ray at `rate` (a
/// deadzoned deflection, positive away from the hand) for `dt` seconds, and the panel follows the
/// hand at `aim`, when its pose is known. A no-op once the grab has ended.
pub(crate) fn hold_grab(hand: Hand, aim: Option<Pose>, rate: f32, dt: f32) {
    let mut slot = SLOT.lock();
    let Some(grab) = slot.grab.as_mut().filter(|grab| grab.hand == hand) else {
        return;
    };
    grab.offset = push_along_ray(grab.offset, rate, dt);
    let offset = grab.offset;
    if let Some(aim) = aim {
        slot.pose = Some(held_pose(aim, offset));
    }
}

/// End the grab, leaving the panel where it was last carried to.
pub(crate) fn end_grab() {
    SLOT.lock().grab = None;
}

/// The hand that holds the panel, if any.
pub(crate) fn grabbing() -> Option<Hand> {
    SLOT.lock().grab.map(|grab| grab.hand)
}

/// What the draw task knows of the frame when it settles the placement.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ResolveInput {
    /// The cockpit-frame head pose the frame was rendered from.
    pub(crate) head: Pose,
    /// How far in front of the head a summoned panel is placed, metres.
    pub(crate) distance: f32,
    /// The grabbing hand and its latest cockpit-frame aim pose, when a hand holds the panel and
    /// its pose is known.
    pub(crate) held: Option<(Hand, Pose)>,
    /// The count of the cockpit frame's re-bases ([`crate::vr::recenter_count`]).
    pub(crate) recenters: u32,
}

/// Settle the panel's cockpit-frame pose for the frame and return it. Called by the draw task on
/// eye 0.
pub(crate) fn resolve(input: ResolveInput) -> Pose {
    SLOT.lock().resolve(input)
}

/// A hand's hold on the panel.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Grab {
    hand: Hand,
    /// The panel's pose in the frame of the hand's aim pose.
    offset: Pose,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot::new());

/// The panel's placement and what is asked of it.
#[derive(Debug, Clone, PartialEq)]
struct Slot {
    /// The cockpit-frame panel pose, `None` until it is first placed.
    pose: Option<Pose>,
    grab: Option<Grab>,
    summon: Option<Summon>,
    /// The re-base count the pose was placed under, `None` until it is first placed.
    recenters: Option<u32>,
}

impl Slot {
    const fn new() -> Self {
        Self {
            pose: None,
            grab: None,
            summon: None,
            recenters: None,
        }
    }

    fn resolve(&mut self, input: ResolveInput) -> Pose {
        // A re-base moves the cockpit frame under the panel, which would carry it off to wherever
        // its old coordinates now lie, so the panel comes back in front of the head.
        let rebased = self.recenters.is_some_and(|count| count != input.recenters);
        self.recenters = Some(input.recenters);
        let summon = self.summon.take();
        let in_front = || in_front_of(input.head, input.distance);
        if rebased || summon == Some(Summon::Recall) {
            self.grab = None;
            return *self.pose.insert(in_front());
        }
        if let (Some(grab), Some((hand, aim))) = (self.grab, input.held)
            && grab.hand == hand
        {
            return *self.pose.insert(held_pose(aim, grab.offset));
        }
        let behind =
            |pose: Pose| summon == Some(Summon::Shown) && is_behind(input.head, pose.position);
        match self.pose {
            Some(pose) if !behind(pose) => pose,
            _ => *self.pose.insert(in_front()),
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use super::*;

    const HEAD: Pose = Pose {
        position: Vec3::new(0.0, 1.6, 0.0),
        orientation: Quat::IDENTITY,
    };

    fn input(head: Pose) -> ResolveInput {
        ResolveInput {
            head,
            distance: 2.0,
            held: None,
            recenters: 0,
        }
    }

    fn turned(yaw: f32) -> Pose {
        Pose {
            position: HEAD.position,
            orientation: Quat::from_rotation_y(yaw),
        }
    }

    #[test]
    fn the_first_frame_places_the_panel_in_front() {
        let mut slot = Slot::new();
        let pose = slot.resolve(input(HEAD));
        assert_eq!(pose, in_front_of(HEAD, 2.0));
    }

    #[test]
    fn the_panel_stays_put_as_the_head_turns() {
        let mut slot = Slot::new();
        let placed = slot.resolve(input(HEAD));
        assert_eq!(slot.resolve(input(turned(2.5))), placed);
        assert_eq!(slot.resolve(input(turned(-1.0))), placed);
    }

    #[test]
    fn showing_the_panel_behind_the_player_brings_it_in_front() {
        let mut slot = Slot::new();
        let placed = slot.resolve(input(HEAD));
        // Shown while the head faces aside, the panel stays.
        slot.summon = Some(Summon::Shown);
        assert_eq!(slot.resolve(input(turned(1.2))), placed);
        // Shown while the head faces away, it comes in front.
        slot.summon = Some(Summon::Shown);
        let back = turned(3.0);
        assert_eq!(slot.resolve(input(back)), in_front_of(back, 2.0));
        // Turning away afterwards leaves it.
        assert_eq!(slot.resolve(input(HEAD)), in_front_of(back, 2.0));
    }

    #[test]
    fn a_recall_places_the_panel_in_front_wherever_it_is() {
        let mut slot = Slot::new();
        slot.resolve(input(HEAD));
        slot.summon = Some(Summon::Recall);
        let aside = turned(1.0);
        assert_eq!(slot.resolve(input(aside)), in_front_of(aside, 2.0));
    }

    #[test]
    fn a_rebase_places_the_panel_in_front() {
        let mut slot = Slot::new();
        slot.resolve(input(HEAD));
        let aside = turned(0.8);
        let rebased = ResolveInput {
            recenters: 1,
            ..input(aside)
        };
        assert_eq!(slot.resolve(rebased), in_front_of(aside, 2.0));
        let after = ResolveInput {
            recenters: 1,
            ..input(HEAD)
        };
        assert_eq!(slot.resolve(after), in_front_of(aside, 2.0));
    }

    #[test]
    fn a_grabbed_panel_follows_the_grabbing_hand_and_stays_on_release() {
        let mut slot = Slot::new();
        let placed = slot.resolve(input(HEAD));
        let aim = Pose {
            position: Vec3::new(0.2, 1.3, -0.3),
            orientation: Quat::from_rotation_x(0.2),
        };
        slot.grab = Some(Grab {
            hand: Hand::Right,
            offset: grab_offset(aim, placed),
        });
        let moved = Pose {
            position: aim.position + Vec3::new(0.5, 0.0, 0.0),
            orientation: aim.orientation,
        };
        let held = ResolveInput {
            held: Some((Hand::Right, moved)),
            ..input(HEAD)
        };
        let carried = slot.resolve(held);
        assert!(
            carried
                .position
                .abs_diff_eq(placed.position + Vec3::X * 0.5, 1e-5)
        );
        // The other hand's pose does not carry it.
        let other = ResolveInput {
            held: Some((Hand::Left, aim)),
            ..input(HEAD)
        };
        assert_eq!(slot.resolve(other), carried);
        // Released, it stays where it was carried.
        slot.grab = None;
        assert_eq!(slot.resolve(input(HEAD)), carried);
    }
}
