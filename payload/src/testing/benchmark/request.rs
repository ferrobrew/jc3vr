//! The `benchmark` request: which render mode to measure, for how long, at what render size, and
//! along which camera path.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{screenshot::harness::RequestId, testing::visual::overrides::SinglePassOverrides};

/// The render mode a benchmark run measures, as a player would run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BenchmarkMode {
    /// The shipped multipass stereo: one walk of the render passes per eye.
    Multipass,
    /// The whole single-pass feature switched on, as the debug UI's one button does.
    SinglePass,
}

/// The `[benchmark]` table of a `benchmark` request file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BenchmarkTable {
    /// The name the run is reported under.
    label: String,
    mode: BenchmarkMode,
    /// How long the run renders before it records, once the configuration has applied.
    #[serde(default = "default_warmup_seconds")]
    warmup_seconds: f64,
    /// How long the run records.
    record_seconds: f64,
    /// The `vr.resolution_scale` to render at; `None` keeps the configured one.
    resolution_scale: Option<f32>,
    /// Bracket every dispatch with GPU timestamp queries.
    #[serde(default = "default_true")]
    gpu: bool,
    /// Time the draw-path detours and their forwarding calls, at the cost of four clock reads per
    /// draw. A diagnostic run of its own, since the timing inflates the cost it measures.
    #[serde(default)]
    detour_timing: bool,
    /// Install the stereo draw path's detours before the run, as a session that has run single-pass
    /// has them, so every mode's submissions are counted and every mode pays for the detours alike.
    #[serde(default)]
    install_draw_detours: bool,
    /// Capture the camera to hold afresh, rather than keep the one an earlier run held.
    #[serde(default)]
    recapture_camera: bool,
    /// Put the configuration back as it was before the first run once this run is done.
    #[serde(default = "default_true")]
    restore: bool,
    /// Single-pass switches forced to a value, by wire name.
    single_pass: Option<BTreeMap<String, bool>>,
    /// `vr.overlap_render_update` for the run; `None` keeps the configured one.
    overlap_render_update: Option<bool>,
    camera: Option<CameraTable>,
}

/// The `[benchmark.camera]` table: a sinusoidal path around the held camera.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraTable {
    #[serde(default)]
    yaw_deg: f32,
    #[serde(default)]
    forward_m: f32,
    #[serde(default = "default_period_seconds")]
    period_seconds: f32,
}

/// A path the held camera follows: its yaw swings by up to `yaw_deg` and its position moves
/// forward and back by up to `forward_m` along the held camera's heading, both as a sine of the
/// time since the run's warm-up began with period `period_seconds`. Zero amplitudes hold it still.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct CameraPath {
    pub(crate) yaw_deg: f32,
    pub(crate) forward_m: f32,
    pub(crate) period_seconds: f32,
}

impl CameraPath {
    pub(crate) const STILL: Self = Self {
        yaw_deg: 0.0,
        forward_m: 0.0,
        period_seconds: DEFAULT_PERIOD_SECONDS,
    };

    pub(crate) fn is_still(&self) -> bool {
        self.yaw_deg == 0.0 && self.forward_m == 0.0
    }
}

/// A validated benchmark request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BenchmarkRequest {
    pub(crate) id: RequestId,
    pub(crate) label: String,
    pub(crate) mode: BenchmarkMode,
    pub(crate) warmup_seconds: f64,
    pub(crate) record_seconds: f64,
    pub(crate) resolution_scale: Option<f32>,
    pub(crate) gpu: bool,
    pub(crate) detour_timing: bool,
    pub(crate) install_draw_detours: bool,
    pub(crate) recapture_camera: bool,
    pub(crate) restore: bool,
    pub(crate) switches: SinglePassOverrides,
    pub(crate) overlap_render_update: Option<bool>,
    pub(crate) camera: CameraPath,
}

/// Why a benchmark table is refused.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BenchmarkError {
    Label,
    Warmup(f64),
    Record(f64),
    ResolutionScale(f32),
    Switches(String),
    SwitchesOnMultipass,
    Camera,
}

impl std::fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "benchmark: ")?;
        match self {
            Self::Label => write!(
                f,
                "a label is 1-64 ASCII letters, digits, underscores, or hyphens"
            ),
            Self::Warmup(value) => write!(
                f,
                "warmup_seconds is {value}; it must be within [0, {MAX_WARMUP_SECONDS}]"
            ),
            Self::Record(value) => write!(
                f,
                "record_seconds is {value}; it must be within (0, {MAX_RECORD_SECONDS}]"
            ),
            Self::ResolutionScale(value) => write!(
                f,
                "resolution_scale is {value}; it must be within [{MIN_RESOLUTION_SCALE}, \
                 {MAX_RESOLUTION_SCALE}]"
            ),
            Self::Switches(error) => write!(f, "{error}"),
            Self::SwitchesOnMultipass => {
                write!(f, "single_pass switches apply only to the single_pass mode")
            }
            Self::Camera => write!(
                f,
                "the camera path needs |yaw_deg| <= {MAX_YAW_DEG}, |forward_m| <= \
                 {MAX_FORWARD_M}, and a period within (0, {MAX_PERIOD_SECONDS}] seconds"
            ),
        }
    }
}

impl std::error::Error for BenchmarkError {}

/// The longest warm-up and recording a request may ask for.
pub(crate) const MAX_WARMUP_SECONDS: f64 = 60.0;
pub(crate) const MAX_RECORD_SECONDS: f64 = 120.0;

impl BenchmarkRequest {
    pub(crate) fn parse(id: RequestId, table: BenchmarkTable) -> Result<Self, BenchmarkError> {
        if RequestId::parse(&table.label).is_err() {
            return Err(BenchmarkError::Label);
        }
        if !(0.0..=MAX_WARMUP_SECONDS).contains(&table.warmup_seconds) {
            return Err(BenchmarkError::Warmup(table.warmup_seconds));
        }
        if !(table.record_seconds > 0.0 && table.record_seconds <= MAX_RECORD_SECONDS) {
            return Err(BenchmarkError::Record(table.record_seconds));
        }
        if let Some(scale) = table.resolution_scale
            && !(MIN_RESOLUTION_SCALE..=MAX_RESOLUTION_SCALE).contains(&scale)
        {
            return Err(BenchmarkError::ResolutionScale(scale));
        }
        let switches = match table.single_pass {
            None => SinglePassOverrides::default(),
            Some(_) if table.mode != BenchmarkMode::SinglePass => {
                return Err(BenchmarkError::SwitchesOnMultipass);
            }
            Some(entries) => SinglePassOverrides::from_names(entries)
                .map_err(|error| BenchmarkError::Switches(error.to_string()))?,
        };
        let camera = match table.camera {
            None => CameraPath::STILL,
            Some(camera) => {
                let valid = camera.yaw_deg.is_finite()
                    && camera.yaw_deg.abs() <= MAX_YAW_DEG
                    && camera.forward_m.is_finite()
                    && camera.forward_m.abs() <= MAX_FORWARD_M
                    && camera.period_seconds > 0.0
                    && camera.period_seconds <= MAX_PERIOD_SECONDS;
                if !valid {
                    return Err(BenchmarkError::Camera);
                }
                CameraPath {
                    yaw_deg: camera.yaw_deg,
                    forward_m: camera.forward_m,
                    period_seconds: camera.period_seconds,
                }
            }
        };
        Ok(Self {
            id,
            label: table.label,
            mode: table.mode,
            warmup_seconds: table.warmup_seconds,
            record_seconds: table.record_seconds,
            resolution_scale: table.resolution_scale,
            gpu: table.gpu,
            detour_timing: table.detour_timing,
            install_draw_detours: table.install_draw_detours,
            recapture_camera: table.recapture_camera,
            restore: table.restore,
            switches,
            overlap_render_update: table.overlap_render_update,
            camera,
        })
    }
}

/// Mirrors the `vr.resolution_scale` clamp at the low end, and the harness's session limit at the
/// high end.
const MIN_RESOLUTION_SCALE: f32 = 0.1;
const MAX_RESOLUTION_SCALE: f32 = 4.0;
const MAX_YAW_DEG: f32 = 90.0;
const MAX_FORWARD_M: f32 = 100.0;
const MAX_PERIOD_SECONDS: f32 = 120.0;
const DEFAULT_PERIOD_SECONDS: f32 = 10.0;

fn default_warmup_seconds() -> f64 {
    3.0
}

fn default_true() -> bool {
    true
}

fn default_period_seconds() -> f32 {
    DEFAULT_PERIOD_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<BenchmarkRequest, BenchmarkError> {
        BenchmarkRequest::parse(
            RequestId::parse("b1").unwrap(),
            toml::from_str::<BenchmarkTable>(text).unwrap(),
        )
    }

    #[test]
    fn a_minimal_table_takes_the_defaults() {
        let request = parse("label = 'town'\nmode = 'multipass'\nrecord_seconds = 10.0\n").unwrap();
        assert_eq!(request.mode, BenchmarkMode::Multipass);
        assert_eq!(request.warmup_seconds, 3.0);
        assert!(request.gpu && request.restore);
        assert!(!request.detour_timing && !request.recapture_camera);
        assert!(!request.install_draw_detours);
        assert!(request.switches.is_empty());
        assert!(request.camera.is_still());
        assert_eq!(request.resolution_scale, None);
        assert_eq!(request.overlap_render_update, None);
    }

    #[test]
    fn the_frame_ordering_is_forced_in_either_mode() {
        for mode in ["multipass", "single_pass"] {
            let request = parse(&format!(
                "label = 'a'\nmode = '{mode}'\nrecord_seconds = 1.0\n\
                 overlap_render_update = false\n"
            ))
            .unwrap();
            assert_eq!(request.overlap_render_update, Some(false));
        }
    }

    #[test]
    fn single_pass_switches_are_parsed_and_need_the_single_pass_mode() {
        let request = parse(
            "label = 'a'\nmode = 'single_pass'\nrecord_seconds = 1.0\n\
             [single_pass]\nfsr_per_eye = false\n",
        )
        .unwrap();
        assert_eq!(request.switches.iter().count(), 1);
        assert_eq!(
            parse(
                "label = 'a'\nmode = 'multipass'\nrecord_seconds = 1.0\n\
                 [single_pass]\nfsr_per_eye = false\n"
            )
            .unwrap_err(),
            BenchmarkError::SwitchesOnMultipass
        );
        assert!(matches!(
            parse(
                "label = 'a'\nmode = 'single_pass'\nrecord_seconds = 1.0\n\
                 [single_pass]\nnot_a_switch = false\n"
            ),
            Err(BenchmarkError::Switches(_))
        ));
    }

    #[test]
    fn out_of_range_values_are_refused() {
        let base = "mode = 'multipass'\n";
        assert_eq!(
            parse(&format!("{base}label = 'a b'\nrecord_seconds = 1.0\n")).unwrap_err(),
            BenchmarkError::Label
        );
        assert_eq!(
            parse(&format!("{base}label = 'a'\nrecord_seconds = 0.0\n")).unwrap_err(),
            BenchmarkError::Record(0.0)
        );
        assert_eq!(
            parse(&format!(
                "{base}label = 'a'\nrecord_seconds = 1.0\nwarmup_seconds = -1.0\n"
            ))
            .unwrap_err(),
            BenchmarkError::Warmup(-1.0)
        );
        assert_eq!(
            parse(&format!(
                "{base}label = 'a'\nrecord_seconds = 1.0\nresolution_scale = 5.0\n"
            ))
            .unwrap_err(),
            BenchmarkError::ResolutionScale(5.0)
        );
        assert_eq!(
            parse(&format!(
                "{base}label = 'a'\nrecord_seconds = 1.0\n[camera]\nperiod_seconds = 0.0\n"
            ))
            .unwrap_err(),
            BenchmarkError::Camera
        );
    }

    #[test]
    fn a_camera_path_is_kept() {
        let request = parse(
            "label = 'a'\nmode = 'multipass'\nrecord_seconds = 1.0\n\
             [camera]\nyaw_deg = 15.0\nforward_m = 20.0\nperiod_seconds = 8.0\n",
        )
        .unwrap();
        assert_eq!(
            request.camera,
            CameraPath {
                yaw_deg: 15.0,
                forward_m: 20.0,
                period_seconds: 8.0
            }
        );
    }
}
