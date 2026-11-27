//! [`Published`]: the latest value one stage of the frame published for the others, with when.

use parking_lot::Mutex;

use crate::published::clock::{Clock, Stamp};

/// The stage of the frame that writes a [`Published`] value. It documents the writer at the
/// declaration, where a reader looks to know how old a value can be and which thread wrote it,
/// and it labels the value in a readout. The stages are the frame's points described in
/// `docs/mod/threading.md`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The input tick's poll stage, inside the engine's input job.
    InputPoll,
    /// The input tick's game stage, at the top of the world's pre-simulation queue.
    InputGame,
    /// A character's, weapon's, or vehicle's own update, on the simulation's worker threads.
    CharacterUpdate,
    /// The pre-simulation callback that runs once per physics step, outside the step.
    PhysicsStep,
    /// The characters' pose-finalization pass, on the character workers.
    PoseFinalize,
    /// The post-simulation updates, after the world update.
    PostSimulation,
    /// The render update on the game thread: the VR frame's begin and the eye dispatches.
    RenderUpdate,
    /// The frame tail, inline or on its worker.
    FrameTail,
    /// The debug UI, on the game thread.
    DebugUi,
    /// A detour on whichever thread the engine calls it from.
    EngineCall,
}

impl Stage {
    /// A short name for a readout.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InputPoll => "input poll",
            Self::InputGame => "input game stage",
            Self::CharacterUpdate => "character update",
            Self::PhysicsStep => "physics step",
            Self::PoseFinalize => "pose finalization",
            Self::PostSimulation => "post-simulation",
            Self::RenderUpdate => "render update",
            Self::FrameTail => "frame tail",
            Self::DebugUi => "debug UI",
            Self::EngineCall => "engine call",
        }
    }
}

/// A value and the reading of clock `C` it was published at.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stamped<T, C = Stamp> {
    pub value: T,
    pub at: C,
}

impl<T, C: Clock> Stamped<T, C> {
    /// The value, while it is current at `now` under `window`.
    #[inline]
    pub fn fresh(&self, now: C, window: C::Window) -> Option<&T> {
        self.at.within(now, window).then_some(&self.value)
    }
}

/// The latest value of type `T` its writer [`Stage`] published, stamped on clock `C` (wall time
/// unless stated). Any thread may publish or read; each call takes the slot's lock briefly, and
/// nothing is locked under it, so it is a leaf lock. A read copies the value out, which is what
/// the hand-written `Mutex<Option<(T, Instant)>>` slots cost.
///
/// Declare one as a `static` beside its writer:
///
/// ```
/// # use jc3vr_core::published::{Published, Stage, Window};
/// static GROUNDED: Published<f32> = Published::new(Stage::PoseFinalize);
/// GROUNDED.publish(1.5);
/// assert_eq!(GROUNDED.fresh(Window::from_millis(250)), Some(1.5));
/// ```
pub struct Published<T, C: Clock = Stamp> {
    stage: Stage,
    slot: Mutex<Option<Stamped<T, C>>>,
}

impl<T, C: Clock> Published<T, C> {
    /// An empty slot written by `stage`.
    pub const fn new(stage: Stage) -> Self {
        Self {
            stage,
            slot: Mutex::new(None),
        }
    }

    /// The stage that writes the value.
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Publish `value`, stamped now.
    #[inline]
    pub fn publish(&self, value: T) {
        self.publish_at(value, C::now());
    }

    /// Publish `value`, stamped `at`: for a writer that took its reading once for its whole pass.
    #[inline]
    pub fn publish_at(&self, value: T, at: C) {
        *self.slot.lock() = Some(Stamped { value, at });
    }

    /// Withdraw the value, so that no reader sees it until the next publication: for a stand-down
    /// or a teleport, where waiting out the window would leave readers acting on a stale value.
    pub fn clear(&self) {
        *self.slot.lock() = None;
    }

    /// Take the value out, leaving the slot empty: for a value meant to be acted on once.
    pub fn take(&self) -> Option<Stamped<T, C>> {
        self.slot.lock().take()
    }

    /// Run `f` on the latest publication under the slot's lock, for a value that is not [`Copy`].
    /// `f` must not take another lock or call back into this slot.
    pub fn read<R>(&self, f: impl FnOnce(Option<&Stamped<T, C>>) -> R) -> R {
        f(self.slot.lock().as_ref())
    }
}

impl<T: Copy, C: Clock> Published<T, C> {
    /// The latest publication, however old.
    #[inline]
    pub fn latest(&self) -> Option<Stamped<T, C>> {
        *self.slot.lock()
    }

    /// The value, while it is current under `window` now.
    #[inline]
    pub fn fresh(&self, window: C::Window) -> Option<T> {
        self.fresh_at(C::now(), window)
    }

    /// The value, while it is current at `now` under `window`: for a reader that took its reading
    /// once for its whole pass.
    #[inline]
    pub fn fresh_at(&self, now: C, window: C::Window) -> Option<T> {
        self.slot
            .lock()
            .as_ref()
            .and_then(|stamped| stamped.fresh(now, window).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::published::clock::{Step, StepWindow, Window};

    const MS: u64 = 1_000_000;

    #[test]
    fn a_value_is_fresh_only_within_its_window() {
        let slot: Published<u32> = Published::new(Stage::InputGame);
        let window = Window::from_millis(250);
        assert_eq!(slot.fresh_at(Stamp::from_nanos(0), window), None);
        slot.publish_at(7, Stamp::from_nanos(1_000 * MS));
        assert_eq!(
            slot.fresh_at(Stamp::from_nanos(1_250 * MS), window),
            Some(7)
        );
        assert_eq!(slot.fresh_at(Stamp::from_nanos(1_251 * MS), window), None);
        // A stale value is still the latest, for a readout that shows its age.
        assert_eq!(
            slot.latest(),
            Some(Stamped {
                value: 7,
                at: Stamp::from_nanos(1_000 * MS)
            })
        );
        assert_eq!(slot.stage(), Stage::InputGame);
    }

    #[test]
    fn a_cleared_value_is_gone_before_its_window_ends() {
        let slot: Published<u32> = Published::new(Stage::PoseFinalize);
        slot.publish(3);
        assert_eq!(slot.fresh(Window::from_millis(10_000)), Some(3));
        slot.clear();
        assert_eq!(slot.fresh(Window::from_millis(10_000)), None);
        assert_eq!(slot.latest(), None);
    }

    #[test]
    fn take_empties_the_slot() {
        let slot: Published<u32> = Published::new(Stage::EngineCall);
        slot.publish(5);
        assert_eq!(slot.take().map(|s| s.value), Some(5));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn a_step_stamped_value_ages_by_steps() {
        let slot: Published<u32, Step> = Published::new(Stage::PhysicsStep);
        slot.publish_at(9, Step::from_count(40));
        let window = StepWindow::new(2);
        assert_eq!(slot.fresh_at(Step::from_count(42), window), Some(9));
        assert_eq!(slot.fresh_at(Step::from_count(43), window), None);
    }

    #[test]
    fn a_stale_stamp_ahead_of_now_is_still_current() {
        let stamped = Stamped {
            value: 1,
            at: Stamp::from_nanos(500 * MS),
        };
        let window = Window::from_millis(10);
        assert!(stamped.fresh(Stamp::from_nanos(400 * MS), window).is_some());
        assert!(stamped.fresh(Stamp::from_nanos(511 * MS), window).is_none());
    }

    #[test]
    fn every_stage_has_its_own_label() {
        use std::collections::HashSet;
        let stages = [
            Stage::InputPoll,
            Stage::InputGame,
            Stage::CharacterUpdate,
            Stage::PhysicsStep,
            Stage::PoseFinalize,
            Stage::PostSimulation,
            Stage::RenderUpdate,
            Stage::FrameTail,
            Stage::DebugUi,
            Stage::EngineCall,
        ];
        let labels: HashSet<_> = stages.iter().map(|stage| stage.label()).collect();
        assert_eq!(labels.len(), stages.len());
    }

    #[test]
    fn a_value_that_is_not_copy_is_read_in_place() {
        let slot: Published<Vec<u32>> = Published::new(Stage::DebugUi);
        slot.publish(vec![1, 2, 3]);
        assert_eq!(slot.read(|s| s.map(|s| s.value.len())), Some(3));
    }
}
