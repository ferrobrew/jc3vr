"""Run a configured rendering-equivalence experiment against a live rendering session.

The experiment attaches to a running private rendering session, poses the camera at a configured world
transform, captures an ordered list of render configurations, and hands the resulting manifest to
the strict image comparator. An optional bright-region feature check records whether the target
configurations lose evidence that the reference configurations keep; it is reported separately and
can only make the overall outcome inconclusive, never pass.
"""

import argparse
import dataclasses
import json
import logging
import math
import os
import sys
import time
import tomllib
import uuid
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

import numpy as np

from tools.rendering.capture.suite import write_manifest
from tools.rendering.capture.validation import read_object, validate_capture
from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import (
    Outcome,
    SampleSpec,
    analyze_manifest,
    load_sample,
)
from tools.rendering.comparison.stages import Stage, load_stage
from tools.rendering.experiment_motion import (
    ExperimentMotion,
    MotionConfigError,
    validate_motion_capture,
)
from tools.rendering.experiment_fsr import FsrVariant, FsrVariantError, parse_fsr_variants
from tools.rendering.experiment_matched import (
    matched_capture_identity,
    matched_cross_metrics,
)
from tools.rendering.experiment_variants import (
    DEFAULT_VARIANT,
    SinglePassVariant,
    borrow_references,
    describe,
    parse_variants,
    require_reference_modes,
    summarize,
    variant_case_name,
    verify_switches,
)
from tools.rendering.protocol import (
    POSE_TOLERANCE,
    Camera,
    Capture,
    CascadeScale,
    Case,
    Command,
    ConfigurationAck,
    EnvironmentObservation,
    EnvironmentRequest,
    Manifest,
    Mode,
    OrientationOffset,
    PoseRequest,
    ProtocolError,
    Request,
    RestorationAck,
    Sample,
    ScreenshotAck,
    SunDepthClip,
    TranslationOffset,
    VisualOverrides,
    WaterReflection,
    StageAck,
    StageRequest,
    WeatherControl,
    WeatherEvent,
    composed_positions_match,
    offset_for_target,
    quaternions_match,
    valid_request_id,
)
from tools.rendering.session.attach import AttachedSession
from tools.rendering.session.errors import SessionError
from tools.rendering.session.stage_record import record as record_stage
from tools.rendering.session.stage_record import reuse_distance, stage_key

LOG = logging.getLogger("tools.rendering.experiment")

RESTORATION_BUDGET = 35.0
SPACING_POLL_SECONDS = 0.2
ENVIRONMENT_HOUR_TOLERANCE = 0.05
ENVIRONMENT_MAX_WEATHER_SEVERITY = 0.5
# The rain and snow events pin the storm severity at its top; anything well below it means the
# event did not hold.
ENVIRONMENT_MIN_STORM_SEVERITY = 3.5
MAX_NAME_LENGTH = 32


class ExperimentError(RuntimeError):
    pass


EXPECTED_ERRORS = (ExperimentError, ProtocolError, SessionError, OSError, ValueError,
                   TypeError)


class FeatureStatus(StrEnum):
    MATCH = "match"
    DIFFERENCE = "difference"
    INCONCLUSIVE = "inconclusive"


class FeatureRole(StrEnum):
    REFERENCE = "reference"
    TARGET = "target"


class FeatureEyeStatus(StrEnum):
    LOST = "lost"
    RETAINED = "retained"
    UNSTABLE = "unstable"
    UNOBSERVED = "unobserved"


@dataclass(frozen=True)
class WorldPose:
    position: tuple[float, float, float]
    orientation: tuple[float, float, float, float]

    def __post_init__(self):
        _vector(self.position, 3, "scene.position")
        _vector(self.orientation, 4, "scene.orientation")

    def camera(self) -> Camera:
        return Camera(self.position, self.orientation)


@dataclass(frozen=True)
class Environment:
    hour: float
    weather: WeatherEvent

    def __post_init__(self):
        EnvironmentRequest(self.hour, self.weather)

    def request(self) -> EnvironmentRequest:
        return EnvironmentRequest(self.hour, self.weather)

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "scene.environment")
        _keys(value, {"hour", "weather"}, set(), "scene.environment")
        try:
            weather = WeatherEvent(_string(value.get("weather"), "scene.environment.weather"))
        except ValueError as error:
            raise ExperimentError(
                "experiment: scene.environment.weather is not a supported weather event"
            ) from error
        return cls(_number(value.get("hour"), "scene.environment.hour"), weather)


@dataclass(frozen=True)
class Scene:
    # The camera's world pose; absent in a motion experiment, whose camera moves about the game's.
    pose: WorldPose | None
    environment: Environment | None
    # Staged before the first configuration: the player moved to a location and objects spawned,
    # with the world left to settle. Its request ID is replaced per run.
    stage: StageRequest | None = None
    # The camera a motion experiment's trajectory is about, pinned so that every run renders from
    # it to the bit; absent, the trajectory is about the game's camera as frozen at the anchor
    # configuration, which lands centimetres and degrees apart from run to run.
    anchor: WorldPose | None = None

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "scene")
        _keys(value, set(), {"position", "orientation", "environment", "stage", "anchor"}, "scene")
        pose = None
        if "position" in value or "orientation" in value:
            pose = WorldPose(_vector(value.get("position"), 3, "scene.position"),
                             _vector(value.get("orientation"), 4, "scene.orientation"))
        environment = (Environment.from_toml(value["environment"])
                       if "environment" in value else None)
        try:
            stage = (StageRequest.from_toml("stage", value["stage"])
                     if "stage" in value else None)
        except ProtocolError as error:
            raise ExperimentError(f"experiment: {error}") from error
        anchor = None
        if "anchor" in value:
            table = _table(value["anchor"], "scene.anchor")
            _keys(table, {"position", "orientation"}, set(), "scene.anchor")
            anchor = WorldPose(_vector(table.get("position"), 3, "scene.anchor.position"),
                               _vector(table.get("orientation"), 4, "scene.anchor.orientation"))
            if pose is not None:
                raise ExperimentError(
                    "experiment: scene.anchor is for a motion experiment; a fixed camera is "
                    "pinned by scene.position and scene.orientation")
        return cls(pose, environment, stage, anchor)


@dataclass(frozen=True)
class Feature:
    stage: Stage
    roi: tuple[float, float, float, float]
    luminance: float
    min_fraction: float
    reference_modes: tuple[CompareMode, ...]
    target_modes: tuple[CompareMode, ...]

    def __post_init__(self):
        if self.stage != Stage.MAIN_COLOR:
            raise ExperimentError(
                f"experiment: feature.stage {self.stage.value!r} is not supported"
            )
        x0, y0, x1, y1 = self.roi
        if not (0 <= x0 < x1 <= 1 and 0 <= y0 < y1 <= 1):
            raise ExperimentError(
                "experiment: feature.roi must be an increasing normalized rectangle"
            )
        if not math.isfinite(self.luminance) or self.luminance <= 0:
            raise ExperimentError(
                "experiment: feature.luminance must be finite and positive"
            )
        if not math.isfinite(self.min_fraction) or not 0 < self.min_fraction <= 1:
            raise ExperimentError(
                "experiment: feature.min_fraction must be within (0, 1]"
            )
        if not self.reference_modes or not self.target_modes:
            raise ExperimentError(
                "experiment: feature.reference_modes and feature.target_modes must be nonempty"
            )
        if set(self.reference_modes) & set(self.target_modes):
            raise ExperimentError(
                "experiment: feature reference and target modes must be disjoint"
            )

    def region(self, height, width):
        x0, y0, x1, y1 = self.roi
        columns = (max(0, int(x0 * width)), min(width, math.ceil(x1 * width)))
        rows = (max(0, int(y0 * height)), min(height, math.ceil(y1 * height)))
        if columns[1] <= columns[0] or rows[1] <= rows[0]:
            raise ExperimentError("experiment: feature.roi selects no pixels")
        return rows, columns

    def bright_fraction(self, values) -> float:
        if values.ndim != 3 or values.shape[2] < 3 or not values.size:
            raise ExperimentError("experiment: bright-region evidence is not an RGB image")
        luminance = (values[..., 0] * 0.2126 + values[..., 1] * 0.7152 +
                     values[..., 2] * 0.0722)
        (y0, y1), (x0, x1) = self.region(*luminance.shape[:2])
        return float(np.mean(luminance[y0:y1, x0:x1] >= self.luminance))

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "feature")
        _keys(value, {"stage", "roi", "luminance", "min_fraction", "reference_modes",
                      "target_modes"}, set(), "feature")
        try:
            stage = Stage(_string(value.get("stage"), "feature.stage"))
        except ValueError as error:
            raise ExperimentError("experiment: feature.stage is not a supported stage") from error
        return cls(stage,
                   tuple(_vector(value.get("roi"), 4, "feature.roi")),
                   _number(value.get("luminance"), "feature.luminance"),
                   _number(value.get("min_fraction"), "feature.min_fraction"),
                   _mode_list(value.get("reference_modes"), "feature.reference_modes"),
                   _mode_list(value.get("target_modes"), "feature.target_modes"))


@dataclass(frozen=True)
class Diagnostics:
    water_inputs: bool = False
    sun_inputs: bool = False
    shader_identity: bool = False
    resolve_inputs: bool = False
    sun_depth_clip: SunDepthClip | None = None
    water_reflection: WaterReflection | None = None
    water_tess_draw: int | None = None
    cascade_scale: CascadeScale | None = None
    water_surface_inputs: bool = False
    aa_color_edge: bool | None = None
    patch63_hull: bool | None = None
    owner_four_domain: bool | None = None
    lake_color_mirror: bool | None = None
    lake_depth_edge: bool | None = None
    lake_ds_mvp: bool | None = None
    card_atlas: bool | None = None
    group_fade: bool | None = None
    group_cards: bool = False
    group_residual: bool = False
    aa_weights: bool = False
    aa_draws: bool = False
    lake_owner_four: bool = False
    lake_producer: bool = False
    lake_gradients: bool = False
    lake_waves: bool = False
    lake_normals: bool = False
    lake_displacement: bool = False
    lake_samplers: bool = False
    tess_writer: bool = False
    tess_owners: bool = False
    base_projection: bool = False
    base_sources: bool = False
    terrain_interval: bool = False
    group_inputs: bool = False
    group_sources: bool = False
    resolve_shadow: bool = False
    dof_inputs: bool = False
    fog_generation: bool = False
    fog_inputs: bool = False
    late_sky_stages: bool = False
    draw_census: bool = False
    second_patch_ordinal: int | None = None
    patch_draw_ordinal: int | None = None
    ssr_prepare: bool = False
    ssr_history: bool = False
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
    terrain_sources: bool = False
    residual_bark: bool = False
    left_interval: bool = False
    impostor_inputs: bool = False
    interval_trace: bool = False
    post_geometry: bool = False
    bark_color_trace: bool = False
    velocity_trace: bool = False
    early_depth: bool = False
    ssr_operands: bool = False
    ssr_bracket: bool = False
    resolve_samples: bool = False
    history_center: bool | None = None
    grid_eye: bool | None = None
    veto_eye: bool | None = None
    march_eye: bool | None = None
    proxy_eye: bool | None = None
    resolve_eye_camera: bool | None = None

    def __post_init__(self):
        if self.second_patch_ordinal is not None and (type(self.second_patch_ordinal) is not int or type(self.patch_draw_ordinal) is not int or not self.patch_draw_ordinal < self.second_patch_ordinal <= 255):
            raise ExperimentError("terrain diagnostic: secondary ordinal must follow a selected first ordinal")
        if self.patch_draw_ordinal is not None and (type(self.patch_draw_ordinal) is not int or not 1 <= self.patch_draw_ordinal <= 255):
            raise ExperimentError("experiment: patch_draw_ordinal requires a positive 8-bit ordinal")
        if self.aa_color_edge is not None and (type(self.aa_color_edge) is not bool or not self.aa_weights or self.patch63_hull is not True):
            raise ExperimentError("AA color edge requires weights and physical hull")
        if self.patch63_hull is not None and (type(self.patch63_hull) is not bool or self.owner_four_domain is not True or self.writer_domain_camera is not True):
            raise ExperimentError("patch 63 hull requires both domain treatments")
        if self.owner_four_domain is not None and (type(self.owner_four_domain) is not bool or not self.lake_owner_four or self.lake_color_mirror is not True):
            raise ExperimentError("owner four domain requires selected terrain and physical lake color")
        if self.lake_color_mirror is not None and (type(self.lake_color_mirror) is not bool or self.lake_depth_edge is not True):
            raise ExperimentError("lake color mirror requires bounded candidate depth")
        if self.aa_weights and (self.aa_weights is not True or not self.aa_draws):
            raise ExperimentError("AA weights require AA draws")
        if self.aa_draws and (self.aa_draws is not True or not self.dof_inputs):
            raise ExperimentError("AA draws require the post-process input capture")
        if self.lake_owner_four and (self.lake_owner_four is not True or not self.lake_producer or not self.tess_writer):
            raise ExperimentError("lake owner four requires selected terrain and water producers")
        if self.lake_depth_edge is not None and (type(self.lake_depth_edge) is not bool or self.lake_ds_mvp is not True or self.lake_waves or self.lake_normals):
            raise ExperimentError("lake depth edge requires physical MVP and displacement layout")
        if self.lake_ds_mvp is not None and (type(self.lake_ds_mvp) is not bool or not self.lake_producer or self.card_atlas is not True or self.water_reflection != WaterReflection.LAKE_HULL_CAMERA):
            raise ExperimentError("lake DS MVP requires lake producer, physical cards, and hull camera")
        if self.card_atlas is not None and (type(self.card_atlas) is not bool or self.group_fade is not True):
            raise ExperimentError("card atlas requires physical group fade")
        if self.group_fade is not None and (type(self.group_fade) is not bool or not self.group_cards or self.lake_inner_edge is not True):
            raise ExperimentError("group fade requires card inputs and the physical lake edge")
        if type(self.group_cards) is not bool or (self.group_cards and not self.group_residual):
            raise ExperimentError("group cards require the residual trace")
        if type(self.group_residual) is not bool or (self.group_residual and not self.group_inputs):
            raise ExperimentError("residual group trace requires group inputs")
        if type(self.lake_producer) is not bool or (self.lake_producer and not self.lake_displacement):
            raise ExperimentError("lake producer requires selected displacement")
        if type(self.lake_gradients) is not bool or (self.lake_gradients and not self.lake_waves):
            raise ExperimentError("lake gradients require the selected wave trace")
        if type(self.lake_waves) is not bool or (self.lake_waves and (not self.lake_displacement or self.lake_normals)):
            raise ExperimentError("lake waves require a dedicated selected response layout")
        if type(self.lake_normals) is not bool or (self.lake_normals and not self.lake_displacement):
            raise ExperimentError("lake normals require the selected displacement trace")
        if type(self.lake_displacement) is not bool or (self.lake_displacement and not (self.lake_samplers and self.tess_writer)):
            raise ExperimentError("lake displacement requires the selected response trace and sampler evidence")
        if type(self.lake_samplers) is not bool or (self.lake_samplers and (not self.water_surface_inputs or self.water_tess_draw != 7)):
            raise ExperimentError("lake samplers require surface inputs at lake draw 7")
        if type(self.tess_writer) is not bool or (self.tess_writer and (not self.tess_owners or not self.terrain_sources or self.terrain_camera is not None)):
            raise ExperimentError("terrain writer requires terrain sources and owners without a camera treatment")
        if type(self.tess_owners) is not bool or (self.tess_owners and not self.terrain_interval):
            raise ExperimentError("terrain owners require the bracketed terrain pass")
        if type(self.base_projection) is not bool or (self.base_projection and not self.base_sources):
            raise ExperimentError("basemesh projection requires basemesh sources")
        if type(self.base_sources) is not bool or (self.base_sources and not self.terrain_interval):
            raise ExperimentError("basemesh sources require the terrain interval")
        if type(self.terrain_interval) is not bool or (self.terrain_interval and not self.post_geometry):
            raise ExperimentError("terrain interval requires the post-geometry boundaries")
        if type(self.group_inputs) is not bool or (self.group_inputs and not self.group_sources):
            raise ExperimentError("group impostor inputs require group sources")
        if type(self.group_sources) is not bool or (self.group_sources and not self.left_interval):
            raise ExperimentError("group depth sources require the interval trace")
        if type(self.resolve_shadow) is not bool or (self.resolve_shadow and not self.resolve_samples):
            raise ExperimentError("resolve shadow operands require the sampled resolve")
        if type(self.dof_inputs) is not bool or (self.dof_inputs and not self.fog_generation):
            raise ExperimentError("DoF inputs require the bracketed fog producer")
        if type(self.fog_generation) is not bool or (self.fog_generation and not self.fog_inputs):
            raise ExperimentError("fog generation requires the composite input")
        if type(self.fog_inputs) is not bool or (self.fog_inputs and not (self.late_sky_stages and self.shader_identity)):
            raise ExperimentError("fog inputs require the pass brackets and shader identity")
        if type(self.late_sky_stages) is not bool:
            raise ExperimentError("experiment: diagnostics.late_sky_stages must be a boolean")
        if type(self.draw_census) is not bool or (self.draw_census and not self.shader_identity):
            raise ExperimentError("experiment: diagnostics.draw_census must be a boolean and requires shader identity")
        if type(self.water_surface_inputs) is not bool or (self.water_surface_inputs and (self.water_tess_draw is None or not self.shader_identity or self.water_reflection not in (None, WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.LAKE_SAMPLES, WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER, WaterReflection.LAKE_RESPONSE, WaterReflection.LAKE_NEAR_PATCH, WaterReflection.LAKE_TESS_PATCH, WaterReflection.LAKE_PATCH_DRAW, WaterReflection.LAKE_PATCH_OWNERS, WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA))):
            raise ExperimentError("experiment: surface inputs require identity and a compatible selected water draw")
        if type(self.water_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.water_inputs must be a boolean")
        if self.water_tess_draw is not None and (not self.water_inputs or type(self.water_tess_draw) is not int or not 0 <= self.water_tess_draw < 32):
            raise ExperimentError("experiment: water_tess_draw requires water_inputs and an index below 32")
        if type(self.sun_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.sun_inputs must be a boolean")
        if type(self.shader_identity) is not bool:
            raise ExperimentError("experiment: diagnostics.shader_identity must be a boolean")
        if self.lake_inner_edge is not None and (type(self.lake_inner_edge) is not bool or not self.lake_samplers or self.writer_domain_camera is not True):
            raise ExperimentError("inner edge requires lake sampler state and physical domain camera")
        if self.writer_domain_camera is not None and (type(self.writer_domain_camera) is not bool or not self.tess_writer or self.base_eye_vp is not True):
            raise ExperimentError("domain camera requires the terrain writer and physical basemesh VP")
        if self.base_eye_vp is not None and (type(self.base_eye_vp) is not bool or not self.base_projection or self.group_atlas_scale is not True):
            raise ExperimentError("basemesh VP requires verified projection and fixed group atlas")
        if self.group_atlas_scale is not None and (type(self.group_atlas_scale) is not bool or not self.group_inputs or self.dof_edge_local is not True or self.left_atlas_scale is not True):
            raise ExperimentError("group atlas trial requires the corrected composite and earlier impostor draw")
        if self.dof_edge_local is not None and (type(self.dof_edge_local) is not bool or not self.dof_inputs):
            raise ExperimentError("DoF edge treatment requires composite inputs")
        if self.fog_local_uv is not None and (type(self.fog_local_uv) is not bool or not self.fog_generation):
            raise ExperimentError("fog reconstruction UV requires generator evidence")
        if self.terrain_camera is not None and (type(self.terrain_camera) is not bool or not self.terrain_sources or self.extra_bark_dither is not True):
            raise ExperimentError("terrain camera requires the terrain trace and coupled bark treatment")
        if self.extra_bark_dither is not None and (type(self.extra_bark_dither) is not bool or not self.residual_bark or self.left_atlas_scale is not True):
            raise ExperimentError("extra bark dither requires residual coordinates and both atlas treatments")
        if self.left_atlas_scale is not None and (type(self.left_atlas_scale) is not bool or not self.left_interval or self.impostor_scale is not True):
            raise ExperimentError("left atlas scale requires the left trace and right treatment")
        if self.impostor_scale is not None and (type(self.impostor_scale) is not bool or not self.impostor_inputs or self.color_dither is not True):
            raise ExperimentError("impostor scale requires the coupled control and card inputs")
        if self.color_dither is not None and (type(self.color_dither) is not bool or not self.bark_color_trace or self.bark_dither is not True):
            raise ExperimentError("color dither requires treated depth and a color trace")
        if self.bark_dither is not None and (type(self.bark_dither) is not bool or not self.velocity_trace):
            raise ExperimentError("bark dither requires velocity trace")
        if type(self.terrain_sources) is not bool or (self.terrain_sources and not self.early_depth):
            raise ExperimentError("terrain sources require the early depth bracket")
        if type(self.residual_bark) is not bool or (self.residual_bark and not self.left_interval):
            raise ExperimentError("residual bark requires left interval and coupled trace")
        if type(self.left_interval) is not bool or (self.left_interval and not self.impostor_inputs):
            raise ExperimentError("left interval requires impostor inputs")
        if type(self.impostor_inputs) is not bool or (self.impostor_inputs and not self.interval_trace):
            raise ExperimentError("impostor inputs require the interval trace")
        if type(self.interval_trace) is not bool or (self.interval_trace and not self.post_geometry):
            raise ExperimentError("interval trace requires post-geometry boundaries")
        if type(self.post_geometry) is not bool or (self.post_geometry and not self.bark_color_trace):
            raise ExperimentError("post-geometry bracket requires a bark color trace")
        if type(self.bark_color_trace) is not bool or (self.bark_color_trace and not self.velocity_trace):
            raise ExperimentError("bark color trace requires velocity trace")
        if type(self.velocity_trace) is not bool or (self.velocity_trace and not self.early_depth):
            raise ExperimentError("velocity trace requires early depth")
        if type(self.early_depth) is not bool or (self.early_depth and (not self.ssr_history or self.patch_draw_ordinal is None)):
            raise ExperimentError("early depth requires history captures and a selected terrain draw")
        if type(self.ssr_history) is not bool or (self.ssr_history and not self.ssr_prepare):
            raise ExperimentError("SSR history requires preparation captures")
        if type(self.ssr_prepare) is not bool or (self.ssr_prepare and not self.ssr_operands):
            raise ExperimentError("SSR preparation requires ray operands")
        if type(self.ssr_operands) is not bool or (self.ssr_operands and not self.ssr_bracket):
            raise ExperimentError("SSR operands require the SSR bracket")
        if type(self.ssr_bracket) is not bool or (self.ssr_bracket and not self.resolve_samples):
            raise ExperimentError("SSR bracket requires resolve samples")
        if self.history_center is not None and (type(self.history_center) is not bool or self.veto_eye is not True or self.ssr_history is not True):
            raise ExperimentError("history center requires packed veto and traced history")
        if self.grid_eye is not None and (type(self.grid_eye) is not bool or self.veto_eye is not True):
            raise ExperimentError("SSR grid trial requires packed veto")
        if self.veto_eye is not None and (type(self.veto_eye) is not bool or self.march_eye is not True):
            raise ExperimentError("SSR veto trial requires packed march")
        if self.march_eye is not None and (type(self.march_eye) is not bool or self.proxy_eye is not True):
            raise ExperimentError("march trial requires the physical proxy")
        if self.proxy_eye is not None and (type(self.proxy_eye) is not bool or not self.ssr_prepare or self.resolve_eye_camera is not True):
            raise ExperimentError("proxy eye requires a physical camera and preparation capture")
        if self.resolve_eye_camera is not None and (type(self.resolve_eye_camera) is not bool or not self.resolve_samples or self.water_reflection != WaterReflection.LAKE_HULL_CAMERA):
            raise ExperimentError("resolve camera requires a selected lake hull camera capture with resolve samples")
        if type(self.resolve_samples) is not bool or (self.resolve_samples and (not self.resolve_inputs or not self.shader_identity or self.cascade_scale is not None)):
            raise ExperimentError("experiment: resolve_samples requires resolve inputs and shader identity without a cascade override")
        if type(self.resolve_inputs) is not bool:
            raise ExperimentError("experiment: diagnostics.resolve_inputs must be a boolean")
        if self.sun_depth_clip is not None and (not isinstance(self.sun_depth_clip, SunDepthClip) or not self.sun_inputs):
            raise ExperimentError("experiment: diagnostics.sun_depth_clip requires sun_inputs")
        if self.water_reflection is not None and (not isinstance(self.water_reflection, WaterReflection) or
                                                   not self.water_inputs or not self.shader_identity):
            raise ExperimentError("experiment: diagnostics.water_reflection requires water_inputs and shader_identity")
        if self.cascade_scale is not None and (not isinstance(self.cascade_scale, CascadeScale) or
                not self.resolve_inputs or not self.shader_identity):
            raise ExperimentError("experiment: cascade_scale requires resolve inputs and shader identity")
        if self.sun_depth_clip is not None and self.shader_identity:
            raise ExperimentError("experiment: sun depth clip treatment and shader census must use separate captures")

    @classmethod
    def from_toml(cls, value):
        value = _table(value, "diagnostics")
        _keys(value, set(), {"terrain_sources", "residual_bark", "left_interval", "impostor_inputs", "interval_trace", "post_geometry", "bark_color_trace", "velocity_trace", "early_depth", "ssr_history", "ssr_prepare", "ssr_operands", "ssr_bracket", "resolve_samples", "second_patch_ordinal", "patch_draw_ordinal", "group_cards", "group_residual", "aa_weights", "aa_draws", "lake_owner_four", "lake_producer", "lake_gradients", "lake_waves", "lake_normals", "lake_displacement", "lake_samplers", "tess_writer", "tess_owners", "base_projection", "base_sources", "terrain_interval", "group_inputs", "group_sources", "resolve_shadow", "dof_inputs", "fog_generation", "fog_inputs", "late_sky_stages", "draw_census", "water_inputs", "water_surface_inputs", "water_tess_draw", "sun_inputs", "shader_identity", "resolve_inputs", "sun_depth_clip", "water_reflection"}, "diagnostics")
        try:
            depth_clip = SunDepthClip(value["sun_depth_clip"]) if "sun_depth_clip" in value else None
            water_reflection = WaterReflection(value["water_reflection"]) if "water_reflection" in value else None
        except (ValueError, TypeError) as error:
            raise ExperimentError("experiment: invalid diagnostic treatment") from error
        return cls(value.get("water_inputs", False), value.get("sun_inputs", False),
                   value.get("shader_identity", False), value.get("resolve_inputs", False), depth_clip, water_reflection, value.get("water_tess_draw"), water_surface_inputs=value.get("water_surface_inputs", False),
                   terrain_sources=value.get("terrain_sources", False), residual_bark=value.get("residual_bark", False), left_interval=value.get("left_interval", False), impostor_inputs=value.get("impostor_inputs", False), interval_trace=value.get("interval_trace", False), post_geometry=value.get("post_geometry", False), bark_color_trace=value.get("bark_color_trace", False), velocity_trace=value.get("velocity_trace", False), early_depth=value.get("early_depth", False), ssr_history=value.get("ssr_history", False), ssr_prepare=value.get("ssr_prepare", False), ssr_operands=value.get("ssr_operands", False), ssr_bracket=value.get("ssr_bracket",False), resolve_samples=value.get("resolve_samples", False), group_cards=value.get("group_cards", False), group_residual=value.get("group_residual", False), aa_weights=value.get("aa_weights",False), aa_draws=value.get("aa_draws",False), lake_owner_four=value.get("lake_owner_four",False), lake_producer=value.get("lake_producer", False), lake_gradients=value.get("lake_gradients", False), lake_waves=value.get("lake_waves", False), lake_normals=value.get("lake_normals", False), lake_displacement=value.get("lake_displacement", False), lake_samplers=value.get("lake_samplers", False), tess_writer=value.get("tess_writer", False), tess_owners=value.get("tess_owners", False), base_projection=value.get("base_projection", False), base_sources=value.get("base_sources", False), terrain_interval=value.get("terrain_interval", False), group_inputs=value.get("group_inputs", False), group_sources=value.get("group_sources", False), resolve_shadow=value.get("resolve_shadow", False), dof_inputs=value.get("dof_inputs", False), fog_generation=value.get("fog_generation", False), fog_inputs=value.get("fog_inputs", False), late_sky_stages=value.get("late_sky_stages", False), draw_census=value.get("draw_census", False), patch_draw_ordinal=value.get("patch_draw_ordinal"), second_patch_ordinal=value.get("second_patch_ordinal"))


class MatchedTreatment(StrEnum):
    WATER_REFLECTION_ABBA = "water_reflection_abba"
    PLANAR_ABBA = "planar_abba"
    REFRACTION_ABBA = "refraction_abba"
    AA_COLOR_EDGE_ABBA = "aa_color_edge_abba"
    PATCH63_HULL_ABBA = "patch63_hull_abba"
    OWNER_FOUR_DOMAIN_ABBA = "owner_four_domain_abba"
    LAKE_COLOR_MIRROR_ABBA = "lake_color_mirror_abba"
    LAKE_DEPTH_EDGE_ABBA = "lake_depth_edge_abba"
    LAKE_DS_MVP_ABBA = "lake_ds_mvp_abba"
    CARD_ATLAS_ABBA = "card_atlas_abba"
    GROUP_FADE_ABBA = "group_fade_abba"
    INNER_EDGE_ABBA = "inner_edge_abba"
    WRITER_DS_ABBA = "writer_ds_abba"
    BASE_VP_ABBA = "base_vp_abba"
    GROUP_ATLAS_ABBA = "group_atlas_abba"
    DOF_EDGE_ABBA = "dof_edge_abba"
    FOG_UV_ABBA = "fog_uv_abba"
    TERRAIN_CAMERA_ABBA = "terrain_camera_abba"
    EXTRA_BARK_ABBA = "extra_bark_abba"
    LEFT_SCALE_ABBA = "left_scale_abba"
    IMPOSTOR_SCALE_ABBA = "impostor_scale_abba"
    COLOR_DITHER_ABBA = "color_dither_abba"
    BARK_DITHER_ABBA = "bark_dither_abba"
    HISTORY_CENTER_ABBA = "history_center_abba"
    GRID_EYE_ABBA = "grid_eye_abba"
    VETO_EYE_ABBA = "veto_eye_abba"
    MARCH_EYE_ABBA = "march_eye_abba"
    PROXY_EYE_ABBA = "proxy_eye_abba"
    RESOLVE_CAMERA_ABBA = "resolve_camera_abba"
    LAKE_HULL_CAMERA_ABBA = "lake_hull_camera_abba"
    LAKE_DOMAIN_CAMERA_ABBA = "lake_domain_camera_abba"
    LAKE_SKY_BUFFER_ABBA = "lake_sky_buffer_abba"
    LAKE_SKY_ABBA = "lake_sky_abba"
    EYE_WIDTH_ABBA = "eye_width_abba"
    MIRRORED_PLANAR_ABBA = "mirrored_planar_abba"
    PROJECTED_PLANAR_ABBA = "projected_planar_abba"
    PRODUCTION_REFRACTION_ABBA = "production_refraction_abba"
    ZERO_PLANAR_ABBA = "zero_planar_abba"
    CASCADE_ABBA = "cascade_abba"

    @property
    def conditions(self) -> tuple[str, str]:
        return {
            self.CASCADE_ABBA: (CascadeScale.SHORE_LOW.value, CascadeScale.SHORE_HIGH.value),
            self.AA_COLOR_EDGE_ABBA: ("aa_native", "aa_local"),
            self.PATCH63_HULL_ABBA: ("hull_native", "hull_physical"),
            self.OWNER_FOUR_DOMAIN_ABBA: ("domain_native", "domain_physical"),
            self.LAKE_COLOR_MIRROR_ABBA: ("color_native", "color_mirror"),
            self.LAKE_DEPTH_EDGE_ABBA: ("depth_native", "depth_bounded"),
            self.LAKE_DS_MVP_ABBA: ("mvp_native", "mvp_physical"),
            self.CARD_ATLAS_ABBA: ("card_native", "card_physical"),
            self.GROUP_FADE_ABBA: ("fade_native", "fade_physical"),
            self.INNER_EDGE_ABBA: ("inner_native", "inner_physical"),
            self.WRITER_DS_ABBA: ("writer_center", "writer_domain"),
            self.BASE_VP_ABBA: ("base_center", "base_eye"),
            self.GROUP_ATLAS_ABBA: ("group_native", "group_physical"),
            self.DOF_EDGE_ABBA: ("edge_packed", "edge_local"),
            self.FOG_UV_ABBA: ("fog_packed", "fog_local"),
            self.TERRAIN_CAMERA_ABBA: ("terrain_center", "terrain_physical"),
            self.EXTRA_BARK_ABBA: ("extra_packed", "extra_physical"),
            self.LEFT_SCALE_ABBA: ("atlas_right", "atlas_both"),
            self.IMPOSTOR_SCALE_ABBA: ("impostor_native", "impostor_physical"),
            self.COLOR_DITHER_ABBA: ("color_packed", "color_physical"),
            self.BARK_DITHER_ABBA: ("dither_packed", "dither_physical"),
            self.HISTORY_CENTER_ABBA: ("history_native", "history_physical"),
            self.GRID_EYE_ABBA: ("grid_native", "grid_physical"),
            self.VETO_EYE_ABBA: ("veto_native", "veto_packed"),
            self.MARCH_EYE_ABBA: ("march_native", "march_packed"),
            self.PROXY_EYE_ABBA: ("proxy_native", "proxy_physical_eye"),
            self.RESOLVE_CAMERA_ABBA: ("resolve_center_clone", "resolve_physical_camera"),
            self.LAKE_HULL_CAMERA_ABBA: (WaterReflection.LAKE_DOMAIN_CAMERA.value, WaterReflection.LAKE_HULL_CAMERA.value),
            self.LAKE_DOMAIN_CAMERA_ABBA: (WaterReflection.LAKE_CAMERA_CONTROL.value, WaterReflection.LAKE_DOMAIN_CAMERA.value),
            self.LAKE_SKY_BUFFER_ABBA: (WaterReflection.LAKE_SKY.value, WaterReflection.LAKE_SKY_BUFFER.value),
            self.LAKE_SKY_ABBA: (WaterReflection.LAKE_LIGHTING.value, WaterReflection.LAKE_SKY.value),
            self.EYE_WIDTH_ABBA: ("production_refraction", WaterReflection.EYE_WIDTH_PLANAR.value),
            self.MIRRORED_PLANAR_ABBA: (WaterReflection.PROJECTED_PLANAR.value, WaterReflection.MIRRORED_PLANAR.value),
            self.PROJECTED_PLANAR_ABBA: ("production_refraction", WaterReflection.PROJECTED_PLANAR.value),
            self.PRODUCTION_REFRACTION_ABBA: ("control", "production_refraction"),
            self.REFRACTION_ABBA: ("control", WaterReflection.PACKED_REFRACTION.value),
            self.ZERO_PLANAR_ABBA: ("control", WaterReflection.ZERO_PLANAR.value),
            self.PLANAR_ABBA: ("control", WaterReflection.CENTER_PLANAR.value),
            self.WATER_REFLECTION_ABBA: ("control", WaterReflection.ZERO_WEIGHT.value),
        }[self]


@dataclass(frozen=True)
class ExperimentConfig:
    name: str
    warmup_frames: int
    samples: int
    scene: Scene
    configurations: tuple[CompareMode, ...]
    feature: Feature | None
    interval_seconds: float = 0.0
    diagnostics: Diagnostics = dataclasses.field(default_factory=Diagnostics)
    visual_overrides: VisualOverrides | None = None
    matched_treatment: MatchedTreatment | None = None
    single_pass_variants: tuple[SinglePassVariant, ...] = ()
    motion: ExperimentMotion | None = None
    # FSR's settings varied in the same schedule; only the fast comparison runs them.
    fsr_variants: tuple[FsrVariant, ...] = ()

    def __post_init__(self):
        if (self.motion is None) == (self.scene.pose is None):
            raise ExperimentError(
                "experiment: a config needs exactly one of scene.position/orientation (a fixed "
                "camera) and [motion] (a trajectory about the game's frozen camera)")
        if self.motion is not None:
            if self.feature is not None or self.matched_treatment is not None:
                raise ExperimentError(
                    "experiment: a motion experiment excludes a feature check and a matched treatment")
            if self.diagnostics not in (Diagnostics(), Diagnostics(shader_identity=True)):
                raise ExperimentError(
                    "experiment: a motion experiment's captures take no diagnostics beyond the "
                    "shader identity census")
        if not valid_request_id(self.name) or len(self.name) > MAX_NAME_LENGTH:
            raise ExperimentError(
                "experiment: config.name must be an ASCII request ID of at most "
                f"{MAX_NAME_LENGTH} characters"
            )
        if type(self.warmup_frames) is not int or not 1 <= self.warmup_frames <= 300:
            raise ExperimentError(
                "experiment: config.warmup_frames must be an integer from 1 through 300"
            )
        if type(self.samples) is not int or self.samples < 3:
            raise ExperimentError("experiment: config.samples must be an integer of at least three")
        if not self.configurations:
            raise ExperimentError("experiment: config.configurations must not be empty")
        if (type(self.interval_seconds) not in (int, float)
                or not math.isfinite(self.interval_seconds) or self.interval_seconds < 0):
            raise ExperimentError(
                "experiment: config.interval_seconds must be a finite nonnegative number"
            )
        object.__setattr__(self, "interval_seconds", float(self.interval_seconds))
        if not isinstance(self.diagnostics, Diagnostics):
            raise ExperimentError("experiment: config.diagnostics must be a diagnostics table")
        if (self.matched_treatment in (MatchedTreatment.EYE_WIDTH_ABBA, MatchedTreatment.LAKE_SKY_ABBA, MatchedTreatment.LAKE_SKY_BUFFER_ABBA, MatchedTreatment.LAKE_DOMAIN_CAMERA_ABBA, MatchedTreatment.LAKE_HULL_CAMERA_ABBA, MatchedTreatment.RESOLVE_CAMERA_ABBA, MatchedTreatment.PROXY_EYE_ABBA, MatchedTreatment.MARCH_EYE_ABBA, MatchedTreatment.VETO_EYE_ABBA, MatchedTreatment.GRID_EYE_ABBA, MatchedTreatment.HISTORY_CENTER_ABBA, MatchedTreatment.BARK_DITHER_ABBA, MatchedTreatment.COLOR_DITHER_ABBA, MatchedTreatment.IMPOSTOR_SCALE_ABBA, MatchedTreatment.LEFT_SCALE_ABBA, MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) or self.diagnostics.water_reflection in (WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.LAKE_SAMPLES, WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER, WaterReflection.LAKE_RESPONSE, WaterReflection.LAKE_NEAR_PATCH, WaterReflection.LAKE_TESS_PATCH, WaterReflection.LAKE_PATCH_DRAW, WaterReflection.LAKE_PATCH_OWNERS, WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA)) and (not self.diagnostics.water_surface_inputs or self.diagnostics.water_tess_draw != 7):
            raise ExperimentError("experiment: eye-width projection requires lake draw 7 with surface inputs")
        if self.matched_treatment in (MatchedTreatment.PROXY_EYE_ABBA, MatchedTreatment.MARCH_EYE_ABBA, MatchedTreatment.VETO_EYE_ABBA, MatchedTreatment.GRID_EYE_ABBA, MatchedTreatment.HISTORY_CENTER_ABBA, MatchedTreatment.BARK_DITHER_ABBA, MatchedTreatment.COLOR_DITHER_ABBA, MatchedTreatment.IMPOSTOR_SCALE_ABBA, MatchedTreatment.LEFT_SCALE_ABBA, MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not self.diagnostics.ssr_prepare:
            raise ExperimentError("proxy trial requires preparation captures")
        if self.matched_treatment in (MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not self.diagnostics.lake_producer:
            raise ExperimentError("lake DS MVP requires measured lake producer")
        if self.matched_treatment in (MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not self.diagnostics.group_cards:
            raise ExperimentError("matched group fade requires measured cards")
        if self.matched_treatment in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.INNER_EDGE_ABBA) and not self.diagnostics.lake_samplers:
            raise ExperimentError("matched inner edge requires selected lake sampler state")
        if self.matched_treatment in (MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not self.diagnostics.tess_writer:
            raise ExperimentError("matched domain camera requires a measured terrain writer")
        if self.matched_treatment in (MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not (self.diagnostics.base_projection and self.diagnostics.group_inputs and self.diagnostics.dof_inputs):
            raise ExperimentError("matched basemesh VP requires projection, atlas, and DoF evidence")
        if self.matched_treatment == MatchedTreatment.GROUP_ATLAS_ABBA and not (self.diagnostics.group_inputs and self.diagnostics.dof_inputs):
            raise ExperimentError("matched group atlas requires the draw operands and DoF inputs")
        if self.matched_treatment == MatchedTreatment.DOF_EDGE_ABBA and not self.diagnostics.dof_inputs:
            raise ExperimentError("matched DoF edge treatment requires composite inputs")
        if self.matched_treatment == MatchedTreatment.FOG_UV_ABBA and not self.diagnostics.fog_generation:
            raise ExperimentError("fog coordinate trial requires generation capture")
        if self.matched_treatment == MatchedTreatment.TERRAIN_CAMERA_ABBA and not self.diagnostics.terrain_sources:
            raise ExperimentError("terrain camera requires the terrain writer trace")
        if self.matched_treatment in (MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and not self.diagnostics.residual_bark:
            raise ExperimentError("extra bark dither requires residual coordinates")
        if self.matched_treatment == MatchedTreatment.LEFT_SCALE_ABBA and not self.diagnostics.left_interval:
            raise ExperimentError("left atlas scale requires the left trace")
        if self.matched_treatment == MatchedTreatment.IMPOSTOR_SCALE_ABBA and not self.diagnostics.impostor_inputs:
            raise ExperimentError("impostor scale requires the selected draw input trace")
        if self.matched_treatment == MatchedTreatment.COLOR_DITHER_ABBA and not self.diagnostics.bark_color_trace:
            raise ExperimentError("color dither requires a color trace")
        if self.matched_treatment == MatchedTreatment.BARK_DITHER_ABBA and not self.diagnostics.velocity_trace:
            raise ExperimentError("bark dither requires velocity trace")
        if self.matched_treatment == MatchedTreatment.HISTORY_CENTER_ABBA and not self.diagnostics.ssr_history:
            raise ExperimentError("history center trial requires the previous-scene trace")
        if self.matched_treatment == MatchedTreatment.RESOLVE_CAMERA_ABBA and not self.diagnostics.resolve_samples:
            raise ExperimentError("resolve camera ABBA requires sampled operands")
        if self.matched_treatment is not None:
            if (not isinstance(self.matched_treatment, MatchedTreatment) or self.feature is not None or
                    not self.diagnostics.shader_identity or self.diagnostics.water_reflection is not None or
                    self.diagnostics.cascade_scale is not None or self.diagnostics.sun_depth_clip is not None):
                raise ExperimentError("experiment: matched conditions require shader identity and no other treatment")
            if self.matched_treatment in (MatchedTreatment.PLANAR_ABBA, MatchedTreatment.REFRACTION_ABBA, MatchedTreatment.PRODUCTION_REFRACTION_ABBA, MatchedTreatment.PROJECTED_PLANAR_ABBA, MatchedTreatment.MIRRORED_PLANAR_ABBA, MatchedTreatment.EYE_WIDTH_ABBA, MatchedTreatment.LAKE_SKY_ABBA, MatchedTreatment.LAKE_SKY_BUFFER_ABBA, MatchedTreatment.LAKE_DOMAIN_CAMERA_ABBA, MatchedTreatment.LAKE_HULL_CAMERA_ABBA, MatchedTreatment.RESOLVE_CAMERA_ABBA, MatchedTreatment.PROXY_EYE_ABBA, MatchedTreatment.MARCH_EYE_ABBA, MatchedTreatment.VETO_EYE_ABBA, MatchedTreatment.GRID_EYE_ABBA, MatchedTreatment.HISTORY_CENTER_ABBA, MatchedTreatment.BARK_DITHER_ABBA, MatchedTreatment.COLOR_DITHER_ABBA, MatchedTreatment.IMPOSTOR_SCALE_ABBA, MatchedTreatment.LEFT_SCALE_ABBA, MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA):
                if self.configurations not in ((CompareMode.SINGLE_PASS,), (CompareMode.LEFT_ONLY,
                        CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS, CompareMode.SINGLE_PASS)):
                    raise ExperimentError("experiment: planar ABBA requires SPS, optionally after both isolated eyes and MP")
            elif self.matched_treatment == MatchedTreatment.ZERO_PLANAR_ABBA:
                if self.configurations != (CompareMode.LEFT_ONLY, CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS, CompareMode.SINGLE_PASS) or self.diagnostics.water_tess_draw is None:
                    raise ExperimentError("experiment: zero planar ABBA requires both references, MP, SPS, and a selected lake draw")
            elif self.configurations != (CompareMode.MULTIPASS,):
                raise ExperimentError("experiment: matched MP treatments require one multipass configuration")
            if self.matched_treatment == MatchedTreatment.CASCADE_ABBA:
                if not self.diagnostics.resolve_inputs:
                    raise ExperimentError("experiment: cascade ABBA requires resolve inputs")
            elif not self.diagnostics.water_inputs:
                raise ExperimentError("experiment: matched water treatments require water inputs")
        if self.diagnostics.cascade_scale is not None and self.configurations != (CompareMode.MULTIPASS,):
            raise ExperimentError("experiment: cascade_scale requires one multipass configuration")
        if ((self.diagnostics.water_reflection in (WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.MIRRORED_PLANAR, WaterReflection.PROJECTED_PLANAR, WaterReflection.PRISTINE_LAKE, WaterReflection.CENTER_PLANAR, WaterReflection.PACKED_REFRACTION) and self.configurations != (CompareMode.SINGLE_PASS,)) or
                ((self.diagnostics.water_reflection in (WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.MIRRORED_PLANAR, WaterReflection.PROJECTED_PLANAR, WaterReflection.PRISTINE_LAKE, WaterReflection.CENTER_PLANAR, WaterReflection.PACKED_REFRACTION) or self.matched_treatment in (MatchedTreatment.PLANAR_ABBA, MatchedTreatment.REFRACTION_ABBA, MatchedTreatment.PRODUCTION_REFRACTION_ABBA, MatchedTreatment.PROJECTED_PLANAR_ABBA, MatchedTreatment.MIRRORED_PLANAR_ABBA, MatchedTreatment.EYE_WIDTH_ABBA, MatchedTreatment.LAKE_SKY_ABBA, MatchedTreatment.LAKE_SKY_BUFFER_ABBA, MatchedTreatment.LAKE_DOMAIN_CAMERA_ABBA, MatchedTreatment.LAKE_HULL_CAMERA_ABBA, MatchedTreatment.RESOLVE_CAMERA_ABBA, MatchedTreatment.PROXY_EYE_ABBA, MatchedTreatment.MARCH_EYE_ABBA, MatchedTreatment.VETO_EYE_ABBA, MatchedTreatment.GRID_EYE_ABBA, MatchedTreatment.HISTORY_CENTER_ABBA, MatchedTreatment.BARK_DITHER_ABBA, MatchedTreatment.COLOR_DITHER_ABBA, MatchedTreatment.IMPOSTOR_SCALE_ABBA, MatchedTreatment.LEFT_SCALE_ABBA, MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA)) and self.diagnostics.water_tess_draw is None)):
            raise ExperimentError("experiment: planar treatment requires a selected single-pass draw")
        if self.diagnostics.water_reflection in (WaterReflection.PLANAR_UV,WaterReflection.LAKE_SAMPLES, WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER, WaterReflection.LAKE_RESPONSE, WaterReflection.LAKE_NEAR_PATCH, WaterReflection.LAKE_TESS_PATCH, WaterReflection.LAKE_PATCH_DRAW, WaterReflection.LAKE_PATCH_OWNERS, WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA) and (self.diagnostics.water_tess_draw != 7 or CompareMode.REVERSE_MULTIPASS in self.configurations):
            raise ExperimentError("experiment: planar_uv requires lake draw 7 and a supported mode")
        if self.diagnostics.water_reflection in (WaterReflection.LAKE_DOMAIN_CAMERA, WaterReflection.LAKE_HULL_CAMERA) and self.configurations != (CompareMode.SINGLE_PASS,):
            raise ExperimentError("experiment: domain camera treatment requires single-pass")
        if self.diagnostics.water_reflection == WaterReflection.ZERO_PLANAR and (self.diagnostics.water_tess_draw is None or CompareMode.REVERSE_MULTIPASS in self.configurations):
            raise ExperimentError("experiment: zero_planar requires a selected lake draw and a supported mode")
        if self.diagnostics.water_reflection == WaterReflection.ZERO_WEIGHT and any(mode != CompareMode.MULTIPASS for mode in self.configurations):
            raise ExperimentError("experiment: water reflection treatment requires multipass configurations")
        if self.visual_overrides is not None and not isinstance(self.visual_overrides, VisualOverrides):
            raise ExperimentError("experiment: invalid visual overrides")
        if self.single_pass_variants:
            if (not isinstance(self.single_pass_variants, tuple) or
                    not all(isinstance(item, SinglePassVariant) for item in self.single_pass_variants)):
                raise ExperimentError("experiment: invalid single-pass variants")
            if self.matched_treatment is not None or self.feature is not None:
                raise ExperimentError(
                    "experiment: single-pass variants exclude a matched treatment and a feature check")
            require_reference_modes(self.configurations)
        if self.fsr_variants:
            if (not isinstance(self.fsr_variants, tuple) or
                    not all(isinstance(item, FsrVariant) for item in self.fsr_variants)):
                raise ExperimentError("experiment: invalid FSR variants")
            if self.matched_treatment is not None or self.feature is not None:
                raise ExperimentError(
                    "experiment: FSR variants exclude a matched treatment and a feature check")
            try:
                for variant in self.fsr_variants:
                    variant.modes_in(self.configurations)
            except FsrVariantError as error:
                raise ExperimentError(f"experiment: {error}") from error
        if self.feature is None:
            return
        references = set(self.feature.reference_modes)
        targets = set(self.feature.target_modes)
        missing = (references | targets) - set(self.configurations)
        if missing:
            names = ", ".join(sorted(mode.value for mode in missing))
            raise ExperimentError(
                f"experiment: feature modes {names} are not configured"
            )
        last_reference = max(
            index for index, mode in enumerate(self.configurations) if mode in references
        )
        first_target = min(
            index for index, mode in enumerate(self.configurations) if mode in targets
        )
        if last_reference > first_target:
            raise ExperimentError(
                "experiment: feature reference modes must be ordered before target modes"
            )
        reference_eyes = {eye for mode in references for eye in mode.eyes}
        target_eyes = {eye for mode in targets for eye in mode.eyes}
        if not target_eyes <= reference_eyes:
            raise ExperimentError(
                "experiment: feature reference modes must cover every target-mode eye"
            )

    @classmethod
    def load(cls, path) -> "ExperimentConfig":
        try:
            with Path(path).open("rb") as handle:
                value = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise ExperimentError(f"experiment: could not read config {path}: {error}") from error
        _keys(value, {"name", "warmup_frames", "samples", "scene", "configurations"},
              {"feature", "interval_seconds", "diagnostics", "visual_overrides", "matched_treatment",
               "single_pass_variants", "motion", "fsr_variants"}, str(path))
        try:
            matched = MatchedTreatment(value["matched_treatment"]) if "matched_treatment" in value else None
        except (ValueError, TypeError) as error:
            raise ExperimentError("experiment: invalid matched_treatment") from error
        return cls(_string(value.get("name"), "config.name"),
                   _integer(value.get("warmup_frames"), "config.warmup_frames"),
                   _integer(value.get("samples"), "config.samples"),
                   Scene.from_toml(value.get("scene")),
                   _configuration_list(value.get("configurations")),
                   (Feature.from_toml(value["feature"]) if "feature" in value else None),
                   _number(value.get("interval_seconds", 0.0), "config.interval_seconds"),
                   Diagnostics.from_toml(value.get("diagnostics", {})),
                   VisualOverrides.from_wire(value["visual_overrides"]) if "visual_overrides" in value else None,
                   matched,
                   parse_variants(value["single_pass_variants"]) if "single_pass_variants" in value else (),
                   _motion(value),
                   _fsr_variants(value))


@dataclass(frozen=True)
class CaptureSummary:
    mode: CompareMode
    capture_id: str
    frame_ordinal: int
    condition: str | None = None


@dataclass(frozen=True)
class FeatureObservation:
    mode: CompareMode
    capture_id: str
    role: FeatureRole
    eyes: tuple[int, ...]
    bright_fraction: tuple[float, ...]
    bright: tuple[bool, ...]
    reasons: tuple[str, ...]


@dataclass(frozen=True)
class FeatureEyeEvidence:
    eye: int
    status: FeatureEyeStatus
    reference_bright: tuple[bool, ...]
    target_bright: tuple[bool, ...]


@dataclass(frozen=True)
class FeatureReport:
    status: FeatureStatus
    stage: str
    roi: tuple[float, float, float, float]
    luminance: float
    min_fraction: float
    reference_modes: tuple[CompareMode, ...]
    target_modes: tuple[CompareMode, ...]
    observations: tuple[FeatureObservation, ...]
    eyes: tuple[FeatureEyeEvidence, ...]
    findings: tuple[str, ...]
    errors: tuple[str, ...]


@dataclass(frozen=True)
class ExperimentReport:
    name: str
    outcome: Outcome
    comparison_outcome: Outcome | None
    manifest: str
    comparison_report: str | None
    captures: tuple[CaptureSummary, ...]
    feature: FeatureReport | None
    findings: tuple[str, ...]
    errors: tuple[str, ...]
    restoration_error: str | None
    environment_restoration: str
    interval_seconds: float = 0.0
    diagnostics: Diagnostics = dataclasses.field(default_factory=Diagnostics)
    visual_overrides: VisualOverrides | None = None
    matched_treatment: MatchedTreatment | None = None
    matched_cross_condition: tuple[dict, ...] = ()
    single_pass_variants: tuple[dict, ...] = ()
    stage: dict | None = None

    @property
    def exit_code(self) -> int:
        return {Outcome.PASS: 0, Outcome.FAIL: 1, Outcome.INCONCLUSIVE: 2}[self.outcome]

    def to_wire(self):
        return dataclasses.asdict(self)


def sample_id(prefix: str, mode: CompareMode, index: int) -> str:
    return f"{prefix}_{mode.value}_{index}"


def run_experiment(runner, config: ExperimentConfig, output: Path, capture=None,
                   analyze=None, monotonic=None, sleep=None, restage=False) -> ExperimentReport:
    if config.fsr_variants:
        raise ExperimentError(f"experiment: {config.name}'s FSR variants run only in the fast "
                              "comparison (python3 -m tools.rendering fast)")
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    capture = capture or _capture
    analyze = analyze or _analyze
    monotonic = monotonic or time.monotonic
    sleep = sleep or time.sleep
    prefix = f"{config.name.replace('-', '_')}_{uuid.uuid4().hex[:8]}"
    matched = config.matched_treatment
    names = matched.conditions if matched else ("default",)
    cases = {name: Case(f"{config.name}-{name.replace('_', '-')}" if matched else config.name, {}) for name in names}
    manifest = Manifest(list(cases.values()))
    manifest_path = output / "manifest.json"
    captures: list[Capture] = []
    configurations: list[ConfigurationAck] = []
    anchor = None
    restoration_error = None
    environment_restoration = ("unobserved" if config.scene.environment is not None
                               else "not_requested")
    failure = None
    early_errors: list[str] = []
    last_capture_completed = None
    conditions: dict[str, str] = {}
    image_only: set[str] = set()
    staged = None
    try:
        if config.scene.stage is not None:
            stage_key_value = stage_key(config.scene.stage, config.scene.environment)
            stage_request = dataclasses.replace(
                config.scene.stage, id=f"{prefix}_stage",
                reuse_within_m=None if restage else reuse_distance(runner.config_dir, stage_key_value),
                environment=config.scene.environment.request() if config.scene.environment else None)
            acknowledgement = runner.request(stage_request)
            if not isinstance(acknowledgement, StageAck) or acknowledgement.id != stage_request.id:
                raise ExperimentError("experiment: expected the stage's acknowledgement")
            record_stage(runner.config_dir, stage_key_value)
            staged = dataclasses.asdict(acknowledgement)
            LOG.info("experiment: %s with the player at %s (%s m from the location), "
                     "%d spawns submitted, %d already present",
                     "reused the stage" if acknowledgement.reused else "staged",
                     acknowledgement.player_position, acknowledgement.distance_m,
                     acknowledgement.spawns_submitted, acknowledgement.spawns_present)
        anchor_mode = Mode(config.configurations[0].value)
        anchor_pose = PoseRequest(f"{prefix}_anchor", OrientationOffset(0.0, 0.0, 0.0),
                                  TranslationOffset())
        _, anchor = _configure(runner, config, f"{prefix}_anchor", anchor_pose, anchor_mode,
                               anchor, configurations)
        if config.motion is None:
            offset, translation = offset_for_target(anchor, config.scene.pose.camera())
            target_pose = PoseRequest(f"{prefix}_target", offset, translation)
        else:
            target_pose = None
        for item in cases.values():
            item.expected_pose = target_pose
            item.expected_motion = config.motion.expected if config.motion else None
        counters: dict[CompareMode, int] = {}
        for entry_index, configured_mode in enumerate(config.configurations, start=1):
            mode = Mode(configured_mode.value)
            _, anchor = _configure(
                runner, config, f"{prefix}_cfg_{entry_index}", target_pose,
                mode, anchor, configurations)
            shared = config.matched_treatment in (MatchedTreatment.PLANAR_ABBA, MatchedTreatment.REFRACTION_ABBA, MatchedTreatment.PRODUCTION_REFRACTION_ABBA, MatchedTreatment.PROJECTED_PLANAR_ABBA, MatchedTreatment.MIRRORED_PLANAR_ABBA, MatchedTreatment.EYE_WIDTH_ABBA, MatchedTreatment.LAKE_SKY_ABBA, MatchedTreatment.LAKE_SKY_BUFFER_ABBA, MatchedTreatment.LAKE_DOMAIN_CAMERA_ABBA, MatchedTreatment.LAKE_HULL_CAMERA_ABBA, MatchedTreatment.RESOLVE_CAMERA_ABBA, MatchedTreatment.PROXY_EYE_ABBA, MatchedTreatment.MARCH_EYE_ABBA, MatchedTreatment.VETO_EYE_ABBA, MatchedTreatment.GRID_EYE_ABBA, MatchedTreatment.HISTORY_CENTER_ABBA, MatchedTreatment.BARK_DITHER_ABBA, MatchedTreatment.COLOR_DITHER_ABBA, MatchedTreatment.IMPOSTOR_SCALE_ABBA, MatchedTreatment.LEFT_SCALE_ABBA, MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) and mode != Mode.SINGLE_PASS
            sequence = ((names[0],) * config.samples if shared else
                        (names[0], names[1], names[1], names[0]) * config.samples
                        if config.matched_treatment else ("default",) * config.samples)
            matched_identity = None
            for condition in sequence:
                entries = cases[condition].modes.setdefault(mode, [])
                counters[configured_mode] = counters.get(configured_mode, 0) + 1
                identifier = sample_id(prefix, configured_mode, counters[configured_mode])
                _wait_for_capture_interval(runner, config.interval_seconds,
                                           last_capture_completed, monotonic, sleep)
                runner.assert_running()
                if runner.deadline is not None and monotonic() >= runner.deadline:
                    raise ExperimentError(
                        "experiment: no capture can start after the run deadline"
                    )
                capture_config = config
                if shared:
                    reference = {MatchedTreatment.AA_COLOR_EDGE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.PATCH63_HULL_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LAKE_DS_MVP_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.CARD_ATLAS_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.GROUP_FADE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.INNER_EDGE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.WRITER_DS_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.BASE_VP_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.GROUP_ATLAS_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.DOF_EDGE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.FOG_UV_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.TERRAIN_CAMERA_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.EXTRA_BARK_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LEFT_SCALE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.IMPOSTOR_SCALE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.COLOR_DITHER_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.BARK_DITHER_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.HISTORY_CENTER_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.GRID_EYE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.VETO_EYE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.MARCH_EYE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.PROXY_EYE_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.RESOLVE_CAMERA_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LAKE_HULL_CAMERA_ABBA: WaterReflection.LAKE_CAMERA_CONTROL, MatchedTreatment.LAKE_DOMAIN_CAMERA_ABBA: WaterReflection.LAKE_CAMERA_CONTROL,
                                 MatchedTreatment.LAKE_SKY_ABBA: WaterReflection.LAKE_LIGHTING,
                                 MatchedTreatment.LAKE_SKY_BUFFER_ABBA: WaterReflection.LAKE_LIGHTING}.get(matched)
                    capture_config = dataclasses.replace(config, matched_treatment=None, diagnostics=dataclasses.replace(config.diagnostics, water_reflection=reference))
                elif config.matched_treatment:
                    if matched in (MatchedTreatment.EXTRA_BARK_ABBA, MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA):
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=True, bark_dither=True, color_dither=True, impostor_scale=True, left_atlas_scale=True, extra_bark_dither=condition == "extra_physical" or matched in (MatchedTreatment.TERRAIN_CAMERA_ABBA, MatchedTreatment.FOG_UV_ABBA, MatchedTreatment.DOF_EDGE_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA), aa_color_edge=(condition=="aa_local") if matched==MatchedTreatment.AA_COLOR_EDGE_ABBA else None, patch63_hull=True if matched==MatchedTreatment.AA_COLOR_EDGE_ABBA else (condition == "hull_physical") if matched == MatchedTreatment.PATCH63_HULL_ABBA else None, owner_four_domain=True if matched in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA) else (condition == "domain_physical") if matched == MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA else None, lake_color_mirror=True if matched in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "color_mirror") if matched == MatchedTreatment.LAKE_COLOR_MIRROR_ABBA else None, lake_depth_edge=True if matched in (MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "depth_bounded") if matched == MatchedTreatment.LAKE_DEPTH_EDGE_ABBA else None, lake_ds_mvp=True if matched in (MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "mvp_physical") if matched == MatchedTreatment.LAKE_DS_MVP_ABBA else None, card_atlas=True if matched in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA) else (condition == "card_physical") if matched == MatchedTreatment.CARD_ATLAS_ABBA else None, group_fade=True if matched in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.CARD_ATLAS_ABBA) else (condition == "fade_physical") if matched == MatchedTreatment.GROUP_FADE_ABBA else None, lake_inner_edge=True if matched in (MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "inner_physical") if matched == MatchedTreatment.INNER_EDGE_ABBA else None, writer_domain_camera=True if matched in (MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "writer_domain") if matched == MatchedTreatment.WRITER_DS_ABBA else None, base_eye_vp=True if matched in (MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "base_eye") if matched == MatchedTreatment.BASE_VP_ABBA else None, group_atlas_scale=True if matched in (MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "group_physical") if matched == MatchedTreatment.GROUP_ATLAS_ABBA else None, dof_edge_local=True if matched in (MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA) else (condition == "edge_local") if matched == MatchedTreatment.DOF_EDGE_ABBA else None, fog_local_uv=True if matched in (MatchedTreatment.AA_COLOR_EDGE_ABBA, MatchedTreatment.PATCH63_HULL_ABBA, MatchedTreatment.OWNER_FOUR_DOMAIN_ABBA, MatchedTreatment.LAKE_COLOR_MIRROR_ABBA, MatchedTreatment.LAKE_DEPTH_EDGE_ABBA, MatchedTreatment.LAKE_DS_MVP_ABBA, MatchedTreatment.CARD_ATLAS_ABBA, MatchedTreatment.GROUP_FADE_ABBA, MatchedTreatment.INNER_EDGE_ABBA, MatchedTreatment.WRITER_DS_ABBA, MatchedTreatment.BASE_VP_ABBA, MatchedTreatment.GROUP_ATLAS_ABBA, MatchedTreatment.DOF_EDGE_ABBA) else (condition == "fog_local") if matched == MatchedTreatment.FOG_UV_ABBA else None, terrain_camera=(condition == "terrain_physical") if matched == MatchedTreatment.TERRAIN_CAMERA_ABBA else None)
                    elif matched == MatchedTreatment.LEFT_SCALE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=True, bark_dither=True, color_dither=True, impostor_scale=True, left_atlas_scale=condition == "atlas_both")
                    elif matched == MatchedTreatment.IMPOSTOR_SCALE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=True, bark_dither=True, color_dither=True, impostor_scale=condition == "impostor_physical")
                    elif matched == MatchedTreatment.COLOR_DITHER_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=True, bark_dither=True, color_dither=condition == "color_physical")
                    elif matched == MatchedTreatment.BARK_DITHER_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=True, bark_dither=condition == "dither_physical")
                    elif matched == MatchedTreatment.HISTORY_CENTER_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, history_center=condition == "history_physical")
                    elif matched == MatchedTreatment.GRID_EYE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=True, grid_eye=condition == "grid_physical")
                    elif matched == MatchedTreatment.VETO_EYE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=True, veto_eye=condition == "veto_packed")
                    elif matched == MatchedTreatment.MARCH_EYE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=True, march_eye=condition == "march_packed")
                    elif matched == MatchedTreatment.PROXY_EYE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=True, proxy_eye=condition == "proxy_physical_eye")
                    elif matched == MatchedTreatment.RESOLVE_CAMERA_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=WaterReflection.LAKE_HULL_CAMERA, resolve_eye_camera=condition == "resolve_physical_camera")
                    elif matched == MatchedTreatment.CASCADE_ABBA:
                        diagnostics = dataclasses.replace(config.diagnostics, cascade_scale=CascadeScale(condition))
                    else:
                        treatment = None if condition in ("control", "production_refraction") else WaterReflection(condition)
                        if condition == "control" and mode == Mode.SINGLE_PASS and matched in (MatchedTreatment.PLANAR_ABBA, MatchedTreatment.REFRACTION_ABBA, MatchedTreatment.ZERO_PLANAR_ABBA, MatchedTreatment.PRODUCTION_REFRACTION_ABBA):
                            treatment = WaterReflection.PRISTINE_LAKE
                        diagnostics = dataclasses.replace(config.diagnostics, water_reflection=treatment)
                    capture_config = dataclasses.replace(
                        config, diagnostics=diagnostics,
                        matched_treatment=None, configurations=(configured_mode,))
                try:
                    result = capture(runner, capture_config, target_pose, anchor, mode,
                                     configurations[-1].generation, identifier)
                except EXPECTED_ERRORS:
                    folder = runner.config_dir / "captures"
                    metadata_path = folder / f"{identifier}.capture.json"
                    if config.matched_treatment and metadata_path.exists():
                        entries.append(Sample(os.path.relpath(folder / f"{identifier}.capture.png", output),
                                              os.path.relpath(metadata_path, output)))
                        write_manifest(manifest_path, manifest)
                    raise
                if not isinstance(result, Capture):
                    raise ExperimentError("experiment: capture is not a diagnostic capture")
                previous = captures[-1] if captures else None
                captures.append(result)
                conditions[result.id] = condition
                last_capture_completed = monotonic()
                folder = runner.config_dir / "captures"
                entries.append(Sample(
                    os.path.relpath(folder / f"{identifier}.capture.png", output),
                    os.path.relpath(folder / f"{identifier}.capture.json", output)))
                write_manifest(manifest_path, manifest)
                if previous is not None and (result.frame_ordinal <= previous.frame_ordinal or
                                             result.predicted_display_time_ns <= previous.predicted_display_time_ns):
                    raise ExperimentError(
                        "experiment: captures do not identify increasing rendered frames"
                    )
                if shared:
                    cases[names[1]].modes.setdefault(mode, []).append(entries[-1])
                    write_manifest(manifest_path, manifest)
                elif config.matched_treatment:
                    metadata = read_object(folder / f"{identifier}.capture.json")
                    identity = matched_capture_identity(result, metadata, configurations[-1].generation)
                    if matched_identity is None:
                        matched_identity = identity
                    elif identity != matched_identity:
                        raise ExperimentError(f"experiment: matched capture {identifier} changed pose or configuration")
                LOG.info("wrote %s after %s", manifest_path, identifier)
            references = (set(config.feature.reference_modes)
                          if config.feature is not None else set())
            if (config.feature is not None and configured_mode in references and
                    not any(mode in references
                            for mode in config.configurations[entry_index:])):
                early_errors = list(_reference_eligibility(
                    config, runner.config_dir / "captures", captures))
                if early_errors:
                    LOG.warning("experiment: reference eligibility failed; "
                                "skipping target configurations")
                    break
        for variant_index, variant in enumerate(config.single_pass_variants, start=1):
            variant_config = _variant_config(config, variant)
            case = Case(variant_case_name(config.name, variant), {}, target_pose,
                        config.motion.expected if config.motion else None)
            borrow_references(cases[DEFAULT_VARIANT], case)
            cases[variant.name] = case
            manifest.cases.append(case)
            _, anchor = _configure(runner, variant_config, f"{prefix}_var_{variant_index}",
                                   target_pose, Mode.SINGLE_PASS, anchor, configurations)
            entries = case.modes.setdefault(Mode.SINGLE_PASS, [])
            for index in range(1, config.samples + 1):
                identifier = f"{prefix}_v{variant_index}_{variant.name.replace('-', '_')}_{index}"
                _wait_for_capture_interval(runner, config.interval_seconds,
                                           last_capture_completed, monotonic, sleep)
                runner.assert_running()
                if runner.deadline is not None and monotonic() >= runner.deadline:
                    raise ExperimentError("experiment: no capture can start after the run deadline")
                result = capture(runner, variant_config, target_pose, anchor, Mode.SINGLE_PASS,
                                 configurations[-1].generation, identifier)
                if not isinstance(result, Capture):
                    raise ExperimentError("experiment: capture is not a diagnostic capture")
                previous = captures[-1] if captures else None
                captures.append(result)
                conditions[result.id] = variant.name
                if not variant.diagnostics:
                    image_only.add(result.id)
                last_capture_completed = monotonic()
                folder = runner.config_dir / "captures"
                entries.append(Sample(
                    os.path.relpath(folder / f"{identifier}.capture.png", output),
                    os.path.relpath(folder / f"{identifier}.capture.json", output)))
                write_manifest(manifest_path, manifest)
                if previous is not None and (result.frame_ordinal <= previous.frame_ordinal or
                                             result.predicted_display_time_ns <= previous.predicted_display_time_ns):
                    raise ExperimentError(
                        "experiment: captures do not identify increasing rendered frames"
                    )
                LOG.info("wrote %s after %s", manifest_path, identifier)
    except EXPECTED_ERRORS as error:
        failure = error
    finally:
        deadline = runner.deadline
        try:
            runner.deadline = max(deadline, time.monotonic() + RESTORATION_BUDGET)
            runner.cancel_pending_request()
            restoration = runner.request(Request(Command.END, f"{prefix}_end"))
            if not isinstance(restoration, RestorationAck):
                raise ExperimentError(
                    "experiment: expected a restoration acknowledgement"
                )
            if restoration.id != f"{prefix}_end":
                raise ExperimentError("experiment: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise ExperimentError("experiment: restoration generation does not increase")
            observed = restoration.metadata.environment
            if config.scene.environment is not None:
                if observed is None or observed.weather_control is None:
                    raise ExperimentError("experiment: environment restoration is unobserved")
                environment_restoration = observed.weather_control.value
                if observed.weather_control != WeatherControl.AMBIENT:
                    raise ExperimentError(
                        "experiment: weather control was not released to ambient"
                    )
        except EXPECTED_ERRORS as error:
            restoration_error = str(error)
            LOG.error("experiment: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
    if failure is not None:
        errors = [str(failure)]
        if restoration_error is not None:
            errors.append(f"restoration: {restoration_error}")
        report = dataclasses.replace(
            _failure_report(config, manifest_path, captures, tuple(errors), restoration_error,
                            environment_restoration, conditions), stage=staged)
        try:
            write_report(output / "report.json", report)
        except OSError as error:
            LOG.error("experiment: could not write the failure report: %s", error)
        return report
    feature = (_feature_report(config, runner.config_dir / "captures", captures)
               if config.feature is not None else None)
    comparison_report = None
    comparison_outcome = None
    errors: list[str] = []
    if restoration_error is not None:
        errors.append(f"restoration: {restoration_error}")
    errors.extend(early_errors)
    if config.diagnostics.resolve_inputs:
        for captured in captures:
            if captured.id in image_only:
                continue
            try:
                metadata = read_object(runner.config_dir / "captures" / f"{captured.id}.capture.json")
                errors.extend(_resolve_content_errors(captured.id, metadata))
            except (*EXPECTED_ERRORS, KeyError, IndexError) as error:
                errors.append(f"resolve inputs: {captured.id}: {error}")
    if captures:
        comparison_output = output / "comparison"
        try:
            report = analyze(manifest_path, comparison_output)
            comparison_outcome = Outcome(report.status)
            comparison_report = str(comparison_output / "report.json")
        except EXPECTED_ERRORS as error:
            errors.append(f"comparison analysis: {error}")
    else:
        errors.append("comparison analysis: no captures were written")
    matched_cross = ()
    if config.matched_treatment and captures:
        try:
            matched_cross = matched_cross_metrics(output, cases)
        except EXPECTED_ERRORS as error:
            errors.append(f"matched treatment comparison: {error}")
    variant_summaries = ()
    if config.single_pass_variants and captures:
        try:
            variant_summaries = summarize(output, cases, _variant_switches(config))
            for summary in variant_summaries:
                LOG.info("%s", describe(summary))
        except EXPECTED_ERRORS as error:
            errors.append(f"single-pass variants: {error}")
    if feature is not None:
        errors.extend(feature.errors)
    overall = comparison_outcome or Outcome.INCONCLUSIVE
    if errors or (feature is not None and feature.status == FeatureStatus.INCONCLUSIVE):
        overall = Outcome.INCONCLUSIVE
    report = ExperimentReport(
        config.name, overall, comparison_outcome, str(manifest_path), comparison_report,
        tuple(CaptureSummary(CompareMode(capture.mode.value), capture.id,
                             capture.frame_ordinal, _condition(config, conditions, capture)) for capture in captures),
        feature, () if feature is None else feature.findings,
        tuple(dict.fromkeys(errors)), restoration_error, environment_restoration,
        config.interval_seconds, config.diagnostics, config.visual_overrides, config.matched_treatment,
        matched_cross, variant_summaries, staged)
    write_report(output / "report.json", report)
    return report


def _fsr_variants(value) -> tuple[FsrVariant, ...]:
    if "fsr_variants" not in value:
        return ()
    try:
        return parse_fsr_variants(value["fsr_variants"])
    except FsrVariantError as error:
        raise ExperimentError(f"experiment: {error}") from error


def _variant_config(config: ExperimentConfig, variant: SinglePassVariant) -> ExperimentConfig:
    """The configuration a variant's captures run under: its switches forced, and unless it asks
    for them, none of the experiment's screenshot diagnostics beyond the shader identity census."""
    diagnostics = (config.diagnostics if variant.diagnostics
                   else Diagnostics(shader_identity=config.diagnostics.shader_identity))
    return dataclasses.replace(config, visual_overrides=variant.overrides(config.visual_overrides),
                               diagnostics=diagnostics)


def _variant_switches(config: ExperimentConfig) -> dict[str, dict[str, bool]]:
    base = config.visual_overrides.switches if config.visual_overrides is not None else {}
    result = {DEFAULT_VARIANT: base}
    for variant in config.single_pass_variants:
        overrides = variant.overrides(config.visual_overrides)
        result[variant.name] = overrides.switches if overrides is not None else {}
    return result


def _condition(config: ExperimentConfig, conditions: dict[str, str], capture) -> str | None:
    if config.matched_treatment or config.single_pass_variants:
        return conditions.get(capture.id)
    return None


def _resolve_content_errors(capture_id: str, metadata: dict) -> list[str]:
    errors = []
    for eye, result in enumerate(metadata["resolve_inputs"]["eyes"]):
        if result["status"] != "observed":
            continue
        for buffer in result["observation"]["constant_buffers"]:
            if buffer["slot"] not in (0, 3):
                continue
            content = buffer["content"]
            if content["status"] != "captured":
                errors.append(f"resolve inputs: {capture_id}: eye {eye} cb{buffer['slot']}: {content['status']}")
    return errors


def _wait_for_capture_interval(runner, interval_seconds, last_completed, monotonic, sleep):
    if last_completed is None or interval_seconds <= 0:
        return
    target = last_completed + interval_seconds
    while True:
        runner.assert_running()
        now = monotonic()
        deadline = runner.deadline
        if deadline is not None and now >= deadline:
            raise ExperimentError("experiment: capture interval reached the run deadline")
        if now >= target:
            return
        wait = min(SPACING_POLL_SECONDS, target - now)
        if deadline is not None:
            wait = min(wait, max(0.0, deadline - now))
        sleep(max(0.0, wait))


def _failure_report(config, manifest_path, captures, errors, restoration_error,
                    environment_restoration, conditions):
    return ExperimentReport(
        config.name, Outcome.INCONCLUSIVE, None, str(manifest_path), None,
        tuple(CaptureSummary(CompareMode(capture.mode.value), capture.id,
                             capture.frame_ordinal, _condition(config, conditions, capture)) for capture in captures),
        None, (), errors, restoration_error, environment_restoration,
        config.interval_seconds, config.diagnostics, config.visual_overrides, config.matched_treatment)


def _feature_report(config: ExperimentConfig, root: Path, captures) -> FeatureReport | None:
    feature = config.feature
    if feature is None:
        return None
    references = set(feature.reference_modes)
    targets = set(feature.target_modes)
    observations: list[FeatureObservation] = []
    errors: list[str] = []
    findings: list[str] = []
    counters: dict[CompareMode, int] = {}
    for capture in captures:
        mode = CompareMode(capture.mode.value)
        if mode not in references and mode not in targets:
            continue
        counters[mode] = counters.get(mode, 0) + 1
        observations.append(_feature_observation(config, root, capture, counters[mode]))
    present = {observation.mode for observation in observations}
    for mode in feature.reference_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no reference capture was recorded")
    for mode in feature.target_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no target capture was recorded")
    for observation in observations:
        errors.extend(f"{observation.capture_id}: {reason}" for reason in observation.reasons)
    reference_eyes = {eye for mode in feature.reference_modes for eye in mode.eyes}
    target_eyes = tuple(sorted({eye for mode in feature.target_modes for eye in mode.eyes}))
    eye_evidence: list[FeatureEyeEvidence] = []
    unstable = False
    lost = False
    for eye in target_eyes:
        reference_bright = tuple(
            observation.bright[observation.eyes.index(eye)]
            for observation in observations
            if observation.role == FeatureRole.REFERENCE and eye in observation.eyes
        )
        target_bright = tuple(
            observation.bright[observation.eyes.index(eye)]
            for observation in observations
            if observation.role == FeatureRole.TARGET and eye in observation.eyes
        )
        if eye not in reference_eyes:
            errors.append(f"eye {eye}: no reference mode covers this eye")
            status = FeatureEyeStatus.UNOBSERVED
        elif not reference_bright or not target_bright:
            errors.append(f"eye {eye}: reference or target captures are missing")
            status = FeatureEyeStatus.UNOBSERVED
        elif not all(reference_bright):
            errors.append(f"eye {eye}: reference lacks the bright region")
            status = FeatureEyeStatus.UNOBSERVED
        elif all(target_bright):
            status = FeatureEyeStatus.RETAINED
        elif not any(target_bright):
            status = FeatureEyeStatus.LOST
            lost = True
        else:
            status = FeatureEyeStatus.UNSTABLE
            unstable = True
        eye_evidence.append(FeatureEyeEvidence(eye, status, reference_bright, target_bright))
    if errors or unstable:
        status = FeatureStatus.INCONCLUSIVE
        if unstable:
            findings.append("target brightness is unstable across samples")
    elif lost:
        status = FeatureStatus.DIFFERENCE
        findings.append("target modes differ from the reference bright region")
    else:
        status = FeatureStatus.MATCH
        findings.append("target modes match the reference bright region")
    return FeatureReport(
        status, feature.stage.value, feature.roi, feature.luminance, feature.min_fraction,
        feature.reference_modes, feature.target_modes,
        tuple(observations), tuple(eye_evidence), tuple(findings), tuple(errors))


def _reference_eligibility(config: ExperimentConfig, root: Path, captures):
    feature = config.feature
    if feature is None:
        return ()
    references = set(feature.reference_modes)
    observations: list[FeatureObservation] = []
    counters: dict[CompareMode, int] = {}
    for capture in captures:
        mode = CompareMode(capture.mode.value)
        if mode not in references:
            continue
        counters[mode] = counters.get(mode, 0) + 1
        observations.append(_feature_observation(config, root, capture, counters[mode]))
    errors: list[str] = []
    present = {observation.mode for observation in observations}
    for mode in feature.reference_modes:
        if mode not in present:
            errors.append(f"{mode.value}: no reference capture was recorded")
    for observation in observations:
        errors.extend(f"{observation.capture_id}: {reason}"
                      for reason in observation.reasons)
    for eye in sorted({eye for mode in feature.reference_modes for eye in mode.eyes}):
        values = [observation.bright[observation.eyes.index(eye)]
                  for observation in observations if eye in observation.eyes]
        if not values:
            errors.append(f"eye {eye}: no reference capture was recorded")
        elif not all(values):
            errors.append(f"eye {eye}: reference lacks the bright region")
    return tuple(dict.fromkeys(errors))


def _feature_observation(config: ExperimentConfig, root: Path, capture: Capture,
                         index: int) -> FeatureObservation:
    feature = config.feature
    mode = CompareMode(capture.mode.value)
    role = (FeatureRole.REFERENCE if mode in feature.reference_modes
            else FeatureRole.TARGET)
    reasons: list[str] = []
    if not _matches_world_pose(config, capture.comparison_inputs.camera):
        reasons.append("camera differs from the configured world pose")
    observation = capture.visual.environment
    if config.scene.environment is not None:
        if observation is None:
            reasons.append("environment observation is unavailable")
        else:
            _check_observation(config, capture.id, observation, reasons)
    try:
        fractions = _bright_fractions(feature, root, capture, index)
    except (ExperimentError, OSError, ValueError, TypeError, ProtocolError) as error:
        reasons.append(str(error))
        fractions = ()
    bright = tuple(value >= feature.min_fraction for value in fractions)
    return FeatureObservation(mode, capture.id, role, tuple(mode.eyes), fractions, bright,
                              tuple(reasons))


def _configure(runner, config: ExperimentConfig, request_id, pose, mode, anchor, configurations,
               hold_still=False, pin=None):
    """Configure `mode`; `hold_still` replaces a motion that never moves the camera with the fixed
    `pose`, so captures need not wait out its cycles, and `pin` (a camera) is the base the payload's
    freeze holds in place of the live camera."""
    environment = config.scene.environment.request() if config.scene.environment else None
    motion = config.motion.request if config.motion and not hold_still else None
    request = Request(Command.CONFIGURE, request_id, mode, config.warmup_frames,
                      None if motion else pose, anchor=pin, motion=motion,
                      environment=environment, visual_overrides=config.visual_overrides,
                      shader_identity=True if config.diagnostics.shader_identity else None)
    acknowledgement = runner.request(request)
    if not isinstance(acknowledgement, ConfigurationAck):
        raise ExperimentError("experiment: expected a configuration acknowledgement")
    if (acknowledgement.id != request.id or acknowledgement.mode != mode or
            acknowledgement.warmup_frames != config.warmup_frames):
        raise ExperimentError(
            "experiment: configuration acknowledgement disagrees with its request"
        )
    if config.diagnostics.shader_identity and (acknowledgement.metadata.shader_identity is None or
            acknowledgement.metadata.shader_identity.get("status") != "complete"):
        raise ExperimentError("experiment: shader identity census is not complete")
    if acknowledgement.metadata.visual_overrides != config.visual_overrides:
        raise ExperimentError("experiment: configuration visual overrides disagree with the request")
    if configurations and acknowledgement.generation <= configurations[-1].generation:
        raise ExperimentError("experiment: configuration generation does not increase")
    anchor = (acknowledgement.validate_motion(request, anchor) if motion
              else acknowledgement.validate_pose(request, anchor))
    configurations.append(acknowledgement)
    LOG.info("configured %s in %s", request_id, mode.value)
    return acknowledgement, anchor


def _capture(runner, config, pose, anchor, mode, generation, identifier):
    acknowledgement = runner.request(Request(Command.SCREENSHOT, identifier,
                                             motion_capture=config.motion.capture if config.motion else None,
                                             group_cards=True if config.diagnostics.group_cards else None, group_residual=True if config.diagnostics.group_residual else None, aa_weights=True if config.diagnostics.aa_weights else None, aa_draws=True if config.diagnostics.aa_draws else None, lake_owner_four=True if config.diagnostics.lake_owner_four else None, lake_producer=True if config.diagnostics.lake_producer else None, lake_gradients=True if config.diagnostics.lake_gradients else None, lake_waves=True if config.diagnostics.lake_waves else None, lake_normals=True if config.diagnostics.lake_normals else None, lake_displacement=True if config.diagnostics.lake_displacement else None, lake_samplers=True if config.diagnostics.lake_samplers else None, tess_writer=True if config.diagnostics.tess_writer else None, tess_owners=True if config.diagnostics.tess_owners else None, base_projection=True if config.diagnostics.base_projection else None, base_sources=True if config.diagnostics.base_sources else None, terrain_interval=True if config.diagnostics.terrain_interval else None, group_inputs=True if config.diagnostics.group_inputs else None, group_sources=True if config.diagnostics.group_sources else None, resolve_shadow=True if config.diagnostics.resolve_shadow else None, dof_inputs=True if config.diagnostics.dof_inputs else None, fog_generation=True if config.diagnostics.fog_generation else None, fog_inputs=True if config.diagnostics.fog_inputs else None, late_sky_stages=True if config.diagnostics.late_sky_stages else None,
                                             water_inputs=True if config.diagnostics.water_inputs else None,
                                             draw_census=True if config.diagnostics.draw_census else None,
                                             water_tess_draw=config.diagnostics.water_tess_draw,
                                             patch_draw_ordinal=config.diagnostics.patch_draw_ordinal,
                                             second_patch_ordinal=config.diagnostics.second_patch_ordinal,
                                             water_surface_inputs=True if config.diagnostics.water_surface_inputs else None,
                                             sun_inputs=True if config.diagnostics.sun_inputs else None,
                                             resolve_inputs=True if config.diagnostics.resolve_inputs else None,
                                             aa_color_edge=config.diagnostics.aa_color_edge, patch63_hull=config.diagnostics.patch63_hull, owner_four_domain=config.diagnostics.owner_four_domain, lake_color_mirror=config.diagnostics.lake_color_mirror, lake_depth_edge=config.diagnostics.lake_depth_edge, lake_ds_mvp=config.diagnostics.lake_ds_mvp, card_atlas=config.diagnostics.card_atlas, group_fade=config.diagnostics.group_fade, lake_inner_edge=config.diagnostics.lake_inner_edge, writer_domain_camera=config.diagnostics.writer_domain_camera, base_eye_vp=config.diagnostics.base_eye_vp, group_atlas_scale=config.diagnostics.group_atlas_scale, dof_edge_local=config.diagnostics.dof_edge_local, fog_local_uv=config.diagnostics.fog_local_uv, terrain_camera=config.diagnostics.terrain_camera, extra_bark_dither=config.diagnostics.extra_bark_dither, left_atlas_scale=config.diagnostics.left_atlas_scale, impostor_scale=config.diagnostics.impostor_scale, color_dither=config.diagnostics.color_dither, bark_dither=config.diagnostics.bark_dither, terrain_sources=True if config.diagnostics.terrain_sources else None, residual_bark=True if config.diagnostics.residual_bark else None, left_interval=True if config.diagnostics.left_interval else None, impostor_inputs=True if config.diagnostics.impostor_inputs else None, interval_trace=True if config.diagnostics.interval_trace else None, post_geometry=True if config.diagnostics.post_geometry else None, bark_color_trace=True if config.diagnostics.bark_color_trace else None, velocity_trace=True if config.diagnostics.velocity_trace else None, early_depth=True if config.diagnostics.early_depth else None, ssr_history=True if config.diagnostics.ssr_history else None, ssr_prepare=True if config.diagnostics.ssr_prepare else None, ssr_operands=True if config.diagnostics.ssr_operands else None, ssr_bracket=True if config.diagnostics.ssr_bracket else None, resolve_samples=True if config.diagnostics.resolve_samples else None, history_center=config.diagnostics.history_center, grid_eye=config.diagnostics.grid_eye, veto_eye=config.diagnostics.veto_eye, march_eye=config.diagnostics.march_eye, proxy_eye=config.diagnostics.proxy_eye, resolve_eye_camera=config.diagnostics.resolve_eye_camera,
                                             sun_depth_clip=config.diagnostics.sun_depth_clip,
                                             cascade_scale=config.diagnostics.cascade_scale,
                                             water_reflection=config.diagnostics.water_reflection))
    if not isinstance(acknowledgement, ScreenshotAck):
        raise ExperimentError("experiment: expected a screenshot acknowledgement")
    capture = validate_capture(runner.config_dir / "captures", identifier,
                               acknowledgement.capture, mode=mode, generation=generation,
                               motion_capture=config.motion.capture if config.motion else None,
                               group_cards=config.diagnostics.group_cards, group_residual=config.diagnostics.group_residual, aa_weights=config.diagnostics.aa_weights, aa_draws=config.diagnostics.aa_draws, lake_owner_four=config.diagnostics.lake_owner_four, lake_producer=config.diagnostics.lake_producer, lake_gradients=config.diagnostics.lake_gradients, lake_waves=config.diagnostics.lake_waves, lake_normals=config.diagnostics.lake_normals, lake_displacement=config.diagnostics.lake_displacement, lake_samplers=config.diagnostics.lake_samplers, tess_writer=config.diagnostics.tess_writer, tess_owners=config.diagnostics.tess_owners, base_projection=config.diagnostics.base_projection, base_sources=config.diagnostics.base_sources, terrain_interval=config.diagnostics.terrain_interval, group_inputs=config.diagnostics.group_inputs, group_sources=config.diagnostics.group_sources, resolve_shadow=config.diagnostics.resolve_shadow, dof_inputs=config.diagnostics.dof_inputs, fog_generation=config.diagnostics.fog_generation, fog_inputs=config.diagnostics.fog_inputs, late_sky_stages=config.diagnostics.late_sky_stages,
                               water_inputs=config.diagnostics.water_inputs,
                               draw_census=config.diagnostics.draw_census,
                               water_tess_draw=config.diagnostics.water_tess_draw,
                               patch_draw_ordinal=config.diagnostics.patch_draw_ordinal,
                               second_patch_ordinal=config.diagnostics.second_patch_ordinal,
                               water_surface_inputs=config.diagnostics.water_surface_inputs,
                               sun_inputs=config.diagnostics.sun_inputs,
                               shader_identity=config.diagnostics.shader_identity,
                               resolve_inputs=config.diagnostics.resolve_inputs, aa_color_edge=config.diagnostics.aa_color_edge, patch63_hull=config.diagnostics.patch63_hull, owner_four_domain=config.diagnostics.owner_four_domain, lake_color_mirror=config.diagnostics.lake_color_mirror, lake_depth_edge=config.diagnostics.lake_depth_edge, lake_ds_mvp=config.diagnostics.lake_ds_mvp, card_atlas=config.diagnostics.card_atlas, group_fade=config.diagnostics.group_fade, lake_inner_edge=config.diagnostics.lake_inner_edge, writer_domain_camera=config.diagnostics.writer_domain_camera, base_eye_vp=config.diagnostics.base_eye_vp, group_atlas_scale=config.diagnostics.group_atlas_scale, dof_edge_local=config.diagnostics.dof_edge_local, fog_local_uv=config.diagnostics.fog_local_uv, terrain_camera=config.diagnostics.terrain_camera, extra_bark_dither=config.diagnostics.extra_bark_dither, left_atlas_scale=config.diagnostics.left_atlas_scale, impostor_scale=config.diagnostics.impostor_scale, color_dither=config.diagnostics.color_dither, bark_dither=config.diagnostics.bark_dither, terrain_sources=config.diagnostics.terrain_sources, residual_bark=config.diagnostics.residual_bark, left_interval=config.diagnostics.left_interval, impostor_inputs=config.diagnostics.impostor_inputs, interval_trace=config.diagnostics.interval_trace, post_geometry=config.diagnostics.post_geometry, bark_color_trace=config.diagnostics.bark_color_trace, velocity_trace=config.diagnostics.velocity_trace, early_depth=config.diagnostics.early_depth, ssr_history=config.diagnostics.ssr_history, ssr_prepare=config.diagnostics.ssr_prepare, ssr_operands=config.diagnostics.ssr_operands, ssr_bracket=config.diagnostics.ssr_bracket, resolve_samples=config.diagnostics.resolve_samples, history_center=config.diagnostics.history_center, grid_eye=config.diagnostics.grid_eye, veto_eye=config.diagnostics.veto_eye, march_eye=config.diagnostics.march_eye, proxy_eye=config.diagnostics.proxy_eye, resolve_eye_camera=config.diagnostics.resolve_eye_camera,
                               sun_depth_clip=config.diagnostics.sun_depth_clip,
                               cascade_scale=config.diagnostics.cascade_scale,
                               water_reflection=config.diagnostics.water_reflection)
    if not isinstance(capture, Capture):
        raise ExperimentError("experiment: expected a diagnostic capture")
    if config.diagnostics.shader_identity and (capture.visual.shader_identity is None or
            capture.visual.shader_identity.get("status") != "complete"):
        raise ExperimentError("experiment: capture shader identity census is not complete")
    if capture.visual.visual_overrides != config.visual_overrides:
        raise ExperimentError("experiment: capture visual overrides disagree with the request")
    if config.single_pass_variants and mode == Mode.SINGLE_PASS:
        verify_switches(identifier, read_object(runner.config_dir / "captures" / f"{identifier}.capture.json"),
                        config.visual_overrides)
    if config.motion is not None:
        validate_motion_capture(config.motion, capture, anchor)
        return capture
    if capture.visual.pose is None:
        raise ExperimentError(f"experiment: capture {identifier} has no pose evidence")
    capture.visual.pose.validate(pose, anchor)
    return capture


def _analyze(manifest_path: Path, comparison_output: Path):
    return analyze_manifest(manifest_path, comparison_output)


def _matches_world_pose(config: ExperimentConfig, camera: Camera) -> bool:
    target = config.scene.pose.camera()
    return (composed_positions_match(target.position, camera.position, target.position,
                                     POSE_TOLERANCE) and
            quaternions_match(target.orientation, camera.orientation, POSE_TOLERANCE))


def _check_observation(config: ExperimentConfig, identifier, observation: EnvironmentObservation,
                       errors: list[str]):
    environment = config.scene.environment
    if environment is None:
        return
    if abs(observation.hour - environment.hour) > ENVIRONMENT_HOUR_TOLERANCE:
        errors.append(f"observed hour {observation.hour:.4f} differs from "
                      f"{environment.hour:.4f}")
    pinned = WeatherControl(environment.weather.value)
    if pinned == WeatherControl.SUNNY:
        if observation.weather_severity > ENVIRONMENT_MAX_WEATHER_SEVERITY:
            errors.append(f"observed weather severity {observation.weather_severity:.3f} "
                          f"is not clear")
    elif observation.weather_severity < ENVIRONMENT_MIN_STORM_SEVERITY:
        errors.append(f"observed weather severity {observation.weather_severity:.3f} "
                      f"is not a storm")
    if observation.weather_control is None:
        errors.append("weather control is unobserved")
    elif observation.weather_control != pinned:
        errors.append(f"weather control is {observation.weather_control.value}, "
                      f"not {pinned.value}")


def _bright_fractions(feature: Feature, root: Path, capture: Capture, index: int):
    compare_mode = CompareMode(capture.mode.value)
    spec = SampleSpec(Path(f"{capture.id}.capture.png"), Path(f"{capture.id}.capture.json"))
    sample = load_sample(root, compare_mode, index, spec)
    fractions = []
    for eye in compare_mode.eyes:
        values = load_stage(root, compare_mode, spec, sample, feature.stage, eye).values
        fractions.append(feature.bright_fraction(values))
    return tuple(fractions)


def write_report(path: Path, report: ExperimentReport):
    temporary = Path(path).with_suffix(".tmp")
    temporary.write_text(json.dumps(report.to_wire(), indent=2) + "\n")
    temporary.replace(path)
    LOG.info("wrote %s", path)


def _motion(value) -> ExperimentMotion | None:
    if "motion" not in value:
        return None
    try:
        return ExperimentMotion.from_toml(_string(value.get("name"), "config.name"), value["motion"])
    except MotionConfigError as error:
        raise ExperimentError(str(error)) from error


def _configuration_list(value):
    if not isinstance(value, list) or not value:
        raise ExperimentError("experiment: config.configurations must be a nonempty array")
    modes = []
    for index, entry in enumerate(value):
        entry = _table(entry, f"configurations[{index}]")
        _keys(entry, {"mode"}, set(), f"configurations[{index}]")
        modes.append(_mode(entry.get("mode"), f"configurations[{index}].mode"))
    return tuple(modes)


def _mode(value, context) -> CompareMode:
    try:
        return CompareMode(_string(value, context))
    except ValueError as error:
        raise ExperimentError(f"experiment: {context} is not a supported mode") from error


def _mode_list(value, context):
    if not isinstance(value, list) or not value:
        raise ExperimentError(f"experiment: {context} must be a nonempty array")
    modes = tuple(_mode(item, f"{context}[{index}]") for index, item in enumerate(value))
    if len(set(modes)) != len(modes):
        raise ExperimentError(f"experiment: {context} must not repeat a mode")
    return modes


def _table(value, context):
    if not isinstance(value, dict):
        raise ExperimentError(f"experiment: {context} must be a table")
    return value


def _keys(value, required, optional, context):
    missing, extra = required - set(value), set(value) - required - optional
    if missing or extra:
        raise ExperimentError(f"experiment: {context} has missing {sorted(missing)} "
                              f"and unknown {sorted(extra)} keys")


def _string(value, context):
    if not isinstance(value, str) or not value:
        raise ExperimentError(f"experiment: {context} must be a nonempty string")
    return value


def _number(value, context):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise ExperimentError(f"experiment: {context} must be a finite number")
    return float(value)


def _integer(value, context):
    if type(value) is not int:
        raise ExperimentError(f"experiment: {context} must be an integer")
    return value


def _vector(value, length, context):
    if not isinstance(value, (list, tuple)) or len(value) != length:
        raise ExperimentError(f"experiment: {context} must have {length} components")
    return tuple(_number(item, f"{context}[{index}]") for index, item in enumerate(value))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Run a configured rendering-equivalence experiment.")
    parser.add_argument("--runner", type=Path, required=True,
                        help="runner.json from the running private rendering session")
    parser.add_argument("--config", type=Path, required=True,
                        help="scene configuration for the experiment")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--restage", action="store_true",
                        help="stage afresh even when this session staged the same scene last")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s",
                        stream=sys.stderr)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        LOG.error("experiment: timeout must be finite and positive")
        return 1
    try:
        config = ExperimentConfig.load(args.config)
        with AttachedSession.attach(args.runner, args.timeout) as runner:
            LOG.info("attached to game pid %d via %s", runner.pid, args.runner)
            report = run_experiment(runner, config, args.output, restage=args.restage)
    except (ExperimentError, ProtocolError, SessionError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 1
    LOG.info("experiment: %s (%s)", report.outcome.value, args.output)
    return report.exit_code


if __name__ == "__main__":
    sys.exit(main())
