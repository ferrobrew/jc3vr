"""Single-pass variants: extra single-pass captures with named production switches overridden.

An experiment that lists `[[single_pass_variants]]` captures, after its configured modes, the
single-pass mode once more per variant with that variant's `single_pass.*` switches forced, in the
same schedule and so from the same scene state as the default single-pass capture and the isolated
eyes. Each variant becomes its own manifest case that borrows the default case's isolated-eye and
multipass samples, so the comparator measures it against the same references, and the report
states, per variant and eye, how many final-image pixels differ from the isolated eye.

A variant with no switches captures the default configuration again: a same-schedule repeat that
gives the other variants a noise floor.
"""

import dataclasses
import statistics
from dataclasses import dataclass
from itertools import product
from pathlib import Path

import numpy as np

from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import SampleSpec, Thresholds, load_sample
from tools.rendering.protocol import (
    Case,
    Mode,
    ProtocolError,
    VisualOverrides,
    single_pass_switches,
    valid_request_id,
)

DEFAULT_VARIANT = "default"
MAX_VARIANT_NAME_LENGTH = 16
REFERENCES = ((Mode.LEFT_ONLY, 0), (Mode.RIGHT_ONLY, 1))


class VariantError(ValueError):
    pass


@dataclass(frozen=True)
class SinglePassVariant:
    name: str
    switches: tuple[tuple[str, bool], ...] = ()
    diagnostics: bool = False

    def __post_init__(self):
        if (not isinstance(self.name, str) or not valid_request_id(self.name) or
                len(self.name) > MAX_VARIANT_NAME_LENGTH or self.name == DEFAULT_VARIANT):
            raise VariantError(
                f"single-pass variant: name {self.name!r} must be a request ID of at most "
                f"{MAX_VARIANT_NAME_LENGTH} characters other than {DEFAULT_VARIANT!r}")
        try:
            object.__setattr__(self, "switches",
                               single_pass_switches(dict(self.switches),
                                                    f"single_pass_variants.{self.name}.switches"))
        except ProtocolError as error:
            raise VariantError(f"single-pass variant: {error}") from error
        if type(self.diagnostics) is not bool:
            raise VariantError(f"single-pass variant: {self.name}.diagnostics must be a boolean")

    def overrides(self, base: VisualOverrides | None) -> VisualOverrides | None:
        """The experiment's own overrides with this variant's switches forced on top."""
        if not self.switches:
            return base
        if base is None:
            return VisualOverrides(single_pass=self.switches)
        return dataclasses.replace(base, single_pass=tuple({**base.switches, **dict(self.switches)}.items()))

    @classmethod
    def from_toml(cls, value, index):
        context = f"single_pass_variants[{index}]"
        if not isinstance(value, dict):
            raise VariantError(f"single-pass variant: {context} must be a table")
        extra = set(value) - {"name", "switches", "diagnostics"}
        if "name" not in value or extra:
            raise VariantError(f"single-pass variant: {context} needs a name and accepts only "
                               f"name, switches, and diagnostics, not {sorted(extra)}")
        switches = value.get("switches", {})
        if not isinstance(switches, dict):
            raise VariantError(f"single-pass variant: {context}.switches must be a table")
        return cls(value["name"], tuple(switches.items()), value.get("diagnostics", False))


def parse_variants(value) -> tuple[SinglePassVariant, ...]:
    if not isinstance(value, list) or not value:
        raise VariantError("single-pass variant: single_pass_variants must be a nonempty array")
    variants = tuple(SinglePassVariant.from_toml(item, index) for index, item in enumerate(value))
    names = [variant.name for variant in variants]
    if len(set(names)) != len(names):
        raise VariantError("single-pass variant: variant names must be unique")
    return variants


def require_reference_modes(configurations) -> None:
    """A variant case borrows every reference the comparator needs, so all four must be captured."""
    required = (CompareMode.LEFT_ONLY, CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS,
                CompareMode.SINGLE_PASS)
    missing = [mode.value for mode in required if mode not in configurations]
    if missing:
        raise VariantError(f"single-pass variant: the configurations must include {missing}")


def variant_case_name(experiment: str, variant: SinglePassVariant) -> str:
    return f"{experiment}-variant-{variant.name}"


def borrow_references(default: Case, variant: Case) -> None:
    """Point a variant case at the default case's isolated-eye and multipass samples."""
    for mode in (Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS):
        variant.modes[mode] = list(default.modes.get(mode, []))


def verify_switches(identifier: str, metadata: dict, overrides: VisualOverrides | None) -> None:
    """Check that a single-pass capture ran with the switches its configuration requested.

    The effective configuration must hold every requested value, and the shaders must have been
    created under the effective configuration: every creation-time switch in the capture's shader
    build agrees with it, so a reload that did not land (or landed under other switches) fails here.
    """
    try:
        effective = metadata["effective_config"]["stereo"]["single_pass"]
        build = metadata["visual"]["shader_build"]
    except (KeyError, TypeError) as error:
        raise VariantError(f"single-pass variant: {identifier} has no switch evidence") from error
    if not isinstance(effective, dict) or not isinstance(build, dict):
        raise VariantError(f"single-pass variant: {identifier} has no switch evidence")
    requested = overrides.switches if overrides is not None else {}
    wrong = sorted(name for name, value in requested.items() if effective.get(name) is not value)
    if wrong:
        raise VariantError(f"single-pass variant: {identifier} ran without the requested {wrong}")
    if build.get("active") is not True or build.get("collapse") is not True:
        raise VariantError(f"single-pass variant: {identifier}'s shaders were not built for single-pass")
    switches = build.get("switches")
    if not isinstance(switches, dict) or not switches:
        raise VariantError(f"single-pass variant: {identifier} has no shader build switches")
    stale = sorted(name for name, value in switches.items() if effective.get(name) is not value)
    if stale:
        raise VariantError(
            f"single-pass variant: {identifier}'s shaders were built with other values of {stale}")


def differing_pixels(a: np.ndarray, b: np.ndarray, threshold: float) -> int:
    error = np.abs(a.astype(np.int16) - b.astype(np.int16)).max(axis=2)
    return int(np.count_nonzero(error > threshold))


def summarize(root: Path, cases: dict[str, Case], switches: dict[str, dict[str, bool]],
              threshold: float = Thresholds().pixel) -> tuple[dict, ...]:
    """Per variant and eye: the final-image pixels that differ from the isolated eye.

    Every pairing of an isolated-eye sample with a single-pass sample is counted; the median is the
    headline and the range shows the spread.
    """
    root = Path(root)
    loaded = {}

    def samples(case: Case, mode: Mode):
        key = id(case), mode
        if key not in loaded:
            loaded[key] = [load_sample(root, CompareMode(mode.value), index,
                                       SampleSpec(sample.png, sample.metadata))
                           for index, sample in enumerate(case.modes.get(mode, []))]
        return loaded[key]

    results = []
    for name, case in cases.items():
        eyes = []
        for reference, eye in REFERENCES:
            counts = [differing_pixels(isolated.image(eye), packed.image(eye), threshold)
                      for isolated, packed in product(samples(case, reference),
                                                      samples(case, Mode.SINGLE_PASS))]
            if not counts:
                raise VariantError(f"single-pass variant: {name} has no eye {eye} pairs")
            image = samples(case, reference)[0].image(eye)
            eyes.append({"eye": eye, "pixels": int(image.shape[0] * image.shape[1]),
                         "median": statistics.median(counts), "min": min(counts),
                         "max": max(counts), "pairs": counts})
        results.append({"variant": name, "case": case.name, "switches": switches[name],
                        "eyes": eyes})
    return tuple(results)


def describe(summary: dict) -> str:
    switches = ", ".join(f"{name}={'on' if value else 'off'}"
                         for name, value in summary["switches"].items()) or "defaults"
    left, right = summary["eyes"]
    return (f"single-pass {summary['variant']} ({switches}): {left['median']:g}/{right['median']:g} "
            f"px differ from the isolated eyes (left/right median over {len(left['pairs'])} pairs; "
            f"ranges {left['min']}-{left['max']} and {right['min']}-{right['max']} "
            f"of {left['pixels']} px)")
