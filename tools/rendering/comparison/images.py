"""Compare frozen-camera repeated captures without image registration or resizing.

Usage: python -m tools.rendering compare manifest.json --output comparison
Paths in the manifest are relative to the manifest. Isolated PNGs contain one eye;
paired PNGs contain the logical left and right eyes side by side, in that order.
"""

import argparse
import json
import math
import sys
from dataclasses import asdict, dataclass, field, fields
from enum import StrEnum
from itertools import combinations, product
from pathlib import Path

import numpy as np
from PIL import Image

from tools.rendering.comparison.provenance import (
    MotionProvenance,
    PoseProvenance,
    RunAnchor,
)
from tools.rendering.comparison.report import to_wire, write_reports
from tools.rendering.protocol import (
    DiagnosticProfile,
    ExpectedMotion,
    MotionEvidence,
    PoseEvidence,
    PoseRequest,
    ProtocolError,
)


class Mode(StrEnum):
    LEFT_ONLY = "left_only"
    RIGHT_ONLY = "right_only"
    MULTIPASS = "multipass"
    SINGLE_PASS = "single_pass"
    REVERSE_MULTIPASS = "reverse_multipass"

    @property
    def eyes(self):
        if self == Mode.LEFT_ONLY:
            return (0,)
        if self == Mode.RIGHT_ONLY:
            return (1,)
        return (0, 1)


class Outcome(StrEnum):
    PASS = "pass"
    FAIL = "fail"
    INCONCLUSIVE = "inconclusive"


@dataclass(frozen=True)
class Thresholds:
    pixel: float = 3.0
    changed_fraction: float = 0.001
    mean: float = 0.25

    def overridden(self, args, prefix=""):
        return Thresholds(
            *(
                getattr(args, prefix + item.name)
                if getattr(args, prefix + item.name) is not None
                else getattr(self, item.name)
                for item in fields(self)
            )
        )

    @classmethod
    def from_wire(cls, value, defaults=None):
        value = object_value(value, "thresholds")
        defaults = defaults or cls()
        if set(value) - {"pixel", "changed_fraction", "mean"}:
            raise ValueError("thresholds: unknown keys")
        result = cls(
            value.get("pixel", defaults.pixel),
            value.get("changed_fraction", defaults.changed_fraction),
            value.get("mean", defaults.mean),
        )
        for name, number, maximum in (
            ("pixel", result.pixel, 255),
            ("changed_fraction", result.changed_fraction, 1),
            ("mean", result.mean, 255),
        ):
            if not finite_number(number) or not 0 <= number <= maximum:
                raise ValueError(
                    f"thresholds.{name}: expected a number in [0, {maximum}]"
                )
        return result


@dataclass(frozen=True)
class CameraInputs:
    position: tuple[float, ...]
    orientation: tuple[float, ...]

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "camera")
        return cls(vector(value, "position", 3), vector(value, "orientation", 4))


@dataclass(frozen=True)
class EyeInputs:
    offset: tuple[float, ...]
    orientation_delta: tuple[float, ...]
    projection_standard: tuple[float, ...]
    projection_reverse_z: tuple[float, ...]

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "eye")
        return cls(
            vector(value, "offset", 3),
            vector(value, "orientation_delta", 4),
            vector(value, "projection_standard", 16),
            vector(value, "projection_reverse_z", 16),
        )


@dataclass(frozen=True)
class ComparisonInputs:
    build_stamp: str
    profile: DiagnosticProfile
    camera: CameraInputs
    eyes: tuple[EyeInputs, EyeInputs]
    eye_width: int
    eye_height: int
    world_scale: float

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "comparison_inputs")
        eyes = value.get("eyes")
        if not isinstance(eyes, list) or len(eyes) != 2:
            raise ValueError("comparison_inputs.eyes: expected two eyes")
        scale = value.get("world_scale")
        if not finite_number(scale) or scale <= 0:
            raise ValueError(
                "comparison_inputs.world_scale: expected a positive finite number"
            )
        try:
            profile = DiagnosticProfile(text(value, "profile"))
        except ValueError as error:
            raise ValueError("comparison_inputs.profile: unknown diagnostic profile") from error
        return cls(
            text(value, "build_stamp"),
            profile,
            CameraInputs.from_wire(value.get("camera")),
            (EyeInputs.from_wire(eyes[0]), EyeInputs.from_wire(eyes[1])),
            integer(value, "eye_width"),
            integer(value, "eye_height"),
            scale,
        )


@dataclass(frozen=True)
class VisualReadiness:
    generation: int
    profile: DiagnosticProfile
    ready: bool
    paused: bool
    frozen: bool
    error: str | None
    warmup_frames: int
    completed_frames: int
    pose: PoseEvidence | None = None
    motion: MotionEvidence | None = None

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "metadata.visual")
        try:
            profile = DiagnosticProfile(text(value, "profile"))
        except ValueError as error:
            raise ValueError("metadata.visual.profile: expected controlled_v1 or controlled_v2") from error
        if not profile.is_controlled:
            raise ValueError("metadata.visual.profile: expected controlled_v1 or controlled_v2")
        for name in ("ready", "paused", "frozen"):
            if value.get(name) is not True:
                raise ValueError(f"metadata.visual.{name}: expected true")
        if "error" not in value or value["error"] is not None:
            raise ValueError("metadata.visual.error: expected null")
        warmup = integer(value, "warmup_frames")
        completed = integer(value, "completed_frames")
        if warmup > 300:
            raise ValueError("metadata.visual.warmup_frames: expected at most 300")
        if completed < warmup:
            raise ValueError("metadata.visual.completed_frames: below warmup_frames")
        pose = None
        if value.get("pose") is not None:
            try:
                pose = PoseEvidence.from_wire(value.get("pose"))
            except ProtocolError as error:
                raise ValueError(f"metadata.visual.pose: {error}") from error
        motion = None
        if value.get("motion") is not None:
            try:
                motion = MotionEvidence.from_wire(value.get("motion"))
            except ProtocolError as error:
                raise ValueError(f"metadata.visual.motion: {error}") from error
        return cls(
            integer(value, "generation"),
            profile,
            True,
            True,
            True,
            None,
            warmup,
            completed,
            pose,
            motion,
        )


@dataclass(frozen=True)
class CaptureMetadata:
    capture_id: str
    mode: Mode
    visual: VisualReadiness
    frame_ordinal: int
    predicted_display_time_ns: int
    width: int
    height: int
    eye_width: int
    eye_height: int
    selected_eye: int | None
    rendered_eyes: tuple[bool, bool]
    fresh_eyes: tuple[bool, bool]
    inputs: ComparisonInputs

    @classmethod
    def from_wire(cls, value, normalized=None):
        value = object_value(value, "metadata")
        visual = object_value(value.get("visual"), "metadata.visual")
        readiness = VisualReadiness.from_wire(visual)
        mode = Mode(text(value, "mode"))
        if Mode(text(visual, "mode")) != mode:
            raise ValueError("metadata.visual.mode: differs from metadata.mode")
        selected = value.get("selected_eye", "missing")
        if selected is not None and (
            type(selected) is not int or selected not in (0, 1)
        ):
            raise ValueError("metadata.selected_eye: expected null, zero, or one")
        recorded = value.get("comparison_inputs")
        inputs = (
            ComparisonInputs.from_wire(recorded) if recorded is not None else normalized
        )
        if inputs is None:
            raise ValueError("comparison_inputs: missing capture inputs")
        if normalized is not None and recorded is not None:
            mismatches = metadata_mismatches(inputs, normalized, 1e-5)
            if mismatches:
                raise ValueError(
                    "manifest comparison_inputs disagree with metadata: "
                    + "; ".join(mismatches)
                )
        if readiness.profile != inputs.profile:
            raise ValueError(
                "metadata.visual.profile: differs from comparison_inputs.profile"
            )
        if readiness.pose is not None:
            try:
                readiness.pose.validate_applied(inputs.camera)
            except ProtocolError as error:
                raise ValueError(f"metadata.visual.pose: {error}") from error
        predicted = integer(value, "predicted_display_time_ns", minimum=None)
        if readiness.motion is not None:
            if readiness.pose is None:
                raise ValueError("metadata.visual.motion: missing pose evidence")
            try:
                readiness.motion.validate_capture(readiness.motion.request, readiness.motion.phase,
                                                  readiness.pose.anchor, readiness.pose,
                                                  inputs.camera, predicted)
            except ProtocolError as error:
                raise ValueError(f"metadata.visual.motion: {error}") from error
        return cls(
            text(value, "id"),
            mode,
            readiness,
            integer(value, "frame_ordinal"),
            predicted,
            integer(value, "width"),
            integer(value, "height"),
            integer(value, "eye_width"),
            integer(value, "eye_height"),
            selected,
            boolean_pair(value, "rendered_eyes"),
            boolean_pair(value, "fresh_eyes"),
            inputs,
        )

    def validate(self, mode):
        if self.mode != mode:
            raise ValueError(f"metadata.mode: expected {mode}, got {self.mode}")
        expected_eye = mode.eyes[0] if len(mode.eyes) == 1 else None
        if self.selected_eye != expected_eye:
            raise ValueError(f"metadata.selected_eye: expected {expected_eye!r}")
        required = tuple(eye in mode.eyes for eye in (0, 1))
        if self.rendered_eyes != required:
            raise ValueError("metadata.rendered_eyes: inconsistent with the mode")
        if any(
            needed and not fresh for needed, fresh in zip(required, self.fresh_eyes)
        ):
            raise ValueError("metadata.fresh_eyes: a rendered eye is stale")
        if (self.eye_width, self.eye_height) != (
            self.inputs.eye_width,
            self.inputs.eye_height,
        ):
            raise ValueError(
                "metadata.eye_width/eye_height: differs from comparison_inputs"
            )
        expected = (self.eye_width * len(mode.eyes), self.eye_height)
        if (self.width, self.height) != expected:
            raise ValueError(
                f"metadata image shape: expected {expected}, got {(self.width, self.height)}"
            )


@dataclass(frozen=True)
class SampleSpec:
    png: Path
    metadata: Path
    inputs: ComparisonInputs | None = None

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "sample")
        inputs = (
            ComparisonInputs.from_wire(value["comparison_inputs"])
            if "comparison_inputs" in value
            else None
        )
        return cls(Path(text(value, "png")), Path(text(value, "metadata")), inputs)


@dataclass(frozen=True)
class ModeSamples:
    mode: Mode
    samples: tuple[SampleSpec, ...]


@dataclass(frozen=True)
class Case:
    name: str
    modes: tuple[ModeSamples, ...]
    expected_pose: PoseRequest | None = None
    expected_motion: ExpectedMotion | None = None

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "case")
        modes = object_value(value.get("modes"), "case.modes")
        parsed = []
        for mode, entries in modes.items():
            if not isinstance(entries, list):
                raise TypeError(f"case.modes.{mode}: expected an array")
            parsed.append(
                ModeSamples(
                    Mode(mode), tuple(SampleSpec.from_wire(entry) for entry in entries)
                )
            )
        expected_pose = None
        if value.get("expected_pose") is not None:
            try:
                expected_pose = PoseRequest.from_wire(value.get("expected_pose"))
            except ProtocolError as error:
                raise ValueError(f"case.expected_pose: {error}") from error
        expected_motion = None
        if value.get("expected_motion") is not None:
            try:
                expected_motion = ExpectedMotion.from_wire(value.get("expected_motion"))
            except ProtocolError as error:
                raise ValueError(f"case.expected_motion: {error}") from error
        if expected_pose is not None and expected_motion is not None:
            raise ValueError("case: expected_pose and expected_motion are mutually exclusive")
        return cls(text(value, "name"), tuple(parsed), expected_pose, expected_motion)


@dataclass(frozen=True)
class Manifest:
    cases: tuple[Case, ...]
    thresholds: Thresholds
    repeatability: Thresholds
    explicit_repeatability: bool = False

    @classmethod
    def from_wire(cls, value):
        value = object_value(value, "manifest")
        cases = value.get("cases")
        if not isinstance(cases, list) or not cases:
            raise ValueError("manifest.cases: expected a nonempty array")
        configured = dict(object_value(value.get("thresholds", {}), "thresholds"))
        repeats = configured.pop("repeatability", {})
        limits = Thresholds.from_wire(configured)
        result = cls(
            tuple(Case.from_wire(case) for case in cases),
            limits,
            Thresholds.from_wire(repeats, limits),
            "repeatability" in value.get("thresholds", {}),
        )
        if len({case.name for case in result.cases}) != len(result.cases):
            raise ValueError("manifest.cases: duplicate case names")
        return result


@dataclass
class LoadedSample:
    label: str
    capture_id: str
    visual: VisualReadiness
    frame_ordinal: int
    predicted_display_time_ns: int
    inputs: ComparisonInputs
    left: np.ndarray | None = field(repr=False, metadata={"serialize": False})
    right: np.ndarray | None = field(repr=False, metadata={"serialize": False})

    def image(self, eye):
        result = self.left if eye == 0 else self.right
        if result is None:
            raise ValueError(f"{self.label}: eye {eye} is absent")
        return result


@dataclass(frozen=True)
class Metrics:
    mean: float
    changed_fraction: float
    p99: float
    max: int

    def passes(self, limits):
        return (
            self.mean <= limits.mean
            and self.changed_fraction <= limits.changed_fraction
        )


@dataclass(frozen=True)
class PairResult:
    a: str
    b: str
    metrics: Metrics
    status: Outcome
    capture_a: str
    capture_b: str


@dataclass(frozen=True)
class Artifacts:
    a: str
    b: str
    diff: str
    diff_x8: str | None = None
    changed: str | None = None


@dataclass(frozen=True)
class Tile:
    box: tuple[int, int, int, int]
    metrics: Metrics
    crops: Artifacts


@dataclass(frozen=True)
class ArtifactPair:
    a: str
    b: str
    eye: int


@dataclass
class Comparison:
    name: str
    status: Outcome = Outcome.INCONCLUSIVE
    errors: list[str] = field(default_factory=list)
    pairs: list[PairResult] = field(default_factory=list)
    worst_metrics: Metrics | None = None
    artifact_pair: ArtifactPair | None = None
    artifacts: Artifacts | None = None
    ranked_tiles: list[Tile] = field(default_factory=list)


@dataclass
class ModeReport:
    mode: Mode
    status: Outcome = Outcome.INCONCLUSIVE
    errors: list[str] = field(default_factory=list)
    repeatability: list[Comparison] = field(default_factory=list)
    samples: list[LoadedSample] = field(default_factory=list, repr=False)


@dataclass
class CaseReport:
    name: str
    status: Outcome = Outcome.INCONCLUSIVE
    modes: list[ModeReport] = field(default_factory=list)
    comparisons: list[Comparison] = field(default_factory=list)


@dataclass
class Report:
    manifest: str
    schema_version: int = 1
    status: Outcome = Outcome.INCONCLUSIVE
    thresholds: Thresholds = field(default_factory=Thresholds)
    repeatability_thresholds: Thresholds = field(default_factory=Thresholds)
    metadata_tolerance: float = 1e-5
    cases: list[CaseReport] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)

    def to_wire(self):
        return to_wire(self)


def analyze_manifest(
    manifest_path,
    output,
    *,
    thresholds=None,
    repeatability=None,
    metadata_tolerance=1e-5,
    tile_size=64,
    top_tiles=5,
):
    """Analyze typed captures; overrides are complete Thresholds, unlike JSON defaults."""
    manifest_path, output = Path(manifest_path).resolve(), Path(output)
    output.mkdir(parents=True, exist_ok=True)
    report = Report(str(manifest_path), metadata_tolerance=metadata_tolerance)
    try:
        if not finite_number(metadata_tolerance) or not 0 <= metadata_tolerance <= 1e-5:
            report.metadata_tolerance = 1e-5
            raise ValueError(
                "metadata tolerance: expected a finite number in [0, 1e-5]"
            )
        if (
            type(tile_size) is not int
            or tile_size < 1
            or type(top_tiles) is not int
            or top_tiles < 1
        ):
            raise ValueError("tiles: size and count must be positive integers")
        manifest = Manifest.from_wire(read_json(manifest_path))
        limits = thresholds or manifest.thresholds
        repeat_limits = repeatability or (
            thresholds
            if thresholds and not manifest.explicit_repeatability
            else manifest.repeatability
        )
        for candidate in (limits, repeat_limits):
            Thresholds.from_wire(asdict(candidate))
        report.thresholds = limits
        report.repeatability_thresholds = repeat_limits
        run_anchor = RunAnchor()
        for index, case in enumerate(manifest.cases):
            report.cases.append(
                analyze_case(
                    case,
                    manifest_path.parent,
                    output,
                    index,
                    report,
                    tile_size,
                    top_tiles,
                    run_anchor=run_anchor,
                )
            )
        report.status = aggregate_status(case.status for case in report.cases)
    except (OSError, ValueError, TypeError) as error:
        report.errors.append(f"visual comparison: {error}")
        report.status = Outcome.INCONCLUSIVE
    write_reports(output, report)
    return report


def object_value(value, path):
    if not isinstance(value, dict):
        raise TypeError(f"{path}: expected an object")
    return value


def finite_number(value):
    return type(value) in (int, float) and math.isfinite(value)


def text(value, key):
    result = value.get(key)
    if not isinstance(result, str) or not result:
        raise ValueError(f"{key}: expected a nonempty string")
    return result


def integer(value, key, minimum=1):
    result = value.get(key)
    if type(result) is not int or (minimum is not None and result < minimum):
        kind = "a positive integer" if minimum == 1 else "an integer"
        raise ValueError(f"{key}: expected {kind}")
    return result


def vector(value, key, length):
    result = value.get(key)
    if (
        not isinstance(result, list)
        or len(result) != length
        or not all(map(finite_number, result))
    ):
        raise ValueError(f"{key}: expected {length} finite numbers")
    return tuple(result)


def boolean_pair(value, key):
    result = value.get(key)
    if (
        not isinstance(result, list)
        or len(result) != 2
        or any(type(v) is not bool for v in result)
    ):
        raise ValueError(f"{key}: expected two booleans")
    return tuple(result)


def read_json(path):
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def aggregate_status(statuses):
    statuses = tuple(statuses)
    if not statuses or Outcome.INCONCLUSIVE in statuses:
        return Outcome.INCONCLUSIVE
    return Outcome.FAIL if Outcome.FAIL in statuses else Outcome.PASS


def analyze_case(case, root, output, case_index, report, tile_size, top_tiles, run_anchor=None):
    result = CaseReport(case.name)
    provenance = PoseProvenance(case.expected_pose)
    motion_provenance = MotionProvenance(case.expected_motion)
    required = (Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS)
    modes = required + (
        (Mode.REVERSE_MULTIPASS,)
        if any(m.mode == Mode.REVERSE_MULTIPASS for m in case.modes)
        else ()
    )
    for mode in modes:
        mode_report = ModeReport(mode)
        result.modes.append(mode_report)
        entries = next((m.samples for m in case.modes if m.mode == mode), ())
        if len(entries) < 3:
            mode_report.errors.append(f"{mode}: at least three samples are required")
            continue
        seen = set()
        seen_metadata = set()
        for index, entry in enumerate(entries):
            try:
                for path, seen_paths, kind in (
                    ((root / entry.png).resolve(), seen, "PNG"),
                    ((root / entry.metadata).resolve(), seen_metadata, "metadata"),
                ):
                    if path in seen_paths:
                        raise ValueError(
                            f"repeated {kind} path is not an independent sample"
                        )
                    seen_paths.add(path)
                sample = load_sample(root, mode, index, entry)
                provenance.add(sample.visual.pose, sample.inputs.camera)
                motion_provenance.add(sample.visual.motion, sample.visual.pose, sample.inputs.camera,
                                      sample.predicted_display_time_ns, mode)
                if any(
                    sample.capture_id == prior.capture_id
                    for prior in mode_report.samples
                ):
                    raise ValueError("repeated capture id is not an independent sample")
                if any(
                    sample.frame_ordinal == prior.frame_ordinal
                    or sample.predicted_display_time_ns
                    == prior.predicted_display_time_ns
                    for prior in mode_report.samples
                ):
                    raise ValueError("repeated frame is not an independent sample")
                mode_report.samples.append(sample)
            except (OSError, ValueError, TypeError) as error:
                mode_report.errors.append(f"{mode}[{index}]: {error}")
        if mode_report.errors:
            continue
        for eye in mode.eyes:
            comparison = compare_samples(
                f"{mode} eye {eye} repeatability",
                [(a, b, eye) for a, b in combinations(mode_report.samples, 2)],
                report.repeatability_thresholds,
                report.metadata_tolerance,
                output,
                f"case{case_index}-{mode}-eye{eye}-repeat",
                tile_size,
                top_tiles,
            )
            mode_report.repeatability.append(comparison)
        if all(c.status == Outcome.PASS for c in mode_report.repeatability):
            mode_report.status = Outcome.PASS
        else:
            mode_report.errors.append("same-mode repeatability is unstable or invalid")
    try:
        provenance.finish()
        motion_provenance.finish()
        if run_anchor is not None:
            for candidate in (provenance.anchor, motion_provenance.anchor):
                if candidate is not None:
                    run_anchor.require(candidate)
    except ValueError as error:
        for mode_report in result.modes:
            mode_report.errors.append(str(error))
            mode_report.status = Outcome.INCONCLUSIVE
    paired = [m for m in result.modes if len(m.mode.eyes) == 2]
    isolated = [m for m in result.modes if len(m.mode.eyes) == 1]
    checks = [(a, b, a.mode.eyes[0]) for a in isolated for b in paired]
    checks += [(a, b, eye) for a, b in combinations(paired, 2) for eye in (0, 1)]
    for a, b, eye in checks:
        label = f"{a.mode} vs {b.mode}, eye {eye}"
        if a.status != Outcome.PASS or b.status != Outcome.PASS:
            comparison = Comparison(
                label, errors=["same-mode repeatability must pass for both modes first"]
            )
        else:
            comparison = compare_samples(
                label,
                [(left, right, eye) for left, right in product(a.samples, b.samples)],
                report.thresholds,
                report.metadata_tolerance,
                output,
                f"case{case_index}-{a.mode}-{b.mode}-eye{eye}",
                tile_size,
                top_tiles,
            )
        result.comparisons.append(comparison)
    result.status = aggregate_status(
        [c.status for c in result.comparisons] + [m.status for m in result.modes]
    )
    return result


def load_sample(root, mode, index, spec):
    metadata = CaptureMetadata.from_wire(read_json(root / spec.metadata), spec.inputs)
    metadata.validate(mode)
    expected = (metadata.width, metadata.height)
    with Image.open(root / spec.png) as image:
        if image.format != "PNG" or image.size != expected:
            raise ValueError(
                f"PNG image shape/format: expected PNG {expected}, got {image.format} {image.size}"
            )
        if image.mode not in ("RGB", "RGBA"):
            raise ValueError(f"PNG image mode: expected RGB or RGBA, got {image.mode}")
        if image.mode == "RGBA" and image.getchannel("A").getextrema() != (255, 255):
            raise ValueError(
                "PNG alpha: nonopaque pixels have no defined RGB comparison"
            )
        pixels = np.array(image.convert("RGB"))
    width = metadata.eye_width
    if mode == Mode.LEFT_ONLY:
        left, right = pixels, None
    elif mode == Mode.RIGHT_ONLY:
        left, right = None, pixels
    else:
        left, right = pixels[:, :width].copy(), pixels[:, width:].copy()
    return LoadedSample(
        f"{mode}[{index}]",
        metadata.capture_id,
        metadata.visual,
        metadata.frame_ordinal,
        metadata.predicted_display_time_ns,
        metadata.inputs,
        left,
        right,
    )


def metadata_mismatches(a, b, tolerance):
    errors = []
    for name in ("build_stamp", "profile", "eye_width", "eye_height", "world_scale"):
        left, right = getattr(a, name), getattr(b, name)
        if left != right:
            errors.append(f"comparison_inputs.{name}: {left!r} != {right!r}")
    compare_vectors(
        a.camera.position, b.camera.position, tolerance, "camera.position", errors
    )
    compare_vectors(
        a.camera.orientation,
        b.camera.orientation,
        tolerance,
        "camera.orientation",
        errors,
    )
    for index, (left, right) in enumerate(zip(a.eyes, b.eyes)):
        for item in fields(EyeInputs):
            compare_vectors(
                getattr(left, item.name),
                getattr(right, item.name),
                tolerance,
                f"eyes[{index}].{item.name}",
                errors,
            )
    return errors


def compare_vectors(a, b, tolerance, path, errors):
    for index, (left, right) in enumerate(zip(a, b)):
        if abs(left - right) > tolerance:
            errors.append(f"comparison_inputs.{path}[{index}]: {left!r} != {right!r}")


def image_metrics(a, b, thresholds):
    if a.shape != b.shape or a.ndim != 3 or a.shape[2] != 3 or not a.size:
        raise ValueError(
            f"image shape: expected matching nonempty RGB arrays, got {a.shape}, {b.shape}"
        )
    error = np.abs(a.astype(np.int16) - b.astype(np.int16))
    pixel_error = error.max(axis=2)
    return Metrics(
        float(error.mean()),
        float(np.mean(pixel_error > thresholds.pixel)),
        float(np.percentile(pixel_error, 99)),
        int(error.max()),
    )


def compare_samples(
    name, pairs, thresholds, tolerance, output, stem, tile_size, top_tiles
):
    result = Comparison(name)
    worst = None
    for a, b, eye in pairs:
        mismatches = metadata_mismatches(a.inputs, b.inputs, tolerance)
        if mismatches:
            result.errors.extend(
                f"{a.label} vs {b.label}: {error}" for error in mismatches
            )
            continue
        try:
            metrics = image_metrics(a.image(eye), b.image(eye), thresholds)
        except ValueError as error:
            result.errors.append(f"{a.label} vs {b.label}: {error}")
            continue
        passed = metrics.passes(thresholds)
        result.pairs.append(
            PairResult(
                a.label,
                b.label,
                metrics,
                Outcome.PASS if passed else Outcome.FAIL,
                a.capture_id,
                b.capture_id,
            )
        )
        score = (not passed, metrics.changed_fraction, metrics.mean, metrics.max)
        if worst is None or score > worst[0]:
            worst = score, a, b, eye
    result.status = (
        Outcome.INCONCLUSIVE
        if result.errors
        else aggregate_status(p.status for p in result.pairs)
    )
    if result.pairs:
        result.worst_metrics = Metrics(
            *(
                max(getattr(p.metrics, item.name) for p in result.pairs)
                for item in fields(Metrics)
            )
        )
    if worst is not None:
        _, a, b, eye = worst
        result.artifact_pair = ArtifactPair(a.label, b.label, eye)
        result.artifacts, result.ranked_tiles = write_artifacts(
            output, stem, a.image(eye), b.image(eye), thresholds, tile_size, top_tiles
        )
    return result


def save_image(output, stem, suffix, pixels):
    relative = f"images/{stem}-{suffix}.png"
    Image.fromarray(pixels).save(output / relative)
    return relative


def write_artifacts(output, stem, a, b, thresholds, tile_size, top_tiles):
    (output / "images").mkdir(exist_ok=True)
    error = np.abs(a.astype(np.int16) - b.astype(np.int16)).astype(np.uint8)
    artifacts = Artifacts(
        save_image(output, stem, "a", a),
        save_image(output, stem, "b", b),
        save_image(output, stem, "diff", error),
        save_image(
            output,
            stem,
            "diff-x8",
            np.minimum(error.astype(np.uint16) * 8, 255).astype(np.uint8),
        ),
        save_image(
            output,
            stem,
            "changed",
            (error.max(axis=2) > thresholds.pixel).astype(np.uint8) * 255,
        ),
    )
    height, width = a.shape[:2]
    ranked = []
    for y in range(0, height, tile_size):
        for x in range(0, width, tile_size):
            right, bottom = min(x + tile_size, width), min(y + tile_size, height)
            metrics = image_metrics(
                a[y:bottom, x:right], b[y:bottom, x:right], thresholds
            )
            ranked.append(((x, y, right, bottom), metrics))
    ranked.sort(
        key=lambda tile: (tile[1].mean, tile[1].changed_fraction, tile[1].max),
        reverse=True,
    )
    tiles = []
    for index, (box, metrics) in enumerate(ranked[:top_tiles]):
        x, y, right, bottom = box
        crops = Artifacts(
            *(
                save_image(
                    output, f"{stem}-tile{index}", suffix, pixels[y:bottom, x:right]
                )
                for suffix, pixels in (("a", a), ("b", b), ("diff", error))
            )
        )
        tiles.append(Tile(box, metrics, crops))
    return artifacts, tiles


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    for flag in ("pixel", "changed-fraction", "mean"):
        parser.add_argument(f"--{flag}", type=float)
        parser.add_argument(f"--repeat-{flag}", type=float)
    parser.add_argument("--metadata-tolerance", type=float, default=1e-5)
    parser.add_argument("--tile-size", type=int, default=64)
    parser.add_argument("--top-tiles", type=int, default=5)
    args = parser.parse_args(argv)
    try:
        manifest = Manifest.from_wire(read_json(args.manifest))
        limits = manifest.thresholds.overridden(args)
        base = manifest.repeatability if manifest.explicit_repeatability else limits
        repeats = base.overridden(args, "repeat_")
        report = analyze_manifest(
            args.manifest,
            args.output,
            thresholds=limits,
            repeatability=repeats,
            metadata_tolerance=args.metadata_tolerance,
            tile_size=args.tile_size,
            top_tiles=args.top_tiles,
        )
    except (OSError, ValueError, TypeError) as error:
        try:
            args.output.mkdir(parents=True, exist_ok=True)
            write_reports(
                args.output,
                Report(str(args.manifest), errors=[f"visual comparison: {error}"]),
            )
        except OSError as output_error:
            print(f"visual comparison: report output: {output_error}", file=sys.stderr)
        print(f"visual comparison: {error}", file=sys.stderr)
        return 2
    print(
        f"visual comparison: {report.status}; report: {args.output / 'report.html'}",
        file=sys.stderr,
    )
    if report.status == Outcome.PASS:
        return 0
    return 1 if report.status == Outcome.FAIL else 2
