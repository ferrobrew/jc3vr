"""Typed values for the harness's `snapshot` and `snapshot_release` requests: GPU snapshots of a
frame that the payload compares on the GPU against snapshots kept earlier in the schedule
(`payload/src/screenshot/harness/snapshot/`)."""

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

# Mirrors `MAX_COMPARED` in `payload/src/screenshot/harness/snapshot/request.rs`.
MAX_COMPARED = 8


class Plane(StrEnum):
    FINAL = "final"
    DEPTH = "depth"
    ALBEDO = "albedo"
    NORMALS = "normals"
    VELOCITY = "velocity"


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
                   f"fail_fraction = {json.dumps(float(self.fail_fraction))}\n"
                   f"fail_mean = {json.dumps(float(self.fail_mean))}\n"
                   f"dump = {json.dumps(self.dump.value)}\n"
                   f"dump_crop = {json.dumps(self.dump_crop)}\n")
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
        if differing > pixels or sum(heat) != differing:
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
                   failed, pixels, differing, numbers[1], numbers[2],
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
                   error, integer(timing, "frames_waited", minimum=0), times[0], times[1], visual)


@dataclass(frozen=True)
class ReleaseAck:
    id: str
    released: int
    outcome: AckOutcome = field(default=AckOutcome.RELEASED, init=False)

    @classmethod
    def from_wire(cls, value) -> "ReleaseAck":
        return cls(text(value, "id"), integer(value, "released", minimum=0))
