"""FSR variants: extra configurations of the fast comparison that render the same scene, in the same
schedule, at the same clock and pose, with FSR's own settings changed.

An experiment that lists `[[fsr_variants]]` renders, after its configured modes, each variant in
each of its modes: FSR on or off (off is the anti-aliasing the game falls back to, as the capture
profile leaves it), the hybrid decode on or off, and optionally another render resolution scale (a
supersampled reference, or a reduced size). Every configuration resets FSR's history and warms up
for the experiment's `warmup_frames`, so each variant accumulates from the same number of frames.
Each variant takes two snapshots, the second compared with the first: at a still camera it is the
next frame, so the comparison is the frame-to-frame change (the shimmer); under a motion it is the
same phase a cycle later.

Only the fast comparison runs FSR variants (`tools/rendering/fast/`); see
docs/mod/testing/fast-comparison.md.
"""

import dataclasses
from dataclasses import dataclass

from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.protocol import (
    RESOLUTION_SCALE_RANGE,
    ProtocolError,
    VisualOverrides,
    valid_request_id,
)

MAX_FSR_VARIANT_NAME_LENGTH = 16


class FsrVariantError(ValueError):
    pass


@dataclass(frozen=True)
class FsrVariant:
    name: str
    # FSR in place of the engine's anti-aliasing, at its shipped settings.
    fsr: bool = True
    # The hybrid decode, while FSR runs.
    hybrid_velocity: bool = True
    # The render resolution scale (each side), when not the session's.
    resolution_scale: float | None = None
    # The modes the variant renders in; empty for every configured mode.
    modes: tuple[CompareMode, ...] = ()

    def __post_init__(self):
        if (not isinstance(self.name, str) or not valid_request_id(self.name)
                or len(self.name) > MAX_FSR_VARIANT_NAME_LENGTH):
            raise FsrVariantError(f"FSR variant: name {self.name!r} must be a request ID of at most "
                                  f"{MAX_FSR_VARIANT_NAME_LENGTH} characters")
        if type(self.fsr) is not bool or type(self.hybrid_velocity) is not bool:
            raise FsrVariantError(f"FSR variant: {self.name}.fsr and .hybrid_velocity must be booleans")
        if not self.fsr and not self.hybrid_velocity:
            raise FsrVariantError(f"FSR variant: {self.name} turns the hybrid decode off without FSR")
        if self.resolution_scale is not None:
            low, high = RESOLUTION_SCALE_RANGE
            if type(self.resolution_scale) not in (int, float) or not low <= self.resolution_scale <= high:
                raise FsrVariantError(f"FSR variant: {self.name}.resolution_scale must be within "
                                      f"[{low}, {high}]")
        try:
            object.__setattr__(self, "modes", tuple(CompareMode(mode) for mode in self.modes))
        except ValueError as error:
            raise FsrVariantError(f"FSR variant: {self.name}.modes: {error}") from error
        if CompareMode.REVERSE_MULTIPASS in self.modes or len(set(self.modes)) != len(self.modes):
            raise FsrVariantError(f"FSR variant: {self.name}.modes must be distinct and not "
                                  "reverse_multipass")

    def overrides(self, base: VisualOverrides | None) -> VisualOverrides | None:
        """The experiment's own overrides with this variant's FSR settings and scale in their place."""
        changes = {
            "fsr": "enabled" if self.fsr else None,
            "fsr_hybrid_velocity": None if self.hybrid_velocity else "disabled",
            "resolution_scale": self.resolution_scale,
        }
        if base is None:
            if not any(value is not None for value in changes.values()):
                return None
            return VisualOverrides(**changes)
        try:
            return dataclasses.replace(base, **changes)
        except ProtocolError:
            # The base named nothing but FSR's settings, which the variant takes back out.
            return None

    def modes_in(self, configured: tuple[CompareMode, ...]) -> tuple[CompareMode, ...]:
        """The modes this variant renders, in the experiment's order."""
        if not self.modes:
            return configured
        missing = [mode.value for mode in self.modes if mode not in configured]
        if missing:
            raise FsrVariantError(f"FSR variant: {self.name} names unconfigured modes {missing}")
        return tuple(mode for mode in configured if mode in self.modes)

    @classmethod
    def from_toml(cls, value, index):
        context = f"fsr_variants[{index}]"
        if not isinstance(value, dict):
            raise FsrVariantError(f"FSR variant: {context} must be a table")
        allowed = {"name", "fsr", "hybrid_velocity", "resolution_scale", "modes"}
        extra = set(value) - allowed
        if "name" not in value or extra:
            raise FsrVariantError(f"FSR variant: {context} needs a name and accepts only "
                                  f"{sorted(allowed)}, not {sorted(extra)}")
        modes = value.get("modes", [])
        if not isinstance(modes, list):
            raise FsrVariantError(f"FSR variant: {context}.modes must be an array")
        return cls(value["name"], value.get("fsr", True), value.get("hybrid_velocity", True),
                   value.get("resolution_scale"), tuple(modes))


def parse_fsr_variants(value) -> tuple[FsrVariant, ...]:
    if not isinstance(value, list) or not value:
        raise FsrVariantError("FSR variant: fsr_variants must be a nonempty array")
    variants = tuple(FsrVariant.from_toml(item, index) for index, item in enumerate(value))
    names = [variant.name for variant in variants]
    if len(set(names)) != len(names):
        raise FsrVariantError("FSR variant: variant names must be unique")
    return variants
