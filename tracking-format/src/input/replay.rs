//! Capture-time scheduling without playback-wall-clock restamping of logical input history.

use std::ops::Range;

use crate::input::CapturedInput;

/// Independent from the pose-frame cursor. Equal capture times remain in recorded order.
#[derive(Debug, Default)]
pub struct InputReplay {
    next: usize,
}

impl InputReplay {
    /// Schedule records by the capture timeline selected by the caller's playback pacing. The
    /// returned records retain their logical clock, epoch, sync, generation, and publication ID.
    pub fn advance(&mut self, records: &[CapturedInput], capture_time_ns: u64) -> Range<usize> {
        let start = self.next.min(records.len());
        let end = start
            + records[start..]
                .iter()
                .take_while(|record| record.capture_time_ns <= capture_time_ns)
                .count();
        self.next = end;
        start..end
    }

    pub fn finished(&self, len: usize) -> bool {
        self.next >= len
    }
}
