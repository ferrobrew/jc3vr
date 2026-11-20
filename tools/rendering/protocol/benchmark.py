"""Typed values for the harness's `benchmark` request: record the live game's per-frame timings in
one render mode (`payload/src/testing/benchmark/`)."""

import json
import math
from dataclasses import dataclass, field
from enum import StrEnum

from tools.rendering.protocol.base import (
    AckOutcome,
    Command,
    ProtocolError,
    object_value,
    text,
    valid_request_id,
)

# Mirrors `MAX_WARMUP_SECONDS` and `MAX_RECORD_SECONDS` in `payload/src/testing/benchmark/request.rs`.
MAX_WARMUP_SECONDS = 60.0
MAX_RECORD_SECONDS = 120.0


class BenchmarkMode(StrEnum):
    MULTIPASS = "multipass"
    SINGLE_PASS = "single_pass"


@dataclass(frozen=True)
class CameraPath:
    """A sinusoidal swing of the held camera's yaw and a dolly along its heading."""
    yaw_deg: float = 0.0
    forward_m: float = 0.0
    period_seconds: float = 10.0

    def __post_init__(self):
        for name in ("yaw_deg", "forward_m", "period_seconds"):
            value = getattr(self, name)
            if type(value) not in (int, float) or not math.isfinite(value):
                raise ProtocolError(f"benchmark: camera {name} must be a finite number")
        if abs(self.yaw_deg) > 90 or abs(self.forward_m) > 100 or not 0 < self.period_seconds <= 120:
            raise ProtocolError("benchmark: camera path out of range")

    @property
    def still(self) -> bool:
        return self.yaw_deg == 0 and self.forward_m == 0

    def to_wire(self) -> dict:
        return {"yaw_deg": float(self.yaw_deg), "forward_m": float(self.forward_m),
                "period_seconds": float(self.period_seconds)}


@dataclass(frozen=True)
class BenchmarkRequest:
    id: str
    label: str
    mode: BenchmarkMode
    record_seconds: float
    warmup_seconds: float = 3.0
    resolution_scale: float | None = None
    gpu: bool = True
    detour_timing: bool = False
    install_draw_detours: bool = True
    recapture_camera: bool = False
    restore: bool = True
    switches: tuple[tuple[str, bool], ...] = ()
    # `vr.overlap_render_update` for the run; None keeps the session's.
    overlap_render_update: bool | None = None
    # FSR and its hybrid decode for the run; None runs them as shipped.
    fsr: bool | None = None
    fsr_hybrid_velocity: bool | None = None
    camera: CameraPath = field(default_factory=CameraPath)
    command: Command = field(default=Command.BENCHMARK, init=False)
    # The channel validates these on configuration acknowledgements only.
    pose = None
    motion = None

    def __post_init__(self):
        if not valid_request_id(self.id) or not valid_request_id(self.label):
            raise ProtocolError("benchmark: invalid request ID or label")
        object.__setattr__(self, "mode", BenchmarkMode(self.mode))
        object.__setattr__(self, "switches", tuple(sorted(dict(self.switches).items())))
        if not 0 <= self.warmup_seconds <= MAX_WARMUP_SECONDS:
            raise ProtocolError(f"benchmark: warmup_seconds must be within [0, {MAX_WARMUP_SECONDS}]")
        if not 0 < self.record_seconds <= MAX_RECORD_SECONDS:
            raise ProtocolError(f"benchmark: record_seconds must be within (0, {MAX_RECORD_SECONDS}]")
        if self.resolution_scale is not None and not 0.1 <= self.resolution_scale <= 4.0:
            raise ProtocolError("benchmark: resolution_scale must be within [0.1, 4.0]")
        for name in ("overlap_render_update", "fsr", "fsr_hybrid_velocity"):
            if getattr(self, name) is not None and type(getattr(self, name)) is not bool:
                raise ProtocolError(f"benchmark: {name} must be a boolean")
        if self.switches and self.mode != BenchmarkMode.SINGLE_PASS:
            raise ProtocolError("benchmark: single_pass switches apply only to the single_pass mode")

    def to_toml(self) -> str:
        lines = [f"command = {json.dumps(self.command.value)}", f"id = {json.dumps(self.id)}", "",
                 "[benchmark]",
                 f"label = {json.dumps(self.label)}",
                 f"mode = {json.dumps(self.mode.value)}",
                 f"warmup_seconds = {json.dumps(float(self.warmup_seconds))}",
                 f"record_seconds = {json.dumps(float(self.record_seconds))}",
                 f"gpu = {json.dumps(self.gpu)}",
                 f"detour_timing = {json.dumps(self.detour_timing)}",
                 f"install_draw_detours = {json.dumps(self.install_draw_detours)}",
                 f"recapture_camera = {json.dumps(self.recapture_camera)}",
                 f"restore = {json.dumps(self.restore)}"]
        if self.resolution_scale is not None:
            lines.append(f"resolution_scale = {json.dumps(float(self.resolution_scale))}")
        for name in ("overlap_render_update", "fsr", "fsr_hybrid_velocity"):
            if getattr(self, name) is not None:
                lines.append(f"{name} = {json.dumps(getattr(self, name))}")
        if self.switches:
            lines += ["", "[benchmark.single_pass]"]
            lines += [f"{name} = {json.dumps(value)}" for name, value in self.switches]
        if not self.camera.still:
            lines += ["", "[benchmark.camera]"]
            lines += [f"{key} = {json.dumps(value)}" for key, value in self.camera.to_wire().items()]
        return "\n".join(lines) + "\n"


@dataclass(frozen=True)
class BenchmarkAck:
    """A completed run's acknowledgement, kept as the payload wrote it once its shape is checked."""
    id: str
    label: str
    value: dict
    outcome: AckOutcome = field(default=AckOutcome.COMPLETED, init=False)

    @classmethod
    def from_wire(cls, value) -> "BenchmarkAck":
        object_value(value)
        validate_ack(value)
        return cls(text(value, "id"), text(value, "label"), value)


def validate_ack(value: dict) -> None:
    """Check a completed acknowledgement's tables: every frame row and GPU row has its columns' width
    and holds integers."""
    columns, frames = value.get("columns"), value.get("frames")
    if (not isinstance(columns, list) or not all(isinstance(c, str) for c in columns)
            or len(set(columns)) != len(columns) or "frame_us" not in columns):
        raise ProtocolError("benchmark: invalid frame columns")
    if not isinstance(frames, list) or any(
            not isinstance(row, list) or len(row) != len(columns)
            or any(type(item) is not int or item < 0 for item in row) for row in frames):
        raise ProtocolError("benchmark: invalid frame rows")
    for key in ("environment", "timing", "request"):
        if not isinstance(value.get(key), dict):
            raise ProtocolError(f"benchmark: missing {key}")
    gpu = value.get("gpu")
    if gpu is not None:
        gpu_columns, dispatches = gpu.get("columns"), gpu.get("dispatches")
        if not isinstance(gpu_columns, list) or not isinstance(dispatches, list) or any(
                not isinstance(row, list) or len(row) != len(gpu_columns)
                or any(type(item) is not int for item in row) for row in dispatches):
            raise ProtocolError("benchmark: invalid GPU rows")
