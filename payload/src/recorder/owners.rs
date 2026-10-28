//! Bounded ownership of active and draining video writers through confirmed thread termination.

use std::time::{Duration, Instant};

use crate::recorder::stream::Writer;

pub(super) struct VideoWriters {
    active: Option<Recording>,
    finishing: Vec<Recording>,
    shutting_down: bool,
}

pub(super) struct Completion {
    pub(super) folder: String,
    pub(super) error: Option<String>,
}

pub(super) struct Shutdown {
    pub(super) stopped: bool,
    pub(super) completions: Vec<Completion>,
}

impl VideoWriters {
    pub(super) const fn new() -> Self {
        Self {
            active: None,
            finishing: Vec::new(),
            shutting_down: false,
        }
    }

    pub(super) fn active(&self) -> Option<&Writer> {
        self.active.as_ref().map(|recording| &recording.writer)
    }

    /// Admission precedes spawning, so a refusal cannot detach a new thread.
    pub(super) fn start(
        &mut self,
        folder: String,
        spawn: impl FnOnce() -> anyhow::Result<Writer>,
    ) -> (Vec<Completion>, anyhow::Result<()>) {
        let completions = self.reap();
        let result = (|| {
            anyhow::ensure!(!self.shutting_down, "recorder: shutdown has started");
            anyhow::ensure!(self.active.is_none(), "recorder: a recording is active");
            anyhow::ensure!(
                self.finishing.len() < MAX_WRITERS,
                "recorder: previous video writers are still finishing; try again after they exit"
            );
            self.active = Some(Recording {
                writer: spawn()?,
                folder,
            });
            Ok(())
        })();
        (completions, result)
    }

    pub(super) fn stop(&mut self) {
        if let Some(mut recording) = self.active.take() {
            recording.writer.finish();
            self.finishing.push(recording);
        }
    }

    /// Only a completed join releases a retained owner and its admission slot.
    pub(super) fn reap(&mut self) -> Vec<Completion> {
        let mut completions = Vec::new();
        let mut index = 0;
        while index < self.finishing.len() {
            if let Some(completion) = self.finishing[index].writer.poll_completion() {
                let recording = self.finishing.swap_remove(index);
                if let Some(error) = &completion.error {
                    tracing::error!(folder = %recording.folder, "recorder: {error}");
                }
                completions.push(Completion {
                    folder: recording.folder,
                    error: completion.error,
                });
            } else {
                index += 1;
            }
        }
        completions
    }

    /// A timeout retains every pending owner. The caller must keep the payload mapped.
    pub(super) fn shutdown(&mut self, budget: Duration) -> Shutdown {
        self.shutting_down = true;
        self.stop();
        let deadline = Instant::now() + budget;
        let mut completions = self.reap();
        while !self.finishing.is_empty() && Instant::now() < deadline {
            std::thread::sleep(
                POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            );
            completions.extend(self.reap());
        }
        Shutdown {
            stopped: self.finishing.is_empty(),
            completions,
        }
    }
}

/// Includes the active writer, as in the tracking recorder's admission policy.
const MAX_WRITERS: usize = 2;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct Recording {
    writer: Writer,
    folder: String,
}

#[cfg(test)]
mod tests;
