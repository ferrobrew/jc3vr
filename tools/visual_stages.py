import argparse
import html
import json
import re
import sys
from dataclasses import asdict, dataclass, field
from enum import IntEnum, StrEnum
from itertools import combinations, product
from pathlib import Path, PureWindowsPath

import numpy as np
from PIL import Image

if __package__:
    from tools.visual_compare import (
        CaptureMetadata,
        Case,
        LoadedSample,
        Manifest,
        Mode,
        SampleSpec,
        Thresholds,
        image_metrics,
        integer,
        load_sample,
        metadata_mismatches,
        object_value,
        read_json,
        text,
    )
else:
    from visual_compare import (
        CaptureMetadata,
        Case,
        LoadedSample,
        Manifest,
        Mode,
        SampleSpec,
        Thresholds,
        image_metrics,
        integer,
        load_sample,
        metadata_mismatches,
        object_value,
        read_json,
        text,
    )


class Stage(StrEnum):
    GEOMETRY_DEPTH = "geometry_depth"
    GBUFFER0 = "gbuffer0"
    GBUFFER1 = "gbuffer1"
    GBUFFER2 = "gbuffer2"
    GBUFFER3 = "gbuffer3"
    MAIN_COLOR = "main_color"
    PRE_AA = "pre_aa"
    POST_AA = "post_aa"


class EvidenceStatus(StrEnum):
    MEASURED = "measured"
    INCONCLUSIVE = "inconclusive"


class DxgiFormat(IntEnum):
    R32G8X24_TYPELESS = 19
    D32_FLOAT_S8X24_UINT = 20
    R32_FLOAT_X8X24_TYPELESS = 21
    R10G10B10A2_TYPELESS = 23
    R10G10B10A2_UNORM = 24
    R10G10B10A2_UINT = 25
    R11G11B10_FLOAT = 26
    R8G8B8A8_TYPELESS = 27
    R8G8B8A8_UNORM = 28
    R8G8B8A8_UNORM_SRGB = 29
    B8G8R8A8_UNORM = 87
    B8G8R8A8_TYPELESS = 90
    B8G8R8A8_UNORM_SRGB = 91

    @property
    def depth(self) -> bool:
        return self in (
            self.R32G8X24_TYPELESS,
            self.D32_FLOAT_S8X24_UINT,
            self.R32_FLOAT_X8X24_TYPELESS,
        )

    @property
    def pixel_bytes(self) -> int:
        return 8 if self.depth else 4

    @classmethod
    def from_wire(cls, value: object) -> "DxgiFormat":
        if not isinstance(value, str):
            raise TypeError("stage.source.format: expected a DXGI format string")
        numeric = re.fullmatch(r"DXGI_FORMAT\((\d+)\)", value)
        try:
            if numeric:
                return cls(int(numeric.group(1)))
            return cls[value.removeprefix("DXGI_FORMAT_")]
        except (KeyError, ValueError) as error:
            raise ValueError(
                f"stage.source.format: unsupported DXGI format {value!r}"
            ) from error


@dataclass(frozen=True)
class Source:
    width: int
    height: int
    format: DxgiFormat

    @classmethod
    def from_wire(cls, value: object) -> "Source":
        value = object_value(value, "stage.source")
        return cls(
            integer(value, "width"),
            integer(value, "height"),
            DxgiFormat.from_wire(value.get("format")),
        )


@dataclass(frozen=True)
class StageRecord:
    stage: Stage
    eye: int
    frame: int
    source: Source
    artifact: str
    encoding: str

    @classmethod
    def from_wire(cls, value: object) -> "StageRecord":
        value = object_value(value, "stage")
        if value.get("error") is not None:
            raise ValueError(f"stage capture error: {value['error']}")
        eye = integer(value, "eye", minimum=0)
        if eye not in (0, 1):
            raise ValueError("stage.eye: expected a physical eye of zero or one")
        encoding = "raw" if value.get("raw") is not None else "png"
        return cls(
            Stage(text(value, "stage")),
            eye,
            integer(value, "frame", minimum=None),
            Source.from_wire(value.get("source")),
            text(value, encoding),
            encoding,
        )


@dataclass(frozen=True)
class StageIdentity:
    sample: str
    capture_id: str
    frame: int
    eye: int
    record_eye: int
    raw: str
    source: Source
    generation: int
    encoding: str


@dataclass
class StageSample:
    identity: StageIdentity
    capture: LoadedSample = field(repr=False)
    bits: np.ndarray = field(repr=False)
    values: np.ndarray = field(repr=False)
    channels: tuple[str, ...]


@dataclass(frozen=True)
class ChannelMetrics:
    channel: str
    exact_changed: int
    mae: float
    maximum: float


@dataclass(frozen=True)
class DepthCoverage:
    pixels: int
    covered_a: int
    covered_b: int
    both_covered: int
    either_covered: int
    coverage_changed: int
    covered_mae: float | None
    covered_maximum: float | None


@dataclass(frozen=True)
class NativeMetrics:
    pixels: int
    exact_changed: int
    exact_changed_fraction: float
    packed_byte_changed: int
    value_changed: int
    mae: float
    maximum: float
    channels: tuple[ChannelMetrics, ...]
    depth: DepthCoverage | None


@dataclass(frozen=True)
class PairMetrics:
    a: StageIdentity
    b: StageIdentity
    metrics: NativeMetrics


@dataclass
class Comparison:
    name: str
    stage: Stage
    eye: int
    modes: tuple[Mode, ...]
    status: EvidenceStatus = EvidenceStatus.INCONCLUSIVE
    pairs: list[PairMetrics] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    within_mode_mae: float | None = None
    within_mode_maximum: float | None = None
    within_mode_exact_changed_fraction: float | None = None
    cross_mae_minus_within_mode: float | None = None


@dataclass
class CaseReport:
    name: str
    repeatability: list[Comparison] = field(default_factory=list)
    comparisons: list[Comparison] = field(default_factory=list)


@dataclass
class Report:
    manifest: str
    status: EvidenceStatus = EvidenceStatus.INCONCLUSIVE
    purpose: str = (
        "Native stage diagnostics only; no stage thresholds or primary-result changes."
    )
    depth_coverage: str = "Reverse-Z depth is covered when its first float is nonzero. Depth exact changes include float32 and stencil8 where present, excluding all X padding. Packed byte changes include padding."
    cases: list[CaseReport] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)

    def to_wire(self) -> dict:
        return asdict(self)


def artifact_path(metadata: Path, recorded: str) -> Path:
    if not recorded or "\x00" in recorded:
        raise ValueError("stage.raw: invalid artifact path")
    windows = PureWindowsPath(recorded)
    if ".." in windows.parts or recorded.endswith(("/", "\\")):
        raise ValueError("stage.raw: invalid artifact basename or traversal")
    name = windows.name
    if not name or name in (".", "..") or ":" in name:
        raise ValueError("stage.raw: invalid artifact basename")
    directory = metadata.parent.resolve()
    path = directory / name
    if path.resolve().parent != directory:
        raise ValueError("stage.raw: artifact escapes the local metadata folder")
    return path


def decode_raw(
    data: bytes, source: Source
) -> tuple[np.ndarray, np.ndarray, tuple[str, ...]]:
    expected = source.width * source.height * source.format.pixel_bytes
    if len(data) != expected:
        raise ValueError(
            f"stage.raw: expected exactly {expected} bytes, got {len(data)}"
        )
    bits = np.frombuffer(data, dtype=np.uint8).reshape(
        source.height, source.width, source.format.pixel_bytes
    )
    if source.format.depth:
        values = np.frombuffer(data, dtype="<f4").reshape(
            source.height, source.width, 2
        )[..., :1]
        channels = ("depth",)
    elif source.format == DxgiFormat.R11G11B10_FLOAT:
        words = np.frombuffer(data, dtype="<u4").reshape(source.height, source.width)
        values = np.stack(
            (
                decode_unsigned_float(words & 2047, 6),
                decode_unsigned_float((words >> 11) & 2047, 6),
                decode_unsigned_float((words >> 22) & 1023, 5),
            ),
            axis=-1,
        )
        channels = ("r", "g", "b")
    elif source.format in (
        DxgiFormat.R10G10B10A2_TYPELESS,
        DxgiFormat.R10G10B10A2_UNORM,
        DxgiFormat.R10G10B10A2_UINT,
    ):
        words = np.frombuffer(data, dtype="<u4").reshape(source.height, source.width)
        values = np.stack(
            (words & 1023, (words >> 10) & 1023, (words >> 20) & 1023, words >> 30),
            axis=-1,
        )
        channels = ("r", "g", "b", "a")
    else:
        values = bits
        if source.format in (
            DxgiFormat.B8G8R8A8_UNORM,
            DxgiFormat.B8G8R8A8_TYPELESS,
            DxgiFormat.B8G8R8A8_UNORM_SRGB,
        ):
            values = bits[..., [2, 1, 0, 3]]
        channels = ("r", "g", "b", "a")
    values = values.astype(np.float64)
    if not np.isfinite(values).all():
        raise ValueError(
            "stage.raw: nonfinite decoded values have no finite native metrics"
        )
    return bits, values, channels


def decode_unsigned_float(words: np.ndarray, mantissa_bits: int) -> np.ndarray:
    mantissa = (words & ((1 << mantissa_bits) - 1)).astype(np.float64)
    exponent = (words >> mantissa_bits).astype(np.int32)
    normal = np.ldexp(1.0 + mantissa / (1 << mantissa_bits), exponent - 15)
    subnormal = np.ldexp(mantissa, 1 - 15 - mantissa_bits)
    return np.where(
        exponent == 31,
        np.where(mantissa == 0, np.inf, np.nan),
        np.where(exponent == 0, subnormal, normal),
    )


def load_stage(
    root: Path,
    mode: Mode,
    spec: SampleSpec,
    sample: LoadedSample,
    stage: Stage,
    eye: int,
) -> StageSample:
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
            sample.image(physical_eye).shape[:2]
            != (capture.eye_height, capture.eye_width)
            for physical_eye in mode.eyes
        )
    ):
        raise ValueError(
            "stage metadata: reread capture differs from the validated sample"
        )
    records = metadata.get("stages")
    if not isinstance(records, list):
        raise TypeError("metadata.stages: missing stage records")
    record_eye = 0 if mode == Mode.SINGLE_PASS else eye
    matches = [
        record
        for record in records
        if isinstance(record, dict)
        and record.get("stage") == stage
        and type(record.get("eye")) is int
        and record["eye"] == record_eye
    ]
    if len(matches) != 1:
        raise ValueError(
            f"{stage} physical record eye {record_eye}: expected one record, got {len(matches)}"
        )
    record = StageRecord.from_wire(matches[0])
    if record.frame != sample.predicted_display_time_ns:
        raise ValueError("stage.frame: differs from capture predicted_display_time_ns")
    if (stage == Stage.GEOMETRY_DEPTH) != record.source.format.depth:
        raise ValueError(
            "stage.source.format: depth and color stage formats are incompatible"
        )
    if mode == Mode.SINGLE_PASS and record.source.width % 2:
        raise ValueError(
            "stage.source.width: single-pass source must have two equal halves"
        )
    path = artifact_path(metadata_path, record.artifact)
    if record.encoding == "raw":
        expected = (
            record.source.width
            * record.source.height
            * record.source.format.pixel_bytes
        )
        if path.stat().st_size != expected:
            raise ValueError(
                f"stage.raw: expected exactly {expected} bytes, got {path.stat().st_size}"
            )
        bits, values, channels = decode_raw(path.read_bytes(), record.source)
    else:
        if stage not in (Stage.PRE_AA, Stage.POST_AA) or record.source.format not in (
            DxgiFormat.R8G8B8A8_TYPELESS,
            DxgiFormat.R8G8B8A8_UNORM,
            DxgiFormat.R8G8B8A8_UNORM_SRGB,
            DxgiFormat.B8G8R8A8_TYPELESS,
            DxgiFormat.B8G8R8A8_UNORM,
            DxgiFormat.B8G8R8A8_UNORM_SRGB,
        ):
            raise ValueError(
                "stage.png: unsupported stage or source format for lossless RGB evidence"
            )
        with Image.open(path) as image:
            if image.format != "PNG" or image.size != (
                record.source.width,
                record.source.height,
            ):
                raise ValueError(
                    "stage.png: actual PNG dimensions or format differ from source descriptor"
                )
            if image.mode not in ("RGB", "RGBA"):
                raise ValueError("stage.png: expected RGB or RGBA")
            if image.mode == "RGBA" and image.getchannel("A").getextrema() != (
                255,
                255,
            ):
                raise ValueError("stage.png: expected the writer's opaque alpha")
            bits = np.array(image.convert("RGB"))
        values, channels = bits.astype(np.float64), ("r", "g", "b")
    if mode == Mode.SINGLE_PASS:
        width = record.source.width // 2
        bits = bits[:, eye * width : (eye + 1) * width]
        values = values[:, eye * width : (eye + 1) * width]
    identity = StageIdentity(
        sample.label,
        sample.capture_id,
        record.frame,
        eye,
        record_eye,
        str(path),
        record.source,
        sample.visual.generation,
        record.encoding,
    )
    return StageSample(identity, sample, bits, values, channels)


def native_metrics(a: StageSample, b: StageSample) -> NativeMetrics:
    if a.identity.eye != b.identity.eye:
        raise ValueError("stage comparison: physical eyes differ")
    if a.identity.encoding != b.identity.encoding:
        raise ValueError("stage comparison: artifact encodings differ")
    if a.identity.source.format != b.identity.source.format:
        raise ValueError("stage comparison: DXGI formats differ")
    if a.values.shape != b.values.shape or a.bits.shape != b.bits.shape:
        raise ValueError("stage comparison: actual source eye dimensions differ")
    errors = metadata_mismatches(a.capture.inputs, b.capture.inputs, 1e-5)
    if errors:
        raise ValueError("stage comparison: " + "; ".join(errors))
    packed_changed = np.any(a.bits != b.bits, axis=-1)
    if a.identity.source.format == DxgiFormat.R32_FLOAT_X8X24_TYPELESS:
        meaningful_bytes = 4
    elif a.identity.source.format.depth:
        meaningful_bytes = 5
    else:
        meaningful_bytes = a.bits.shape[-1]
    changed = np.any(
        a.bits[..., :meaningful_bytes] != b.bits[..., :meaningful_bytes], axis=-1
    )
    delta = np.abs(a.values - b.values)
    pixels = int(changed.size)
    channel_changed = a.values.view(np.uint64) != b.values.view(np.uint64)
    channels = tuple(
        ChannelMetrics(
            name,
            int(np.count_nonzero(channel_changed[..., index])),
            float(delta[..., index].mean()),
            float(delta[..., index].max()),
        )
        for index, name in enumerate(a.channels)
    )
    depth = None
    if a.identity.source.format.depth:
        covered_a, covered_b = a.values[..., 0] != 0, b.values[..., 0] != 0
        both, either = covered_a & covered_b, covered_a | covered_b
        covered_delta = delta[..., 0][both]
        depth = DepthCoverage(
            pixels,
            int(covered_a.sum()),
            int(covered_b.sum()),
            int(both.sum()),
            int(either.sum()),
            int(np.count_nonzero(covered_a != covered_b)),
            float(covered_delta.mean()) if covered_delta.size else None,
            float(covered_delta.max()) if covered_delta.size else None,
        )
    return NativeMetrics(
        pixels,
        int(changed.sum()),
        float(changed.mean()),
        int(packed_changed.sum()),
        int(np.count_nonzero(np.any(delta != 0, axis=-1))),
        float(delta.mean()),
        float(delta.max()),
        channels,
        depth,
    )


def compare(
    name: str,
    stage: Stage,
    eye: int,
    modes: tuple[Mode, ...],
    pairs: list[tuple[StageSample, StageSample]],
) -> Comparison:
    result = Comparison(name, stage, eye, modes)
    for a, b in pairs:
        try:
            result.pairs.append(
                PairMetrics(a.identity, b.identity, native_metrics(a, b))
            )
        except ValueError as error:
            result.errors.append(f"{a.identity.sample} vs {b.identity.sample}: {error}")
    if result.pairs and not result.errors:
        result.status = EvidenceStatus.MEASURED
    return result


def validated_samples(
    root: Path, mode: Mode, entries: tuple[SampleSpec, ...]
) -> tuple[list[tuple[SampleSpec, LoadedSample]], list[str]]:
    loaded = []
    errors = []
    seen_png, seen_metadata, seen_ids, seen_frames, seen_times = (
        set(),
        set(),
        set(),
        set(),
        set(),
    )
    if len(entries) < 3:
        errors.append(
            "at least three independent captures are required for repeatability"
        )
    for index, spec in enumerate(entries):
        try:
            png, metadata = (
                (root / spec.png).resolve(),
                (root / spec.metadata).resolve(),
            )
            sample = load_sample(root, mode, index, spec)
            if (
                png in seen_png
                or metadata in seen_metadata
                or sample.capture_id in seen_ids
                or sample.frame_ordinal in seen_frames
                or sample.predicted_display_time_ns in seen_times
            ):
                raise ValueError(
                    "reused path, capture id, or frame is not independent evidence"
                )
            seen_png.add(png)
            seen_metadata.add(metadata)
            seen_ids.add(sample.capture_id)
            seen_frames.add(sample.frame_ordinal)
            seen_times.add(sample.predicted_display_time_ns)
            loaded.append((spec, sample))
        except (OSError, ValueError, TypeError) as error:
            errors.append(f"{mode}[{index}]: {error}")
    return loaded, errors


def analyze_case(case: Case, root: Path, repeatability: Thresholds) -> CaseReport:
    result = CaseReport(case.name)
    modes = (Mode.MULTIPASS, Mode.SINGLE_PASS)
    entries = {group.mode: group.samples for group in case.modes}
    captures = {
        mode: validated_samples(root, mode, entries.get(mode, ())) for mode in modes
    }
    for mode in modes:
        samples, errors = captures[mode]
        if errors:
            continue
        for (_, a), (_, b) in combinations(samples, 2):
            mismatches = metadata_mismatches(a.inputs, b.inputs, 1e-5)
            if mismatches:
                errors.append(
                    "final-image repeatability inputs: " + "; ".join(mismatches)
                )
            for eye in (0, 1):
                if not image_metrics(a.image(eye), b.image(eye), repeatability).passes(
                    repeatability
                ):
                    errors.append(
                        f"final-image repeatability is unstable: {a.label} vs {b.label}, eye {eye}"
                    )
    for stage in Stage:
        for eye in (0, 1):
            loaded = {}
            repeats = {}
            for mode in modes:
                key = (stage, eye, mode)
                samples, errors = [], list(captures[mode][1])
                if not errors:
                    seen = set()
                    for spec, sample in captures[mode][0]:
                        try:
                            evidence = load_stage(root, mode, spec, sample, stage, eye)
                            path = Path(evidence.identity.raw).resolve()
                            if path in seen:
                                raise ValueError(
                                    "stage.raw: reused artifact is not independent evidence"
                                )
                            seen.add(path)
                            samples.append(evidence)
                        except (OSError, ValueError, TypeError) as error:
                            errors.append(f"{sample.label}: {error}")
                loaded[key] = samples
                repeat = (
                    Comparison(
                        f"{mode} repeatability", stage, eye, (mode,), errors=errors
                    )
                    if errors
                    else compare(
                        f"{mode} repeatability",
                        stage,
                        eye,
                        (mode,),
                        list(combinations(samples, 2)),
                    )
                )
                repeats[key] = repeat
                result.repeatability.append(repeat)
            within = [repeats[(stage, eye, mode)] for mode in modes]
            if any(item.status != EvidenceStatus.MEASURED for item in within):
                cross = Comparison(
                    "multipass vs single_pass",
                    stage,
                    eye,
                    modes,
                    errors=[
                        "valid same-mode repeatability evidence is required for both modes first"
                    ],
                )
            else:
                cross = compare(
                    "multipass vs single_pass",
                    stage,
                    eye,
                    modes,
                    list(product(*(loaded[(stage, eye, mode)] for mode in modes))),
                )
                noise = [pair.metrics for item in within for pair in item.pairs]
                cross.within_mode_mae = max(metric.mae for metric in noise)
                cross.within_mode_maximum = max(metric.maximum for metric in noise)
                cross.within_mode_exact_changed_fraction = max(
                    metric.exact_changed_fraction for metric in noise
                )
                if cross.pairs:
                    cross.cross_mae_minus_within_mode = (
                        max(pair.metrics.mae for pair in cross.pairs)
                        - cross.within_mode_mae
                    )
            result.comparisons.append(cross)
    return result


def analyze_manifest(manifest_path: Path, output: Path) -> Report:
    manifest_path, output = Path(manifest_path), Path(output)
    report = Report(str(manifest_path))
    try:
        manifest = Manifest.from_wire(read_json(manifest_path))
        report.cases = [
            analyze_case(case, manifest_path.parent, manifest.repeatability)
            for case in manifest.cases
        ]
        checks = [
            comparison
            for case in report.cases
            for comparison in case.repeatability + case.comparisons
        ]
        if checks and all(check.status == EvidenceStatus.MEASURED for check in checks):
            report.status = EvidenceStatus.MEASURED
    except (OSError, ValueError, TypeError) as error:
        report.errors.append(f"visual stages: {error}")
    write_reports(output, report)
    return report


def comparison_html(comparison: Comparison) -> str:
    rows = []
    for pair in comparison.pairs:
        metric = pair.metrics
        channels = "; ".join(
            f"{item.channel}: changed={item.exact_changed}, MAE={item.mae:.9g}, max={item.maximum:.9g}"
            for item in metric.channels
        )
        depth = json.dumps(asdict(metric.depth)) if metric.depth else ""
        rows.append(
            "<tr>"
            + "".join(
                f"<td>{html.escape(str(value))}</td>"
                for value in (
                    pair.a.sample + " vs " + pair.b.sample,
                    metric.exact_changed,
                    f"{metric.exact_changed_fraction:.9g}",
                    metric.value_changed,
                    f"{metric.mae:.9g}",
                    f"{metric.maximum:.9g}",
                    channels,
                    depth,
                )
            )
            + "</tr>"
        )
    errors = "".join(f"<li>{html.escape(error)}</li>" for error in comparison.errors)
    noise = ""
    if comparison.within_mode_mae is not None:
        noise = (
            f"<p>Within-mode worst MAE: {comparison.within_mode_mae:.9g}; "
            f"max: {comparison.within_mode_maximum:.9g}; "
            f"exact changed fraction: {comparison.within_mode_exact_changed_fraction:.9g}. "
            f"Cross MAE minus within-mode worst: {comparison.cross_mae_minus_within_mode}.</p>"
        )
    return (
        f"<h3>{html.escape(comparison.name)} / {comparison.stage} / eye {comparison.eye}: {comparison.status}</h3>"
        + noise
        + f"<ul>{errors}</ul><table><tr><th>Pair</th><th>Exact bit-changed pixels</th>"
        "<th>Fraction</th><th>Value-changed pixels</th><th>MAE</th><th>Maximum</th>"
        "<th>Native channels</th><th>Depth coverage</th></tr>"
        + "".join(rows)
        + "</table>"
    )


def write_reports(output: Path, report: Report) -> None:
    output.mkdir(parents=True, exist_ok=True)
    (output / "stages.json").write_text(
        json.dumps(report.to_wire(), indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    parts = [
        "<!doctype html><html><head><meta charset='utf-8'><title>Native stage diagnostics</title>",
        "<style>body{font-family:sans-serif}table{border-collapse:collapse}td,th{border:1px solid #aaa;padding:.3em}</style>",
        "</head><body><h1>Native stage diagnostics</h1>",
        f"<p>{html.escape(report.purpose)}</p><p>{html.escape(report.depth_coverage)}</p>",
        (
            "<p>Raw integer color channels use native stored code values, including alpha. PNG stages use encoded RGB code values; the writer forces opaque alpha, so PNG alpha is not evidence. Packed float channels use decoded floats. "
            "No gamma conversion, normalization, resizing, registration, or stage pass/fail thresholds apply. "
            "Measured means compatible evidence is available, not visual equivalence. "
            "Cross-mode errors must be read with the preceding same-mode noise and depth coverage.</p>"
        ),
        f"<p>Evidence: {report.status}</p>",
    ]
    parts.extend(f"<p>{html.escape(error)}</p>" for error in report.errors)
    for case in report.cases:
        parts.append(
            f"<h2>{html.escape(case.name)}</h2><h2>Same-mode repeatability</h2>"
        )
        parts.extend(comparison_html(item) for item in case.repeatability)
        parts.append("<h2>Cross-mode comparisons</h2>")
        parts.extend(comparison_html(item) for item in case.comparisons)
    parts.append("</body></html>")
    (output / "stages.html").write_text("\n".join(parts) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Native raw stage diagnostics for a visual suite manifest."
    )
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        report = analyze_manifest(args.manifest, args.output)
    except OSError as error:
        print(f"visual stages: report output: {error}", file=sys.stderr)
        return 2
    print(
        f"visual stages: {report.status}; report: {args.output / 'stages.html'}",
        file=sys.stderr,
    )
    return 0 if report.status == EvidenceStatus.MEASURED else 2


if __name__ == "__main__":
    sys.exit(main())
