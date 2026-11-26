//! Coordinate-frame verification probe (see `docs/mod/vr-runtime.md` "Blocker 3"): records the
//! render camera's `m_TransformF` basis and the frame-over-frame position delta into the render trace
//! as a [`TraceEvent::CoordFrame`], so a trace reader can confirm JC3's world frame before a real HMD
//! pose is trusted. A sustained dot product of ≈ +1 between the normalized travel direction and
//! `-z_basis` while walking forward confirms right-handed, Y-up with forward = −Z.
//!
//! Samples once per real frame while a trace collects and records nothing otherwise. When the trace
//! ends it logs one summary line (the mean forward dot while moving), so the log keeps a hint of the
//! run even when the trace file is never read.

use glam::Vec3;
use jc3gi::camera::camera::Camera;
use parking_lot::Mutex;

use crate::debug::trace::{TraceEvent, TraceState, tracing_active};

/// One frame's render-camera basis and travel direction. The dot products are zero when the camera
/// did not move since the previous sample (or on the first sample), where the direction is
/// undefined.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct CoordFrameSample {
    pub position: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub z_basis: [f32; 3],
    /// The position change since the previous sample in this trace.
    pub delta: [f32; 3],
    pub dot_neg_z: f32,
    pub dot_pos_z: f32,
    pub dot_right: f32,
    pub dot_up: f32,
}

/// Record the render camera's `m_TransformF` frame into the active trace. Call once per real frame
/// with the pristine (pre-offset) centre camera. A no-op when no trace is collecting.
pub(crate) fn record_render_camera_frame(camera: &Camera) {
    if !tracing_active() {
        return;
    }
    let mut state = STATE.lock();
    let sample = sample(&camera.m_TransformF.data, state.last_position);
    state.last_position = Some(Vec3::from_array(sample.position));
    state.summary.add(&sample);
    drop(state);
    TraceState::record(TraceEvent::CoordFrame(sample));
}

/// Resets the previous position and run summary at the start of a trace.
pub(crate) fn begin_run() {
    *STATE.lock() = State::new();
}

/// Logs the run summary at the end of a trace, if the probe sampled any frame.
pub(crate) fn finish_run() {
    let summary = std::mem::replace(&mut STATE.lock().summary, Summary::new());
    if summary.samples > 0 {
        tracing::info!(target: "coord_frame", "{}", summary.line());
    }
}

/// Build a sample from engine transform rows (row-major, row-vector): `data[0..2]` right,
/// `data[4..6]` up, `data[8..10]` +Z basis (forward = −z_basis), `data[12..14]` translation.
fn sample(m: &[f32; 16], last_position: Option<Vec3>) -> CoordFrameSample {
    let position = Vec3::new(m[12], m[13], m[14]);
    let right = Vec3::new(m[0], m[1], m[2]);
    let up = Vec3::new(m[4], m[5], m[6]);
    let z_basis = Vec3::new(m[8], m[9], m[10]);

    let delta = last_position.map_or(Vec3::ZERO, |prev| position - prev);
    let (dot_neg_z, dot_pos_z, dot_right, dot_up) = if delta.length() > f32::EPSILON {
        let dir = delta.normalize();
        (
            dir.dot(-z_basis),
            dir.dot(z_basis),
            dir.dot(right),
            dir.dot(up),
        )
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    CoordFrameSample {
        position: position.to_array(),
        right: right.to_array(),
        up: up.to_array(),
        z_basis: z_basis.to_array(),
        delta: delta.to_array(),
        dot_neg_z,
        dot_pos_z,
        dot_right,
        dot_up,
    }
}

/// The per-frame travel below which a sample counts as standing still for the summary: a walk at
/// game frame rates moves a couple of centimetres a frame, while the camera's idle sway and
/// sub-frame interpolation stay well under this.
const MOVING_THRESHOLD_M: f32 = 0.005;

/// The previous position and run summary.
struct State {
    /// The camera world position at the last sample, for the frame-over-frame delta.
    last_position: Option<Vec3>,
    summary: Summary,
}

impl State {
    const fn new() -> Self {
        Self {
            last_position: None,
            summary: Summary::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// The samples of one trace run, condensed into the one log line the probe keeps.
#[derive(Debug, PartialEq)]
struct Summary {
    samples: u32,
    /// Samples whose travel exceeded [`MOVING_THRESHOLD_M`].
    moving: u32,
    /// Sum of `dot_neg_z` over the moving samples.
    dot_neg_z_sum: f64,
}

impl Summary {
    const fn new() -> Self {
        Self {
            samples: 0,
            moving: 0,
            dot_neg_z_sum: 0.0,
        }
    }

    fn add(&mut self, sample: &CoordFrameSample) {
        self.samples += 1;
        if Vec3::from_array(sample.delta).length() > MOVING_THRESHOLD_M {
            self.moving += 1;
            self.dot_neg_z_sum += f64::from(sample.dot_neg_z);
        }
    }

    fn line(&self) -> String {
        if self.moving == 0 {
            format!(
                "coord frame: {} samples, camera stationary; details in the trace",
                self.samples
            )
        } else {
            format!(
                "coord frame: {} samples, {} moving, mean dot(travel, -z_basis) {:+.3} while moving \
                 (≈ +1 walking forward confirms forward = −Z); details in the trace",
                self.samples,
                self.moving,
                self.dot_neg_z_sum / f64::from(self.moving),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// An identity basis at `position`: right +X, up +Y, z_basis +Z.
    fn transform(position: [f32; 3]) -> [f32; 16] {
        [
            1.0,
            0.0,
            0.0,
            0.0, //
            0.0,
            1.0,
            0.0,
            0.0, //
            0.0,
            0.0,
            1.0,
            0.0, //
            position[0],
            position[1],
            position[2],
            1.0,
        ]
    }

    #[test]
    fn first_sample_has_no_direction() {
        let s = sample(&transform([1.0, 2.0, 3.0]), None);
        assert_eq!(s.position, [1.0, 2.0, 3.0]);
        assert_eq!(s.delta, [0.0; 3]);
        assert_eq!((s.dot_neg_z, s.dot_right), (0.0, 0.0));
    }

    #[test]
    fn walking_along_neg_z_reads_forward() {
        let s = sample(&transform([0.0, 0.0, -0.02]), Some(Vec3::ZERO));
        assert_eq!(s.delta, [0.0, 0.0, -0.02]);
        assert!((s.dot_neg_z - 1.0).abs() < 1e-6);
        assert!((s.dot_pos_z + 1.0).abs() < 1e-6);
        assert_eq!(s.dot_right, 0.0);
    }

    #[test]
    fn summary_averages_only_moving_samples() {
        let mut summary = Summary::new();
        summary.add(&sample(&transform([0.0; 3]), None));
        summary.add(&sample(&transform([0.0, 0.0, -0.02]), Some(Vec3::ZERO)));
        summary.add(&sample(
            &transform([0.001, 0.0, -0.02]),
            Some(Vec3::new(0.0, 0.0, -0.02)),
        ));
        assert_eq!(summary.samples, 3);
        assert_eq!(summary.moving, 1);
        assert!(summary.line().contains("mean dot(travel, -z_basis) +1.000"));
    }
}
