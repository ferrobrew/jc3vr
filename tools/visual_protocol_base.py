"""Shared typed wire primitives for the visual and motion protocols."""

import json
import math
from dataclasses import asdict, dataclass
from enum import StrEnum


class ProtocolError(RuntimeError):
    pass


class Mode(StrEnum):
    LEFT_ONLY = "left_only"
    RIGHT_ONLY = "right_only"
    MULTIPASS = "multipass"
    SINGLE_PASS = "single_pass"
    REVERSE_MULTIPASS = "reverse_multipass"

    @property
    def selected_eye(self):
        return {Mode.LEFT_ONLY: 0, Mode.RIGHT_ONLY: 1}.get(self)

    @property
    def rendered_eyes(self):
        return tuple(self.selected_eye is None or eye == self.selected_eye for eye in (0, 1))


class DiagnosticProfile(StrEnum):
    NATIVE = "native"
    CONTROLLED_V1 = "controlled_v1"
    CONTROLLED_V2 = "controlled_v2"

    @property
    def is_controlled(self):
        return self in (DiagnosticProfile.CONTROLLED_V1, DiagnosticProfile.CONTROLLED_V2)


class Outcome(StrEnum):
    PASS = "pass"
    FAIL = "fail"
    INCONCLUSIVE = "inconclusive"


class AckOutcome(StrEnum):
    COMPLETED = "completed"
    READY = "ready"
    RESTORED = "restored"
    FAILED = "failed"


class RunOutcome(StrEnum):
    NOT_STARTED = "not_started"
    STARTED = "started"
    COMPLETED = "completed"
    NOT_VERIFIED = "not_verified"
    FAILED = "failed"


class Command(StrEnum):
    SCREENSHOT = "screenshot"
    CONFIGURE = "visual_configure"
    END = "visual_end"


POSE_TOLERANCE = 1e-4


@dataclass(frozen=True)
class OrientationOffset:
    yaw_deg: float
    pitch_deg: float
    roll_deg: float

    def __post_init__(self):
        for name in ("yaw_deg", "pitch_deg", "roll_deg"):
            value = getattr(self, name)
            if type(value) not in (int, float) or not math.isfinite(value) or not -180 <= value <= 180:
                raise ProtocolError(f"visual protocol: {name} must be finite and within [-180, 180]")
            object.__setattr__(self, name, float(value))

    @classmethod
    def from_wire(cls, value):
        return cls(finite_number(value, "yaw_deg"), finite_number(value, "pitch_deg"),
                   finite_number(value, "roll_deg"))

    def to_wire(self):
        return asdict(self)


@dataclass(frozen=True)
class PoseRequest:
    case_id: str
    offset: OrientationOffset

    def __post_init__(self):
        if not valid_request_id(self.case_id):
            raise ProtocolError("visual protocol: pose case_id must be an ASCII request ID")
        if not isinstance(self.offset, OrientationOffset):
            raise ProtocolError("visual protocol: pose request requires an orientation offset")

    @classmethod
    def from_wire(cls, value):
        return cls(text(value, "case_id"), OrientationOffset.from_wire(object_value(value).get("offset")))

    def to_wire(self):
        return {"case_id": self.case_id, "offset": self.offset.to_wire()}

    def to_toml(self) -> str:
        return ("[pose]\n"
                f"case_id = {json.dumps(self.case_id)}\n"
                f"yaw_deg = {json.dumps(self.offset.yaw_deg)}\n"
                f"pitch_deg = {json.dumps(self.offset.pitch_deg)}\n"
                f"roll_deg = {json.dumps(self.offset.roll_deg)}\n")


@dataclass(frozen=True)


class Camera:
    position: tuple[float, ...]
    orientation: tuple[float, ...]

    @classmethod
    def from_wire(cls, value):
        return cls(vector(value, "position", 3), vector(value, "orientation", 4))


@dataclass(frozen=True)
class PoseEvidence:
    case_id: str
    offset: OrientationOffset
    anchor: Camera
    applied: Camera

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        result = cls(text(value, "case_id"), OrientationOffset.from_wire(object_value(value).get("offset")),
                     Camera.from_wire(value.get("anchor")), Camera.from_wire(value.get("applied")))
        if not valid_request_id(result.case_id):
            raise ProtocolError("visual protocol: pose evidence case_id must be an ASCII request ID")
        result.validate_composition()
        return result

    def validate_composition(self, tolerance=POSE_TOLERANCE):
        if not isinstance(self.offset, OrientationOffset):
            raise ProtocolError("visual protocol: pose evidence offset is invalid")
        for name, camera in (("anchor", self.anchor), ("applied", self.applied)):
            if not isinstance(camera, Camera):
                raise ProtocolError(f"visual protocol: pose evidence {name} is not a camera")
            finite_vector(camera.position, 3, f"{name}.position")
            quaternion_normalize(camera.orientation)
        if not vectors_match(self.anchor.position, self.applied.position, tolerance):
            raise ProtocolError("visual protocol: pose evidence applied position differs from its anchor")
        expected = quaternion_multiply(quaternion_normalize(self.anchor.orientation), euler_yxz_quaternion(self.offset))
        if not quaternions_match(expected, self.applied.orientation, tolerance):
            raise ProtocolError("visual protocol: pose evidence applied orientation is not anchor * YXZ offset")

    def validate(self, request, anchor=None, tolerance=POSE_TOLERANCE):
        if not isinstance(request, PoseRequest) and hasattr(request, "pose"):
            request = request.pose
        if not isinstance(request, PoseRequest):
            raise ProtocolError("visual protocol: pose evidence requires a pose request")
        if not valid_request_id(self.case_id):
            raise ProtocolError("visual protocol: pose evidence case_id must be an ASCII request ID")
        if self.case_id != request.case_id:
            raise ProtocolError("visual protocol: pose evidence case_id differs from its request")
        if not offsets_match(self.offset, request.offset, tolerance):
            raise ProtocolError("visual protocol: pose evidence offset differs from its request")
        if anchor is not None and not cameras_match(anchor, self.anchor, tolerance):
            raise ProtocolError("visual protocol: pose evidence anchor changed during the run")
        self.validate_composition(tolerance)
        return self.anchor

    def validate_applied(self, camera, tolerance=POSE_TOLERANCE):
        self.validate_composition(tolerance)
        if not isinstance(camera, Camera):
            try:
                camera = Camera(tuple(camera.position), tuple(camera.orientation))
            except (AttributeError, TypeError) as error:
                raise ProtocolError("visual protocol: pose evidence applied camera is invalid") from error
        if not cameras_match(self.applied, camera, tolerance):
            raise ProtocolError("visual protocol: pose evidence applied camera differs from the captured camera")

    def to_wire(self):
        return {
            "case_id": self.case_id,
            "offset": self.offset.to_wire(),
            "anchor": {"position": list(self.anchor.position), "orientation": list(self.anchor.orientation)},
            "applied": {"position": list(self.applied.position), "orientation": list(self.applied.orientation)},
        }


def compose_pose(anchor, offset):
    if not isinstance(anchor, Camera) or not isinstance(offset, OrientationOffset):
        raise ProtocolError("visual protocol: pose composition requires a camera and an offset")
    orientation = quaternion_multiply(quaternion_normalize(anchor.orientation), euler_yxz_quaternion(offset))
    return Camera(anchor.position, quaternion_normalize(orientation))


def euler_yxz_quaternion(offset):
    yaw, pitch, roll = (math.radians(value) * 0.5 for value in
                        (offset.yaw_deg, offset.pitch_deg, offset.roll_deg))
    yaw_quaternion = (0.0, math.sin(yaw), 0.0, math.cos(yaw))
    pitch_quaternion = (math.sin(pitch), 0.0, 0.0, math.cos(pitch))
    roll_quaternion = (0.0, 0.0, math.sin(roll), math.cos(roll))
    return quaternion_normalize(quaternion_multiply(quaternion_multiply(yaw_quaternion, pitch_quaternion), roll_quaternion))


def quaternion_multiply(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz)


def quaternion_normalize(value):
    finite_vector(value, 4, "quaternion")
    length = math.sqrt(sum(item * item for item in value))
    if length <= 0:
        raise ProtocolError("visual protocol: pose evidence quaternion has zero length")
    return tuple(item / length for item in value)


def quaternions_match(a, b, tolerance=POSE_TOLERANCE):
    left, right = quaternion_normalize(a), quaternion_normalize(b)
    direct = max(abs(x - y) for x, y in zip(left, right))
    negated = max(abs(x + y) for x, y in zip(left, right))
    return min(direct, negated) <= tolerance


def cameras_match(a, b, tolerance=POSE_TOLERANCE):
    if not isinstance(a, Camera) or not isinstance(b, Camera):
        raise ProtocolError("visual protocol: camera comparison requires two cameras")
    return (vectors_match(a.position, b.position, tolerance) and
            quaternions_match(a.orientation, b.orientation, tolerance))


def offsets_match(a, b, tolerance=POSE_TOLERANCE):
    return all(abs(left - right) <= tolerance for left, right in
               zip((a.yaw_deg, a.pitch_deg, a.roll_deg), (b.yaw_deg, b.pitch_deg, b.roll_deg)))


def vectors_match(a, b, tolerance):
    return len(a) == len(b) and all(abs(left - right) <= tolerance for left, right in zip(a, b))


def finite_vector(value, length, name):
    if not isinstance(value, (tuple, list)) or len(value) != length or any(
            type(item) not in (int, float) or not math.isfinite(item) for item in value):
        raise ProtocolError(f"visual protocol: pose evidence {name} is invalid")
    return tuple(value)


def object_value(value):
    if not isinstance(value, dict):
        raise ProtocolError("visual protocol: expected a JSON object")
    return value


def integer(value, key, minimum=None):
    result = object_value(value).get(key)
    if type(result) is not int or (minimum is not None and result < minimum):
        raise ProtocolError(f"visual protocol: invalid integer {key}")
    return result


def text(value, key):
    result = object_value(value).get(key)
    if not isinstance(result, str) or not result:
        raise ProtocolError(f"visual protocol: invalid string {key}")
    return result


def finite_number(value, key):
    result = object_value(value).get(key)
    if type(result) not in (int, float) or not math.isfinite(result):
        raise ProtocolError(f"visual protocol: invalid number {key}")
    return result


def valid_request_id(value):
    return isinstance(value, str) and 1 <= len(value) <= 64 and all(
        character.isascii() and (character.isalnum() or character in "_-") for character in value)


def parse_mode(value):
    try:
        return Mode(text(value, "mode"))
    except ValueError as error:
        raise ProtocolError("visual protocol: unknown mode") from error


def boolean(value, key):
    result = object_value(value).get(key)
    if type(result) is not bool:
        raise ProtocolError(f"visual protocol: invalid boolean {key}")
    return result


def boolean_pair(value, key):
    result = object_value(value).get(key)
    if not isinstance(result, list) or len(result) != 2 or any(type(item) is not bool for item in result):
        raise ProtocolError(f"visual protocol: {key} must contain two booleans")
    return tuple(result)


def vector(value, key, length):
    result = object_value(value).get(key)
    if not isinstance(result, list) or len(result) != length or any(
            type(item) not in (int, float) or not math.isfinite(item) for item in result):
        raise ProtocolError(f"visual protocol: invalid numeric vector {key}")
    return tuple(result)
