"""FSR variants: extra configurations of the fast comparison that render the same scene, in the same
schedule, at the same clock and pose, with FSR's own settings changed.

An experiment that lists `[[fsr_variants]]` renders, after its configured modes, each variant in
each of its modes: FSR on or off (off is the anti-aliasing the game falls back to, as the capture
profile leaves it, or none at all with `antialiasing = false`), the mod's own temporal resolve in
FSR's place (`taa = true`), the hybrid decode and the sub-pixel
jitter on or off, and optionally another render resolution scale (a supersampled reference, or a
reduced size). Every configuration resets FSR's history and warms up
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
    # The mod's own temporal resolve in place of FSR, with FSR's inputs (the jitter and the hybrid
    # decode follow `jitter` and `hybrid_velocity`).
    taa: bool = False
    # The hybrid decode, while FSR runs.
    hybrid_velocity: bool = True
    # The sub-pixel camera jitter, while FSR runs.
    jitter: bool = True
    # The game's own anti-aliasing, while FSR does not run; off leaves the image with none.
    antialiasing: bool = True
    # FSR's sharpening pass as shipped (off unless the shipped settings turn it on), or off; or the
    # pass turned on at a strength.
    sharpen: bool = True
    sharpness: float | None = None
    # The temporal resolve's history share and clip box at rest, when not the shipped ones.
    history_weight: float | None = None
    clip_gamma_rest: float | None = None
    # The unjitter kernel's falloff, in tenths (0.8 is a falloff of 8).
    current_sharpness: float | None = None
    # The history share in motion.
    motion_history_weight: float | None = None
    # The render resolution scale (each side), when not the session's.
    resolution_scale: float | None = None
    # The modes the variant renders in; empty for every configured mode.
    modes: tuple[CompareMode, ...] = ()

    def __post_init__(self):
        if (not isinstance(self.name, str) or not valid_request_id(self.name)
                or len(self.name) > MAX_FSR_VARIANT_NAME_LENGTH):
            raise FsrVariantError(f"FSR variant: name {self.name!r} must be a request ID of at most "
                                  f"{MAX_FSR_VARIANT_NAME_LENGTH} characters")
        if any(type(value) is not bool for value in
               (self.fsr, self.taa, self.hybrid_velocity, self.jitter, self.antialiasing,
                self.sharpen)):
            raise FsrVariantError(f"FSR variant: {self.name}.fsr, .taa, .hybrid_velocity, .jitter, "
                                  ".antialiasing, and .sharpen must be booleans")
        for name in ("history_weight", "clip_gamma_rest", "current_sharpness",
                     "motion_history_weight"):
            value = getattr(self, name)
            if value is not None and (type(value) not in (int, float) or not 0 <= value <= 4):
                raise FsrVariantError(f"FSR variant: {self.name}.{name} must be within [0, 4]")
            if value is not None and not self.taa:
                raise FsrVariantError(f"FSR variant: {self.name}.{name} tunes the temporal "
                                      "resolve, which needs taa = true")
        if self.taa and not self.fsr:
            raise FsrVariantError(f"FSR variant: {self.name} runs the temporal resolve with the "
                                  "temporal anti-aliasing off (fsr = false)")
        if self.taa and not (self.sharpen and self.sharpness is None):
            raise FsrVariantError(f"FSR variant: {self.name} changes FSR's sharpening, which the "
                                  "temporal resolve does not run")
        if self.sharpness is not None and (type(self.sharpness) not in (int, float)
                                           or not 0 <= self.sharpness <= 1):
            raise FsrVariantError(f"FSR variant: {self.name}.sharpness must be within [0, 1]")
        if not self.fsr and not (self.hybrid_velocity and self.jitter and self.sharpen
                                 and self.sharpness is None):
            raise FsrVariantError(f"FSR variant: {self.name} changes FSR's settings without FSR")
        if self.fsr and not self.antialiasing:
            raise FsrVariantError(f"FSR variant: {self.name} turns the game's anti-aliasing off "
                                  "with FSR, which replaces it")
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
            "fsr": "enabled" if self.fsr and not self.taa else None,
            "taa": "enabled" if self.taa else None,
            "fsr_hybrid_velocity": None if self.hybrid_velocity else "disabled",
            "fsr_jitter": None if self.jitter else "disabled",
            "anti_aliasing": None if self.antialiasing else "disabled",
            "fsr_sharpen": None if self.sharpen else "disabled",
            "fsr_sharpness": self.sharpness,
            "resolution_scale": self.resolution_scale,
            "taa_history_weight": self.history_weight,
            "taa_clip_gamma_rest": self.clip_gamma_rest,
            "taa_current_sharpness": self.current_sharpness,
            "taa_motion_history_weight": self.motion_history_weight,
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
        allowed = {"name", "fsr", "taa", "hybrid_velocity", "jitter", "antialiasing", "sharpen",
                   "sharpness", "resolution_scale", "modes", "history_weight",
                   "clip_gamma_rest", "current_sharpness", "motion_history_weight"}
        extra = set(value) - allowed
        if "name" not in value or extra:
            raise FsrVariantError(f"FSR variant: {context} needs a name and accepts only "
                                  f"{sorted(allowed)}, not {sorted(extra)}")
        modes = value.get("modes", [])
        if not isinstance(modes, list):
            raise FsrVariantError(f"FSR variant: {context}.modes must be an array")
        return cls(value["name"], value.get("fsr", True), value.get("taa", False),
                   value.get("hybrid_velocity", True),
                   value.get("jitter", True), value.get("antialiasing", True),
                   value.get("sharpen", True), value.get("sharpness"),
                   history_weight=value.get("history_weight"),
                   clip_gamma_rest=value.get("clip_gamma_rest"),
                   current_sharpness=value.get("current_sharpness"),
                   motion_history_weight=value.get("motion_history_weight"),
                   resolution_scale=value.get("resolution_scale"), modes=tuple(modes))


def parse_fsr_variants(value) -> tuple[FsrVariant, ...]:
    if not isinstance(value, list) or not value:
        raise FsrVariantError("FSR variant: fsr_variants must be a nonempty array")
    variants = tuple(FsrVariant.from_toml(item, index) for index, item in enumerate(value))
    names = [variant.name for variant in variants]
    if len(set(names)) != len(names):
        raise FsrVariantError("FSR variant: variant names must be unique")
    return variants
