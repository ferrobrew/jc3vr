"""Typed values for the harness's `snapshot` and `snapshot_release` requests: GPU snapshots of a
frame that the payload compares on the GPU against snapshots kept earlier in the schedule
(`crates/jc3vr-render/src/screenshot/harness/snapshot/`)."""

import json
import math
from dataclasses import dataclass, field
from enum import StrEnum

from tools.rendering.protocol.base import (
    AckOutcome,
    Command,
    ProtocolError,
    integer,
    object_value,
    text,
    valid_request_id,
)

# Mirrors `MAX_COMPARED` and `MAX_DEPTH_ULPS` in `crates/jc3vr-render/src/screenshot/harness/snapshot/request.rs`.
MAX_COMPARED = 8
MAX_DEPTH_ULPS = 512


class Plane(StrEnum):
    FINAL = "final"
    DEPTH = "depth"
    ALBEDO = "albedo"
    NORMALS = "normals"
    VELOCITY = "velocity"
    POST_RESOLVE = "post_resolve"
    PRE_ATMOSPHERE = "pre_atmosphere"
    POST_ATMOSPHERE = "post_atmosphere"
    POST_SUN = "post_sun"
    MAIN_COLOR = "main_color"
    PRE_AA = "pre_aa"
    POST_AA = "post_aa"
    SSAO = "ssao"
    SSAO_VOLUMES = "ssao_volumes"
    SSAO_STEP0 = "ssao_step0"
    SSAO_STEP1 = "ssao_step1"
    SSAO_STEP2 = "ssao_step2"
    SSAO_STEP3 = "ssao_step3"
    SSAO_STEP4 = "ssao_step4"
    SSAO_STEP5 = "ssao_step5"
    SSAO_STEP6 = "ssao_step6"
    SSAO_STEP7 = "ssao_step7"
    # The motion vectors and the depth FSR is handed for each eye, taken at the velocity decode.
    FSR_MOTION = "fsr_motion"
    FSR_DEPTH = "fsr_depth"


class DumpPolicy(StrEnum):
    NEVER = "never"
    ON_FAILURE = "on_failure"
    ALWAYS = "always"


class Kind(StrEnum):
    UNORM8 = "unorm8"
    UINT4 = "uint4"
    FLOAT3 = "float3"
    DEPTH = "depth"


@dataclass(frozen=True)
class SnapshotRequest:
    id: str
    label: str
    planes: tuple[Plane, ...]
    keep: bool = False
    compare: tuple[str, ...] = ()
    phase: int | None = None
    pixel_threshold: float = 3.0
    fail_fraction: float = 0.001
    fail_mean: float = 0.25
    dump: DumpPolicy = DumpPolicy.NEVER
    dump_crop: bool = True
    # How many representable floats apart two depths may be and still match; zero counts any change.
    depth_ulps: int = 0
    # Write every plane whole, per eye, whether or not it is compared.
    save: bool = False
    # At a still camera, wait for a frame whose FSR jitter is at this index of its sequence, while
    # the jitter runs, so every configuration's snapshot takes the same sub-pixel offset.
    jitter_phase: int | None = None
    command: Command = field(default=Command.SNAPSHOT, init=False)
    # The channel validates these on configuration acknowledgements only.
    pose = None
    motion = None

    def __post_init__(self):
        if not valid_request_id(self.id) or not valid_request_id(self.label):
            raise ProtocolError("snapshot: invalid request ID or label")
        object.__setattr__(self, "planes", tuple(Plane(plane) for plane in self.planes))
        object.__setattr__(self, "compare", tuple(self.compare))
        object.__setattr__(self, "dump", DumpPolicy(self.dump))
        if Plane.FINAL not in self.planes or len(set(self.planes)) != len(self.planes):
            raise ProtocolError("snapshot: planes must include the final image once, and no plane twice")
        if len(self.compare) > MAX_COMPARED or not all(valid_request_id(label) for label in self.compare):
            raise ProtocolError(f"snapshot: at most {MAX_COMPARED} valid labels to compare with")
        if self.phase is not None and (type(self.phase) is not int or self.phase < 0):
            raise ProtocolError("snapshot: phase must be a non-negative integer")
        if not 0 <= self.pixel_threshold <= 255 or not 0 <= self.fail_fraction <= 1 or not (
                math.isfinite(self.fail_mean) and self.fail_mean >= 0):
            raise ProtocolError("snapshot: thresholds out of range")
        if type(self.depth_ulps) is not int or not 0 <= self.depth_ulps <= MAX_DEPTH_ULPS:
            raise ProtocolError(f"snapshot: depth_ulps must be an integer within [0, {MAX_DEPTH_ULPS}]")
        if type(self.save) is not bool:
            raise ProtocolError("snapshot: save must be a boolean")
        if self.jitter_phase is not None and (type(self.jitter_phase) is not int
                                              or self.jitter_phase < 0):
            raise ProtocolError("snapshot: jitter_phase must be a non-negative integer")

    def to_toml(self) -> str:
        result = (f"command = {json.dumps(self.command.value)}\n"
                  f"id = {json.dumps(self.id)}\n")
        if self.phase is not None:
            result += f"\n[motion_capture]\nphase = {self.phase}\n"
        result += ("\n[snapshot]\n"
                   f"label = {json.dumps(self.label)}\n"
                   f"planes = {json.dumps([plane.value for plane in self.planes])}\n"
                   f"keep = {json.dumps(self.keep)}\n"
                   f"compare = {json.dumps(list(self.compare))}\n"
                   f"pixel_threshold = {json.dumps(float(self.pixel_threshold))}\n"
                   f"depth_ulps = {self.depth_ulps}\n"
                   f"fail_fraction = {json.dumps(float(self.fail_fraction))}\n"
                   f"fail_mean = {json.dumps(float(self.fail_mean))}\n"
                   f"dump = {json.dumps(self.dump.value)}\n"
                   f"dump_crop = {json.dumps(self.dump_crop)}\n"
                   f"save = {json.dumps(self.save)}\n")
        if self.jitter_phase is not None:
            result += f"jitter_phase = {self.jitter_phase}\n"
        return result


@dataclass(frozen=True)
class ReleaseRequest:
    id: str
    command: Command = field(default=Command.SNAPSHOT_RELEASE, init=False)
    pose = None
    motion = None

    def __post_init__(self):
        if not valid_request_id(self.id):
            raise ProtocolError("snapshot: invalid request ID")

    def to_toml(self) -> str:
        return f"command = {json.dumps(self.command.value)}\nid = {json.dumps(self.id)}\n"


@dataclass(frozen=True)
class Comparison:
    """One plane of one eye compared on the GPU against a kept snapshot."""
    against: str
    plane: Plane
    eye: int
    kind: Kind
    threshold: float
    failed: bool
    pixels: int
    differing: int
    # The pixels whose stored values change at all: `differing` for an exact rule, and for a depth
    # compared with a tolerance also the changes within it.
    changed: int
    mean: float
    largest: float
    bbox: tuple[int, int, int, int] | None
    grid: tuple[int, int]
    heat: tuple[int, ...]
    dump: dict | None = None

    @property
    def fraction(self) -> float:
        return self.differing / self.pixels if self.pixels else 0.0

    @classmethod
    def from_wire(cls, value) -> "Comparison":
        object_value(value)
        grid = value.get("grid")
        heat = value.get("heat")
        bbox = value.get("bbox")
        if (not isinstance(grid, list) or len(grid) != 2 or any(type(v) is not int or v <= 0 for v in grid)
                or not isinstance(heat, list) or len(heat) != grid[0] * grid[1]
                or any(type(v) is not int or v < 0 for v in heat)):
            raise ProtocolError("snapshot: invalid comparison heat map")
        if bbox is not None and (not isinstance(bbox, list) or len(bbox) != 4 or
                                 any(type(v) is not int or v < 0 for v in bbox)):
            raise ProtocolError("snapshot: invalid comparison bounding box")
        pixels = integer(value, "pixels", minimum=0)
        differing = integer(value, "differing", minimum=0)
        changed = integer(value, "changed", minimum=0)
        if differing > pixels or sum(heat) != differing or changed > pixels:
            raise ProtocolError("snapshot: comparison counts disagree")
        numbers = []
        for key in ("threshold", "mean", "largest"):
            number = value.get(key)
            if type(number) not in (int, float) or not math.isfinite(number) or number < 0:
                raise ProtocolError(f"snapshot: invalid comparison {key}")
            numbers.append(float(number))
        failed = value.get("failed")
        if type(failed) is not bool:
            raise ProtocolError("snapshot: invalid comparison failure flag")
        dump = value.get("dump")
        if dump is not None and not isinstance(dump, dict):
            raise ProtocolError("snapshot: invalid comparison dump")
        try:
            plane, kind = Plane(text(value, "plane")), Kind(text(value, "kind"))
        except ValueError as error:
            raise ProtocolError("snapshot: unknown comparison plane or kind") from error
        return cls(text(value, "against"), plane, integer(value, "eye", minimum=0), kind, numbers[0],
                   failed, pixels, differing, changed, numbers[1], numbers[2],
                   None if bbox is None else tuple(bbox), tuple(grid), tuple(heat), dump)


@dataclass(frozen=True)
class SnapshotAck:
    id: str
    label: str
    mode: str
    frame: int
    eye_width: int
    eye_height: int
    planes: tuple[tuple[Plane, int], ...]
    comparisons: tuple[Comparison, ...]
    plane_errors: tuple[str, ...]
    error: str | None
    frames_waited: int
    wait_ms: float
    gpu_ms: float
    visual: dict
    # The planes written whole (`stem`, `file`, `encoding`, `width`, `height`, `format`,
    # `pixel_bytes`, `error`), when the request asked for them.
    saved: tuple[dict, ...] = ()
    # What FSR resolved each eye with on the snapshot's frame (`fsr::DispatchRecord`).
    fsr: tuple[dict, ...] = ()
    outcome: AckOutcome = field(default=AckOutcome.COMPLETED, init=False)

    @classmethod
    def from_wire(cls, value) -> "SnapshotAck":
        object_value(value)
        planes = value.get("planes")
        comparisons = value.get("comparisons")
        plane_errors = value.get("plane_errors")
        timing = object_value(value.get("timing"))
        visual = value.get("visual")
        if not isinstance(planes, list) or not isinstance(comparisons, list) or not isinstance(
                plane_errors, list) or not isinstance(visual, dict):
            raise ProtocolError("snapshot: invalid acknowledgement")
        try:
            parsed_planes = tuple((Plane(text(plane, "plane")), integer(plane, "eye", minimum=0))
                                  for plane in planes)
        except ValueError as error:
            raise ProtocolError("snapshot: unknown plane") from error
        error = value.get("error")
        if error is not None and not isinstance(error, str):
            raise ProtocolError("snapshot: invalid error")
        saved = value.get("saved", [])
        fsr = value.get("fsr", [])
        if (not isinstance(saved, list) or not all(isinstance(item, dict) for item in saved)
                or not isinstance(fsr, list) or not all(isinstance(item, dict) for item in fsr)):
            raise ProtocolError("snapshot: invalid saved planes or FSR records")
        times = []
        for key in ("wait_ms", "gpu_ms"):
            number = timing.get(key)
            if type(number) not in (int, float) or not math.isfinite(number):
                raise ProtocolError(f"snapshot: invalid timing {key}")
            times.append(float(number))
        return cls(text(value, "id"), text(value, "label"), text(value, "mode"),
                   integer(value, "frame"), integer(value, "eye_width", minimum=1),
                   integer(value, "eye_height", minimum=1), parsed_planes,
                   tuple(Comparison.from_wire(item) for item in comparisons),
                   tuple(f"{text(item, 'plane')}: {text(item, 'error')}" for item in plane_errors),
                   error, integer(timing, "frames_waited", minimum=0), times[0], times[1], visual,
                   tuple(saved), tuple(fsr))


@dataclass(frozen=True)
class ReleaseAck:
    id: str
    released: int
    outcome: AckOutcome = field(default=AckOutcome.RELEASED, init=False)

    @classmethod
    def from_wire(cls, value) -> "ReleaseAck":
        return cls(text(value, "id"), integer(value, "released", minimum=0))
