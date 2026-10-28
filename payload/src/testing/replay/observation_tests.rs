//! Test controls for scenario observation without native replay work.

use std::time::Duration;

use crate::testing::replay::{
    Observation, PhaseState, ReplayOptions, ReplayOutcome, Request, Run, RunId, State,
    tests::fixture,
};

pub(crate) struct Session(State);

impl Session {
    pub(crate) fn new() -> Self {
        Self(State {
            pending: None,
            stop: false,
            next_run: 0,
            phase: PhaseState::Idle,
            last: None,
            orphans: Vec::new(),
            shutting_down: false,
            stop_owned: None,
        })
    }

    pub(crate) fn request(&mut self) -> RunId {
        self.0.next_run += 1;
        let id = RunId(self.0.next_run);
        self.0.pending = Some(Request {
            run: id,
            path: "observation.tracking".into(),
            options: ReplayOptions::default(),
        });
        id
    }

    pub(crate) fn playing(&mut self, id: RunId) {
        self.0.pending = None;
        self.0.phase = PhaseState::Playing {
            run: Run {
                id,
                options: ReplayOptions::default(),
            },
            loaded: fixture(),
            started: None,
            cursor: Default::default(),
            input_cursor: Default::default(),
            head: None,
        };
    }

    pub(crate) fn loading(&mut self, id: RunId) {
        self.0.pending = None;
        self.0.phase = PhaseState::Loading {
            request: Request {
                run: id,
                path: "observation.tracking".into(),
                options: ReplayOptions::default(),
            },
            handle: std::thread::spawn(|| Err(crate::testing::replay::ReplayError::Disabled)),
        };
    }

    pub(crate) fn complete(&mut self, id: RunId) {
        self.clear_phase();
        self.0.last = Some((
            id,
            ReplayOutcome::Completed {
                frames: 1,
                duration: Duration::ZERO,
            },
        ));
    }

    pub(crate) fn stopped_preserving_last(&mut self) {
        self.0.pending = None;
        self.clear_phase();
    }

    pub(crate) fn observation(&self, id: RunId) -> Observation {
        self.0.observation(id)
    }

    fn clear_phase(&mut self) {
        if let PhaseState::Loading { handle, .. } =
            std::mem::replace(&mut self.0.phase, PhaseState::Idle)
        {
            let _ = handle.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.clear_phase();
    }
}
