//! Flow pages: pages whose rows depend on a step held in Rust, such as a guided calibration that
//! moves from one instruction to the next. A flow's page builder reads the current step and lists
//! that step's rows; its buttons move the step and return
//! [`ButtonOutcome::Rebuild`](crate::pause_menu::model::page::ButtonOutcome::Rebuild), and the
//! runtime reinstalls the page and focuses its first button.
//!
//! [`Flow`] is the step cursor: a linear sequence of steps that can advance, go back, and restart.
//! Keep one per flow page in a static beside the page definition.

/// A cursor over a flow's steps `0..len`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Flow {
    step: usize,
    len: usize,
}

/// What a step move did.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum StepChange {
    /// The step changed; the page must be rebuilt.
    Moved { from: usize, to: usize },
    /// The cursor was already at the end the move goes towards.
    Unchanged,
}

impl StepChange {
    /// Whether the step changed.
    pub(crate) fn moved(self) -> bool {
        matches!(self, StepChange::Moved { .. })
    }
}

impl Flow {
    /// A flow of `len` steps (at least one), at its first step.
    pub(crate) const fn new(len: usize) -> Self {
        Self {
            step: 0,
            len: if len == 0 { 1 } else { len },
        }
    }

    /// The current step.
    pub(crate) fn step(&self) -> usize {
        self.step
    }

    /// The number of steps.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Whether the cursor is at the last step.
    pub(crate) fn is_last(&self) -> bool {
        self.step + 1 == self.len
    }

    /// Move to the next step, unless at the last.
    pub(crate) fn advance(&mut self) -> StepChange {
        self.go_to(self.step + 1)
    }

    /// Move to the previous step, unless at the first.
    pub(crate) fn back(&mut self) -> StepChange {
        match self.step.checked_sub(1) {
            Some(to) => self.go_to(to),
            None => StepChange::Unchanged,
        }
    }

    /// Move to the first step.
    pub(crate) fn restart(&mut self) -> StepChange {
        self.go_to(0)
    }

    /// Move to step `to`, unless it is past the end or already current.
    pub(crate) fn go_to(&mut self, to: usize) -> StepChange {
        if to >= self.len || to == self.step {
            return StepChange::Unchanged;
        }
        let from = std::mem::replace(&mut self.step, to);
        StepChange::Moved { from, to }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_to_the_end_and_stops() {
        let mut flow = Flow::new(3);
        assert_eq!(flow.step(), 0);
        assert_eq!(flow.advance(), StepChange::Moved { from: 0, to: 1 });
        assert_eq!(flow.advance(), StepChange::Moved { from: 1, to: 2 });
        assert!(flow.is_last());
        assert_eq!(flow.advance(), StepChange::Unchanged);
        assert_eq!(flow.step(), 2);
    }

    #[test]
    fn goes_back_and_restarts() {
        let mut flow = Flow::new(3);
        assert_eq!(flow.back(), StepChange::Unchanged);
        flow.advance();
        flow.advance();
        assert_eq!(flow.back(), StepChange::Moved { from: 2, to: 1 });
        assert_eq!(flow.restart(), StepChange::Moved { from: 1, to: 0 });
        assert_eq!(flow.restart(), StepChange::Unchanged);
    }

    #[test]
    fn jumps_only_within_range() {
        let mut flow = Flow::new(4);
        assert!(flow.go_to(3).moved());
        assert!(!flow.go_to(3).moved());
        assert!(!flow.go_to(4).moved());
        assert_eq!(flow.step(), 3);
    }

    #[test]
    fn an_empty_flow_has_one_step() {
        let mut flow = Flow::new(0);
        assert_eq!(flow.len(), 1);
        assert!(flow.is_last());
        assert!(!flow.advance().moved());
    }
}
