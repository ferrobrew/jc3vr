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
from tools.rendering.protocol.headset import HEADSET_RIGS, HeadsetGeometry
from tools.rendering.protocol.benchmark import BenchmarkAck
from tools.rendering.protocol.snapshot import ReleaseAck, SnapshotAck
from tools.rendering.protocol.stage import (
    Placement,
    StageAck,
    StageRequest,
    StageSpawn,
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
    "HEADSET_RIGS",
    "HeadsetGeometry",
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
    "Placement",
    "StageAck",
    "StageRequest",
    "StageSpawn",
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
    RAIN = "rain"
    SNOW = "snow"


class WeatherControl(StrEnum):
    SUNNY = "sunny"
    RAIN = "rain"
    SNOW = "snow"
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
    LAKE_SAMPLES = "lake_samples"
    LAKE_LIGHTING = "lake_lighting"
    LAKE_SKY = "lake_sky"
    LAKE_SKY_BUFFER = "lake_sky_buffer"
    LAKE_RESPONSE = "lake_response"
    LAKE_NEAR_PATCH = "lake_near_patch"
    LAKE_TESS_PATCH = "lake_tess_patch"
    LAKE_PATCH_DRAW = "lake_patch_draw"
    LAKE_PATCH_OWNERS = "lake_patch_owners"
    LAKE_CAMERA_CONTROL = "lake_camera_control"
    LAKE_HULL_CAMERA = "lake_hull_camera"
    LAKE_DOMAIN_CAMERA = "lake_domain_camera"
    PLANAR_UV = "planar_uv"
    EYE_WIDTH_PLANAR = "eye_width_planar"
    MIRRORED_PLANAR = "mirrored_planar"
    PROJECTED_PLANAR = "projected_planar"
    PRISTINE_LAKE = "pristine_lake"
    ZERO_WEIGHT = "zero_weight"
    ZERO_PLANAR = "zero_planar"
    CENTER_PLANAR = "center_planar"
    PACKED_REFRACTION = "packed_refraction"


class SunDepthClip(StrEnum):
    DISABLED_RIGHT = "disabled_right"


class SunShadows(StrEnum):
    DISABLED = "disabled"


# The single-pass production switches a configuration may override, by their configuration field
# names. Mirrors `SinglePassSwitch` in `payload/src/stereo/switches.rs`; a test keeps the two equal.
SINGLE_PASS_SWITCHES = (
    "reproject", "reproject_camera_only", "terrain", "terrain_patch_per_eye", "terrain_eye_rows", "full_view_projection_rows", "tree_impostors",
    "tree_impostor_pixel_size_per_eye", "tree_impostor_screen_door_per_eye", "static_lights_per_eye", "moon_per_eye", "water_plane_per_eye", "clouds_per_eye", "stars_per_eye", "bark", "foliage",
    "vegetation_once", "occluder",
    "instanced_per_eye", "hw_instanced_once", "indirect_per_eye", "vertex_globals_per_eye", "reconstruct_per_eye", "fullscreen_camera_per_eye",
    "pixel_camera_per_eye", "local_lights_per_eye", "screen_door_per_eye", "velocity_per_eye", "clip_origin_per_eye", "terrain_detail_eye_rows",
    "terrain_cull_both_eyes",
    "atmospheric_per_eye",
    "fog_volume_per_eye", "water_uv_per_eye",
    "nvwater_per_eye", "water_reflection_per_eye", "water_reflection_eye_size",
    "water_camera_per_eye",
    "ssdecal_geometry_per_eye", "slot13_per_eye", "non_indexed_geometry_once",
    "collapse_viewport_follows_target",
    "ssao_per_eye", "ssao_exact_sampling", "ao_volumes_per_eye", "ssr_per_eye", "subsurface_per_eye", "dof_per_eye", "ssdecal_per_eye",
    "composite_overlays_per_eye", "glare_per_eye", "foveation_per_eye", "fsr_per_eye", "clustered_per_eye",
    "clustered_per_eye_light_view", "uniform_viewport_slots",
)


def single_pass_switches(value, context="single_pass") -> tuple[tuple[str, bool], ...]:
    """Validate a table of single-pass switch overrides into sorted (name, value) pairs."""
    if not isinstance(value, dict):
        raise ProtocolError(f"visual protocol: {context} must be a table of switches")
    unknown = sorted(set(value) - set(SINGLE_PASS_SWITCHES))
    if unknown:
        raise ProtocolError(f"visual protocol: {context} names unknown single-pass switches "
                            f"{unknown}; expected some of {list(SINGLE_PASS_SWITCHES)}")
    for name, item in value.items():
        if type(item) is not bool:
            raise ProtocolError(f"visual protocol: {context}.{name} must be a boolean")
    return tuple(sorted(value.items()))


# Settings the capture profile turns off or pins, put back at the mod's shipped value in every mode.
# Mirrors `VisualOverrides::shipped` in `payload/src/testing/visual/overrides.rs`.
# Sources left out in every mode, to attribute a difference to one.
DISABLING_OVERRIDES = ("ssao", "ao_volumes", "global_illumination", "fsr_mv_correction",
                       "fsr_hybrid_velocity")
SHIPPED_OVERRIDES = ("fsr", "foveation", "auto_exposure", "screen_fade", "player_damage")
# The range of the resolution-scale override (each side). Mirrors `ResolutionScaleOverride` in
# `payload/src/testing/visual/overrides.rs`: the top is twice the session's pixels.
RESOLUTION_SCALE_RANGE = (0.5, 1.415)


@dataclass(frozen=True)
class VisualOverrides:
    sun_shadows: SunShadows | None = None
    screen_space_reflections: str | None = None
    single_pass: tuple[tuple[str, bool], ...] = ()
    # Ambient-occlusion sources left out in every mode, to attribute a difference to one.
    ssao: str | None = None
    ao_volumes: str | None = None
    # The global illumination pass left out in every mode.
    global_illumination: str | None = None
    # FSR, the foveation, the engine's exposure metering, the screen fade, and the player-damage
    # overlay, each at its shipped behaviour in every mode.
    fsr: str | None = None
    foveation: str | None = None
    auto_exposure: str | None = None
    screen_fade: str | None = None
    player_damage: str | None = None
    # FSR's stereo motion-vector correction left out in every mode.
    fsr_mv_correction: str | None = None
    # FSR's hybrid motion vectors left out in every mode.
    fsr_hybrid_velocity: str | None = None
    # A headset-like eye rig substituted for the located views in every mode, by preset name.
    headset: str | None = None
    # The render resolution scale (each side) in every mode, in place of the session's, to three
    # decimals.
    resolution_scale: float | None = None
    # The stage images every capture writes: "geometry" keeps only the depth, the albedo and normal
    # G-buffers, and the velocity, for light captures at a large eye size.
    stages: str | None = None

    def __post_init__(self):
        if self.sun_shadows is not None and not isinstance(self.sun_shadows, SunShadows):
            raise ProtocolError("visual protocol: invalid sun_shadows override")
        if self.screen_space_reflections not in (None, "enabled", "disabled"):
            raise ProtocolError("visual protocol: invalid screen_space_reflections override")
        for name in DISABLING_OVERRIDES:
            if getattr(self, name) not in (None, "disabled"):
                raise ProtocolError(f"visual protocol: invalid {name} override")
        for name in SHIPPED_OVERRIDES:
            if getattr(self, name) not in (None, "enabled"):
                raise ProtocolError(f"visual protocol: invalid {name} override")
        if self.headset is not None and self.headset not in HEADSET_RIGS:
            raise ProtocolError(f"visual protocol: unknown headset rig {self.headset!r}; expected one of "
                                f"{sorted(HEADSET_RIGS)}")
        if self.stages not in (None, "geometry"):
            raise ProtocolError("visual protocol: invalid stages override")
        if self.resolution_scale is not None:
            low, high = RESOLUTION_SCALE_RANGE
            if (type(self.resolution_scale) not in (int, float)
                    or not low <= self.resolution_scale <= high):
                raise ProtocolError(f"visual protocol: resolution_scale must be within [{low}, {high}]")
            object.__setattr__(self, "resolution_scale", round(float(self.resolution_scale), 3))
        object.__setattr__(self, "single_pass", single_pass_switches(dict(self.single_pass)))
        if (self.sun_shadows is None and self.screen_space_reflections is None and
                all(getattr(self, name) is None for name in (*DISABLING_OVERRIDES, *SHIPPED_OVERRIDES)) and
                self.headset is None and self.resolution_scale is None and self.stages is None
                and not self.single_pass):
            raise ProtocolError("visual protocol: visual overrides must name at least one override")

    @property
    def switches(self) -> dict[str, bool]:
        return dict(self.single_pass)

    def to_wire(self):
        """The table as the payload reports it: absent overrides are left out."""
        value = {}
        if self.sun_shadows is not None:
            value["sun_shadows"] = self.sun_shadows.value
        if self.screen_space_reflections is not None:
            value["screen_space_reflections"] = self.screen_space_reflections
        for name in (*DISABLING_OVERRIDES, *SHIPPED_OVERRIDES):
            if getattr(self, name) is not None:
                value[name] = getattr(self, name)
        if self.headset is not None:
            value["headset"] = self.headset
        if self.resolution_scale is not None:
            value["resolution_scale"] = self.resolution_scale
        if self.stages is not None:
            value["stages"] = self.stages
        if self.single_pass:
            value["single_pass"] = self.switches
        return value

    def to_toml(self):
        result = "[visual_overrides]\n"
        if self.sun_shadows is not None:
            result += f"sun_shadows = {json.dumps(self.sun_shadows.value)}\n"
        if self.screen_space_reflections is not None:
            result += f"screen_space_reflections = {json.dumps(self.screen_space_reflections)}\n"
        for name in (*DISABLING_OVERRIDES, *SHIPPED_OVERRIDES):
            if getattr(self, name) is not None:
                result += f"{name} = {json.dumps(getattr(self, name))}\n"
        if self.headset is not None:
            result += f"headset = {json.dumps(self.headset)}\n"
        if self.resolution_scale is not None:
            result += f"resolution_scale = {json.dumps(self.resolution_scale)}\n"
        if self.stages is not None:
            result += f"stages = {json.dumps(self.stages)}\n"
        if self.single_pass:
            result += "\n[visual_overrides.single_pass]\n" + "".join(
                f"{name} = {json.dumps(value)}\n" for name, value in self.single_pass)
        return result

    @classmethod
    def from_wire(cls, value):
        if not isinstance(value, dict) or not value or set(value) - {
                "sun_shadows", "screen_space_reflections", "single_pass", "headset", "resolution_scale",
                "stages", *DISABLING_OVERRIDES, *SHIPPED_OVERRIDES}:
            raise ProtocolError("visual protocol: invalid visual_overrides table")
        try:
            sun_shadows = SunShadows(value["sun_shadows"]) if "sun_shadows" in value else None
        except (ValueError, TypeError) as error:
            raise ProtocolError("visual protocol: invalid sun_shadows override") from error
        if "single_pass" in value and not value["single_pass"]:
            raise ProtocolError("visual protocol: the single_pass override table is empty")
        return cls(sun_shadows, value.get("screen_space_reflections"),
                   single_pass_switches(value.get("single_pass", {})),
                   **{name: value.get(name) for name in (*DISABLING_OVERRIDES, *SHIPPED_OVERRIDES)},
                   headset=value.get("headset"), resolution_scale=value.get("resolution_scale"),
                   stages=value.get("stages"))


@dataclass(frozen=True)
class ShaderBuild:
    """The state the live shaders were created under, as the payload last rebuilt them."""
    active: bool
    collapse: bool
    switches: tuple[tuple[str, bool], ...]

    @classmethod
    def from_wire(cls, value):
        if (not isinstance(value, dict) or set(value) != {"active", "collapse", "switches"} or
                type(value["active"]) is not bool or type(value["collapse"]) is not bool):
            raise ProtocolError("visual protocol: invalid shader build")
        return cls(value["active"], value["collapse"],
                   single_pass_switches(value["switches"], "shader_build.switches"))


@dataclass(frozen=True)
class Request:
    command: Command
    id: str
    mode: Mode | None = None
    warmup_frames: int | None = None
    pose: PoseRequest | None = None
    # The base the payload's full-camera freeze holds in place of the live camera it would capture.
    anchor: Camera | None = None
    motion: MotionRequest | None = None
    motion_capture: PhaseCapture | None = None
    environment: EnvironmentRequest | None = None
    aa_color_edge: bool | None = None
    patch63_hull: bool | None = None
    owner_four_domain: bool | None = None
    lake_color_mirror: bool | None = None
    lake_depth_edge: bool | None = None
    lake_ds_mvp: bool | None = None
    card_atlas: bool | None = None
    group_fade: bool | None = None
    group_cards: bool | None = None
    group_residual: bool | None = None
    aa_weights: bool | None = None
    aa_draws: bool | None = None
    lake_owner_four: bool | None = None
    lake_producer: bool | None = None
    lake_gradients: bool | None = None
    lake_waves: bool | None = None
    lake_normals: bool | None = None
    lake_displacement: bool | None = None
    lake_samplers: bool | None = None
    tess_writer: bool | None = None
    tess_owners: bool | None = None
    base_projection: bool | None = None
    base_sources: bool | None = None
    terrain_interval: bool | None = None
    group_inputs: bool | None = None
    group_sources: bool | None = None
    resolve_shadow: bool | None = None
    dof_inputs: bool | None = None
    fog_generation: bool | None = None
    fog_inputs: bool | None = None
    late_sky_stages: bool | None = None
    draw_census: bool | None = None
    water_surface_inputs: bool | None = None
    second_patch_ordinal: int | None = None
    patch_draw_ordinal: int | None = None
    water_tess_draw: int | None = None
    water_inputs: bool | None = None
    water_reflection: WaterReflection | None = None
    cascade_scale: CascadeScale | None = None
    history_center: bool | None = None
    grid_eye: bool | None = None
    veto_eye: bool | None = None
    march_eye: bool | None = None
    proxy_eye: bool | None = None
    resolve_eye_camera: bool | None = None
    ssr_prepare: bool | None = None
    ssr_history: bool | None = None
    lake_inner_edge: bool | None = None
    writer_domain_camera: bool | None = None
    base_eye_vp: bool | None = None
    group_atlas_scale: bool | None = None
    dof_edge_local: bool | None = None
    fog_local_uv: bool | None = None
    terrain_camera: bool | None = None
    extra_bark_dither: bool | None = None
    left_atlas_scale: bool | None = None
    impostor_scale: bool | None = None
    color_dither: bool | None = None
    bark_dither: bool | None = None
    terrain_sources: bool | None = None
    residual_bark: bool | None = None
    left_interval: bool | None = None
    impostor_inputs: bool | None = None
    interval_trace: bool | None = None
    post_geometry: bool | None = None
    bark_color_trace: bool | None = None
    velocity_trace: bool | None = None
    early_depth: bool | None = None
    ssr_operands: bool | None = None
    ssr_bracket: bool | None = None
    resolve_samples: bool | None = None
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
        if self.anchor is not None:
            if self.command != Command.CONFIGURE or not isinstance(self.anchor, Camera):
                raise ProtocolError("visual protocol: only configuration requests accept an anchor")
            finite_vector(self.anchor.position, 3, "anchor.position")
            finite_vector(self.anchor.orientation, 4, "anchor.orientation")
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
        if self.visual_overrides is not None and self.visual_overrides.single_pass and self.mode != Mode.SINGLE_PASS:
            raise ProtocolError("visual protocol: single-pass switch overrides require the single_pass mode")
        if self.aa_color_edge is not None and (type(self.aa_color_edge) is not bool or not self.aa_weights or self.patch63_hull is not True):
            raise ProtocolError("AA color edge requires weights and physical hull")
        if self.patch63_hull is not None and (type(self.patch63_hull) is not bool or self.owner_four_domain is not True or self.writer_domain_camera is not True):
            raise ProtocolError("patch 63 hull requires both domain treatments")
        if self.owner_four_domain is not None and (type(self.owner_four_domain) is not bool or not self.lake_owner_four or self.lake_color_mirror is not True):
            raise ProtocolError("owner four domain requires selected terrain and physical lake color")
        if self.lake_color_mirror is not None and (type(self.lake_color_mirror) is not bool or self.lake_depth_edge is not True):
            raise ProtocolError("lake color mirror requires bounded candidate depth")
        if self.aa_weights and (self.aa_weights is not True or not self.aa_draws):
            raise ProtocolError("AA weights require AA draws")
        if self.aa_draws and (self.aa_draws is not True or not self.dof_inputs):
            raise ProtocolError("AA draws require the post-process input capture")
        if self.lake_owner_four and (self.lake_owner_four is not True or not self.lake_producer or not self.tess_writer):
            raise ProtocolError("lake owner four requires selected terrain and water producers")
        if self.lake_depth_edge is not None and (type(self.lake_depth_edge) is not bool or self.lake_ds_mvp is not True or self.lake_waves or self.lake_normals):
            raise ProtocolError("lake depth edge requires physical MVP and displacement layout")
        if self.lake_ds_mvp is not None and (type(self.lake_ds_mvp) is not bool or not self.lake_producer or self.card_atlas is not True or self.water_reflection != WaterReflection.LAKE_HULL_CAMERA):
            raise ProtocolError("lake DS MVP requires lake producer, physical cards, and hull camera")
        if self.card_atlas is not None and (type(self.card_atlas) is not bool or self.group_fade is not True):
            raise ProtocolError("card atlas requires physical group fade")
        if self.group_fade is not None and (type(self.group_fade) is not bool or self.group_cards is not True or self.lake_inner_edge is not True):
            raise ProtocolError("group fade requires card inputs and the physical lake edge")
        if self.group_cards is not None and (self.group_cards is not True or self.group_residual is not True):
            raise ProtocolError("group cards require the residual trace")
        if self.group_residual is not None and (self.group_residual is not True or self.group_inputs is not True):
            raise ProtocolError("residual group trace requires group inputs")
        if self.lake_producer is not None and (self.lake_producer is not True or self.lake_displacement is not True):
            raise ProtocolError("lake producer requires selected displacement")
        if self.lake_gradients is not None and (self.lake_gradients is not True or self.lake_waves is not True):
            raise ProtocolError("lake gradients require the selected wave trace")
        if self.lake_waves is not None and (self.lake_waves is not True or self.lake_displacement is not True or self.lake_normals is not None):
            raise ProtocolError("lake waves require a dedicated selected response layout")
        if self.lake_normals is not None and (self.lake_normals is not True or self.lake_displacement is not True):
            raise ProtocolError("lake normals require the selected displacement trace")
        if self.lake_displacement is not None and (self.lake_displacement is not True or self.lake_samplers is not True or self.tess_writer is not True or self.water_reflection not in (WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_HULL_CAMERA)):
            raise ProtocolError("lake displacement requires the selected response trace and sampler evidence")
        if self.lake_samplers is not None and (self.lake_samplers is not True or self.water_surface_inputs is not True or self.water_tess_draw != 7):
            raise ProtocolError("lake samplers require surface inputs at lake draw 7")
        if self.tess_writer is not None and (self.tess_writer is not True or self.tess_owners is not True or self.terrain_sources is not True or self.terrain_camera is not None):
            raise ProtocolError("terrain writer requires terrain sources and owners without a camera treatment")
        if self.tess_owners is not None and (self.tess_owners is not True or self.terrain_interval is not True):
            raise ProtocolError("terrain owners require the bracketed terrain pass")
        if self.base_projection is not None and (self.base_projection is not True or self.base_sources is not True):
            raise ProtocolError("basemesh projection requires basemesh sources")
        if self.base_sources is not None and (self.base_sources is not True or self.terrain_interval is not True):
            raise ProtocolError("basemesh sources require the terrain interval")
        if self.terrain_interval is not None and (self.terrain_interval is not True or self.post_geometry is not True):
            raise ProtocolError("terrain interval requires the post-geometry boundaries")
        if self.group_inputs is not None and (self.group_inputs is not True or self.group_sources is not True):
            raise ProtocolError("group impostor inputs require group sources")
        if self.group_sources is not None and (self.group_sources is not True or self.left_interval is not True):
            raise ProtocolError("group sources require the interval trace")
        if self.resolve_shadow is not None and (self.resolve_shadow is not True or self.resolve_samples is not True):
            raise ProtocolError("resolve shadow operands require the sampled resolve")
        if self.dof_inputs is not None and (self.dof_inputs is not True or self.fog_generation is not True):
            raise ProtocolError("DoF inputs require the bracketed fog producer")
        if self.fog_generation is not None and (self.fog_generation is not True or self.fog_inputs is not True):
            raise ProtocolError("fog generation requires the composite input")
        if self.fog_inputs is not None and (self.fog_inputs is not True or self.late_sky_stages is not True):
            raise ProtocolError("fog inputs require the pass brackets")
        if self.late_sky_stages is not None and (self.command != Command.SCREENSHOT or self.motion_capture is not None or self.late_sky_stages is not True):
            raise ProtocolError("visual protocol: late_sky_stages requires a screenshot without motion")
        if self.draw_census is not None and (self.command != Command.SCREENSHOT or self.motion_capture is not None or self.draw_census is not True):
            raise ProtocolError("visual protocol: draw_census requires a screenshot without motion")
        if self.water_inputs is not None and (self.command != Command.SCREENSHOT or
                                              type(self.water_inputs) is not bool):
            raise ProtocolError("visual protocol: only screenshot requests accept a boolean water_inputs")
        if self.second_patch_ordinal is not None and (type(self.second_patch_ordinal) is not int or type(self.patch_draw_ordinal) is not int or not self.patch_draw_ordinal < self.second_patch_ordinal <= 255):
            raise ProtocolError("terrain diagnostic: secondary ordinal must follow a selected first ordinal")
        if self.patch_draw_ordinal is not None and (type(self.patch_draw_ordinal) is not int or not 1 <= self.patch_draw_ordinal <= 255 or self.water_reflection not in (WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA)):
            raise ProtocolError("visual protocol: invalid patch camera draw selection")
        if self.water_tess_draw is not None and (self.water_inputs is not True or type(self.water_tess_draw) is not int or not 0 <= self.water_tess_draw < 32):
            raise ProtocolError("visual protocol: water_tess_draw requires water_inputs and an index below 32")
        if self.water_surface_inputs is not None and (self.water_surface_inputs is not True or self.water_tess_draw is None or self.water_reflection not in (None, WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.LAKE_SAMPLES, WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER, WaterReflection.LAKE_RESPONSE, WaterReflection.LAKE_NEAR_PATCH, WaterReflection.LAKE_TESS_PATCH, WaterReflection.LAKE_PATCH_DRAW, WaterReflection.LAKE_PATCH_OWNERS, WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA) or self.motion_capture is not None):
            raise ProtocolError("visual protocol: surface inputs require a compatible selected water draw")
        if self.water_reflection in (WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.LAKE_SAMPLES, WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER, WaterReflection.LAKE_RESPONSE, WaterReflection.LAKE_NEAR_PATCH, WaterReflection.LAKE_TESS_PATCH, WaterReflection.LAKE_PATCH_DRAW, WaterReflection.LAKE_PATCH_OWNERS, WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA) and (self.water_tess_draw != 7 or not self.water_surface_inputs):
            raise ProtocolError("visual protocol: eye-width projection requires lake draw 7 with surface inputs")
        if self.water_reflection == WaterReflection.PLANAR_UV and self.water_tess_draw != 7:
            raise ProtocolError("visual protocol: planar_uv requires lake draw 7")
        if self.water_reflection in (WaterReflection.MIRRORED_PLANAR, WaterReflection.PROJECTED_PLANAR, WaterReflection.PRISTINE_LAKE, WaterReflection.CENTER_PLANAR, WaterReflection.ZERO_PLANAR, WaterReflection.PACKED_REFRACTION) and self.water_tess_draw is None:
            raise ProtocolError("visual protocol: lake treatment requires a selected draw")
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
        if self.lake_inner_edge is not None and (type(self.lake_inner_edge) is not bool or self.lake_samplers is not True or self.writer_domain_camera is not True):
            raise ProtocolError("inner edge requires lake sampler state and physical domain camera")
        if self.writer_domain_camera is not None and (type(self.writer_domain_camera) is not bool or self.tess_writer is not True or self.base_eye_vp is not True):
            raise ProtocolError("domain camera requires the terrain writer and physical basemesh VP")
        if self.base_eye_vp is not None and (type(self.base_eye_vp) is not bool or self.base_projection is not True or self.group_atlas_scale is not True):
            raise ProtocolError("basemesh VP requires verified projection and fixed group atlas")
        if self.group_atlas_scale is not None and (type(self.group_atlas_scale) is not bool or self.group_inputs is not True or self.dof_edge_local is not True or self.left_atlas_scale is not True):
            raise ProtocolError("group atlas trial requires draw operands and fixed local DoF and 3300 treatments")
        if self.dof_edge_local is not None and (type(self.dof_edge_local) is not bool or self.dof_inputs is not True):
            raise ProtocolError("DoF edge treatment requires composite inputs")
        if self.fog_local_uv is not None and (type(self.fog_local_uv) is not bool or self.fog_generation is not True):
            raise ProtocolError("fog reconstruction UV requires generator evidence")
        if self.terrain_camera is not None and (type(self.terrain_camera) is not bool or self.terrain_sources is not True or self.extra_bark_dither is not True):
            raise ProtocolError("terrain camera requires the terrain trace and coupled bark treatment")
        if self.extra_bark_dither is not None and (type(self.extra_bark_dither) is not bool or self.residual_bark is not True or self.left_atlas_scale is not True):
            raise ProtocolError("extra bark dither requires residual coordinates and both atlas treatments")
        if self.left_atlas_scale is not None and (type(self.left_atlas_scale) is not bool or self.left_interval is not True or self.impostor_scale is not True):
            raise ProtocolError("left atlas scale requires the left trace and right treatment")
        if self.impostor_scale is not None and (type(self.impostor_scale) is not bool or self.impostor_inputs is not True or self.color_dither is not True):
            raise ProtocolError("impostor scale requires the coupled control and card inputs")
        if self.color_dither is not None and (type(self.color_dither) is not bool or self.bark_color_trace is not True or self.bark_dither is not True):
            raise ProtocolError("color dither requires treated depth and a color trace")
        if self.bark_dither is not None and (type(self.bark_dither) is not bool or self.velocity_trace is not True):
            raise ProtocolError("bark dither requires velocity trace")
        if self.terrain_sources is not None and (self.terrain_sources is not True or self.early_depth is not True):
            raise ProtocolError("terrain sources require the early depth bracket")
        if self.residual_bark is not None and (self.residual_bark is not True or self.left_interval is not True):
            raise ProtocolError("residual bark requires left interval and coupled trace")
        if self.left_interval is not None and (self.left_interval is not True or self.impostor_inputs is not True):
            raise ProtocolError("left interval requires impostor inputs")
        if self.impostor_inputs is not None and (self.impostor_inputs is not True or self.interval_trace is not True):
            raise ProtocolError("impostor inputs require the interval trace")
        if self.interval_trace is not None and (self.interval_trace is not True or self.post_geometry is not True):
            raise ProtocolError("interval trace requires post-geometry boundaries")
        if self.post_geometry is not None and (self.post_geometry is not True or self.bark_color_trace is not True):
            raise ProtocolError("post-geometry bracket requires a bark color trace")
        if self.bark_color_trace is not None and (self.bark_color_trace is not True or self.velocity_trace is not True):
            raise ProtocolError("bark color trace requires velocity trace")
        if self.velocity_trace is not None and (self.velocity_trace is not True or self.early_depth is not True):
            raise ProtocolError("velocity trace requires early depth")
        if self.early_depth is not None and (self.early_depth is not True or self.ssr_history is not True or self.patch_draw_ordinal is None):
            raise ProtocolError("early depth requires history captures and a selected terrain draw")
        if self.ssr_history is not None and (self.ssr_history is not True or self.ssr_prepare is not True):
            raise ProtocolError("SSR history requires preparation captures")
        if self.ssr_prepare is not None and (self.ssr_prepare is not True or self.ssr_operands is not True):
            raise ProtocolError("SSR preparation requires ray operands")
        if self.ssr_operands is not None and (self.ssr_operands is not True or self.ssr_bracket is not True):
            raise ProtocolError("SSR operands require a bracket capture")
        if self.ssr_bracket is not None and (self.ssr_bracket is not True or self.resolve_samples is not True):
            raise ProtocolError("SSR bracket requires resolve samples")
        if self.history_center is not None and (type(self.history_center) is not bool or self.veto_eye is not True or self.ssr_history is not True):
            raise ProtocolError("history center requires packed veto and traced history")
        if self.grid_eye is not None and (type(self.grid_eye) is not bool or self.veto_eye is not True):
            raise ProtocolError("SSR grid trial requires packed veto")
        if self.veto_eye is not None and (type(self.veto_eye) is not bool or self.march_eye is not True):
            raise ProtocolError("SSR veto trial requires packed march")
        if self.march_eye is not None and (type(self.march_eye) is not bool or self.proxy_eye is not True):
            raise ProtocolError("march trial requires the physical proxy")
        if self.proxy_eye is not None and (type(self.proxy_eye) is not bool or self.ssr_prepare is not True or self.resolve_eye_camera is not True):
            raise ProtocolError("proxy eye requires the physical camera and preparation capture")
        if self.resolve_eye_camera is not None and (type(self.resolve_eye_camera) is not bool or self.resolve_samples is not True or self.water_reflection != WaterReflection.LAKE_HULL_CAMERA):
            raise ProtocolError("resolve camera requires lake hull camera and resolve samples")
        if self.resolve_samples is not None and (type(self.resolve_samples) is not bool or self.resolve_inputs is not True or self.cascade_scale is not None or self.motion_capture is not None):
            raise ProtocolError("visual protocol: resolve_samples requires an untimed resolve screenshot without a cascade override")
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
                         if key not in ("pose", "anchor", "motion", "motion_capture", "environment",
                                        "visual_overrides"))
        if self.pose is not None:
            result += "\n" + self.pose.to_toml()
        if self.anchor is not None:
            result += ("\n[anchor]\n"
                       f"position = {json.dumps(list(self.anchor.position))}\n"
                       f"orientation = {json.dumps(list(self.anchor.orientation))}\n")
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
    shader_build: ShaderBuild | None = None
    headset: HeadsetGeometry | None = None
    # The frames the configuration took to become ready, when the payload reports them.
    configure_frames: int | None = None

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
                   parse_shader_identity(value.get("shader_identity")),
                   None if value.get("shader_build") is None else ShaderBuild.from_wire(value["shader_build"]),
                   None if value.get("headset") is None else HeadsetGeometry.from_wire(value["headset"]),
                   None if value.get("configure_frames") is None
                   else integer(value, "configure_frames", minimum=0))



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
        self.validate_headset()
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


    def validate_headset(self):
        """Check that the capture was rendered under the headset rig its overrides name, or under the
        located views when they name none."""
        overrides = self.visual.visual_overrides
        requested = overrides.headset if overrides is not None else None
        if requested is None:
            if self.visual.headset is not None:
                raise ProtocolError("visual protocol: screenshot records a headset rig no override requested")
            return
        if self.visual.headset is None or not self.visual.headset.matches(HEADSET_RIGS[requested]):
            raise ProtocolError(f"visual protocol: screenshot was not rendered under the {requested} rig")
        self.visual.headset.validate_eyes(self.comparison_inputs.eyes, self.comparison_inputs.world_scale)


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
        for case, source in zip(value["cases"], self.cases):
            if case.get("expected_pose") is None:
                case.pop("expected_pose", None)
            if case.get("expected_motion") is None:
                case.pop("expected_motion", None)
            else:
                case["expected_motion"] = source.expected_motion.to_wire()
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


Acknowledgement: TypeAlias = (ConfigurationAck | RestorationAck | ScreenshotAck | StageAck | SnapshotAck
                              | ReleaseAck | BenchmarkAck | FailedAck)


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
                Command.END: AckOutcome.RESTORED, Command.STAGE: AckOutcome.STAGED,
                Command.SNAPSHOT: AckOutcome.COMPLETED,
                Command.SNAPSHOT_RELEASE: AckOutcome.RELEASED,
                Command.BENCHMARK: AckOutcome.COMPLETED}[command]
    if outcome != expected:
        raise ProtocolError("visual protocol: incomplete acknowledgement")
    if command == Command.CONFIGURE:
        return ConfigurationAck.from_wire(value)
    if command == Command.END:
        return RestorationAck.from_wire(value)
    if command == Command.STAGE:
        return StageAck.from_wire(value)
    if command == Command.SNAPSHOT:
        return SnapshotAck.from_wire(value)
    if command == Command.SNAPSHOT_RELEASE:
        return ReleaseAck.from_wire(value)
    if command == Command.BENCHMARK:
        return BenchmarkAck.from_wire(value)
    capture_type = NativeCapture if value.get("mode") in (None, DiagnosticProfile.NATIVE) else Capture
    return ScreenshotAck(request_id, capture_type.from_wire(value))
