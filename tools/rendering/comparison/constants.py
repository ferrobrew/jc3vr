"""Native shader-constant diagnostics for a visual suite manifest."""

import argparse
import html
import json
import math
import struct
import sys
from dataclasses import asdict, dataclass, field
from enum import IntEnum, StrEnum
from itertools import combinations, product
from pathlib import Path

from tools.rendering.comparison.images import (
    CaptureMetadata,
    Case,
    LoadedSample,
    Manifest,
    Mode,
    SampleSpec,
    Thresholds,
    image_metrics,
    integer,
    metadata_mismatches,
    object_value,
    read_json,
)
from tools.rendering.comparison.stages import validated_samples


class EvidenceStatus(StrEnum):
    MEASURED = "measured"
    INCONCLUSIVE = "inconclusive"


class PhysicalEye(StrEnum):
    LEFT = "left"
    RIGHT = "right"

    @property
    def index(self) -> int:
        return 0 if self == PhysicalEye.LEFT else 1


class ConstantSource(StrEnum):
    MULTIPASS_CB0 = "multipass_cb0"
    SINGLE_PASS_CB13 = "single_pass_cb13"


class GeometryPassBound(IntEnum):
    RP_Z_OCCLUDERS = 47
    RP_LAST_GBUFFER = 85


class Cb13Status(StrEnum):
    UPLOADED = "uploaded"
    UPLOAD_UNAVAILABLE = "upload_unavailable"
    LIVE_ROWS_UNAVAILABLE = "live_rows_unavailable"


class FieldName(StrEnum):
    EFFECTIVE_VP = "effective.offset_view_projection"
    EFFECTIVE_POSITION = "effective.camera_position"
    CONTEXT_VIEW = "render_context.view"
    CONTEXT_PROJECTION = "render_context.projection"
    CONTEXT_VP = "render_context.offset_view_projection"
    CONTEXT_POSITION = "render_context.camera_position"
    CENTERED_VP = "centered_cb0.offset_view_projection"
    CENTERED_POSITION = "centered_cb0.camera_position"
    REPROJECTION = "live_cb13.reprojection"


@dataclass(frozen=True)
class F32s:
    bits: tuple[int, ...]

    @property
    def values(self) -> tuple[float, ...]:
        return tuple(
            struct.unpack("<f", struct.pack("<I", word))[0] for word in self.bits
        )

    @classmethod
    def from_wire(cls, value: object, length: int) -> "F32s":
        value = object_value(value, "f32s")
        bits = array(value.get("bits"), length, "f32s.bits")
        if any(type(word) is not int or not 0 <= word <= 0xFFFFFFFF for word in bits):
            raise ValueError("f32s.bits: expected native u32 words")
        result = cls(tuple(bits))
        if not all(math.isfinite(item) for item in result.values):
            raise ValueError("f32s.bits: nonfinite native values are incompatible")
        if "values" in value:
            values = array(value["values"], length, "f32s.values")
            for index, (item, word) in enumerate(zip(values, bits)):
                if type(item) not in (int, float):
                    raise ValueError("f32s.values: expected finite numbers")
                try:
                    if not math.isfinite(item):
                        raise ValueError("f32s.values: expected finite numbers")
                    actual = struct.unpack("<I", struct.pack("<f", item))[0]
                except (OverflowError, struct.error) as error:
                    raise ValueError("f32s.values: outside native f32 range") from error
                if actual != word:
                    raise ValueError(
                        f"f32s.values[{index}]: disagrees with native bits"
                    )
        return result


@dataclass(frozen=True)
class Cb0Rows:
    offset_view_projection: F32s
    camera_position: F32s

    @classmethod
    def from_wire(cls, value: object) -> "Cb0Rows":
        value = object_value(value, "cb0 rows")
        return cls(
            matrix_rows(value.get("offset_view_projection")),
            F32s.from_wire(value.get("camera_position"), 4),
        )


@dataclass(frozen=True)
class ContextInputs:
    view: F32s
    projection: F32s
    offset_view_projection: F32s
    camera_position: F32s

    @classmethod
    def from_wire(cls, value: object) -> "ContextInputs":
        value = object_value(value, "render_context")
        return cls(
            *(
                F32s.from_wire(value.get(name), length)
                for name, length in (
                    ("view", 16),
                    ("projection", 16),
                    ("offset_view_projection", 16),
                    ("camera_position", 3),
                )
            )
        )


@dataclass(frozen=True)
class ShaderConstants:
    source: ConstantSource
    cb0: Cb0Rows
    eyes: tuple[Cb0Rows, Cb0Rows] | None = None
    reprojection: tuple[F32s, F32s] | None = None
    cb13_status: Cb13Status | None = None

    @classmethod
    def from_wire(cls, value: object) -> "ShaderConstants":
        value = object_value(value, "constants record.constants")
        source = ConstantSource(value.get("source"))
        if source == ConstantSource.MULTIPASS_CB0:
            return cls(source, Cb0Rows.from_wire(value.get("cb0")))
        centered = Cb0Rows.from_wire(value.get("centered_cb0"))
        raw_status = value.get("cb13_status")
        if raw_status is None:
            raise ValueError(
                "constants.cb13_status: unavailable; legacy live rows do not establish a successful upload"
            )
        status = Cb13Status(raw_status)
        if status != Cb13Status.UPLOADED:
            raise ValueError(
                f"constants.cb13_status: {status}; effective uploaded rows unavailable"
            )
        live = object_value(value.get("live_cb13"), "constants.live_cb13")
        eyes = tuple(
            Cb0Rows.from_wire(item)
            for item in array(live.get("eyes"), 2, "live_cb13.eyes")
        )
        reprojection = tuple(
            matrix_rows(item)
            for item in array(live.get("reprojection"), 2, "live_cb13.reprojection")
        )
        return cls(source, centered, eyes, reprojection, status)


@dataclass(frozen=True)
class ConstantRecord:
    frame: int
    generation: int
    mode: Mode
    physical_eye: PhysicalEye
    pass_id: int
    render_context: ContextInputs
    constants: ShaderConstants

    @classmethod
    def from_wire(cls, value: object) -> "ConstantRecord":
        value = object_value(value, "constants record")
        pass_id = integer(value, "pass", minimum=GeometryPassBound.RP_Z_OCCLUDERS)
        if pass_id >= GeometryPassBound.RP_LAST_GBUFFER:
            raise ValueError("constants record.pass: outside the geometry-pass range")
        return cls(
            integer(value, "frame", minimum=None),
            integer(value, "generation"),
            Mode(value.get("mode")),
            PhysicalEye(value.get("physical_eye")),
            pass_id,
            ContextInputs.from_wire(value.get("render_context")),
            ShaderConstants.from_wire(value.get("constants")),
        )


@dataclass(frozen=True)
class ConstantIdentity:
    sample: str
    capture_id: str
    frame_ordinal: int
    frame: int
    generation: int
    mode: Mode
    pass_id: int
    physical_eye: PhysicalEye
    record_eye: PhysicalEye
    source: ConstantSource
    cb13_status: Cb13Status | None
    metadata: str


@dataclass(frozen=True)
class ConstantSample:
    identity: ConstantIdentity
    capture: LoadedSample = field(repr=False)
    fields: dict[FieldName, F32s]


@dataclass(frozen=True)
class CapturedConstants:
    records: dict[tuple[int, PhysicalEye], ConstantSample]
    unscoped_uploads: int


@dataclass(frozen=True)
class CaptureSummary:
    sample: str
    capture_id: str
    unscoped_uploads: int


@dataclass(frozen=True)
class FieldMetrics:
    field: FieldName
    count: int
    unequal: int
    maxulp: int
    maxabs: float
    a_values: tuple[float, ...] | None = None
    b_values: tuple[float, ...] | None = None
    a_bits: tuple[int, ...] | None = None
    b_bits: tuple[int, ...] | None = None


@dataclass(frozen=True)
class PairMetrics:
    a: ConstantIdentity
    b: ConstantIdentity
    fields: list[FieldMetrics]


@dataclass
class Comparison:
    name: str
    pass_id: int
    physical_eye: PhysicalEye
    modes: tuple[Mode, ...]
    status: EvidenceStatus = EvidenceStatus.INCONCLUSIVE
    pairs: list[PairMetrics] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    within_mode: list[FieldMetrics] = field(default_factory=list)
    worst_fields: list[FieldMetrics] = field(default_factory=list)


@dataclass
class ModeReport:
    mode: Mode
    final_image_status: EvidenceStatus = EvidenceStatus.INCONCLUSIVE
    final_image_pairs: int = 0
    captures: list[CaptureSummary] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)


@dataclass
class CaseReport:
    name: str
    modes: list[ModeReport] = field(default_factory=list)
    repeatability: list[Comparison] = field(default_factory=list)
    comparisons: list[Comparison] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)


@dataclass
class Report:
    manifest: str
    status: EvidenceStatus = EvidenceStatus.INCONCLUSIVE
    purpose: str = (
        "Source constants: multipass uses actual cb0 eye rows; single-pass uses the physical eye's "
        "effective live cb13 rows, not centered cb0. Context matrices are reported separately: "
        "single-pass center context and multipass eye context can differ by design. "
        "Measured means compatible diagnostic evidence, not bitwise or visual equivalence. "
        "No constant pass/fail thresholds, normalization, or registration apply."
    )
    cases: list[CaseReport] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    repeatability_thresholds: Thresholds | None = None

    def to_wire(self) -> dict:
        return json.loads(json.dumps(asdict(self), allow_nan=False))


def load_constants(
    root: Path,
    mode: Mode,
    spec: SampleSpec,
    sample: LoadedSample,
) -> CapturedConstants:
    metadata_path = root / spec.metadata
    metadata = read_json(metadata_path)
    capture = CaptureMetadata.from_wire(metadata, spec.inputs)
    capture.validate(mode)
    if (
        capture.capture_id != sample.capture_id
        or capture.frame_ordinal != sample.frame_ordinal
        or capture.predicted_display_time_ns != sample.predicted_display_time_ns
        or capture.visual != sample.visual
        or capture.inputs != sample.inputs
        or any(
            sample.image(eye).shape[:2] != (capture.eye_height, capture.eye_width)
            for eye in mode.eyes
        )
    ):
        raise ValueError(
            "constants metadata: reread capture differs from the validated sample"
        )
    captured = object_value(metadata.get("constants"), "metadata.constants")
    unscoped = integer(captured, "unscoped_uploads", minimum=0)
    records = captured.get("records")
    if not isinstance(records, list) or not records:
        raise ValueError("constants.records: expected nonempty scoped records")
    result = {}
    for raw in records:
        record = ConstantRecord.from_wire(raw)
        if (record.frame, record.generation, record.mode) != (
            sample.predicted_display_time_ns,
            sample.visual.generation,
            mode,
        ):
            raise ValueError(
                "constants record frame/generation/mode: differs from its capture"
            )
        single = mode == Mode.SINGLE_PASS
        expected = (
            ConstantSource.SINGLE_PASS_CB13 if single else ConstantSource.MULTIPASS_CB0
        )
        if record.constants.source != expected:
            raise ValueError("constants.source: incompatible with capture mode")
        if single and record.physical_eye != PhysicalEye.LEFT:
            raise ValueError(
                "constants.physical_eye: single-pass upload must use dispatch eye left"
            )
        if record.physical_eye.index not in mode.eyes:
            raise ValueError("constants.physical_eye: absent from capture mode")
        for eye in PhysicalEye if single else (record.physical_eye,):
            key = (record.pass_id, eye)
            if key in result:
                raise ValueError(
                    "constants.records: duplicate pass/physical-eye attribution"
                )
            shader = record.constants
            effective = shader.eyes[eye.index] if single else shader.cb0
            context = record.render_context
            values = {
                FieldName.EFFECTIVE_VP: effective.offset_view_projection,
                FieldName.EFFECTIVE_POSITION: effective.camera_position,
                FieldName.CONTEXT_VIEW: context.view,
                FieldName.CONTEXT_PROJECTION: context.projection,
                FieldName.CONTEXT_VP: context.offset_view_projection,
                FieldName.CONTEXT_POSITION: context.camera_position,
            }
            if single:
                values.update(
                    {
                        FieldName.CENTERED_VP: shader.cb0.offset_view_projection,
                        FieldName.CENTERED_POSITION: shader.cb0.camera_position,
                        FieldName.REPROJECTION: shader.reprojection[eye.index],
                    }
                )
            identity = ConstantIdentity(
                sample.label,
                sample.capture_id,
                sample.frame_ordinal,
                record.frame,
                record.generation,
                mode,
                record.pass_id,
                eye,
                record.physical_eye,
                shader.source,
                shader.cb13_status,
                str(metadata_path),
            )
            result[key] = ConstantSample(identity, sample, values)
    return CapturedConstants(result, unscoped)


def native_metrics(
    a: ConstantSample,
    b: ConstantSample,
    *,
    include_values: bool = False,
) -> list[FieldMetrics]:
    if (a.identity.pass_id, a.identity.physical_eye) != (
        b.identity.pass_id,
        b.identity.physical_eye,
    ):
        raise ValueError("constants comparison: pass/physical-eye attribution differs")
    errors = metadata_mismatches(a.capture.inputs, b.capture.inputs, 1e-5)
    if errors:
        raise ValueError("constants comparison inputs: " + "; ".join(errors))
    if a.identity.mode == b.identity.mode and a.fields.keys() != b.fields.keys():
        raise ValueError("constants comparison: same-mode field sets differ")
    result = []
    for name in FieldName:
        if name not in a.fields or name not in b.fields:
            continue
        left, right = a.fields[name], b.fields[name]
        if len(left.bits) != len(right.bits):
            raise ValueError(f"constants comparison {name}: incompatible dimensions")
        result.append(
            FieldMetrics(
                name,
                len(left.bits),
                sum(x != y for x, y in zip(left.bits, right.bits)),
                max(
                    abs(ordered_f32(x) - ordered_f32(y))
                    for x, y in zip(left.bits, right.bits)
                ),
                max(abs(x - y) for x, y in zip(left.values, right.values)),
                left.values if include_values else None,
                right.values if include_values else None,
                left.bits if include_values else None,
                right.bits if include_values else None,
            )
        )
    return result


def analyze_manifest(
    manifest_path: Path, output: Path, *, include_values: bool = False
) -> Report:
    manifest_path, output = Path(manifest_path), Path(output)
    report = Report(str(manifest_path))
    try:
        manifest = Manifest.from_wire(read_json(manifest_path))
        report.repeatability_thresholds = manifest.repeatability
        report.cases = [
            analyze_case(
                case, manifest_path.parent, manifest.repeatability, include_values
            )
            for case in manifest.cases
        ]
        checks = [
            item
            for case in report.cases
            for item in case.repeatability + case.comparisons
        ]
        if (
            checks
            and all(item.status == EvidenceStatus.MEASURED for item in checks)
            and not any(case.errors for case in report.cases)
        ):
            report.status = EvidenceStatus.MEASURED
    except (OSError, ValueError, TypeError) as error:
        report.errors.append(f"visual constants: {error}")
    write_reports(output, report)
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Native constants diagnostics for a visual suite manifest."
    )
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--include-values",
        action="store_true",
        help="Include decoded native values and u32 bits for each field.",
    )
    args = parser.parse_args(argv)
    try:
        report = analyze_manifest(
            args.manifest, args.output, include_values=args.include_values
        )
    except OSError as error:
        print(f"visual constants: report output: {error}", file=sys.stderr)
        return 2
    print(
        f"visual constants: {report.status}; report: {args.output / 'constants.html'}",
        file=sys.stderr,
    )
    return 0 if report.status == EvidenceStatus.MEASURED else 2


def array(value: object, length: int, context: str) -> list:
    if not isinstance(value, list) or len(value) != length:
        raise ValueError(f"{context}: expected an array of length {length}")
    return value


def matrix_rows(value: object) -> F32s:
    rows = array(value, 4, "matrix rows")
    return F32s(tuple(word for row in rows for word in F32s.from_wire(row, 4).bits))


def ordered_f32(word: int) -> int:
    # Adjacent signed zeros remain bit-distinct and one ordered representation step apart.
    return (~word & 0xFFFFFFFF) if word & 0x80000000 else word | 0x80000000


def final_image_repeatability(
    mode: Mode,
    samples: list[tuple[SampleSpec, LoadedSample]],
    errors: list[str],
    limits: Thresholds,
) -> ModeReport:
    report = ModeReport(mode, errors=list(errors))
    if report.errors:
        return report
    for (_, a), (_, b) in combinations(samples, 2):
        report.final_image_pairs += 1
        mismatches = metadata_mismatches(a.inputs, b.inputs, 1e-5)
        if mismatches:
            report.errors.append(
                "final-image repeatability inputs: " + "; ".join(mismatches)
            )
        for eye in mode.eyes:
            if not image_metrics(a.image(eye), b.image(eye), limits).passes(limits):
                report.errors.append(
                    f"final-image repeatability is unstable: {a.label} vs {b.label}, eye {eye}"
                )
    if not report.errors:
        report.final_image_status = EvidenceStatus.MEASURED
    return report


def compare(
    name: str,
    key: tuple[int, PhysicalEye],
    modes: tuple[Mode, ...],
    pairs: list[tuple[ConstantSample, ConstantSample]],
    errors: list[str],
    include_values: bool,
) -> Comparison:
    result = Comparison(name, key[0], key[1], modes, errors=list(errors))
    if result.errors:
        return result
    if not pairs:
        result.errors.append("constants comparison: missing independent pairs")
        return result
    for a, b in pairs:
        try:
            result.pairs.append(
                PairMetrics(
                    a.identity,
                    b.identity,
                    native_metrics(a, b, include_values=include_values),
                )
            )
        except (ValueError, TypeError) as error:
            result.errors.append(str(error))
    result.worst_fields = summarize_fields(result.pairs)
    if not result.errors:
        result.status = EvidenceStatus.MEASURED
    return result


def analyze_case(
    case: Case, root: Path, limits: Thresholds, include_values: bool
) -> CaseReport:
    result = CaseReport(case.name)
    entries = {group.mode: group.samples for group in case.modes}
    modes = (Mode.MULTIPASS, Mode.SINGLE_PASS) + (
        (Mode.REVERSE_MULTIPASS,) if Mode.REVERSE_MULTIPASS in entries else ()
    )
    loaded, mode_errors = {}, {}
    all_keys = set()
    for mode in modes:
        samples, errors = validated_samples(root, mode, entries.get(mode, ()))
        final = final_image_repeatability(mode, samples, errors, limits)
        result.modes.append(final)
        mode_errors[mode] = list(final.errors)
        loaded[mode] = []
        if not final.errors:
            for spec, sample in samples:
                try:
                    captured = load_constants(root, mode, spec, sample)
                    final.captures.append(
                        CaptureSummary(
                            sample.label, sample.capture_id, captured.unscoped_uploads
                        )
                    )
                    all_keys.update(captured.records)
                    loaded[mode].append(captured.records)
                except (OSError, ValueError, TypeError) as error:
                    mode_errors[mode].append(f"{sample.label}: {error}")
    if not all_keys:
        result.errors.append("constants: no compatible scoped pass records")
        result.errors.extend(
            error for errors in mode_errors.values() for error in errors
        )
        return result
    passes = {key[0] for key in all_keys}
    keys = sorted(product(passes, PhysicalEye), key=lambda key: (key[0], key[1].index))
    for key in keys:
        selected, repeats = {}, {}
        for mode in modes:
            errors = list(mode_errors[mode])
            selected[mode] = [
                records[key] for records in loaded[mode] if key in records
            ]
            if len(selected[mode]) != len(loaded[mode]):
                errors.append(
                    f"constants: missing pass {key[0]} / physical eye {key[1]}"
                )
            repeat = compare(
                f"{mode} constants repeatability",
                key,
                (mode,),
                list(combinations(selected[mode], 2)),
                errors,
                include_values,
            )
            repeats[mode] = repeat
            result.repeatability.append(repeat)
        for a, b in combinations(modes, 2):
            errors = []
            if any(repeats[mode].status != EvidenceStatus.MEASURED for mode in (a, b)):
                errors.append(
                    "valid final-image and constant repeatability evidence is required for both modes first"
                )
            cross = compare(
                f"{a} vs {b}",
                key,
                (a, b),
                list(product(selected[a], selected[b])),
                errors,
                include_values,
            )
            cross.within_mode = summarize_fields(
                [pair for mode in (a, b) for pair in repeats[mode].pairs]
            )
            result.comparisons.append(cross)
    return result


def summarize_fields(pairs: list[PairMetrics]) -> list[FieldMetrics]:
    result = []
    for name in FieldName:
        metrics = [item for pair in pairs for item in pair.fields if item.field == name]
        if metrics:
            result.append(
                FieldMetrics(
                    name,
                    max(item.count for item in metrics),
                    max(item.unequal for item in metrics),
                    max(item.maxulp for item in metrics),
                    max(item.maxabs for item in metrics),
                )
            )
    return result


def comparison_html(comparison: Comparison) -> str:
    parts = [
        f"<h3>{html.escape(comparison.name)} / pass {comparison.pass_id} / {comparison.physical_eye}: {comparison.status}</h3>"
    ]
    parts.extend(f"<p>{html.escape(error)}</p>" for error in comparison.errors)
    if comparison.worst_fields:
        parts.append(
            "<h4>Worst per-field metrics (independent maxima)</h4><table><tr><th>Field</th><th>Count</th><th>Unequal bits</th><th>Max ULP</th><th>Max absolute</th></tr>"
        )
        for metric in comparison.worst_fields:
            parts.append(
                "<tr>"
                + "".join(
                    f"<td>{html.escape(str(value))}</td>"
                    for value in (
                        metric.field,
                        metric.count,
                        metric.unequal,
                        metric.maxulp,
                        f"{metric.maxabs:.9g}",
                    )
                )
                + "</tr>"
            )
        parts.append("</table>")
    if comparison.within_mode:
        parts.append(
            "<details><summary>Same-mode worst per-field noise (independent maxima)</summary><pre>"
            + html.escape(
                json.dumps([asdict(item) for item in comparison.within_mode], indent=2)
            )
            + "</pre></details>"
        )
    parts.append(
        "<table><tr><th>Pair / sources</th><th>Field</th><th>Count</th><th>Unequal bits</th><th>Max ULP</th><th>Max absolute</th></tr>"
    )
    for pair in comparison.pairs:
        label = (
            f"{pair.a.sample} ({pair.a.source}) vs {pair.b.sample} ({pair.b.source})"
        )
        for metric in pair.fields:
            parts.append(
                "<tr>"
                + "".join(
                    f"<td>{html.escape(str(value))}</td>"
                    for value in (
                        label,
                        metric.field,
                        metric.count,
                        metric.unequal,
                        metric.maxulp,
                        f"{metric.maxabs:.9g}",
                    )
                )
                + "</tr>"
            )
    parts.append("</table>")
    for pair in comparison.pairs:
        parts.append(
            "<details><summary>Capture and pass attribution</summary><pre>"
            + html.escape(
                json.dumps({"a": asdict(pair.a), "b": asdict(pair.b)}, indent=2)
            )
            + "</pre></details>"
        )
        if any(item.a_values is not None for item in pair.fields):
            parts.append(
                "<details><summary>Native field values and bits</summary><pre>"
                + html.escape(
                    json.dumps([asdict(item) for item in pair.fields], indent=2)
                )
                + "</pre></details>"
            )
    return "\n".join(parts)


def write_reports(output: Path, report: Report) -> None:
    output.mkdir(parents=True, exist_ok=True)
    (output / "constants.json").write_text(
        json.dumps(report.to_wire(), indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    parts = [
        "<!doctype html><html><head><meta charset='utf-8'><title>Native constants diagnostics</title>",
        "<style>body{font-family:sans-serif}table{border-collapse:collapse}td,th{border:1px solid #aaa;padding:.3em}</style>",
        "</head><body><h1>Native constants diagnostics</h1>",
        f"<p>{html.escape(report.purpose)}</p><p>Evidence: {report.status}</p>",
        (
            "<p>Native f32 bits are authoritative. Unequal counts compare u32 words; maximum absolute errors use decoded native values. "
            "ULP distances use monotonic f32 bit ordering, including one step between signed zeros. "
            "Nonfinite values and inconsistent optional values are incompatible evidence. "
            "Frames and generations are validated against their own capture, not equated across captures. "
            "Constant repeatability is measured separately from final-image repeatability; constant differences do not imply a bug. "
            "Unscoped uploads count RP_NONE observations excluded from geometry-pass comparisons; their presence does not invalidate scoped records. "
            "Effective SPS evidence requires an explicit successful cb13 upload status.</p>"
        ),
        f"<p>Unmodified final-image repeatability thresholds: {html.escape(str(report.repeatability_thresholds))}</p>",
    ]
    parts.extend(f"<p>{html.escape(error)}</p>" for error in report.errors)
    for case in report.cases:
        parts.append(f"<h2>{html.escape(case.name)}</h2>")
        parts.extend(f"<p>{html.escape(error)}</p>" for error in case.errors)
        for mode in case.modes:
            parts.append(
                f"<h3>{mode.mode} final-image repeatability: {mode.final_image_status}</h3><p>Independent pairs: {mode.final_image_pairs}</p>"
            )
            parts.extend(f"<p>{html.escape(error)}</p>" for error in mode.errors)
            parts.append(
                "<details><summary>Ignored unscoped uploads per capture</summary><pre>"
                + html.escape(
                    json.dumps([asdict(item) for item in mode.captures], indent=2)
                )
                + "</pre></details>"
            )
        parts.append("<h2>Constants repeatability</h2>")
        parts.extend(comparison_html(item) for item in case.repeatability)
        parts.append("<h2>Cross-mode diagnostics</h2>")
        parts.extend(comparison_html(item) for item in case.comparisons)
    parts.append("</body></html>")
    (output / "constants.html").write_text("\n".join(parts) + "\n", encoding="utf-8")
