//! The map from the runtime's predicted display time to the take's video, from the video sidecar's
//! frame stamps.

/// A map from the runtime's predicted display time to seconds into the take's video.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoClock {
    /// `(display_time_ns, unix_ms)`, in display-time order.
    points: Vec<(i64, f64)>,
    start_unix_ms: f64,
}

impl VideoClock {
    /// Read the video sidecar's frame stamps (`frames.jsonl`) and the stream's start
    /// (`stream.json`).
    pub fn parse(frames_jsonl: &str, stream_json: &str) -> Option<Self> {
        let stream: serde_json::Value = serde_json::from_str(stream_json).ok()?;
        let start_unix_ms = stream.get("video_start_unix_ms")?.as_f64()?;
        let mut points: Vec<(i64, f64)> = frames_jsonl
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|v| {
                Some((
                    v.get("predicted_display_time_ns")?.as_i64()?,
                    v.get("unix_ms")?.as_f64()?,
                ))
            })
            .collect();
        points.sort_by_key(|p| p.0);
        points.dedup_by_key(|p| p.0);
        (!points.is_empty()).then_some(Self {
            points,
            start_unix_ms,
        })
    }

    /// Seconds into the video for a frame at `display_time_ns`, interpolated between the stamps,
    /// and extrapolated at real time for at most [`EDGE_NS`] past either end.
    pub fn video_s(&self, display_time_ns: i64) -> Option<f64> {
        Some((self.unix_ms(display_time_ns)? - self.start_unix_ms) / 1000.0)
    }

    /// The wall-clock time of a frame at `display_time_ns`, milliseconds since the Unix epoch,
    /// placed as [`video_s`](Self::video_s) places it.
    pub fn unix_ms(&self, display_time_ns: i64) -> Option<f64> {
        let index = self.points.partition_point(|p| p.0 < display_time_ns);
        if index > 0 && index < self.points.len() {
            let ((t0, u0), (t1, u1)) = (self.points[index - 1], self.points[index]);
            let span = (t1 - t0) as f64;
            Some(u0 + (u1 - u0) * ((display_time_ns - t0) as f64 / span))
        } else {
            let (t, u) = if index == 0 {
                *self.points.first()?
            } else {
                *self.points.last()?
            };
            if (display_time_ns - t).abs() > EDGE_NS {
                return None;
            }
            Some(u + (display_time_ns - t) as f64 / 1.0e6)
        }
    }

    /// The wall-clock time of the video's first frame, milliseconds since the Unix epoch.
    pub fn start_unix_ms(&self) -> f64 {
        self.start_unix_ms
    }
}

/// How far past the sidecar's first or last stamp a frame is still placed on the video, nanoseconds.
const EDGE_NS: i64 = 100_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_clock_interpolates_between_stamps() {
        let frames = "{\"frame\":0,\"unix_ms\":1000.0,\"predicted_display_time_ns\":0}\n\
                      {\"frame\":1,\"unix_ms\":1100.0,\"predicted_display_time_ns\":100000000}\n";
        let clock = VideoClock::parse(frames, "{\"video_start_unix_ms\": 900.0}").unwrap();
        assert!((clock.video_s(50_000_000).unwrap() - 0.15).abs() < 1e-9);
        assert!((clock.video_s(0).unwrap() - 0.1).abs() < 1e-9);
        assert_eq!(clock.video_s(10_000_000_000), None);
    }
}
