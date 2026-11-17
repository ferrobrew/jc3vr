"""Typed motion trajectory values for the visual harness protocol."""

import json
import math
from dataclasses import asdict, dataclass

if __package__:
    from .visual_protocol_base import (
        POSE_TOLERANCE,
        Camera,
        OrientationOffset,
        PoseEvidence,
        PoseRequest,
        ProtocolError,
        cameras_match,
        compose_pose,
        finite_number,
        integer,
        object_value,
        text,
        valid_request_id,
    )
else:
    from visual_protocol_base import (
        POSE_TOLERANCE,
        Camera,
        OrientationOffset,
        PoseEvidence,
        PoseRequest,
        ProtocolError,
        cameras_match,
        compose_pose,
        finite_number,
        integer,
        object_value,
        text,
        valid_request_id,
    )


@dataclass(frozen=True)
class MotionRequest:
    case_id: str
    period_frames: int
    yaw_deg: float
    pitch_deg: float
    roll_deg: float

    def __post_init__(self):
        if not valid_request_id(self.case_id):
            raise ProtocolError("visual protocol: motion case_id must be an ASCII request ID")
        if type(self.period_frames) is not int or not 32 <= self.period_frames <= 600:
            raise ProtocolError("visual protocol: motion period_frames must be an integer from 32 through 600")
        for name in ("yaw_deg", "pitch_deg", "roll_deg"):
            value = getattr(self, name)
            if type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 45:
                raise ProtocolError(f"visual protocol: motion {name} must be finite and within [0, 45]")
            object.__setattr__(self, name, float(value))

    @classmethod
    def from_wire(cls, value):
        return cls(text(value, "case_id"), integer(value, "period_frames"),
                   finite_number(value, "yaw_deg"), finite_number(value, "pitch_deg"),
                   finite_number(value, "roll_deg"))

    def to_wire(self):
        return asdict(self)

    def to_toml(self) -> str:
        return ("[motion]\n"
                f"case_id = {json.dumps(self.case_id)}\n"
                f"period_frames = {self.period_frames}\n"
                f"yaw_deg = {json.dumps(self.yaw_deg)}\n"
                f"pitch_deg = {json.dumps(self.pitch_deg)}\n"
                f"roll_deg = {json.dumps(self.roll_deg)}\n")

    def offset_at(self, phase):
        if type(phase) is not int or not 0 <= phase < self.period_frames:
            raise ProtocolError("visual protocol: motion phase is outside the period")
        theta = 2.0 * math.pi * phase / self.period_frames
        return OrientationOffset(self.yaw_deg * math.sin(theta),
                                 self.pitch_deg * math.sin(2.0 * theta),
                                 self.roll_deg * math.sin(theta))


@dataclass(frozen=True)
class PhaseCapture:
    phase: int

    def __post_init__(self):
        if type(self.phase) is not int or self.phase < 0:
            raise ProtocolError("visual protocol: motion capture phase must be a nonnegative integer")

    def validate(self, period_frames):
        if self.phase >= period_frames:
            raise ProtocolError("visual protocol: motion capture phase is outside the period")

    @classmethod
    def from_wire(cls, value):
        return cls(integer(value, "phase", minimum=0))

    def to_wire(self):
        return {"phase": self.phase}

    def to_toml(self) -> str:
        return f"[motion_capture]\nphase = {self.phase}\n"


@dataclass(frozen=True)
class ExpectedMotion:
    request: MotionRequest
    phase: int

    def __post_init__(self):
        if not isinstance(self.request, MotionRequest):
            raise ProtocolError("visual protocol: expected motion requires a motion request")
        if type(self.phase) is not int or not 0 <= self.phase < self.request.period_frames:
            raise ProtocolError("visual protocol: expected motion phase is outside the period")

    @classmethod
    def from_wire(cls, value):
        return cls(MotionRequest.from_wire(object_value(value).get("request")),
                   integer(value, "phase", minimum=0))

    def to_wire(self):
        return {"request": self.request.to_wire(), "phase": self.phase}


@dataclass(frozen=True)
class MotionHistory:
    phase: int
    rendered_frame: int
    applied: Camera

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        return cls(integer(value, "phase", minimum=0), integer(value, "rendered_frame", minimum=0),
                   Camera.from_wire(value.get("applied")))

    def to_wire(self):
        return {"phase": self.phase, "rendered_frame": self.rendered_frame,
                "applied": {"position": list(self.applied.position),
                            "orientation": list(self.applied.orientation)}}


@dataclass(frozen=True)
class MotionEvidence:
    request: MotionRequest
    phase: int
    cycle: int
    rendered_frame: int
    history: tuple[MotionHistory, ...]

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        request = MotionRequest.from_wire(value.get("request"))
        phase = integer(value, "phase", minimum=0)
        if phase >= request.period_frames:
            raise ProtocolError("visual protocol: motion evidence phase is outside the period")
        history = value.get("history")
        if not isinstance(history, list) or len(history) > 8:
            raise ProtocolError("visual protocol: motion evidence history must contain at most eight frames")
        entries = tuple(MotionHistory.from_wire(entry) for entry in history)
        if any(entry.phase >= request.period_frames for entry in entries):
            raise ProtocolError("visual protocol: motion history phase is outside the period")
        return cls(request, phase, integer(value, "cycle", minimum=0),
                   integer(value, "rendered_frame", minimum=0), entries)

    def validate_request(self, request, phase=None):
        if not isinstance(request, MotionRequest):
            raise ProtocolError("visual protocol: motion evidence requires a motion request")
        if self.request != request:
            raise ProtocolError("visual protocol: motion evidence request differs from its request")
        if phase is not None and self.phase != phase:
            raise ProtocolError("visual protocol: motion evidence phase differs from its request")

    def validate_configuration(self, request, pose, anchor=None, tolerance=POSE_TOLERANCE):
        self.validate_request(request)
        if self.cycle != 0 or self.history:
            raise ProtocolError("visual protocol: configuration motion evidence must have cycle zero and no history")
        if not isinstance(pose, PoseEvidence):
            raise ProtocolError("visual protocol: configuration motion evidence requires pose evidence")
        return pose.validate(PoseRequest(request.case_id, request.offset_at(self.phase)), anchor, tolerance)

    def validate_capture(self, request, phase, anchor, pose, camera, predicted_display_time_ns,
                         tolerance=POSE_TOLERANCE):
        self.validate_request(request, phase)
        if self.cycle < 1:
            raise ProtocolError("visual protocol: capture motion evidence has no completed cycle")
        if self.rendered_frame != predicted_display_time_ns:
            raise ProtocolError("visual protocol: motion rendered_frame differs from the predicted display time")
        if not isinstance(pose, PoseEvidence):
            raise ProtocolError("visual protocol: capture motion evidence requires pose evidence")
        pose.validate(PoseRequest(request.case_id, request.offset_at(phase)), anchor, tolerance)
        pose.validate_applied(camera, tolerance)
        self.validate_history(anchor if anchor is not None else pose.anchor, tolerance)
        return pose.anchor

    def validate_history(self, anchor, tolerance=POSE_TOLERANCE):
        if len(self.history) != 8:
            raise ProtocolError("visual protocol: capture motion history must contain eight frames")
        expected = tuple((self.phase - 8 + index) % self.request.period_frames for index in range(8))
        if tuple(entry.phase for entry in self.history) != expected:
            raise ProtocolError("visual protocol: motion history phases are not contiguous before the capture")
        previous = None
        for entry in self.history:
            if previous is not None and entry.rendered_frame <= previous:
                raise ProtocolError("visual protocol: motion history rendered frames are not strictly increasing")
            previous = entry.rendered_frame
            expected_camera = compose_pose(anchor, self.request.offset_at(entry.phase))
            if not cameras_match(entry.applied, expected_camera, tolerance):
                raise ProtocolError("visual protocol: motion history camera differs from the trajectory and anchor")
        if previous is None or previous >= self.rendered_frame:
            raise ProtocolError("visual protocol: motion history does not end before the capture")

    def to_wire(self):
        return {"request": self.request.to_wire(), "phase": self.phase, "cycle": self.cycle,
                "rendered_frame": self.rendered_frame,
                "history": [entry.to_wire() for entry in self.history]}


__all__ = [
    "ExpectedMotion",
    "MotionEvidence",
    "MotionHistory",
    "MotionRequest",
    "PhaseCapture",
]
