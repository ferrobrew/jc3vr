"""Typed wire values for the visual harness protocol."""

import json
import math
from dataclasses import asdict, dataclass, field
from enum import StrEnum
from typing import TypeAlias

from tools.rendering.protocol.base import (
    POSE_COMPOSITION_ULPS,
    POSE_TOLERANCE,
    AckOutcome,
    Camera,
    Command,
    DiagnosticProfile,
    Mode,
    OrientationOffset,
    Outcome,
    PoseEvidence,
    PoseRequest,
    ProtocolError,
    RunOutcome,
    TranslationOffset,
    boolean,
    boolean_pair,
    cameras_match,
    compose_pose,
    composed_position_tolerances,
    composed_positions_match,
    euler_yxz_degrees,
    euler_yxz_quaternion,
    finite_number,
    finite_vector,
    float32_ulp,
    integer,
    object_value,
    offset_for_target,
    offsets_match,
    parse_mode,
    quaternion_conjugate,
    quaternion_multiply,
    quaternion_normalize,
    quaternions_match,
    text,
    valid_request_id,
    vector,
    vectors_match,
)
from tools.rendering.protocol.motion import (
    ExpectedMotion,
    MotionEvidence,
    MotionHistory,
    MotionRequest,
    PhaseCapture,
)

__all__ = [
    "POSE_COMPOSITION_ULPS",
    "POSE_TOLERANCE",
    "AckOutcome",
    "Camera",
    "Command",
    "DiagnosticProfile",
    "EnvironmentObservation",
    "EnvironmentRequest",
    "ExpectedMotion",
    "Mode",
    "MotionEvidence",
    "MotionHistory",
    "MotionRequest",
    "OrientationOffset",
    "Outcome",
    "PhaseCapture",
    "PoseEvidence",
    "PoseRequest",
    "ProtocolError",
    "RunOutcome",
    "TranslationOffset",
    "WeatherControl",
    "WeatherEvent",
    "boolean",
    "boolean_pair",
    "cameras_match",
    "compose_pose",
    "composed_position_tolerances",
    "composed_positions_match",
    "euler_yxz_degrees",
    "euler_yxz_quaternion",
    "finite_number",
    "finite_vector",
    "float32_ulp",
    "integer",
    "object_value",
    "offset_for_target",
    "offsets_match",
    "parse_mode",
    "quaternion_conjugate",
    "quaternion_multiply",
    "quaternion_normalize",
    "quaternions_match",
    "text",
    "valid_request_id",
    "vector",
    "vectors_match",
]


class WeatherEvent(StrEnum):
    SUNNY = "sunny"


class WeatherControl(StrEnum):
    SUNNY = "sunny"
    AMBIENT = "ambient"


@dataclass(frozen=True)
class EnvironmentRequest:
    hour: float
    weather: WeatherEvent

    def __post_init__(self):
        if type(self.hour) not in (int, float) or not math.isfinite(self.hour) or not 0 <= self.hour < 24:
            raise ProtocolError("visual protocol: environment hour must be finite and within [0, 24)")
        object.__setattr__(self, "hour", float(self.hour))
        if not isinstance(self.weather, WeatherEvent):
            raise ProtocolError("visual protocol: environment weather must be a known event")

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        try:
            weather = WeatherEvent(text(value, "weather"))
        except ValueError as error:
            raise ProtocolError("visual protocol: unknown weather event") from error
        return cls(finite_number(value, "hour"), weather)

    def to_wire(self):
        return {"hour": self.hour, "weather": self.weather.value}

    def to_toml(self) -> str:
        return ("[environment]\n"
                f"hour = {json.dumps(self.hour)}\n"
                f"weather = {json.dumps(self.weather.value)}\n")


@dataclass(frozen=True)
class EnvironmentObservation:
    hour: float
    day_cycle_speed: float
    weather_severity: float
    weather_control: WeatherControl | None = None

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        control = value.get("weather_control")
        if control is not None:
            try:
                control = WeatherControl(text(value, "weather_control"))
            except ValueError as error:
                raise ProtocolError("visual protocol: unknown weather control") from error
        result = cls(finite_number(value, "hour"), finite_number(value, "day_cycle_speed"),
                     finite_number(value, "weather_severity"), control)
        if not 0 <= result.hour <= 24 or result.day_cycle_speed < 0 or result.weather_severity < 0:
            raise ProtocolError("visual protocol: environment observation is out of range")
        return result

    def to_wire(self):
        return asdict(self)


class CascadeScale(StrEnum):
    SHORE_LOW = "shore_low"
    SHORE_HIGH = "shore_high"

    @property
    def bits(self) -> int:
        return {self.SHORE_LOW: 0x3C07E14C, self.SHORE_HIGH: 0x3C07EC65}[self]


class WaterReflection(StrEnum):
    ZERO_WEIGHT = "zero_weight"
    CENTER_PLANAR = "center_planar"


class SunDepthClip(StrEnum):
    DISABLED_RIGHT = "disabled_right"


class SunShadows(StrEnum):
    DISABLED = "disabled"


@dataclass(frozen=True)
class VisualOverrides:
    sun_shadows: SunShadows

    def __post_init__(self):
        if not isinstance(self.sun_shadows, SunShadows):
            raise ProtocolError("visual protocol: invalid sun_shadows override")

    def to_toml(self):
        return f'[visual_overrides]\nsun_shadows = {json.dumps(self.sun_shadows.value)}\n'

    @classmethod
    def from_wire(cls, value):
        if not isinstance(value, dict) or set(value) != {"sun_shadows"}:
            raise ProtocolError("visual protocol: invalid visual_overrides table")
        try:
            return cls(SunShadows(value["sun_shadows"]))
        except (ValueError, TypeError) as error:
            raise ProtocolError("visual protocol: invalid sun_shadows override") from error


@dataclass(frozen=True)
class Request:
    command: Command
    id: str
    mode: Mode | None = None
    warmup_frames: int | None = None
    pose: PoseRequest | None = None
    motion: MotionRequest | None = None
    motion_capture: PhaseCapture | None = None
    environment: EnvironmentRequest | None = None
    water_tess_draw: int | None = None
    water_inputs: bool | None = None
    water_reflection: WaterReflection | None = None
    cascade_scale: CascadeScale | None = None
    resolve_inputs: bool | None = None
    sun_depth_clip: SunDepthClip | None = None
    sun_inputs: bool | None = None
    visual_overrides: VisualOverrides | None = None
    shader_identity: bool | None = None

    def __post_init__(self):
        if not isinstance(self.command, Command) or not valid_request_id(self.id):
            raise ProtocolError("visual protocol: invalid request command or ID")
        if self.command == Command.CONFIGURE:
            if not isinstance(self.mode, Mode) or type(self.warmup_frames) is not int or not 1 <= self.warmup_frames <= 300:
                raise ProtocolError("visual protocol: configuration requires a mode and a frame count from 1 through 300")
        elif self.mode is not None or self.warmup_frames is not None:
            raise ProtocolError("visual protocol: only configuration requests accept a mode or frame count")
        if self.pose is not None and (self.command != Command.CONFIGURE or not isinstance(self.pose, PoseRequest)):
            raise ProtocolError("visual protocol: only configuration requests accept a pose")
        if self.motion is not None and (self.command != Command.CONFIGURE or not isinstance(self.motion, MotionRequest)):
            raise ProtocolError("visual protocol: only configuration requests accept motion")
        if self.pose is not None and self.motion is not None:
            raise ProtocolError("visual protocol: pose and motion are mutually exclusive")
        if self.environment is not None and (self.command != Command.CONFIGURE or
                                             not isinstance(self.environment, EnvironmentRequest)):
            raise ProtocolError("visual protocol: only configuration requests accept an environment")
        if self.shader_identity is not None and (self.command != Command.CONFIGURE or
                                                 type(self.shader_identity) is not bool):
            raise ProtocolError("visual protocol: only configuration accepts a boolean shader_identity")
        if self.visual_overrides is not None and (self.command != Command.CONFIGURE or
                                                   not isinstance(self.visual_overrides, VisualOverrides)):
            raise ProtocolError("visual protocol: only configuration accepts visual overrides")
        if self.water_inputs is not None and (self.command != Command.SCREENSHOT or
                                              type(self.water_inputs) is not bool):
            raise ProtocolError("visual protocol: only screenshot requests accept a boolean water_inputs")
        if self.water_tess_draw is not None and (self.water_inputs is not True or type(self.water_tess_draw) is not int or not 0 <= self.water_tess_draw < 32):
            raise ProtocolError("visual protocol: water_tess_draw requires water_inputs and an index below 32")
        if self.water_reflection is not None and (self.command != Command.SCREENSHOT or
                                                  not isinstance(self.water_reflection, WaterReflection) or
                                                  self.water_inputs is not True or self.motion_capture is not None):
            raise ProtocolError("visual protocol: water_reflection requires screenshot water_inputs without motion")
        if self.sun_depth_clip is not None and (self.command != Command.SCREENSHOT or
                                                not isinstance(self.sun_depth_clip, SunDepthClip) or
                                                self.sun_inputs is not True):
            raise ProtocolError("visual protocol: sun_depth_clip requires screenshot sun_inputs")
        if self.cascade_scale is not None and (not isinstance(self.cascade_scale, CascadeScale) or
                self.command != Command.SCREENSHOT or self.resolve_inputs is not True or self.motion_capture is not None):
            raise ProtocolError("visual protocol: cascade_scale requires a resolve screenshot without motion")
        if self.resolve_inputs is not None and (self.command != Command.SCREENSHOT or
                                                type(self.resolve_inputs) is not bool):
            raise ProtocolError("visual protocol: only screenshot requests accept a boolean resolve_inputs")
        if self.sun_inputs is not None and (self.command != Command.SCREENSHOT or
                                            type(self.sun_inputs) is not bool):
            raise ProtocolError("visual protocol: only screenshot requests accept a boolean sun_inputs")
        if self.motion_capture is not None and (self.command != Command.SCREENSHOT or
                                                not isinstance(self.motion_capture, PhaseCapture)):
            raise ProtocolError("visual protocol: only screenshot requests accept a motion capture phase")

    def to_wire(self):
        return {key: item for key, item in asdict(self).items() if item is not None}

    def to_toml(self) -> str:
        result = "".join(f"{key} = {json.dumps(value)}\n"
                         for key, value in self.to_wire().items()
                         if key not in ("pose", "motion", "motion_capture", "environment", "visual_overrides"))
        if self.pose is not None:
            result += "\n" + self.pose.to_toml()
        if self.motion is not None:
            result += "\n" + self.motion.to_toml()
        if self.motion_capture is not None:
            result += "\n" + self.motion_capture.to_toml()
        if self.environment is not None:
            result += "\n" + self.environment.to_toml()
        if self.visual_overrides is not None:
            result += "\n" + self.visual_overrides.to_toml()
        return result


@dataclass(frozen=True)
class ConfigurationAck:
    id: str
    metadata: "VisualState"
    outcome: AckOutcome = field(default=AckOutcome.READY, init=False)

    @property
    def generation(self):
        return self.metadata.generation

    @property
    def mode(self):
        return self.metadata.mode

    @property
    def warmup_frames(self):
        return self.metadata.warmup_frames

    def validate_pose(self, request, anchor=None):
        pose = request.pose if isinstance(request, Request) else request
        if pose is None:
            return self.metadata.pose
        if not isinstance(pose, PoseRequest):
            raise ProtocolError("visual protocol: pose validation requires a pose request")
        if self.metadata.pose is None:
            raise ProtocolError("visual protocol: configuration is missing requested pose evidence")
        return self.metadata.pose.validate(pose, anchor)

    def validate_motion(self, request, anchor=None):
        motion = request.motion if isinstance(request, Request) else request
        if motion is None:
            return self.metadata.motion
        if not isinstance(motion, MotionRequest):
            raise ProtocolError("visual protocol: motion validation requires a motion request")
        if self.metadata.motion is None:
            raise ProtocolError("visual protocol: configuration is missing requested motion evidence")
        if self.metadata.pose is None:
            raise ProtocolError("visual protocol: configuration motion has no pose evidence")
        return self.metadata.motion.validate_configuration(motion, self.metadata.pose, anchor)

    @classmethod
    def from_wire(cls, value):
        metadata = VisualState.from_wire(object_value(value).get("metadata"))
        if ack_outcome(value) != AckOutcome.READY or not metadata.ready:
            raise ProtocolError("visual protocol: configuration is not ready")
        if not metadata.profile.is_controlled or not metadata.paused or not metadata.frozen:
            raise ProtocolError("visual protocol: configuration is not controlled, paused, and frozen")
        if metadata.error is not None or metadata.generation < 1 or metadata.mode is None:
            raise ProtocolError("visual protocol: configuration has no diagnostic mode or generation")
        if not 1 <= metadata.warmup_frames <= 300 or metadata.completed_frames < metadata.warmup_frames:
            raise ProtocolError("visual protocol: configuration warmup is incomplete")
        return cls(text(value, "id"), metadata)


@dataclass(frozen=True)
class RestorationAck:
    id: str
    metadata: "VisualState"
    outcome: AckOutcome = field(default=AckOutcome.RESTORED, init=False)

    @classmethod
    def from_wire(cls, value):
        metadata = VisualState.from_wire(object_value(value).get("metadata"))
        if (ack_outcome(value) != AckOutcome.RESTORED or metadata.mode is not None or
                metadata.profile != DiagnosticProfile.NATIVE or not metadata.ready or
                metadata.error is not None or metadata.engine_dimensions is None):
            raise ProtocolError("visual protocol: visual controls are not restored")
        return cls(text(value, "id"), metadata)


def parse_shader_identity(value):
    if value is None:
        return None
    if (not isinstance(value, dict) or set(value) != {"requested", "status", "registered", "overflow", "invalid"} or
            type(value.get("requested")) is not bool or
            not isinstance(value.get("status"), str) or
            value["status"] not in ("unarmed", "install_failed", "incomplete", "empty", "complete") or
            any(type(value.get(key)) is not int or value[key] < 0 for key in ("registered", "overflow", "invalid"))):
        raise ProtocolError("visual protocol: invalid shader identity census metadata")
    if value["status"] == "complete" and (not value["requested"] or value["registered"] == 0 or value["overflow"] or value["invalid"]):
        raise ProtocolError("visual protocol: inconsistent shader identity census metadata")
    if value["status"] == "unarmed" and value["requested"]:
        raise ProtocolError("visual protocol: inconsistent shader identity census metadata")
    return value


@dataclass(frozen=True)
class VisualState:
    generation: int
    mode: Mode | None
    profile: DiagnosticProfile
    ready: bool
    paused: bool
    frozen: bool
    error: str | None
    warmup_frames: int
    completed_frames: int
    engine_dimensions: tuple[int, int] | None
    pose: "PoseEvidence | None" = None
    motion: "MotionEvidence | None" = None
    environment: EnvironmentObservation | None = None
    visual_overrides: VisualOverrides | None = None
    shader_identity: dict | None = None

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        mode = None if value.get("mode") is None else parse_mode(value)
        try:
            profile = DiagnosticProfile(text(value, "profile"))
        except ValueError as error:
            raise ProtocolError("visual protocol: unknown diagnostic profile") from error
        error = value.get("error")
        if error is not None and not isinstance(error, str):
            raise ProtocolError("visual protocol: invalid visual error")
        dimensions = value.get("engine_dimensions")
        if dimensions is not None and (not isinstance(dimensions, list) or len(dimensions) != 2 or any(
                type(item) is not int or item <= 0 for item in dimensions)):
            raise ProtocolError("visual protocol: invalid engine dimensions")
        return cls(integer(value, "generation", minimum=0), mode, profile,
                   boolean(value, "ready"), boolean(value, "paused"), boolean(value, "frozen"), error,
                   integer(value, "warmup_frames", minimum=0), integer(value, "completed_frames", minimum=0),
                   tuple(dimensions) if dimensions is not None else None,
                   None if value.get("pose") is None else PoseEvidence.from_wire(value.get("pose")),
                   None if value.get("motion") is None else MotionEvidence.from_wire(value.get("motion")),
                   None if value.get("environment") is None else EnvironmentObservation.from_wire(value.get("environment")),
                   None if value.get("visual_overrides") is None else VisualOverrides.from_wire(value["visual_overrides"]),
                   parse_shader_identity(value.get("shader_identity")))



@dataclass(frozen=True)
class Eye:
    offset: tuple[float, ...]
    orientation_delta: tuple[float, ...]
    projection_standard: tuple[float, ...]
    projection_reverse_z: tuple[float, ...]

    @classmethod
    def from_wire(cls, value):
        return cls(vector(value, "offset", 3), vector(value, "orientation_delta", 4),
                   vector(value, "projection_standard", 16), vector(value, "projection_reverse_z", 16))


@dataclass(frozen=True)
class ComparisonInputs:
    build_stamp: str
    profile: DiagnosticProfile
    camera: Camera
    eyes: tuple[Eye, Eye]
    eye_width: int
    eye_height: int
    world_scale: float

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        eyes = value.get("eyes")
        if not isinstance(eyes, list) or len(eyes) != 2:
            raise ProtocolError("visual protocol: comparison inputs require two eyes")
        scale = value.get("world_scale")
        if type(scale) not in (int, float) or not math.isfinite(scale) or scale <= 0:
            raise ProtocolError("visual protocol: invalid world scale")
        try:
            profile = DiagnosticProfile(text(value, "profile"))
        except ValueError as error:
            raise ProtocolError("visual protocol: unknown diagnostic profile") from error
        return cls(text(value, "build_stamp"), profile, Camera.from_wire(value.get("camera")),
                   tuple(Eye.from_wire(eye) for eye in eyes), integer(value, "eye_width", minimum=1),
                   integer(value, "eye_height", minimum=1), scale)


@dataclass(frozen=True)
class NativeCapture:
    id: str
    frame_ordinal: int
    predicted_display_time_ns: int
    source: str
    width: int
    height: int
    eye_width: int
    eye_height: int
    fresh_eyes: tuple[bool, bool]
    gameplay_ready: bool
    xr_session_running: bool

    @classmethod
    def from_wire(cls, value):
        return cls(text(value, "id"), integer(value, "frame_ordinal", minimum=1),
                   integer(value, "predicted_display_time_ns"), text(value, "source"),
                   integer(value, "width", minimum=1), integer(value, "height", minimum=1),
                   integer(value, "eye_width", minimum=1), integer(value, "eye_height", minimum=1),
                   boolean_pair(value, "fresh_eyes"), boolean(value, "gameplay_ready"),
                   boolean(value, "xr_session_running"))


@dataclass(frozen=True)
class Capture(NativeCapture):
    mode: Mode
    selected_eye: int | None
    rendered_eyes: tuple[bool, bool]
    visual: VisualState
    comparison_inputs: ComparisonInputs

    @classmethod
    def from_wire(cls, value):
        object_value(value)
        selected = value.get("selected_eye", "missing")
        if selected is not None and (type(selected) is not int or selected not in (0, 1)):
            raise ProtocolError("visual protocol: selected_eye must be null, zero, or one")
        return cls(text(value, "id"), integer(value, "frame_ordinal", minimum=1),
                   integer(value, "predicted_display_time_ns"), text(value, "source"),
                   integer(value, "width", minimum=1), integer(value, "height", minimum=1),
                   integer(value, "eye_width", minimum=1), integer(value, "eye_height", minimum=1),
                   boolean_pair(value, "fresh_eyes"), boolean(value, "gameplay_ready"),
                   boolean(value, "xr_session_running"), parse_mode(value), selected,
                   boolean_pair(value, "rendered_eyes"), VisualState.from_wire(value.get("visual")),
                   ComparisonInputs.from_wire(value.get("comparison_inputs")))

    def validate(self, capture_id, mode, generation, dimensions, motion_capture=None):
        if self.id != capture_id or not self.gameplay_ready or not self.xr_session_running:
            raise ProtocolError("visual protocol: screenshot is not a gameplay XR capture")
        if self.mode != mode or self.visual.mode != mode or self.visual.generation != generation:
            raise ProtocolError("visual protocol: screenshot diagnostic mode or generation mismatch")
        if (not self.visual.ready or not self.visual.paused or not self.visual.frozen or self.visual.error is not None or
                not self.visual.profile.is_controlled or self.comparison_inputs.profile != self.visual.profile):
            raise ProtocolError("visual protocol: screenshot diagnostic profile is not ready")
        if self.selected_eye != mode.selected_eye or self.rendered_eyes != mode.rendered_eyes:
            raise ProtocolError("visual protocol: screenshot eye selection mismatch")
        if any(required and not fresh for required, fresh in zip(mode.rendered_eyes, self.fresh_eyes)):
            raise ProtocolError("visual protocol: screenshot has stale rendered eyes")
        sources = ("isolated_eye",) if mode.selected_eye is not None else ("eye_textures", "collapsed_back_buffer")
        if self.source not in sources:
            raise ProtocolError("visual protocol: screenshot has an invalid source")
        columns = 1 if mode.selected_eye is not None else 2
        if dimensions != (self.width, self.height) or dimensions != (self.eye_width * columns, self.eye_height):
            raise ProtocolError("visual protocol: PNG dimensions disagree with the eye metadata")
        if (self.eye_width, self.eye_height) != (self.comparison_inputs.eye_width, self.comparison_inputs.eye_height):
            raise ProtocolError("visual protocol: comparison input dimensions disagree with the capture")
        if self.visual.pose is not None:
            self.visual.pose.validate_applied(self.comparison_inputs.camera)
        if motion_capture is not None:
            if not isinstance(motion_capture, PhaseCapture):
                raise ProtocolError("visual protocol: screenshot motion capture phase is invalid")
            if self.visual.motion is None or self.visual.pose is None:
                raise ProtocolError("visual protocol: screenshot is missing requested motion evidence")
            motion_capture.validate(self.visual.motion.request.period_frames)
            self.visual.motion.validate_capture(self.visual.motion.request, motion_capture.phase,
                                                self.visual.pose.anchor, self.visual.pose,
                                                self.comparison_inputs.camera,
                                                self.predicted_display_time_ns)
        elif self.visual.motion is not None:
            raise ProtocolError("visual protocol: screenshot has unexpected motion evidence")


@dataclass(frozen=True)
class Sample:
    png: str
    metadata: str


@dataclass
class Case:
    name: str
    modes: dict[Mode, list[Sample]]
    expected_pose: PoseRequest | None = None
    expected_motion: ExpectedMotion | None = None


@dataclass(frozen=True)
class Manifest:
    cases: list[Case]

    def to_wire(self):
        value = asdict(self)
        for case in value["cases"]:
            if case.get("expected_pose") is None:
                case.pop("expected_pose", None)
            if case.get("expected_motion") is None:
                case.pop("expected_motion", None)
        return value


@dataclass(frozen=True)
class ComparisonReport:
    status: Outcome

    @property
    def exit_code(self) -> int:
        return {Outcome.PASS: 0, Outcome.FAIL: 1, Outcome.INCONCLUSIVE: 2}[self.status]

    @classmethod
    def from_wire(cls, value):
        try:
            return cls(Outcome(text(value, "status")))
        except ValueError as error:
            raise ProtocolError("visual protocol: unknown comparison status") from error


@dataclass(frozen=True)
class ScreenshotAck:
    id: str
    capture: NativeCapture | Capture
    outcome: AckOutcome = field(default=AckOutcome.COMPLETED, init=False)


@dataclass(frozen=True)
class FailedAck:
    id: str
    error: str
    gameplay_ready: bool | None
    outcome: AckOutcome = field(default=AckOutcome.FAILED, init=False)


Acknowledgement: TypeAlias = ConfigurationAck | RestorationAck | ScreenshotAck | FailedAck


@dataclass(frozen=True)
class SmokeResult:
    smoke_succeeded: bool
    captures: tuple[NativeCapture, ...] = ()

    def to_wire(self):
        return asdict(self)


@dataclass(frozen=True)
class SuiteResult(SmokeResult):
    suite_succeeded: bool = False
    suite_manifest: str = ""
    suite_captures: tuple[Capture, ...] = ()
    configurations: tuple[ConfigurationAck, ...] = ()
    comparison: ComparisonReport | None = None

    @property
    def visual_passed(self) -> bool:
        return self.comparison is not None and self.comparison.status == Outcome.PASS

    def to_wire(self):
        value = asdict(self)
        value["visual_passed"] = self.visual_passed
        for configuration in value["configurations"]:
            configuration.pop("outcome")
        return value


@dataclass(frozen=True)
class ProbeResult(SmokeResult):
    probe_manifest: str = ""
    probe_captures: tuple[Capture, ...] = ()
    configurations: tuple[ConfigurationAck, ...] = ()
    diagnostic: Outcome = Outcome.INCONCLUSIVE

    @property
    def exit_code(self) -> int:
        return {Outcome.PASS: 0, Outcome.FAIL: 1, Outcome.INCONCLUSIVE: 2}[self.diagnostic]

    def to_wire(self):
        value = asdict(self)
        for configuration in value["configurations"]:
            configuration.pop("outcome")
        return value


@dataclass
class RunReport:
    status: RunOutcome = RunOutcome.NOT_STARTED
    smoke_succeeded: bool = False
    result: SmokeResult | SuiteResult | ProbeResult | None = None
    error: str | None = None
    restoration_error: str | None = None
    cleanup_errors: list[str] = field(default_factory=list)

    @property
    def exit_code(self) -> int:
        if self.status != RunOutcome.COMPLETED or not self.smoke_succeeded or self.cleanup_errors:
            return 1
        if isinstance(self.result, SuiteResult):
            if not self.result.suite_succeeded or self.result.comparison is None:
                return 1
            return self.result.comparison.exit_code
        if isinstance(self.result, ProbeResult):
            return self.result.exit_code
        return 0

    def complete(self, result: SmokeResult | SuiteResult | ProbeResult | None):
        if result is not None and not isinstance(result, SmokeResult):
            raise ProtocolError("visual protocol: a run callback must return a typed smoke, suite, or probe result")
        self.result = result
        self.smoke_succeeded = result is not None and result.smoke_succeeded
        self.status = RunOutcome.COMPLETED if self.smoke_succeeded else RunOutcome.NOT_VERIFIED

    def fail(self, error: BaseException):
        self.status = RunOutcome.FAILED
        self.smoke_succeeded = False
        self.error = str(error)

    def to_wire(self):
        if self.cleanup_errors:
            self.status = RunOutcome.FAILED
            self.smoke_succeeded = False
        value = self.result.to_wire() if self.result is not None else {}
        value.update(status=self.status, smoke_succeeded=self.smoke_succeeded, cleanup_errors=self.cleanup_errors)
        if self.error is not None:
            value["error"] = self.error
        if self.restoration_error is not None:
            value["restoration_error"] = self.restoration_error
        return value


def ack_outcome(value):
    try:
        return AckOutcome(text(value, "state"))
    except ValueError as error:
        raise ProtocolError("visual protocol: unknown acknowledgement state") from error


def acknowledgement_from_wire(value, command: Command, request_id: str) -> Acknowledgement:
    if text(value, "id") != request_id:
        raise ProtocolError("visual protocol: acknowledgement ID mismatch")
    outcome = ack_outcome(value)
    if outcome == AckOutcome.FAILED:
        metadata = value.get("metadata")
        message = value.get("error") or (metadata.get("error") if isinstance(metadata, dict) else None)
        ready = value.get("gameplay_ready")
        if ready is not None and type(ready) is not bool:
            raise ProtocolError("visual protocol: invalid acknowledgement gameplay readiness")
        if message is not None and not isinstance(message, str):
            raise ProtocolError("visual protocol: invalid acknowledgement error")
        return FailedAck(request_id, message or "unknown capture failure", ready)
    expected = {Command.SCREENSHOT: AckOutcome.COMPLETED, Command.CONFIGURE: AckOutcome.READY,
                Command.END: AckOutcome.RESTORED}[command]
    if outcome != expected:
        raise ProtocolError("visual protocol: incomplete acknowledgement")
    if command == Command.CONFIGURE:
        return ConfigurationAck.from_wire(value)
    if command == Command.END:
        return RestorationAck.from_wire(value)
    capture_type = NativeCapture if value.get("mode") in (None, DiagnosticProfile.NATIVE) else Capture
    return ScreenshotAck(request_id, capture_type.from_wire(value))
