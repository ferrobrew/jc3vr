//! The `snapshot` request: what to keep of the next eligible frame, and what to compare it with.

use serde::{Deserialize, Serialize};

use crate::screenshot::harness::{RequestId, snapshot::summary::FailLimits};

/// One image a snapshot can hold per eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Plane {
    /// The eye's final image, as the headset receives it.
    Final,
    /// The main depth buffer as the G-buffer fill leaves it.
    Depth,
    /// The albedo G-buffer (`gbuffer0`).
    Albedo,
    /// The normals G-buffer (`gbuffer1`).
    Normals,
    /// The velocity buffer.
    Velocity,
    /// The scene colour once the deferred lights' resolve has lit it.
    PostResolve,
    /// The scene colour before the atmospheric scattering.
    PreAtmosphere,
    /// The scene colour after the atmospheric scattering.
    PostAtmosphere,
    /// The scene colour after the sun pass.
    PostSun,
    /// The scene colour at the start of the post chain, before the tone map.
    MainColor,
    /// The tone-mapped image the anti-aliasing reads.
    PreAa,
    /// The tone-mapped image the anti-aliasing writes.
    PostAa,
    /// The target the ambient-occlusion block's composite multiplies the occlusion into (its alpha),
    /// as the block leaves it.
    Ssao,
    /// The ambient-occlusion volumes' target the block's composite multiplies in, as the block
    /// leaves it bound.
    SsaoVolumes,
    /// The render target of the ambient-occlusion block's draws, in order: the target each draw
    /// leaves, at its own size (the reduced-resolution ones too), for attributing a difference in
    /// [`Ssao`](Self::Ssao) to the draw that makes it.
    SsaoStep0,
    SsaoStep1,
    SsaoStep2,
    SsaoStep3,
    SsaoStep4,
    SsaoStep5,
    SsaoStep6,
    SsaoStep7,
}

impl Plane {
    /// The [`SsaoStep0`](Self::SsaoStep0) family's plane for the block's `step`th draw, if the family
    /// reaches that far.
    pub(crate) fn ssao_step(step: u32) -> Option<Self> {
        [
            Self::SsaoStep0,
            Self::SsaoStep1,
            Self::SsaoStep2,
            Self::SsaoStep3,
            Self::SsaoStep4,
            Self::SsaoStep5,
            Self::SsaoStep6,
            Self::SsaoStep7,
        ]
        .get(step as usize)
        .copied()
    }

    /// Whether the plane may be smaller than the eye, compared at its own size.
    pub(crate) fn may_be_reduced(self) -> bool {
        Self::ssao_step_index(self).is_some()
    }

    fn ssao_step_index(self) -> Option<u32> {
        (0..8).find(|&step| Self::ssao_step(step) == Some(self))
    }
}

/// When a snapshot's comparisons dump the compared pixels to files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpPolicy {
    #[default]
    Never,
    /// Only the comparisons whose differences exceed the failure limits.
    OnFailure,
    Always,
}

/// The `[snapshot]` table of a `snapshot` request file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotTable {
    /// The name the snapshot is kept and compared under.
    label: String,
    planes: Vec<Plane>,
    /// Keep the snapshot's textures for later snapshots to compare with; otherwise they are released
    /// once its own comparisons are done.
    #[serde(default)]
    keep: bool,
    /// Labels of kept snapshots to compare this one with, plane by plane and eye by eye.
    #[serde(default)]
    compare: Vec<String>,
    /// The final image's per-pixel threshold in 8-bit codes.
    #[serde(default = "default_pixel_threshold")]
    pixel_threshold: f32,
    /// How many representable floats apart two depths may be and still match; zero (the default)
    /// counts any changed bit.
    #[serde(default)]
    depth_ulps: u32,
    /// The failure limits for the dump policy.
    #[serde(default = "default_fail_fraction")]
    fail_fraction: f64,
    #[serde(default = "default_fail_mean")]
    fail_mean: f64,
    #[serde(default)]
    dump: DumpPolicy,
    /// Dump only the differing region (grown by a margin) rather than the whole eye.
    #[serde(default = "default_true")]
    dump_crop: bool,
}

/// A validated snapshot request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SnapshotRequest {
    pub id: RequestId,
    pub label: String,
    /// The motion phase to take the snapshot at, under a motion configuration.
    pub phase: Option<u32>,
    pub planes: Vec<Plane>,
    pub keep: bool,
    pub compare: Vec<String>,
    pub pixel_threshold: f32,
    pub depth_ulps: u32,
    pub limits: FailLimits,
    pub dump: DumpPolicy,
    pub dump_crop: bool,
}

/// Why a snapshot table is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotError {
    Label,
    NoPlanes,
    NoFinal,
    RepeatedPlane(Plane),
    CompareLabel,
    TooManyComparisons(usize),
    Threshold,
    DepthTolerance,
    Limits,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "snapshot: ")?;
        match self {
            Self::Label => write!(
                f,
                "a label is 1-64 ASCII letters, digits, underscores, or hyphens"
            ),
            Self::NoPlanes => write!(f, "planes must name at least one plane"),
            Self::NoFinal => write!(f, "planes must include the final image"),
            Self::RepeatedPlane(plane) => write!(f, "plane {plane:?} is listed twice"),
            Self::CompareLabel => write!(f, "a compare label is not a valid label"),
            Self::TooManyComparisons(count) => write!(
                f,
                "{count} snapshots to compare with; at most {MAX_COMPARED} are allowed"
            ),
            Self::Threshold => write!(f, "pixel_threshold must be within [0, 255]"),
            Self::DepthTolerance => {
                write!(f, "depth_ulps must be at most {MAX_DEPTH_ULPS}")
            }
            Self::Limits => {
                write!(
                    f,
                    "fail_fraction must be within [0, 1] and fail_mean non-negative"
                )
            }
        }
    }
}

/// How many kept snapshots one snapshot may be compared with.
pub(crate) const MAX_COMPARED: usize = 8;

/// The largest depth tolerance a request may ask for, in representable floats: a relative
/// difference of about one part in sixteen thousand, beyond which a tolerance would hide displaced
/// geometry rather than rounding.
pub(crate) const MAX_DEPTH_ULPS: u32 = 512;

impl SnapshotRequest {
    pub(crate) fn parse(
        id: RequestId,
        phase: Option<u32>,
        table: SnapshotTable,
    ) -> Result<Self, SnapshotError> {
        if !valid_label(&table.label) {
            return Err(SnapshotError::Label);
        }
        if table.planes.is_empty() {
            return Err(SnapshotError::NoPlanes);
        }
        if !table.planes.contains(&Plane::Final) {
            return Err(SnapshotError::NoFinal);
        }
        let mut planes = table.planes.clone();
        planes.sort();
        if let Some(pair) = planes.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(SnapshotError::RepeatedPlane(pair[0]));
        }
        if table.compare.iter().any(|label| !valid_label(label)) {
            return Err(SnapshotError::CompareLabel);
        }
        if table.compare.len() > MAX_COMPARED {
            return Err(SnapshotError::TooManyComparisons(table.compare.len()));
        }
        if !(0.0..=255.0).contains(&table.pixel_threshold) {
            return Err(SnapshotError::Threshold);
        }
        if table.depth_ulps > MAX_DEPTH_ULPS {
            return Err(SnapshotError::DepthTolerance);
        }
        if !(0.0..=1.0).contains(&table.fail_fraction)
            || !table.fail_mean.is_finite()
            || table.fail_mean < 0.0
        {
            return Err(SnapshotError::Limits);
        }
        Ok(Self {
            id,
            label: table.label,
            phase,
            planes,
            keep: table.keep,
            compare: table.compare,
            pixel_threshold: table.pixel_threshold,
            depth_ulps: table.depth_ulps,
            limits: FailLimits {
                fraction: table.fail_fraction,
                mean: table.fail_mean,
            },
            dump: table.dump,
            dump_crop: table.dump_crop,
        })
    }
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn default_pixel_threshold() -> f32 {
    3.0
}

fn default_fail_fraction() -> f64 {
    0.001
}

fn default_fail_mean() -> f64 {
    0.25
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> SnapshotTable {
        toml::from_str(text).unwrap()
    }

    fn parse(text: &str) -> Result<SnapshotRequest, SnapshotError> {
        SnapshotRequest::parse(RequestId::parse("snap").unwrap(), None, table(text))
    }

    #[test]
    fn defaults_follow_the_python_thresholds() {
        let request = parse("label = \"left_a\"\nplanes = [\"final\", \"depth\"]\n").unwrap();
        assert_eq!(request.planes, vec![Plane::Final, Plane::Depth]);
        assert_eq!(request.pixel_threshold, 3.0);
        assert_eq!(request.depth_ulps, 0);
        assert_eq!(
            request.limits,
            FailLimits {
                fraction: 0.001,
                mean: 0.25
            }
        );
        assert_eq!(request.dump, DumpPolicy::Never);
        assert!(request.dump_crop);
        assert!(!request.keep);
    }

    #[test]
    fn full_table_parses() {
        let request = parse(
            "label = \"sp_a\"\nplanes = [\"velocity\", \"final\"]\nkeep = true\n\
             compare = [\"left_a\", \"right_a\"]\npixel_threshold = 5\ndump = \"on_failure\"\n\
             dump_crop = false\nfail_fraction = 0.01\nfail_mean = 1.0\n",
        )
        .unwrap();
        assert_eq!(request.planes, vec![Plane::Final, Plane::Velocity]);
        assert_eq!(request.compare, vec!["left_a", "right_a"]);
        assert_eq!(request.dump, DumpPolicy::OnFailure);
        assert!(request.keep && !request.dump_crop);
    }

    #[test]
    fn invalid_tables_are_refused() {
        assert_eq!(
            parse("label = \"\"\nplanes = [\"final\"]\n"),
            Err(SnapshotError::Label)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = []\n"),
            Err(SnapshotError::NoPlanes)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"depth\"]\n"),
            Err(SnapshotError::NoFinal)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"final\", \"depth\", \"depth\"]\n"),
            Err(SnapshotError::RepeatedPlane(Plane::Depth))
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"final\"]\ncompare = [\"no spaces\"]\n"),
            Err(SnapshotError::CompareLabel)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"final\"]\npixel_threshold = 300\n"),
            Err(SnapshotError::Threshold)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"final\"]\ndepth_ulps = 513\n"),
            Err(SnapshotError::DepthTolerance)
        );
        assert_eq!(
            parse("label = \"a\"\nplanes = [\"final\"]\nfail_fraction = 2\n"),
            Err(SnapshotError::Limits)
        );
        assert!(toml::from_str::<SnapshotTable>("label = \"a\"\nplanes = [\"sky\"]\n").is_err());
        assert!(
            toml::from_str::<SnapshotTable>("label = \"a\"\nplanes = [\"final\"]\nextra = 1\n")
                .is_err()
        );
    }
}
