"""Run a configured rendering-equivalence experiment against a live rendering session.

The experiment attaches to a running private rendering session, poses the camera at a configured world
transform, captures an ordered list of render configurations, and hands the resulting manifest to
the strict image comparator. An optional bright-region feature check records whether the target
configurations lose evidence that the reference configurations keep; it is reported separately and
can only make the overall outcome inconclusive, never pass.
"""

import argparse
import dataclasses
import json
import logging
import math
import os
import sys
import time
import tomllib
import uuid
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

import numpy as np

from tools.rendering.capture.suite import write_manifest
from tools.rendering.capture.validation import read_object, validate_capture
from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import (
    Outcome,
    SampleSpec,
    analyze_manifest,
    load_sample,
)
from tools.rendering.comparison.stages import Stage, load_stage
from tools.rendering.experiment_matched import (
    matched_capture_identity,
    matched_cross_metrics,
)
from tools.rendering.protocol import (
    POSE_TOLERANCE,
    Camera,
    Capture,
    CascadeScale,
    Case,
    Command,
    ConfigurationAck,
    EnvironmentObservation,
    EnvironmentRequest,
    Manifest,
    Mode,
    OrientationOffset,
    PoseRequest,
    ProtocolError,
    Request,
    RestorationAck,
    Sample,
    ScreenshotAck,
    SunDepthClip,
    TranslationOffset,
    VisualOverrides,
    WaterReflection,
    WeatherControl,
    WeatherEvent,
    composed_positions_match,
    offset_for_target,
    quaternions_match,
    valid_request_id,
)
from tools.rendering.session.attach import AttachedSession
from tools.rendering.session.errors import SessionError

LOG = logging.getLogger("tools.rendering.experiment")

RESTORATION_BUDGET = 35.0
SPACING_POLL_SECONDS = 0.2
ENVIRONMENT_HOUR_TOLERANCE = 0.05
ENVIRONMENT_MAX_WEATHER_SEVERITY = 0.5
MAX_NAME_LENGTH = 32


class ExperimentError(RuntimeError):
    pass


EXPECTED_ERRORS = (ExperimentError, ProtocolError, SessionError, OSError, ValueError,
                   TypeError)


class FeatureStatus(StrEnum):
    MATCH = "match"
    DIFFERENCE = "difference"
    INCONCLUSIVE = "inconclusive"


class FeatureRole(StrEnum):
    REFERENCE = "reference"
    TARGET = "target"


class FeatureEyeStatus(StrEnum):
    LOST = "lost"
    RETAINED = "retained"
    UNSTABLE = "unstable"
    UNOBSERVED = "unobserved"


@dataclass(frozen=True)
class WorldPose:
    position: tuple[float, float, float]
    orientation: tuple[float, float, float, float]

    def __post_init__(self):
        _vector(self.position, 3, "scene.position")
        _vector(self.orientation, 4, "scene.orientation")

    def camera(self) -> Camera:
        return Camera(self.position, self.orientation)


@dataclass(frozen=True)
class Environment:
    hour: float
    weather: WeatherEvent

    def __post_init__(self):
        EnvironmentRequest(self.hour, self.weather)

    def request(self) -> EnvironmentRequest:
        return EnvironmentRequest(self.hour, self.weather)

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "scene.environment")
        _keys(value, {"hour", "weather"}, set(), "scene.environment")
        try:
            weather = WeatherEvent(_string(value.get("weather"), "scene.environment.weather"))
        except ValueError as error:
            raise ExperimentError(
                "experiment: scene.environment.weather is not a supported weather event"
            ) from error
        return cls(_number(value.get("hour"), "scene.environment.hour"), weather)


@dataclass(frozen=True)
class Scene:
    pose: WorldPose
    environment: Environment | None

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "scene")
        _keys(value, {"position", "orientation"}, {"environment"}, "scene")
        pose = WorldPose(_vector(value.get("position"), 3, "scene.position"),
                         _vector(value.get("orientation"), 4, "scene.orientation"))
        environment = (Environment.from_toml(value["environment"])
                       if "environment" in value else None)
        return cls(pose, environment)


@dataclass(frozen=True)
class Feature:
    stage: Stage
    roi: tuple[float, float, float, float]
    luminance: float
    min_fraction: float
    reference_modes: tuple[CompareMode, ...]
    target_modes: tuple[CompareMode, ...]

    def __post_init__(self):
        if self.stage != Stage.MAIN_COLOR:
            raise ExperimentError(
                f"experiment: feature.stage {self.stage.value!r} is not supported"
            )
        x0, y0, x1, y1 = self.roi
        if not (0 <= x0 < x1 <= 1 and 0 <= y0 < y1 <= 1):
            raise ExperimentError(
                "experiment: feature.roi must be an increasing normalized rectangle"
            )
        if not math.isfinite(self.luminance) or self.luminance <= 0:
            raise ExperimentError(
                "experiment: feature.luminance must be finite and positive"
            )
        if not math.isfinite(self.min_fraction) or not 0 < self.min_fraction <= 1:
            raise ExperimentError(
                "experiment: feature.min_fraction must be within (0, 1]"
            )
        if not self.reference_modes or not self.target_modes:
            raise ExperimentError(
                "experiment: feature.reference_modes and feature.target_modes must be nonempty"
            )
        if set(self.reference_modes) & set(self.target_modes):
            raise ExperimentError(
                "experiment: feature reference and target modes must be disjoint"
            )

    def region(self, height, width):
        x0, y0, x1, y1 = self.roi
        columns = (max(0, int(x0 * width)), min(width, math.ceil(x1 * width)))
        rows = (max(0, int(y0 * height)), min(height, math.ceil(y1 * height)))
        if columns[1] <= columns[0] or rows[1] <= rows[0]:
            raise ExperimentError("experiment: feature.roi selects no pixels")
        return rows, columns

    def bright_fraction(self, values) -> float:
        if values.ndim != 3 or values.shape[2] < 3 or not values.size:
            raise ExperimentError("experiment: bright-region evidence is not an RGB image")
        luminance = (values[..., 0] * 0.2126 + values[..., 1] * 0.7152 +
                     values[..., 2] * 0.0722)
        (y0, y1), (x0, x1) = self.region(*luminance.shape[:2])
        return float(np.mean(luminance[y0:y1, x0:x1] >= self.luminance))

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "feature")
        _keys(value, {"stage", "roi", "luminance", "min_fraction", "reference_modes",
                      "target_modes"}, set(), "feature")
        try:
            stage = Stage(_string(value.get("stage"), "feature.stage"))
        except ValueError as error:
            raise ExperimentError("experiment: feature.stage is not a supported stage") from error
        return cls(stage,
                   tuple(_vector(value.get("roi"), 4, "feature.roi")),
                   _number(value.get("luminance"), "feature.luminance"),
                   _number(value.get("min_fraction"), "feature.min_fraction"),
                   _mode_list(value.get("reference_modes"), "feature.reference_modes"),
                   _mode_list(value.get("target_modes"), "feature.target_modes"))


@dataclass(frozen=True)
class Diagnostics:
    water_inputs: bool = False
    sun_inputs: bool = False
    shader_identity: bool = False
    resolve_inputs: bool = False
    sun_depth_clip: SunDepthClip | None = None
    water_reflection: WaterReflection | None = None
    water_tess_draw: int | None = None
    cascade_scale: CascadeScale | None = None

    def __post_init__(self):
        if type(self.water_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.water_inputs must be a boolean")
        if self.water_tess_draw is not None and (not self.water_inputs or type(self.water_tess_draw) is not int or not 0 <= self.water_tess_draw < 32):
            raise ExperimentError("experiment: water_tess_draw requires water_inputs and an index below 32")
        if type(self.sun_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.sun_inputs must be a boolean")
        if type(self.shader_identity) is not bool:
            raise ExperimentError("experiment: diagnostics.shader_identity must be a boolean")
        if type(self.resolve_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.resolve_inputs must be a boolean")
        if self.sun_depth_clip is not None and (not isinstance(self.sun_depth_clip, SunDepthClip) or not self.sun_inputs):
            raise ExperimentError("experiment: diagnostics.sun_depth_clip requires sun_inputs")
        if self.water_reflection is not None and (not isinstance(self.water_reflection, WaterReflection) or
                                                   not self.water_inputs or not self.shader_identity):
            raise ExperimentError("experiment: diagnostics.water_reflection requires water_inputs and shader_identity")
        if self.cascade_scale is not None and (not isinstance(self.cascade_scale, CascadeScale) or
                not self.resolve_inputs or not self.shader_identity):
            raise ExperimentError("experiment: cascade_scale requires resolve inputs and shader identity")
        if self.sun_depth_clip is not None and self.shader_identity:
            raise ExperimentError("experiment: sun depth clip treatment and shader census must use separate captures")

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "diagnostics")
        _keys(value, set(), {"water_inputs", "water_tess_draw", "sun_inputs", "shader_identity", "resolve_inputs", "sun_depth_clip", "water_reflection"}, "diagnostics")
        try:
            depth_clip = SunDepthClip(value["sun_depth_clip"]) if "sun_depth_clip" in value else None
            water_reflection = WaterReflection(value["water_reflection"]) if "water_reflection" in value else None
        except (ValueError, TypeError) as error:
            raise ExperimentError("experiment: invalid diagnostic treatment") from error
        return cls(value.get("water_inputs", False), value.get("sun_inputs", False),
                   value.get("shader_identity", False), value.get("resolve_inputs", False), depth_clip, water_reflection, value.get("water_tess_draw"))


class MatchedTreatment(StrEnum):
    WATER_REFLECTION_ABBA = "water_reflection_abba"
    PLANAR_ABBA = "planar_abba"
    CASCADE_ABBA = "cascade_abba"

    @property
    def conditions(self) -> tuple[str, str]:
        return {
            self.CASCADE_ABBA: (CascadeScale.SHORE_LOW.value, CascadeScale.SHORE_HIGH.value),
            self.PLANAR_ABBA: ("control", WaterReflection.CENTER_PLANAR.value),
            self.WATER_REFLECTION_ABBA: ("control", WaterReflection.ZERO_WEIGHT.value),
        }[self]


@dataclass(frozen=True)
class ExperimentConfig:
    name: str
    warmup_frames: int
    samples: int
    scene: Scene
    configurations: tuple[CompareMode, ...]
    feature: Feature | None
    interval_seconds: float = 0.0
    diagnostics: Diagnostics = dataclasses.field(default_factory=Diagnostics)
    visual_overrides: VisualOverrides | None = None
    matched_treatment: MatchedTreatment | None = None

    def __post_init__(self):
        if not valid_request_id(self.name) or len(self.name) > MAX_NAME_LENGTH:
            raise ExperimentError(
                "experiment: config.name must be an ASCII request ID of at most "
                f"{MAX_NAME_LENGTH} characters"
            )
        if type(self.warmup_frames) is not int or not 1 <= self.warmup_frames <= 300:
            raise ExperimentError(
                "experiment: config.warmup_frames must be an integer from 1 through 300"
            )
        if type(self.samples) is not int or self.samples < 3:
            raise ExperimentError("experiment: config.samples must be an integer of at least three")
        if not self.configurations:
            raise ExperimentError("experiment: config.configurations must not be empty")
        if (type(self.interval_seconds) not in (int, float)
                or not math.isfinite(self.interval_seconds) or self.interval_seconds < 0):
            raise ExperimentError(
                "experiment: config.interval_seconds must be a finite nonnegative number"
            )
        object.__setattr__(self, "interval_seconds", float(self.interval_seconds))
        if not isinstance(self.diagnostics, Diagnostics):
            raise ExperimentError("experiment: config.diagnostics must be a diagnostics table")
        if self.matched_treatment is not None:
            if (not isinstance(self.matched_treatment, MatchedTreatment) or self.feature is not None or
                    not self.diagnostics.shader_identity or self.diagnostics.water_reflection is not None or
                    self.diagnostics.cascade_scale is not None or self.diagnostics.sun_depth_clip is not None):
                raise ExperimentError("experiment: matched conditions require shader identity and no other treatment")
            if self.matched_treatment == MatchedTreatment.PLANAR_ABBA:
                if self.configurations not in ((CompareMode.SINGLE_PASS,), (CompareMode.LEFT_ONLY,
                        CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS, CompareMode.SINGLE_PASS)):
                    raise ExperimentError("experiment: planar ABBA requires SPS, optionally after both isolated eyes and MP")
            elif self.configurations != (CompareMode.MULTIPASS,):
                raise ExperimentError("experiment: matched MP treatments require one multipass configuration")
            if self.matched_treatment == MatchedTreatment.CASCADE_ABBA:
                if not self.diagnostics.resolve_inputs:
                    raise ExperimentError("experiment: cascade ABBA requires resolve inputs")
            elif not self.diagnostics.water_inputs:
                raise ExperimentError("experiment: matched water treatments require water inputs")
        if self.diagnostics.cascade_scale is not None and self.configurations != (CompareMode.MULTIPASS,):
            raise ExperimentError("experiment: cascade_scale requires one multipass configuration")
        if ((self.diagnostics.water_reflection == WaterReflection.CENTER_PLANAR and self.configurations != (CompareMode.SINGLE_PASS,)) or
                ((self.diagnostics.water_reflection == WaterReflection.CENTER_PLANAR or self.matched_treatment == MatchedTreatment.PLANAR_ABBA) and self.diagnostics.water_tess_draw is None)):
            raise ExperimentError("experiment: planar treatment requires a selected single-pass draw")
        if self.diagnostics.water_reflection == WaterReflection.ZERO_WEIGHT and any(mode != CompareMode.MULTIPASS for mode in self.configurations):
            raise ExperimentError("experiment: water reflection treatment requires multipass configurations")
        if self.visual_overrides is not None and not isinstance(self.visual_overrides, VisualOverrides):
            raise ExperimentError("experiment: invalid visual overrides")
        if self.feature is None:
            return
        references = set(self.feature.reference_modes)
        targets = set(self.feature.target_modes)
        missing = (references | targets) - set(self.configurations)
        if missing:
            names = ", ".join(sorted(mode.value for mode in missing))
            raise ExperimentError(
                f"experiment: feature modes {names} are not configured"
            )
        last_reference = max(
            index for index, mode in enumerate(self.configurations) if mode in references
        )
        first_target = min(
            index for index, mode in enumerate(self.configurations) if mode in targets
        )
        if last_reference > first_target:
            raise ExperimentError(
                "experiment: feature reference modes must be ordered before target modes"
            )
        reference_eyes = {eye for mode in references for eye in mode.eyes}
        target_eyes = {eye for mode in targets for eye in mode.eyes}
        if not target_eyes <= reference_eyes:
            raise ExperimentError(
                "experiment: feature reference modes must cover every target-mode eye"
            )

    @classmethod
    def load(cls, path) -> "ExperimentConfig":
        try:
            with Path(path).open("rb") as handle:
                value = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise ExperimentError(f"experiment: could not read config {path}: {error}") from error
        _keys(value, {"name", "warmup_frames", "samples", "scene", "configurations"},
              {"feature", "interval_seconds", "diagnostics", "visual_overrides", "matched_treatment"}, str(path))
        try:
            matched = MatchedTreatment(value["matched_treatment"]) if "matched_treatment" in value else None
        except (ValueError, TypeError) as error:
            raise ExperimentError("experiment: invalid matched_treatment") from error
        return cls(_string(value.get("name"), "config.name"),
                   _integer(value.get("warmup_frames"), "config.warmup_frames"),
                   _integer(value.get("samples"), "config.samples"),
                   Scene.from_toml(value.get("scene")),
                   _configuration_list(value.get("configurations")),
                   (Feature.from_toml(value["feature"]) if "feature" in value else None),
                   _number(value.get("interval_seconds", 0.0), "config.interval_seconds"),
                   Diagnostics.from_toml(value.get("diagnostics", {})),
                   VisualOverrides.from_wire(value["visual_overrides"]) if "visual_overrides" in value else None,
                   matched)


@dataclass(frozen=True)
class CaptureSummary:
    mode: CompareMode
    capture_id: str
    frame_ordinal: int
    condition: str | None = None


@dataclass(frozen=True)
class FeatureObservation:
    mode: CompareMode
    capture_id: str
    role: FeatureRole
    eyes: tuple[int, ...]
    bright_fraction: tuple[float, ...]
    bright: tuple[bool, ...]
    reasons: tuple[str, ...]


@dataclass(frozen=True)
class FeatureEyeEvidence:
    eye: int
    status: FeatureEyeStatus
    reference_bright: tuple[bool, ...]
    target_bright: tuple[bool, ...]


@dataclass(frozen=True)
class FeatureReport:
    status: FeatureStatus
    stage: str
    roi: tuple[float, float, float, float]
    luminance: float
    min_fraction: float
    reference_modes: tuple[CompareMode, ...]
    target_modes: tuple[CompareMode, ...]
    observations: tuple[FeatureObservation, ...]
    eyes: tuple[FeatureEyeEvidence, ...]
    findings: tuple[str, ...]
    errors: tuple[str, ...]


@dataclass(frozen=True)
class ExperimentReport:
    name: str
    outcome: Outcome
    comparison_outcome: Outcome | None
    manifest: str
    comparison_report: str | None
    captures: tuple[CaptureSummary, ...]
    feature: FeatureReport | None
    findings: tuple[str, ...]
    errors: tuple[str, ...]
    restoration_error: str | None
    environment_restoration: str
    interval_seconds: float = 0.0
    diagnostics: Diagnostics = dataclasses.field(default_factory=Diagnostics)
    visual_overrides: VisualOverrides | None = None
    matched_treatment: MatchedTreatment | None = None
    matched_cross_condition: tuple[dict, ...] = ()

    @property
    def exit_code(self) -> int:
        return {Outcome.PASS: 0, Outcome.FAIL: 1, Outcome.INCONCLUSIVE: 2}[self.outcome]

    def to_wire(self):
        return dataclasses.asdict(self)


def sample_id(prefix: str, mode: CompareMode, index: int) -> str:
    return f"{prefix}_{mode.value}_{index}"


def run_experiment(runner, config: ExperimentConfig, output: Path, capture=None,
                   analyze=None, monotonic=None, sleep=None) -> ExperimentReport:
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    capture = capture or _capture
    analyze = analyze or _analyze
    monotonic = monotonic or time.monotonic
    sleep = sleep or time.sleep
    prefix = f"{config.name.replace('-', '_')}_{uuid.uuid4().hex[:8]}"
    matched = config.matched_treatment
    names = matched.conditions if matched else ("default",)
    cases = {name: Case(f"{config.name}-{name.replace('_', '-')}" if matched else config.name, {}) for name in names}
    manifest = Manifest(list(cases.values()))
    manifest_path = output / "manifest.json"
    captures: list[Capture] = []
    configurations: list[ConfigurationAck] = []
    anchor = None
    restoration_error = None
    environment_restoration = ("unobserved" if config.scene.environment is not None
                               else "not_requested")
    failure = None
    early_errors: list[str] = []
    last_capture_completed = None
    conditions: dict[str, str] = {}
    matched_identity = None
    try:
        anchor_mode = Mode(config.configurations[0].value)
        anchor_pose = PoseRequest(f"{prefix}_anchor", OrientationOffset(0.0, 0.0, 0.0),
                                  TranslationOffset())
        _, anchor = _configure(runner, config, f"{prefix}_anchor", anchor_pose, anchor_mode,
                               anchor, configurations)
        offset, translation = offset_for_target(anchor, config.scene.pose.camera())
        target_pose = PoseRequest(f"{prefix}_target", offset, translation)
        for item in cases.values():
            item.expected_pose = target_pose
        counters: dict[CompareMode, int] = {}
        for entry_index, configured_mode in enumerate(config.configurations, start=1):
            mode = Mode(configured_mode.value)
            _, anchor = _configure(
                runner, config, f"{prefix}_{mode.value}_configure_{entry_index}", target_pose,
                mode, anchor, configurations)
            shared = config.matched_treatment == MatchedTreatment.PLANAR_ABBA and mode != Mode.SINGLE_PASS
            sequence = ((names[0],) * config.samples if shared else
                        (names[0], names[1], names[1], names[0]) * config.samples
                        if config.matched_treatment else ("default",) * config.samples)
            for condition in sequence:
                entries = cases[condition].modes.setdefault(mode, [])
                counters[configured_mode] = counters.get(configured_mode, 0) + 1
                identifier = sample_id(prefix, configured_mode, counters[configured_mode])
                _wait_for_capture_interval(runner, config.interval_seconds,
                                           last_capture_completed, monotonic, sleep)
                runner.assert_running()
                if runner.deadline is not None and monotonic() >= runner.deadline:
                    raise ExperimentError(
                        "experiment: no capture can start after the run deadline"
                    )
                capture_config = config
                if shared:
                    capture_config = dataclasses.replace(config, matched_treatment=None)
                elif config.matched_treatment:
                    if matched == MatchedTreatment.CASCADE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, cascade_scale=CascadeScale(condition))
                    else:
                        treatment = None if condition == "control" else WaterReflection(condition)
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=treatment)
                    capture_config = dataclasses.replace(
                        config, diagnostics=diagnostics,
                        matched_treatment=None, configurations=(configured_mode,))
                try:
                    result = capture(runner, capture_config, target_pose, anchor, mode,
                                     configurations[-1].generation, identifier)
                except EXPECTED_ERRORS:
                    folder = runner.config_dir / "captures"
                    metadata_path = folder / f"{identifier}.capture.json"
                    if config.matched_treatment and metadata_path.exists():
                        entries.append(Sample(os.path.relpath(folder / f"{identifier}.capture.png", output),
                                              os.path.relpath(metadata_path, output)))
                        write_manifest(manifest_path, manifest)
                    raise
                if not isinstance(result, Capture):
                    raise ExperimentError("experiment: capture is not a diagnostic capture")
                previous = captures[-1] if captures else None
                captures.append(result)
                conditions[result.id] = condition
                last_capture_completed = monotonic()
                folder = runner.config_dir / "captures"
                entries.append(Sample(
                    os.path.relpath(folder / f"{identifier}.capture.png", output),
                    os.path.relpath(folder / f"{identifier}.capture.json", output)))
                write_manifest(manifest_path, manifest)
                if previous is not None and (result.frame_ordinal <= previous.frame_ordinal or
                                             result.predicted_display_time_ns <= previous.predicted_display_time_ns):
                    raise ExperimentError(
                        "experiment: captures do not identify increasing rendered frames"
                    )
                if shared:
                    cases[names[1]].modes.setdefault(mode, []).append(entries[-1])
                    write_manifest(manifest_path, manifest)
                elif config.matched_treatment:
                    metadata = read_object(folder / f"{identifier}.capture.json")
                    identity = matched_capture_identity(result, metadata, configurations[-1].generation)
                    if matched_identity is None:
                        matched_identity = identity
                    elif identity != matched_identity:
                        raise ExperimentError(f"experiment: matched capture {identifier} changed pose or configuration")
                LOG.info("wrote %s after %s", manifest_path, identifier)
            references = (set(config.feature.reference_modes)
                          if config.feature is not None else set())
            if (config.feature is not None and configured_mode in references and
                    not any(mode in references
                            for mode in config.configurations[entry_index:])):
                early_errors = list(_reference_eligibility(
                    config, runner.config_dir / "captures", captures))
                if early_errors:
                    LOG.warning("experiment: reference eligibility failed; "
                                "skipping target configurations")
                    break
    except EXPECTED_ERRORS as error:
        failure = error
    finally:
        deadline = runner.deadline
        try:
            runner.deadline = max(deadline, time.monotonic() + RESTORATION_BUDGET)
            runner.cancel_pending_request()
            restoration = runner.request(Request(Command.END, f"{prefix}_end"))
            if not isinstance(restoration, RestorationAck):
                raise ExperimentError(
                    "experiment: expected a restoration acknowledgement"
                )
            if restoration.id != f"{prefix}_end":
                raise ExperimentError("experiment: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise ExperimentError("experiment: restoration generation does not increase")
            observed = restoration.metadata.environment
            if config.scene.environment is not None:
                if observed is None or observed.weather_control is None:
                    raise ExperimentError("experiment: environment restoration is unobserved")
                environment_restoration = observed.weather_control.value
                if observed.weather_control != WeatherControl.AMBIENT:
                    raise ExperimentError(
                        "experiment: weather control was not released to ambient"
                    )
        except EXPECTED_ERRORS as error:
            restoration_error = str(error)
            LOG.error("experiment: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
    if failure is not None:
        errors = [str(failure)]
        if restoration_error is not None:
            errors.append(f"restoration: {restoration_error}")
        report = _failure_report(config, manifest_path, captures, tuple(errors),
                                 restoration_error, environment_restoration, conditions)
        try:
            write_report(output / "report.json", report)
        except OSError as error:
            LOG.error("experiment: could not write the failure report: %s", error)
        return report
    feature = (_feature_report(config, runner.config_dir / "captures", captures)
               if config.feature is not None else None)
    comparison_report = None
    comparison_outcome = None
    errors: list[str] = []
    if restoration_error is not None:
        errors.append(f"restoration: {restoration_error}")
    errors.extend(early_errors)
    if config.diagnostics.resolve_inputs:
        for captured in captures:
            try:
                metadata = read_object(runner.config_dir / "captures" / f"{captured.id}.capture.json")
                errors.extend(_resolve_content_errors(captured.id, metadata))
            except (*EXPECTED_ERRORS, KeyError, IndexError) as error:
                errors.append(f"resolve inputs: {captured.id}: {error}")
    if captures:
        comparison_output = output / "comparison"
        try:
            report = analyze(manifest_path, comparison_output)
            comparison_outcome = Outcome(report.status)
            comparison_report = str(comparison_output / "report.json")
        except EXPECTED_ERRORS as error:
            errors.append(f"comparison analysis: {error}")
    else:
        errors.append("comparison analysis: no captures were written")
    matched_cross = ()
    if config.matched_treatment and captures:
        try:
            matched_cross = matched_cross_metrics(output, cases)
        except EXPECTED_ERRORS as error:
            errors.append(f"matched treatment comparison: {error}")
    if feature is not None:
        errors.extend(feature.errors)
    overall = comparison_outcome or Outcome.INCONCLUSIVE
    if errors or (feature is not None and feature.status == FeatureStatus.INCONCLUSIVE):
        overall = Outcome.INCONCLUSIVE
    report = ExperimentReport(
        config.name, overall, comparison_outcome, str(manifest_path), comparison_report,
        tuple(CaptureSummary(CompareMode(capture.mode.value), capture.id,
                             capture.frame_ordinal, conditions.get(capture.id) if config.matched_treatment else None) for capture in captures),
        feature, () if feature is None else feature.findings,
        tuple(dict.fromkeys(errors)), restoration_error, environment_restoration,
        config.interval_seconds, config.diagnostics, config.visual_overrides, config.matched_treatment,
        matched_cross)
    write_report(output / "report.json", report)
    return report


def _resolve_content_errors(capture_id: str, metadata: dict) -> list[str]:
    errors = []
    for eye, result in enumerate(metadata["resolve_inputs"]["eyes"]):
        if result["status"] != "observed":
            continue
        for buffer in result["observation"]["constant_buffers"]:
            if buffer["slot"] not in (0, 3):
                continue
            content = buffer["content"]
            if content["status"] != "captured":
                errors.append(f"resolve inputs: {capture_id}: eye {eye} cb{buffer['slot']}: {content['status']}")
    return errors


def _wait_for_capture_interval(runner, interval_seconds, last_completed, monotonic, sleep):
    if last_completed is None or interval_seconds <= 0:
        return
    target = last_completed + interval_seconds
    while True:
        runner.assert_running()
        now = monotonic()
        deadline = runner.deadline
        if deadline is not None and now >= deadline:
            raise ExperimentError("experiment: capture interval reached the run deadline")
        if now >= target:
            return
        wait = min(SPACING_POLL_SECONDS, target - now)
        if deadline is not None:
            wait = min(wait, max(0.0, deadline - now))
        sleep(max(0.0, wait))


def _failure_report(config, manifest_path, captures, errors, restoration_error,
                    environment_restoration, conditions):
    return ExperimentReport(
        config.name, Outcome.INCONCLUSIVE, None, str(manifest_path), None,
        tuple(CaptureSummary(CompareMode(capture.mode.value), capture.id,
                             capture.frame_ordinal, conditions.get(capture.id) if config.matched_treatment else None) for capture in captures),
        None, (), errors, restoration_error, environment_restoration,
        config.interval_seconds, config.diagnostics, config.visual_overrides, config.matched_treatment)


def _feature_report(config: ExperimentConfig, root: Path, captures) -> FeatureReport | None:
    feature = config.feature
    if feature is None:
        return None
    references = set(feature.reference_modes)
    targets = set(feature.target_modes)
    observations: list[FeatureObservation] = []
    errors: list[str] = []
    findings: list[str] = []
    counters: dict[CompareMode, int] = {}
    for capture in captures:
        mode = CompareMode(capture.mode.value)
        if mode not in references and mode not in targets:
            continue
        counters[mode] = counters.get(mode, 0) + 1
        observations.append(_feature_observation(config, root, capture, counters[mode]))
    present = {observation.mode for observation in observations}
    for mode in feature.reference_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no reference capture was recorded")
    for mode in feature.target_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no target capture was recorded")
    for observation in observations:
        errors.extend(f"{observation.capture_id}: {reason}" for reason in observation.reasons)
    reference_eyes = {eye for mode in feature.reference_modes for eye in mode.eyes}
    target_eyes = tuple(sorted({eye for mode in feature.target_modes for eye in mode.eyes}))
    eye_evidence: list[FeatureEyeEvidence] = []
    unstable = False
    lost = False
    for eye in target_eyes:
        reference_bright = tuple(
            observation.bright[observation.eyes.index(eye)]
            for observation in observations
            if observation.role == FeatureRole.REFERENCE and eye in observation.eyes
        )
        target_bright = tuple(
            observation.bright[observation.eyes.index(eye)]
            for observation in observations
            if observation.role == FeatureRole.TARGET and eye in observation.eyes
        )
        if eye not in reference_eyes:
            errors.append(f"eye {eye}: no reference mode covers this eye")
            status = FeatureEyeStatus.UNOBSERVED
        elif not reference_bright or not target_bright:
            errors.append(f"eye {eye}: reference or target captures are missing")
            status = FeatureEyeStatus.UNOBSERVED
        elif not all(reference_bright):
            errors.append(f"eye {eye}: reference lacks the bright region")
            status = FeatureEyeStatus.UNOBSERVED
        elif all(target_bright):
            status = FeatureEyeStatus.RETAINED
        elif not any(target_bright):
            status = FeatureEyeStatus.LOST
            lost = True
        else:
            status = FeatureEyeStatus.UNSTABLE
            unstable = True
        eye_evidence.append(FeatureEyeEvidence(eye, status, reference_bright, target_bright))
    if errors or unstable:
        status = FeatureStatus.INCONCLUSIVE
        if unstable:
            findings.append("target brightness is unstable across samples")
    elif lost:
        status = FeatureStatus.DIFFERENCE
        findings.append("target modes differ from the reference bright region")
    else:
        status = FeatureStatus.MATCH
        findings.append("target modes match the reference bright region")
    return FeatureReport(
        status, feature.stage.value, feature.roi, feature.luminance, feature.min_fraction,
        feature.reference_modes, feature.target_modes,
        tuple(observations), tuple(eye_evidence), tuple(findings), tuple(errors))


def _reference_eligibility(config: ExperimentConfig, root: Path, captures):
    feature = config.feature
    if feature is None:
        return ()
    references = set(feature.reference_modes)
    observations: list[FeatureObservation] = []
    counters: dict[CompareMode, int] = {}
    for capture in captures:
        mode = CompareMode(capture.mode.value)
        if mode not in references:
            continue
        counters[mode] = counters.get(mode, 0) + 1
        observations.append(_feature_observation(config, root, capture, counters[mode]))
    errors: list[str] = []
    present = {observation.mode for observation in observations}
    for mode in feature.reference_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no reference capture was recorded")
    for observation in observations:
        errors.extend(f"{observation.capture_id}: {reason}"
                      for reason in observation.reasons)
    for eye in sorted({eye for mode in feature.reference_modes for eye in mode.eyes}):
        values = [observation.bright[observation.eyes.index(eye)]
                  for observation in observations if eye in observation.eyes]
        if not values:
            errors.append(f"eye {eye}: no reference capture was recorded")
        elif not all(values):
            errors.append(f"eye {eye}: reference lacks the bright region")
    return tuple(dict.fromkeys(errors))


def _feature_observation(config: ExperimentConfig, root: Path, capture: Capture,
                         index: int) -> FeatureObservation:
    feature = config.feature
    mode = CompareMode(capture.mode.value)
    role = (FeatureRole.REFERENCE if mode in feature.reference_modes
            else FeatureRole.TARGET)
    reasons: list[str] = []
    if not _matches_world_pose(config, capture.comparison_inputs.camera):
        reasons.append("camera differs from the configured world pose")
    observation = capture.visual.environment
    if config.scene.environment is not None:
        if observation is None:
            reasons.append("environment observation is unavailable")
        else:
            _check_observation(config, capture.id, observation, reasons)
    try:
        fractions = _bright_fractions(feature, root, capture, index)
    except (ExperimentError, OSError, ValueError, TypeError, ProtocolError) as error:
        reasons.append(str(error))
        fractions = ()
    bright = tuple(value >= feature.min_fraction for value in fractions)
    return FeatureObservation(mode, capture.id, role, tuple(mode.eyes), fractions, bright,
                              tuple(reasons))


def _configure(runner, config: ExperimentConfig, request_id, pose, mode, anchor, configurations):
    environment = config.scene.environment.request() if config.scene.environment else None
    request = Request(Command.CONFIGURE, request_id, mode, config.warmup_frames, pose,
                      environment=environment, visual_overrides=config.visual_overrides,
                      shader_identity=True if config.diagnostics.shader_identity else None)
    acknowledgement = runner.request(request)
    if not isinstance(acknowledgement, ConfigurationAck):
        raise ExperimentError("experiment: expected a configuration acknowledgement")
    if (acknowledgement.id != request.id or acknowledgement.mode != mode or
            acknowledgement.warmup_frames != config.warmup_frames):
        raise ExperimentError(
            "experiment: configuration acknowledgement disagrees with its request"
        )
    if config.diagnostics.shader_identity and (acknowledgement.metadata.shader_identity is None or
            acknowledgement.metadata.shader_identity.get("status") != "complete"):
        raise ExperimentError("experiment: shader identity census is not complete")
    if acknowledgement.metadata.visual_overrides != config.visual_overrides:
        raise ExperimentError("experiment: configuration visual overrides disagree with the request")
    if configurations and acknowledgement.generation <= configurations[-1].generation:
        raise ExperimentError("experiment: configuration generation does not increase")
    anchor = acknowledgement.validate_pose(request, anchor)
    configurations.append(acknowledgement)
    LOG.info("configured %s in %s", request_id, mode.value)
    return acknowledgement, anchor


def _capture(runner, config, pose, anchor, mode, generation, identifier):
    acknowledgement = runner.request(Request(Command.SCREENSHOT, identifier,
                                             water_inputs=True if config.diagnostics.water_inputs else None,
                                             water_tess_draw=config.diagnostics.water_tess_draw,
                                             sun_inputs=True if config.diagnostics.sun_inputs else None,
                                             resolve_inputs=True if config.diagnostics.resolve_inputs else None,
                                             sun_depth_clip=config.diagnostics.sun_depth_clip,
                                             cascade_scale=config.diagnostics.cascade_scale,
                                             water_reflection=config.diagnostics.water_reflection))
    if not isinstance(acknowledgement, ScreenshotAck):
        raise ExperimentError("experiment: expected a screenshot acknowledgement")
    capture = validate_capture(runner.config_dir / "captures", identifier,
                               acknowledgement.capture, mode=mode, generation=generation,
                               water_inputs=config.diagnostics.water_inputs,
                               water_tess_draw=config.diagnostics.water_tess_draw,
                               sun_inputs=config.diagnostics.sun_inputs,
                               shader_identity=config.diagnostics.shader_identity,
                               resolve_inputs=config.diagnostics.resolve_inputs,
                               sun_depth_clip=config.diagnostics.sun_depth_clip,
                               cascade_scale=config.diagnostics.cascade_scale,
                               water_reflection=config.diagnostics.water_reflection)
    if not isinstance(capture, Capture):
        raise ExperimentError("experiment: expected a diagnostic capture")
    if config.diagnostics.shader_identity and (capture.visual.shader_identity is None or
            capture.visual.shader_identity.get("status") != "complete"):
        raise ExperimentError("experiment: capture shader identity census is not complete")
    if capture.visual.visual_overrides != config.visual_overrides:
        raise ExperimentError("experiment: capture visual overrides disagree with the request")
    if capture.visual.pose is None:
        raise ExperimentError(f"experiment: capture {identifier} has no pose evidence")
    capture.visual.pose.validate(pose, anchor)
    return capture


def _analyze(manifest_path: Path, comparison_output: Path):
    return analyze_manifest(manifest_path, comparison_output)


def _matches_world_pose(config: ExperimentConfig, camera: Camera) -> bool:
    target = config.scene.pose.camera()
    return (composed_positions_match(target.position, camera.position, target.position,
                                     POSE_TOLERANCE) and
            quaternions_match(target.orientation, camera.orientation, POSE_TOLERANCE))


def _check_observation(config: ExperimentConfig, identifier, observation: EnvironmentObservation,
                       errors: list[str]):
    environment = config.scene.environment
    if environment is None:
        return
    if abs(observation.hour - environment.hour) > ENVIRONMENT_HOUR_TOLERANCE:
        errors.append(f"observed hour {observation.hour:.4f} differs from "
                      f"{environment.hour:.4f}")
    if observation.weather_severity > ENVIRONMENT_MAX_WEATHER_SEVERITY:
        errors.append(f"observed weather severity {observation.weather_severity:.3f} "
                      f"is not clear")
    if observation.weather_control is None:
        errors.append("weather control is unobserved")
    elif observation.weather_control != WeatherControl.SUNNY:
        errors.append(f"weather control is {observation.weather_control.value}, not sunny")


def _bright_fractions(feature: Feature, root: Path, capture: Capture, index: int):
    compare_mode = CompareMode(capture.mode.value)
    spec = SampleSpec(Path(f"{capture.id}.capture.png"), Path(f"{capture.id}.capture.json"))
    sample = load_sample(root, compare_mode, index, spec)
    fractions = []
    for eye in compare_mode.eyes:
        values = load_stage(root, compare_mode, spec, sample, feature.stage, eye).values
        fractions.append(feature.bright_fraction(values))
    return tuple(fractions)


def write_report(path: Path, report: ExperimentReport):
    temporary = Path(path).with_suffix(".tmp")
    temporary.write_text(json.dumps(report.to_wire(), indent=2) + "\n")
    temporary.replace(path)
    LOG.info("wrote %s", path)


def _configuration_list(value):
    if not isinstance(value, list) or not value:
        raise ExperimentError("experiment: config.configurations must be a nonempty array")
    modes = []
    for index, entry in enumerate(value):
        entry = _table(entry, f"configurations[{index}]")
        _keys(entry, {"mode"}, set(), f"configurations[{index}]")
        modes.append(_mode(entry.get("mode"), f"configurations[{index}].mode"))
    return tuple(modes)


def _mode(value, context) -> CompareMode:
    try:
        return CompareMode(_string(value, context))
    except ValueError as error:
        raise ExperimentError(f"experiment: {context} is not a supported mode") from error


def _mode_list(value, context):
    if not isinstance(value, list) or not value:
        raise ExperimentError(f"experiment: {context} must be a nonempty array")
    modes = tuple(_mode(item, f"{context}[{index}]") for index, item in enumerate(value))
    if len(set(modes)) != len(modes):
        raise ExperimentError(f"experiment: {context} must not repeat a mode")
    return modes


def _table(value, context):
    if not isinstance(value, dict):
        raise ExperimentError(f"experiment: {context} must be a table")
    return value


def _keys(value, required, optional, context):
    missing, extra = required - set(value), set(value) - required - optional
    if missing or extra:
        raise ExperimentError(f"experiment: {context} has missing {sorted(missing)} "
                              f"and unknown {sorted(extra)} keys")


def _string(value, context):
    if not isinstance(value, str) or not value:
        raise ExperimentError(f"experiment: {context} must be a nonempty string")
    return value


def _number(value, context):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ExperimentError(f"experiment: {context} must be a finite number")
    return float(value)


def _integer(value, context):
    if type(value) is not int:
        raise ExperimentError(f"experiment: {context} must be an integer")
    return value


def _vector(value, length, context):
    if not isinstance(value, (list, tuple)) or len(value) != length:
        raise ExperimentError(f"experiment: {context} must have {length} components")
    return tuple(_number(item, f"{context}[{index}]") for index, item in enumerate(value))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Run a configured rendering-equivalence experiment.")
    parser.add_argument("--runner", type=Path, required=True,
                        help="runner.json from the running private rendering session")
    parser.add_argument("--config", type=Path, required=True,
                        help="scene configuration for the experiment")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=600.0)
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s",
                        stream=sys.stderr)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        LOG.error("experiment: timeout must be finite and positive")
        return 1
    try:
        config = ExperimentConfig.load(args.config)
        with AttachedSession.attach(args.runner, args.timeout) as runner:
            LOG.info("attached to game pid %d via %s", runner.pid, args.runner)
            report = run_experiment(runner, config, args.output)
    except (ExperimentError, ProtocolError, SessionError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 1
    LOG.info("experiment: %s (%s)", report.outcome.value, args.output)
    return report.exit_code


if __name__ == "__main__":
    sys.exit(main())
