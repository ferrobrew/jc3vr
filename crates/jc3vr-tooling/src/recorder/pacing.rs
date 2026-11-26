//! Keeping the recorded stream at a constant rate in real time.
//!
//! The stream is `yuv4mpegpipe`, which times frames by their count at the rate its header names,
//! while the game renders at whatever rate it manages. The recorder therefore decides per rendered
//! frame how many stream frames are due by the wall clock ([`Pacer`]): none when the game renders
//! faster than the stream's rate (the frame is not captured at all, which also spares its readback),
//! one usually, and more after a slow frame, the extra ones repeating the previous image. The stream
//! then stays as long as the take really was, which keeps the host's audio, recorded against the
//! same clock, in step with it.
//!
//! A frame the writer's queue cannot take is not lost from the count: [`Backlog`] owes its stream
//! frames to the next frame that is queued, as repeats. Only a backlog beyond a second's worth is
//! given up, since a host that has fallen that far behind cannot catch up by being sent more.

use std::time::Duration;

/// How many stream frames each rendered frame owes, by the wall clock.
#[derive(Debug)]
pub struct Pacer {
    rate: u32,
    /// The stream frames accounted for so far.
    emitted: u64,
}

impl Pacer {
    /// A pacer at `rate` frames a second, clamped to at least one.
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            emitted: 0,
        }
    }

    /// The stream frames due at `elapsed` since the stream's first frame. The first call is due one
    /// frame; afterwards, a frame is due at every multiple of the frame interval passed since the
    /// previous call.
    pub fn due(&mut self, elapsed: Duration) -> u32 {
        let target = (elapsed.as_nanos() * u128::from(self.rate) / 1_000_000_000) as u64 + 1;
        let due = target.saturating_sub(self.emitted);
        self.emitted = self.emitted.max(target);
        u32::try_from(due).unwrap_or(u32::MAX)
    }
}

/// The stream frames that frames the writer could not queue still owe.
#[derive(Debug)]
pub struct Backlog {
    owed: u32,
    limit: u32,
}

impl Backlog {
    /// A backlog that gives up beyond `limit` owed frames.
    pub fn new(limit: u32) -> Self {
        Self { owed: 0, limit }
    }

    /// Take the owed frames, to send as repeats with a frame that is about to be queued.
    pub fn take(&mut self) -> u32 {
        std::mem::take(&mut self.owed)
    }

    /// Owe `frames` more stream frames, from a frame the writer could not queue. Returns how many
    /// were given up because the backlog is at its limit.
    pub fn owe(&mut self, frames: u32) -> u32 {
        let total = self.owed.saturating_add(frames);
        self.owed = total.min(self.limit);
        total - self.owed
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn the_first_frame_is_due_at_once() {
        assert_eq!(Pacer::new(30).due(ms(0)), 1);
    }

    #[test]
    fn a_faster_game_skips_frames_and_a_slower_one_repeats_them() {
        let mut pacer = Pacer::new(30);
        // 60 fps against a 30 fps stream: every other frame is due.
        let fast: Vec<u32> = (0..6).map(|i| pacer.due(ms(i * 1000 / 60 + 1))).collect();
        assert_eq!(fast, [1, 0, 1, 0, 1, 0]);
        // A hitch from 84 ms to 301 ms owes the seven intervals it spans.
        assert_eq!(pacer.due(ms(301)), 7);
    }

    #[test]
    fn the_stream_keeps_real_time_at_an_uneven_render_rate() {
        let mut pacer = Pacer::new(30);
        let mut total = 0u64;
        let mut t = 0u64;
        // About 41 fps with jitter, for ten seconds.
        for i in 0..410u64 {
            total += u64::from(pacer.due(Duration::from_micros(t)));
            t += 24_390 + (i % 7) * 100 - 300;
        }
        let elapsed_s = t as f64 / 1e6;
        assert!(
            (total as f64 - elapsed_s * 30.0).abs() <= 2.0,
            "{total} frames over {elapsed_s} s"
        );
    }

    #[test]
    fn time_going_backwards_owes_nothing() {
        let mut pacer = Pacer::new(30);
        assert_eq!(pacer.due(ms(1000)), 31);
        assert_eq!(pacer.due(ms(500)), 0);
        assert_eq!(pacer.due(ms(1034)), 1);
    }

    #[test]
    fn the_backlog_owes_frames_up_to_its_limit() {
        let mut backlog = Backlog::new(30);
        assert_eq!(backlog.owe(2), 0);
        assert_eq!(backlog.owe(20), 0);
        assert_eq!(backlog.owe(10), 2);
        assert_eq!(backlog.take(), 30);
        assert_eq!(backlog.take(), 0);
    }
}
