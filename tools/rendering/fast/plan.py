"""The pure half of a fast comparison: which snapshots a scene takes and what each compares with,
and how the comparisons become rows with noise floors and verdicts."""

from dataclasses import asdict, dataclass, field
from enum import StrEnum

from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import Thresholds
from tools.rendering.experiment_fsr import FsrVariant
from tools.rendering.protocol.snapshot import Comparison, Plane

# The modes a fast comparison renders as references for the others, in the order it renders them.
REFERENCE_MODES = (CompareMode.LEFT_ONLY, CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS)
DEFAULT_PLANES = (Plane.FINAL, Plane.DEPTH, Plane.ALBEDO, Plane.NORMALS)
# How many representable floats apart two depths may be and still match: a relative difference of
# about one part in 130,000, about 0.08 mm at ten metres. Two renders of the same surface through
# transforms that are equal in real arithmetic but rounded differently (single-pass's reprojected
# families against an isolated eye's own matrices) land a few to a few dozen floats apart over the
# whole surface; displaced geometry lands thousands apart. See "Comparing depth" in
# docs/mod/stereo/single-pass-stereo.md.
DEFAULT_DEPTH_ULPS = 64


class FastError(ValueError):
    pass


@dataclass(frozen=True)
class SnapshotStep:
    """One snapshot: the label it is kept under, whether it is kept, and what it is compared with."""
    label: str
    keep: bool
    compare: tuple[str, ...]
    # The mode (or `single_pass` for a variant) whose repeat this snapshot is, when it is one.
    repeat_of: str | None = None


@dataclass(frozen=True)
class ConfigureStep:
    """A configuration for one mode, or the single-pass mode under a variant's switches, or a mode
    under an FSR variant's settings, followed by its snapshots."""
    mode: CompareMode
    variant: str | None
    snapshots: tuple[SnapshotStep, ...]
    fsr_variant: str | None = None

    @property
    def name(self) -> str:
        if self.fsr_variant is not None:
            return f"{self.fsr_variant} {self.mode.value}"
        return self.variant if self.variant is not None else self.mode.value


def schedule(modes: tuple[CompareMode, ...], variants: tuple[str, ...], repeat: bool = True,
             fsr_variants: tuple[FsrVariant, ...] = ()) -> tuple[ConfigureStep, ...]:
    """The configurations and snapshots of a fast comparison over `modes`, the single-pass
    `variants`, and the `fsr_variants`. The isolated eyes and multipass are kept as references and
    taken first; every other snapshot is compared with each reference before it. With `repeat`, each
    mode (and the single-pass mode once for its variants) takes a second snapshot compared with its
    first, which gives the same-schedule noise floor; the repeat is not kept.

    Each FSR variant renders in each of its modes after everything else, and always takes a second
    snapshot compared with its first (the frame-to-frame change at a still camera). Its first is
    compared with the mode's own first snapshot when that is kept and the variant renders at the
    session's size; its floor is its own, not the mode's."""
    if CompareMode.REVERSE_MULTIPASS in modes:
        raise FastError("fast comparison: reverse_multipass is not supported")
    if len(set(modes)) != len(modes):
        raise FastError("fast comparison: a mode is configured twice")
    if variants and CompareMode.SINGLE_PASS not in modes:
        raise FastError("fast comparison: single-pass variants need the single_pass mode")
    ordered = [mode for mode in REFERENCE_MODES if mode in modes]
    ordered += [mode for mode in modes if mode not in REFERENCE_MODES]
    steps = []
    references: list[str] = []
    for mode in ordered:
        first = f"{mode.value}_a"
        compare = tuple(references)
        is_reference = mode in REFERENCE_MODES
        keep = is_reference or repeat
        snapshots = [SnapshotStep(first, keep, compare)]
        if repeat:
            snapshots.append(SnapshotStep(f"{mode.value}_b", False, (first,), repeat_of=mode.value))
        steps.append(ConfigureStep(mode, None, tuple(snapshots)))
        if is_reference:
            references.append(first)
    for index, variant in enumerate(variants, start=1):
        label = f"v{index}_{variant}".replace("-", "_")[:64]
        steps.append(ConfigureStep(CompareMode.SINGLE_PASS, variant,
                                   (SnapshotStep(label, False, tuple(references)),)))
    kept = {step.mode for step in steps if step.variant is None and step.snapshots[0].keep}
    for index, variant in enumerate(fsr_variants, start=1):
        for mode in variant.modes_in(tuple(ordered)):
            base = f"f{index}_{variant.name}_{mode.value}".replace("-", "_")[:62]
            compare = ((f"{mode.value}_a",) if mode in kept and variant.resolution_scale is None
                       else ())
            steps.append(ConfigureStep(mode, None, (
                SnapshotStep(f"{base}_a", True, compare),
                SnapshotStep(f"{base}_b", False, (f"{base}_a",), repeat_of=base),
            ), fsr_variant=variant.name))
    return tuple(steps)


class Verdict(StrEnum):
    PASS = "pass"
    NOISE = "noise"
    FAIL = "fail"
    REGRESS = "regress"


@dataclass(frozen=True)
class Limits:
    """How a row's count is judged. A row passes within the Python comparison's thresholds; one
    that exceeds them is within the noise when its count is at most `noise_factor` times the noise
    floor plus `noise_slack` of its pixels, and fails otherwise. Against a baseline, a row regresses
    when its count exceeds the baseline's by more than that same noise allowance."""
    thresholds: Thresholds = field(default_factory=Thresholds)
    noise_factor: float = 2.0
    noise_slack: float = 0.0002

    def allowance(self, floor: int, pixels: int) -> float:
        return self.noise_factor * floor + self.noise_slack * pixels

    def within_thresholds(self, comparison: Comparison) -> bool:
        return (comparison.fraction <= self.thresholds.changed_fraction
                and comparison.mean <= self.thresholds.mean)


@dataclass(frozen=True)
class Row:
    scene: str
    test: str
    against: str
    plane: str
    eye: int
    pixels: int
    differing: int
    mean: float
    largest: float
    bbox: tuple[int, int, int, int] | None
    floor: int | None
    verdict: Verdict
    baseline: int | None = None
    dump: dict | None = None
    # The pixels whose stored values change at all (see `Comparison.changed`).
    changed: int | None = None

    def to_wire(self) -> dict:
        value = asdict(self)
        value["verdict"] = self.verdict.value
        return value


def label_mode(label: str) -> str:
    """The mode a snapshot label belongs to: `left_only_a` is `left_only`; a variant's is
    `single_pass`; an FSR variant's is the variant in its mode (`f1_hybrid_off_multipass`), whose
    floor is its own."""
    if label.startswith("v") and label[1:2].isdigit():
        return CompareMode.SINGLE_PASS.value
    return label.rsplit("_", 1)[0]


def rows(scene: str, results: list[tuple[SnapshotStep, tuple[Comparison, ...]]], limits: Limits,
         baseline: dict[tuple, int] | None = None) -> tuple[list[Row], dict[tuple, int]]:
    """Judge every cross comparison against the noise floors the repeats gave. Returns the rows
    (cross comparisons only) and the floors, keyed by `(mode, plane, eye)`."""
    floors: dict[tuple, int] = {}
    for step, comparisons in results:
        if step.repeat_of is None:
            continue
        for comparison in comparisons:
            floors[(step.repeat_of, comparison.plane.value, comparison.eye)] = comparison.differing
    result = []
    for step, comparisons in results:
        if step.repeat_of is not None:
            continue
        for comparison in comparisons:
            plane, eye = comparison.plane.value, comparison.eye
            sides = [floors.get((label_mode(name), plane, eye))
                     for name in (step.label, comparison.against)]
            known = [side for side in sides if side is not None]
            floor = max(known) if known else None
            allowance = limits.allowance(floor or 0, comparison.pixels)
            if limits.within_thresholds(comparison):
                verdict = Verdict.PASS
            elif floor is not None and comparison.differing <= allowance:
                verdict = Verdict.NOISE
            else:
                verdict = Verdict.FAIL
            key = (scene, step.label, comparison.against, plane, eye)
            previous = None if baseline is None else baseline.get(key)
            if baseline is not None:
                if previous is not None and comparison.differing > previous + allowance:
                    verdict = Verdict.REGRESS
                elif verdict == Verdict.FAIL:
                    # Against a baseline, only a regression fails; a standing difference is noted.
                    verdict = Verdict.NOISE if previous is not None else verdict
            result.append(Row(scene, step.label, comparison.against, plane, eye, comparison.pixels,
                              comparison.differing, comparison.mean, comparison.largest,
                              comparison.bbox, floor, verdict, previous, comparison.dump,
                              comparison.changed))
    return result, floors


def baseline_counts(report: dict) -> dict[tuple, int]:
    """The counts of a previous fast report's rows, for judging regressions."""
    counts = {}
    for scene in report.get("scenes", []):
        for row in scene.get("rows", []):
            counts[(row["scene"], row["test"], row["against"], row["plane"], row["eye"])] = row["differing"]
    return counts


def failed(rows_: list[Row]) -> bool:
    return any(row.verdict in (Verdict.FAIL, Verdict.REGRESS) for row in rows_)


def table(rows_: list[Row], planes: tuple[str, ...]) -> str:
    """A compact text table: one line per scene, test, reference, and eye, with each plane's count
    and its noise floor."""
    groups: dict[tuple, dict[str, Row]] = {}
    for row in rows_:
        groups.setdefault((row.scene, row.test, row.against, row.eye), {})[row.plane] = row
    header = ["scene", "test", "against", "eye", *planes, "verdict"]
    lines = [header]
    for (scene, test, against, eye), by_plane in groups.items():
        cells = []
        for plane in planes:
            row = by_plane.get(plane)
            if row is None:
                cells.append("-")
                continue
            floor = "?" if row.floor is None else str(row.floor)
            mark = {Verdict.FAIL: "!", Verdict.REGRESS: "!!"}.get(row.verdict, "")
            cells.append(f"{row.differing}/{floor}{mark}")
        worst = max((row.verdict for row in by_plane.values()), key=_severity)
        lines.append([scene, test, against.removesuffix("_a"), str(eye), *cells, worst.value])
    widths = [max(len(line[index]) for line in lines) for index in range(len(header))]
    return "\n".join("  ".join(cell.ljust(width) for cell, width in zip(line, widths)).rstrip()
                     for line in lines)


def _severity(verdict: Verdict) -> int:
    return [Verdict.PASS, Verdict.NOISE, Verdict.FAIL, Verdict.REGRESS].index(verdict)
