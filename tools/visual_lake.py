# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///
"""Reproduce the missing right-eye water specular in single-pass stereo at a fixed world pose.

Attaches to a running visual harness session, configures the fixed world pose and the clear
weather at the reference time of day, captures single-pass and multipass repeats, and gates the
result on bright-region evidence decoded from the existing HDR stage capture.
"""

import argparse
import dataclasses
import fcntl
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

if __package__:
    from .visual_compare import Mode as CompareMode
    from .visual_compare import SampleSpec, load_sample
    from .visual_harness import HarnessError, VisualHarness, validate_capture
    from .visual_processes import processes
    from .visual_protocol import (
        POSE_TOLERANCE,
        Camera,
        Capture,
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
        TranslationOffset,
        WeatherControl,
        WeatherEvent,
        composed_positions_match,
        offset_for_target,
        quaternions_match,
        valid_request_id,
    )
    from .visual_stages import Stage, load_stage
    from .visual_suite import write_manifest
else:
    from visual_compare import Mode as CompareMode
    from visual_compare import SampleSpec, load_sample
    from visual_harness import HarnessError, VisualHarness, validate_capture
    from visual_processes import processes
    from visual_protocol import (
        POSE_TOLERANCE,
        Camera,
        Capture,
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
        TranslationOffset,
        WeatherControl,
        WeatherEvent,
        composed_positions_match,
        offset_for_target,
        quaternions_match,
        valid_request_id,
    )
    from visual_stages import Stage, load_stage
    from visual_suite import write_manifest

LOG = logging.getLogger("visual_lake")

DEFAULT_CONFIG = Path(__file__).with_name("visual_lake.toml")
RESTORATION_BUDGET = 35.0


class LakeError(RuntimeError):
    pass


class LakeOutcome(StrEnum):
    REPRODUCED = "reproduced"
    NOT_REPRODUCED = "not_reproduced"
    INCONCLUSIVE = "inconclusive"

    @property
    def exit_code(self) -> int:
        return {LakeOutcome.REPRODUCED: 0, LakeOutcome.NOT_REPRODUCED: 1,
                LakeOutcome.INCONCLUSIVE: 2}[self]


@dataclass(frozen=True)
class WorldPose:
    position: tuple[float, float, float]
    orientation: tuple[float, float, float, float]

    def __post_init__(self):
        _vector(self.position, 3, "pose.position")
        _vector(self.orientation, 4, "pose.orientation")

    def camera(self) -> Camera:
        return Camera(self.position, self.orientation)

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "pose")
        _exact(value, {"position", "orientation"}, "pose")
        return cls(_vector(value.get("position"), 3, "pose.position"),
                   _vector(value.get("orientation"), 4, "pose.orientation"))


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
        value = _table(value, "environment")
        _exact(value, {"hour", "weather"}, "environment")
        try:
            weather = WeatherEvent(_string(value.get("weather"), "environment.weather"))
        except ValueError as error:
            raise LakeError("visual lake: unknown environment weather") from error
        return cls(_number(value.get("hour"), "environment.hour"), weather)


@dataclass(frozen=True)
class Gate:
    roi: tuple[float, float, float, float]
    luminance: float
    min_fraction: float
    hour_tolerance: float
    max_weather_severity: float

    def __post_init__(self):
        x0, y0, x1, y1 = self.roi
        if not (0 <= x0 < x1 <= 1 and 0 <= y0 < y1 <= 1):
            raise LakeError("visual lake: gate.roi must be an increasing normalized rectangle")
        if not math.isfinite(self.luminance) or self.luminance <= 0:
            raise LakeError("visual lake: gate.luminance must be finite and positive")
        if not math.isfinite(self.min_fraction) or not 0 < self.min_fraction <= 1:
            raise LakeError("visual lake: gate.min_fraction must be within (0, 1]")
        if not math.isfinite(self.hour_tolerance) or self.hour_tolerance <= 0:
            raise LakeError("visual lake: gate.hour_tolerance must be finite and positive")
        if not math.isfinite(self.max_weather_severity) or self.max_weather_severity < 0:
            raise LakeError("visual lake: gate.max_weather_severity must be finite and nonnegative")

    def region(self, height, width):
        x0, y0, x1, y1 = self.roi
        columns = (max(0, int(x0 * width)), min(width, math.ceil(x1 * width)))
        rows = (max(0, int(y0 * height)), min(height, math.ceil(y1 * height)))
        if columns[1] <= columns[0] or rows[1] <= rows[0]:
            raise LakeError("visual lake: gate.roi selects no pixels")
        return rows, columns

    def bright_fraction(self, values) -> float:
        if values.ndim != 3 or values.shape[2] < 3 or not values.size:
            raise LakeError("visual lake: bright-region evidence is not an RGB image")
        luminance = (values[..., 0] * 0.2126 + values[..., 1] * 0.7152 +
                     values[..., 2] * 0.0722)
        (y0, y1), (x0, x1) = self.region(*luminance.shape[:2])
        return float(np.mean(luminance[y0:y1, x0:x1] >= self.luminance))

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "gate")
        _exact(value, {"roi", "luminance", "min_fraction", "hour_tolerance",
                       "max_weather_severity"}, "gate")
        return cls(tuple(_vector(value.get("roi"), 4, "gate.roi")),
                   _number(value.get("luminance"), "gate.luminance"),
                   _number(value.get("min_fraction"), "gate.min_fraction"),
                   _number(value.get("hour_tolerance"), "gate.hour_tolerance"),
                   _number(value.get("max_weather_severity"), "gate.max_weather_severity"))


@dataclass(frozen=True)
class LakeConfig:
    name: str
    warmup_frames: int
    samples: int
    pose: WorldPose
    environment: Environment
    gate: Gate

    def __post_init__(self):
        if not valid_request_id(self.name) or len(self.name) > 32:
            raise LakeError("visual lake: config.name must be an ASCII request ID of at most 32 characters")
        if type(self.warmup_frames) is not int or not 1 <= self.warmup_frames <= 300:
            raise LakeError("visual lake: config.warmup_frames must be an integer from 1 through 300")
        if type(self.samples) is not int or self.samples < 3:
            raise LakeError("visual lake: config.samples must be an integer of at least three")

    @classmethod
    def load(cls, path) -> "LakeConfig":
        try:
            with Path(path).open("rb") as handle:
                value = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise LakeError(f"visual lake: could not read config {path}: {error}") from error
        _exact(value, {"name", "warmup_frames", "samples", "pose", "environment", "gate"}, str(path))
        return cls(_string(value.get("name"), "config.name"),
                   _integer(value.get("warmup_frames"), "config.warmup_frames"),
                   _integer(value.get("samples"), "config.samples"),
                   WorldPose.from_toml(value.get("pose")),
                   Environment.from_toml(value.get("environment")),
                   Gate.from_toml(value.get("gate")))


@dataclass(frozen=True)
class RunnerState:
    token: str
    config_dir: Path

    @classmethod
    def load(cls, path) -> "RunnerState":
        try:
            value = json.loads(Path(path).read_text())
        except (OSError, ValueError) as error:
            raise LakeError(f"visual lake: could not read runner state {path}: {error}") from error
        if not isinstance(value, dict):
            raise LakeError("visual lake: runner state is not a JSON object")
        token = _string(value.get("token"), "runner.token")
        config_dir = Path(_string(value.get("config_dir"), "runner.config_dir"))
        if not valid_request_id(token) or not config_dir.is_absolute() or not config_dir.is_dir():
            raise LakeError("visual lake: runner state has an invalid token or config directory")
        return cls(token, config_dir)


@dataclass
class AttachedRunner:
    config_dir: Path
    deadline: float
    token: str
    pid: int
    pending_request: Request | None = None

    def assert_running(self):
        if time.monotonic() >= self.deadline:
            raise LakeError("visual lake: the attached run deadline expired")
        try:
            fields = set(Path(f"/proc/{self.pid}/environ").read_bytes().split(b"\0"))
            os.kill(self.pid, 0)
        except (FileNotFoundError, ProcessLookupError, PermissionError) as error:
            raise LakeError(f"visual lake: the attached game process {self.pid} is gone") from error
        if f"JC3VR_HARNESS_TOKEN={self.token}".encode() not in fields:
            raise LakeError("visual lake: the attached game process no longer owns this runner")

    def pump_menu(self):
        pass

    def request(self, value):
        return VisualHarness.request(self, value)

    def cancel_pending_request(self):
        return VisualHarness.cancel_pending_request(self)


@dataclass(frozen=True)
class CaptureEvidence:
    mode: Mode
    capture_id: str
    bright_fraction: tuple[float, float]
    observed_hour: float | None
    weather_severity: float | None
    weather_control: WeatherControl | None


@dataclass(frozen=True)
class LakeReport:
    name: str
    outcome: LakeOutcome
    manifest: str
    evidence: tuple[CaptureEvidence, ...]
    findings: tuple[str, ...]
    errors: tuple[str, ...]
    restoration_error: str | None
    environment_restoration: str

    def to_wire(self):
        return dataclasses.asdict(self)


def owning_process(state: RunnerState, proc: Path = Path("/proc")) -> int:
    expected = {f"JC3VR_HARNESS_TOKEN={state.token}".encode(),
                f"JC3VR_CONFIG_DIR={state.config_dir}".encode()}
    matches = []
    for process in processes(proc):
        if not process.comm.startswith("justcause3"):
            continue
        if not process.argv or Path(process.argv[0].replace("\\", "/")).name.lower() != "justcause3.exe":
            continue
        try:
            fields = set((proc / str(process.pid) / "environ").read_bytes().split(b"\0"))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
        if expected <= fields:
            matches.append(process.pid)
    if len(matches) != 1:
        raise LakeError(f"visual lake: expected exactly one game process owning {state.config_dir}, "
                        f"found {len(matches)}")
    return matches[0]


class ExclusiveLock:
    def __init__(self, handle):
        self.handle = handle

    @classmethod
    def acquire(cls, config_dir: Path) -> "ExclusiveLock":
        handle = (config_dir / "visual_lake.lock").open("a+")
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            handle.close()
            raise LakeError(f"visual lake: another attached tool holds {config_dir}") from error
        return cls(handle)

    def release(self):
        fcntl.flock(self.handle, fcntl.LOCK_UN)
        self.handle.close()


def sample_id(prefix: str, mode: Mode, index: int) -> str:
    return f"{prefix}_{mode.value}_{index}"


def run_experiment(runner, config: LakeConfig, output: Path, capture=None, analyze=None,
                   reference_multi=None, reference_single=None) -> LakeReport:
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    capture = capture or _capture
    analyze = analyze or analyze_evidence
    prefix = f"lake_{config.name.replace('-', '_')}_{uuid.uuid4().hex[:8]}"
    case = Case(config.name, {})
    manifest = Manifest([case])
    manifest_path = output / "manifest.json"
    captures: list[Capture] = []
    configurations: list[ConfigurationAck] = []
    anchor = None
    restoration_error = None
    environment_restoration = "unobserved"
    gate_errors = ()
    gate_evidence = ()
    failure = None
    try:
        anchor_pose = PoseRequest(f"{prefix}_anchor", OrientationOffset(0.0, 0.0, 0.0),
                                  TranslationOffset())
        acknowledgement, anchor = _configure(runner, config, f"{prefix}_anchor", anchor_pose,
                                              Mode.SINGLE_PASS, anchor, configurations)
        offset, translation = offset_for_target(anchor, config.pose.camera())
        target_pose = PoseRequest(f"{prefix}_target", offset, translation)
        case.expected_pose = target_pose
        for mode in (Mode.MULTIPASS, Mode.SINGLE_PASS):
            acknowledgement, anchor = _configure(
                runner, config, f"{prefix}_{mode.value}_configure", target_pose, mode, anchor,
                configurations)
            entries = case.modes.setdefault(mode, [])
            for index in range(1, config.samples + 1):
                identifier = sample_id(prefix, mode, index)
                result = capture(runner, config, target_pose, anchor, mode,
                                 acknowledgement.generation, identifier)
                if not isinstance(result, Capture):
                    raise LakeError("visual lake: capture is not a diagnostic capture")
                if captures and (result.frame_ordinal <= captures[-1].frame_ordinal or
                                 result.predicted_display_time_ns <=
                                 captures[-1].predicted_display_time_ns):
                    raise LakeError("visual lake: captures do not identify increasing rendered frames")
                captures.append(result)
                folder = runner.config_dir / "captures"
                entries.append(Sample(
                    os.path.relpath(folder / f"{identifier}.capture.png", output),
                    os.path.relpath(folder / f"{identifier}.capture.json", output)))
                write_manifest(manifest_path, manifest)
                LOG.info("wrote %s after %s", manifest_path, identifier)
            if mode == Mode.MULTIPASS:
                gate_errors, gate_evidence = _multipass_gate(
                    config, runner.config_dir / "captures", captures)
                if gate_errors:
                    LOG.warning("visual lake: multipass gate failed; skipping single-pass samples")
                    break
    except BaseException as error:
        failure = error
        raise
    finally:
        deadline = runner.deadline
        try:
            runner.deadline = max(deadline, time.monotonic() + RESTORATION_BUDGET)
            runner.cancel_pending_request()
            restoration = runner.request(Request(Command.END, f"{prefix}_end"))
            if not isinstance(restoration, RestorationAck):
                raise LakeError("visual lake: expected a restoration acknowledgement")
            if restoration.id != f"{prefix}_end":
                raise LakeError("visual lake: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise LakeError("visual lake: restoration generation does not increase")
            observed = restoration.metadata.environment
            if observed is None or observed.weather_control is None:
                raise LakeError("visual lake: environment restoration is unobserved")
            environment_restoration = observed.weather_control.value
            if observed.weather_control != WeatherControl.AMBIENT:
                raise LakeError("visual lake: weather control was not released to ambient")
        except BaseException as error:
            restoration_error = str(error)
            if failure is None:
                _write_failure_report(output, config, manifest_path, captures,
                                      restoration_error, environment_restoration,
                                      (restoration_error,))
                raise
            failure.add_note(f"visual lake: restoration failed: {error}")
            LOG.error("visual lake: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
        if failure is not None:
            _write_failure_report(output, config, manifest_path, captures, restoration_error,
                                  environment_restoration, (str(failure),))
    if gate_errors:
        report = LakeReport(config.name, LakeOutcome.INCONCLUSIVE, str(manifest_path),
                            gate_evidence, (), gate_errors, restoration_error,
                            environment_restoration)
        write_report(output / "report.json", report)
        return report
    outcome, evidence, findings, errors = analyze(
        config, runner.config_dir / "captures", captures, reference_multi, reference_single)
    report = LakeReport(config.name, outcome, str(manifest_path), evidence, findings, errors,
                        restoration_error, environment_restoration)
    write_report(output / "report.json", report)
    return report


def _write_failure_report(output, config, manifest_path, captures, restoration_error,
                          environment_restoration, errors):
    evidence = tuple(_capture_evidence(capture, (0.0, 0.0)) for capture in captures)
    report = LakeReport(config.name, LakeOutcome.INCONCLUSIVE, str(manifest_path), evidence, (),
                        errors, restoration_error, environment_restoration)
    try:
        write_report(Path(output) / "report.json", report)
    except OSError as error:
        LOG.error("visual lake: could not write the failure report: %s", error)


def _capture_evidence(capture: Capture, fractions) -> CaptureEvidence:
    observation = capture.visual.environment
    return CaptureEvidence(
        capture.mode, capture.id, fractions,
        None if observation is None else observation.hour,
        None if observation is None else observation.weather_severity,
        None if observation is None else observation.weather_control)


def analyze_evidence(config: LakeConfig, root: Path, captures, reference_multi=None,
                     reference_single=None):
    evidence: list[CaptureEvidence] = []
    errors: list[str] = []
    for index, capture in enumerate(captures):
        observation = capture.visual.environment
        if not _matches_world_pose(config, capture.comparison_inputs.camera):
            errors.append(f"{capture.id}: camera differs from the configured world pose")
        if observation is None:
            errors.append(f"{capture.id}: environment observation is unavailable")
        else:
            _check_observation(config, capture.id, observation, errors)
        try:
            fractions = _bright_fractions(config, root, capture, index)
        except (OSError, ValueError, TypeError, ProtocolError) as error:
            errors.append(f"{capture.id}: {error}")
            fractions = (0.0, 0.0)
        evidence.append(_capture_evidence(capture, fractions))
    if reference_multi is not None:
        _check_reference(config, reference_multi, Mode.MULTIPASS, errors)
    if reference_single is not None:
        _check_reference(config, reference_single, Mode.SINGLE_PASS, errors)
    outcome, findings, errors = classify(config, tuple(evidence), tuple(errors))
    return outcome, tuple(evidence), findings, errors


def _multipass_gate(config: LakeConfig, root: Path, captures):
    errors: list[str] = []
    evidence: list[CaptureEvidence] = []
    for index, capture in enumerate(captures):
        observation = capture.visual.environment
        if not _matches_world_pose(config, capture.comparison_inputs.camera):
            errors.append(f"{capture.id}: camera differs from the configured world pose")
        if observation is None:
            errors.append(f"{capture.id}: environment observation is unavailable")
        else:
            _check_observation(config, capture.id, observation, errors)
        try:
            fractions = _bright_fractions(config, root, capture, index)
        except (OSError, ValueError, TypeError, ProtocolError) as error:
            errors.append(f"{capture.id}: {error}")
            fractions = (0.0, 0.0)
        for eye, fraction in enumerate(fractions):
            if fraction < config.gate.min_fraction:
                errors.append(f"{capture.id}: multipass eye {eye} lacks the bright region")
        evidence.append(_capture_evidence(capture, fractions))
    return tuple(errors), tuple(evidence)


def classify(config: LakeConfig, evidence: tuple[CaptureEvidence, ...], errors: tuple[str, ...]):
    findings: list[str] = []
    if errors:
        return LakeOutcome.INCONCLUSIVE, (), errors
    multipass = [item for item in evidence if item.mode == Mode.MULTIPASS]
    single_pass = [item for item in evidence if item.mode == Mode.SINGLE_PASS]
    if not multipass or not single_pass:
        return LakeOutcome.INCONCLUSIVE, ("missing multipass or single-pass evidence",), ()
    for item in multipass:
        for eye, fraction in enumerate(item.bright_fraction):
            if fraction < config.gate.min_fraction:
                findings.append(f"{item.capture_id}: multipass eye {eye} lacks the bright region")
    if findings:
        return LakeOutcome.INCONCLUSIVE, tuple(findings), ()
    for item in single_pass:
        if item.bright_fraction[0] < config.gate.min_fraction:
            findings.append(f"{item.capture_id}: single-pass left eye lacks the bright region")
    if findings:
        return LakeOutcome.INCONCLUSIVE, tuple(findings), ()
    missing = [item for item in single_pass
               if item.bright_fraction[1] < config.gate.min_fraction]
    if not missing:
        findings.append("single-pass right eye keeps the bright region")
        return LakeOutcome.NOT_REPRODUCED, tuple(findings), ()
    if len(missing) < len(single_pass):
        findings.append("right-eye reflection is unstable across samples")
        return LakeOutcome.INCONCLUSIVE, tuple(findings), ()
    findings.append("single-pass right eye lacks the bright region that multipass keeps in both eyes")
    return LakeOutcome.REPRODUCED, tuple(findings), ()


def _configure(runner, config, request_id, pose, mode, anchor, configurations):
    request = Request(Command.CONFIGURE, request_id, mode, config.warmup_frames, pose,
                      environment=config.environment.request())
    acknowledgement = runner.request(request)
    if not isinstance(acknowledgement, ConfigurationAck):
        raise LakeError("visual lake: expected a configuration acknowledgement")
    if (acknowledgement.id != request.id or acknowledgement.mode != mode or
            acknowledgement.warmup_frames != config.warmup_frames):
        raise LakeError("visual lake: configuration acknowledgement disagrees with its request")
    if configurations and acknowledgement.generation <= configurations[-1].generation:
        raise LakeError("visual lake: configuration generation does not increase")
    anchor = acknowledgement.validate_pose(request, anchor)
    configurations.append(acknowledgement)
    LOG.info("configured %s in %s", request_id, mode.value)
    return acknowledgement, anchor


def _capture(runner, config, pose, anchor, mode, generation, identifier):
    acknowledgement = runner.request(Request(Command.SCREENSHOT, identifier))
    if not isinstance(acknowledgement, ScreenshotAck):
        raise LakeError("visual lake: expected a screenshot acknowledgement")
    capture = validate_capture(runner.config_dir / "captures", identifier,
                               acknowledgement.capture, mode=mode, generation=generation)
    if not isinstance(capture, Capture):
        raise LakeError("visual lake: expected a diagnostic capture")
    if capture.visual.pose is None:
        raise LakeError(f"visual lake: capture {identifier} has no pose evidence")
    capture.visual.pose.validate(pose, anchor)
    return capture


def _matches_world_pose(config: LakeConfig, camera: Camera) -> bool:
    target = config.pose.camera()
    return (composed_positions_match(target.position, camera.position, target.position,
                                     POSE_TOLERANCE) and
            quaternions_match(target.orientation, camera.orientation, POSE_TOLERANCE))


def _check_observation(config: LakeConfig, identifier, observation: EnvironmentObservation,
                       errors: list[str]):
    if abs(observation.hour - config.environment.hour) > config.gate.hour_tolerance:
        errors.append(f"{identifier}: observed hour {observation.hour:.4f} differs from "
                      f"{config.environment.hour:.4f}")
    if observation.weather_severity > config.gate.max_weather_severity:
        errors.append(f"{identifier}: observed weather severity {observation.weather_severity:.3f} "
                      f"is not clear")
    if observation.weather_control is None:
        errors.append(f"{identifier}: weather control is unobserved")
    elif observation.weather_control != WeatherControl.SUNNY:
        errors.append(f"{identifier}: weather control is {observation.weather_control.value}, "
                      f"not sunny")


def _bright_fractions(config: LakeConfig, root: Path, capture: Capture, index: int):
    compare_mode = CompareMode(capture.mode.value)
    spec = SampleSpec(Path(f"{capture.id}.capture.png"), Path(f"{capture.id}.capture.json"))
    sample = load_sample(root, compare_mode, index, spec)
    fractions = []
    for eye in (0, 1):
        values = load_stage(root, compare_mode, spec, sample, Stage.MAIN_COLOR, eye).values
        fractions.append(config.gate.bright_fraction(values))
    return tuple(fractions)


def _check_reference(config: LakeConfig, path, mode: Mode, errors: list[str]):
    try:
        metadata = json.loads(Path(path).read_text())
        capture = Capture.from_wire(metadata)
        if capture.mode != mode:
            raise LakeError(f"reference mode is {capture.mode.value}, not {mode.value}")
        if not _matches_world_pose(config, capture.comparison_inputs.camera):
            raise LakeError("reference camera differs from the configured world pose")
        fractions = _bright_fractions(config, Path(path).parent, capture, 0)
        if mode == Mode.MULTIPASS:
            if any(fraction < config.gate.min_fraction for fraction in fractions):
                raise LakeError("multipass reference is not bright in both eyes")
        elif fractions[1] >= config.gate.min_fraction:
            raise LakeError("single-pass reference keeps the bright region in the right eye")
    except (OSError, ValueError, TypeError, ProtocolError, LakeError) as error:
        errors.append(f"historical reference {path}: {error}")


def write_report(path: Path, report: LakeReport):
    temporary = Path(path).with_suffix(".tmp")
    temporary.write_text(json.dumps(report.to_wire(), indent=2) + "\n")
    temporary.replace(path)
    LOG.info("wrote %s", path)


def _table(value, context):
    if not isinstance(value, dict):
        raise LakeError(f"visual lake: {context} must be a table")
    return value


def _exact(value, keys, context):
    if set(value) != keys:
        missing, extra = keys - set(value), set(value) - keys
        raise LakeError(f"visual lake: {context} has missing {sorted(missing)} "
                        f"and unknown {sorted(extra)} keys")


def _string(value, context):
    if not isinstance(value, str) or not value:
        raise LakeError(f"visual lake: {context} must be a nonempty string")
    return value


def _number(value, context):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise LakeError(f"visual lake: {context} must be a finite number")
    return float(value)


def _integer(value, context):
    if type(value) is not int:
        raise LakeError(f"visual lake: {context} must be an integer")
    return value


def _vector(value, length, context):
    if not isinstance(value, (list, tuple)) or len(value) != length:
        raise LakeError(f"visual lake: {context} must have {length} components")
    return tuple(_number(item, f"{context}[{index}]") for index, item in enumerate(value))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="Reproduce the missing single-pass right-eye "
                                                 "water specular at a fixed world pose.")
    parser.add_argument("--runner", type=Path, required=True,
                        help="runner.json from the running visual harness session")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--reference-multi", type=Path,
                        help="optional historical multipass capture for the bright-region gate")
    parser.add_argument("--reference-single", type=Path,
                        help="optional historical single-pass capture for the bright-region gate")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s",
                        stream=sys.stderr)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        LOG.error("visual lake: timeout must be finite and positive")
        return 1
    try:
        config = LakeConfig.load(args.config)
        state = RunnerState.load(args.runner)
        pid = owning_process(state)
        lock = ExclusiveLock.acquire(state.config_dir)
        try:
            runner = AttachedRunner(state.config_dir, time.monotonic() + args.timeout,
                                    state.token, pid)
            LOG.info("attached to game pid %d via %s", pid, args.runner)
            report = run_experiment(runner, config, args.output,
                                    reference_multi=args.reference_multi,
                                    reference_single=args.reference_single)
        finally:
            lock.release()
    except (LakeError, ProtocolError, HarnessError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 1
    LOG.info("lake experiment: %s (%s)", report.outcome.value, args.output)
    return report.outcome.exit_code


if __name__ == "__main__":
    sys.exit(main())
