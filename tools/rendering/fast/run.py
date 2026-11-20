"""Run a fast comparison of one scene against a live session: stage (or reuse the stage this
session staged last), configure each mode, take its GPU snapshots, and gather the payload's
comparisons into rows."""

import dataclasses
import json
import logging
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path

from tools.rendering.experiment import (
    Diagnostics,
    ExperimentConfig,
    ExperimentError,
    _configure,
    _variant_config,
)
from tools.rendering.fast.plan import (
    DEFAULT_DEPTH_ULPS,
    DEFAULT_PLANES,
    FastError,
    Limits,
    Row,
    SnapshotStep,
    rows,
    schedule,
)
from tools.rendering.protocol import (
    Camera,
    Command,
    ConfigurationAck,
    Mode,
    OrientationOffset,
    PoseRequest,
    ProtocolError,
    Request,
    RestorationAck,
    StageAck,
    TranslationOffset,
    offset_for_target,
)
from tools.rendering.protocol.snapshot import (
    DumpPolicy,
    Plane,
    ReleaseAck,
    ReleaseRequest,
    SnapshotAck,
    SnapshotRequest,
)
from tools.rendering.session.errors import SessionError
from tools.rendering.session.stage_record import (
    record,
    record_anchor,
    recorded_anchor,
    reuse_distance,
    stage_key,
)

LOG = logging.getLogger("tools.rendering.fast")

# The failures a scene reports rather than raises: the next scene can still run.
EXPECTED_ERRORS = (ExperimentError, FastError, ProtocolError, SessionError, OSError, ValueError)


@dataclass(frozen=True)
class FastOptions:
    planes: tuple[Plane, ...] = DEFAULT_PLANES
    dump: DumpPolicy = DumpPolicy.NEVER
    dump_crop: bool = True
    limits: Limits = field(default_factory=Limits)
    # Stage afresh even when this session staged the same scene last.
    restage: bool = False
    # Take a second snapshot of each mode for the noise floor.
    repeat: bool = True
    # Run a motion that never moves the camera as configured, rather than as a fixed pose.
    full_cycles: bool = False
    # How many representable floats apart two depths may be and still match.
    depth_ulps: int = DEFAULT_DEPTH_ULPS
    # How long to wait before each repeat snapshot, so the floor includes what changes with time.
    repeat_after_seconds: float = 0.0


@dataclass
class SceneResult:
    name: str
    config: str
    rows: list[Row] = field(default_factory=list)
    floors: dict = field(default_factory=dict)
    timings: list[dict] = field(default_factory=list)
    snapshots: list[dict] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    stage_reused: bool | None = None
    # The camera every configuration after the anchor's rendered from, when pinned.
    anchor: dict | None = None
    seconds: float = 0.0

    def to_wire(self) -> dict:
        return {
            "name": self.name,
            "config": self.config,
            "seconds": self.seconds,
            "stage_reused": self.stage_reused,
            "anchor": self.anchor,
            "errors": self.errors,
            "timings": self.timings,
            "snapshots": self.snapshots,
            "floors": [{"mode": mode, "plane": plane, "eye": eye, "differing": count}
                       for (mode, plane, eye), count in sorted(self.floors.items())],
            "rows": [row.to_wire() for row in self.rows],
        }


def fast_config(config: ExperimentConfig) -> ExperimentConfig:
    """The configuration a fast comparison runs: the experiment's scene, modes, overrides, and
    variants, without its screenshot diagnostics (the snapshots take none)."""
    if config.matched_treatment is not None or config.feature is not None:
        raise FastError(f"fast comparison: {config.name} has a matched treatment or a feature check")
    return dataclasses.replace(config, diagnostics=Diagnostics(), interval_seconds=0.0)


def holds_still(config: ExperimentConfig, full_cycles: bool) -> bool:
    """Whether the scene's motion, if any, never moves the camera, so the fast comparison holds the
    game's camera with a fixed pose instead: its snapshots then wait only for the configuration's
    warm-up, and each repeat is the next frame, rather than a cycle each."""
    return config.motion is not None and not full_cycles and _still(config.motion.request)


def pinned_anchor(config_dir, config: ExperimentConfig) -> tuple[Camera | None, str | None]:
    """The camera a scene's configurations render from, pinned so that it is the same to the bit
    from run to run and between the scenes at one stage, and the stage key to record a camera under
    when none is pinned yet. A fixed-camera scene pins its own camera, a scene with a
    `scene.anchor` that, and a staged scene the camera the first fast comparison at its stage in
    this session froze (recorded in the session's configuration directory); a scene with none of
    these is not pinned."""
    if config.scene.pose is not None:
        return config.scene.pose.camera(), None
    if config.scene.anchor is not None:
        return config.scene.anchor.camera(), None
    if config.scene.stage is None:
        return None, None
    key = stage_key(config.scene.stage, config.scene.environment)
    recorded = recorded_anchor(config_dir, key)
    if recorded is not None:
        return Camera(*recorded), None
    return None, key


def _still(request) -> bool:
    return not any((request.yaw_deg, request.pitch_deg, request.roll_deg, request.right_m,
                    request.up_m, request.forward_m))


def run_scene(runner, config: ExperimentConfig, name: str, options: FastOptions,
              baseline: dict | None = None, monotonic=time.monotonic) -> SceneResult:
    """Run one scene's fast comparison on an attached session `runner`."""
    started = monotonic()
    result = SceneResult(name, config.name)
    config = fast_config(config)
    still = holds_still(config, options.full_cycles)
    # A variant that forces no switch renders the single-pass mode again, which the repeat already
    # does in the same schedule.
    variants = tuple(variant.name for variant in config.single_pass_variants if variant.switches)
    steps = schedule(config.configurations, variants, options.repeat)
    prefix = f"f{uuid.uuid4().hex[:8]}"
    configurations: list[ConfigurationAck] = []
    gathered: list[tuple[SnapshotStep, tuple]] = []

    def timed(step, request):
        begun = monotonic()
        acknowledgement = runner.request(request)
        entry = {"step": step, "seconds": round(monotonic() - begun, 4)}
        result.timings.append(entry)
        return acknowledgement, entry

    try:
        if config.scene.stage is not None:
            result.stage_reused = _stage(runner, config, prefix, options, timed)
        # The first configuration after a restoration renders differently from the ones after it
        # (an isolated eye's snapshot from it differed from multipass at nearly every pixel in run
        # 297), so an anchor configuration always comes first, as in the original path.
        anchor_mode = Mode(config.configurations[0].value)
        anchor_pose = PoseRequest(f"{prefix}_anchor", OrientationOffset(0.0, 0.0, 0.0),
                                  TranslationOffset())
        pin, record_key = pinned_anchor(runner.config_dir, config)
        begun = monotonic()
        _, anchor = _configure(runner, config, f"{prefix}_anchor", anchor_pose, anchor_mode, None,
                               configurations, still, pin)
        result.timings.append({"step": "anchor", "seconds": round(monotonic() - begun, 4),
                               "frames": configurations[-1].metadata.configure_frames})
        if pin is None and record_key is not None:
            # The first scene at this stage pins the camera the freeze captured for every
            # configuration after the anchor's and for every later scene at the stage.
            pin = Camera(tuple(anchor.position), tuple(anchor.orientation))
            record_anchor(runner.config_dir, record_key, pin.position, pin.orientation)
        result.anchor = None if pin is None else {"position": list(pin.position),
                                                  "orientation": list(pin.orientation)}
        target_pose = None
        if still or (config.motion is None and pin is not None):
            target_pose = PoseRequest(f"{prefix}_target", OrientationOffset(0.0, 0.0, 0.0),
                                      TranslationOffset())
        elif config.motion is None:
            offset, translation = offset_for_target(anchor, config.scene.pose.camera())
            target_pose = PoseRequest(f"{prefix}_target", offset, translation)
        phase = config.motion.phase if config.motion is not None and not still else None
        for index, step in enumerate(steps, start=1):
            step_config = config
            if step.variant is not None:
                variant = next(v for v in config.single_pass_variants if v.name == step.variant)
                step_config = dataclasses.replace(_variant_config(config, variant),
                                                  diagnostics=Diagnostics())
            begun = monotonic()
            _, anchor = _configure(runner, step_config, f"{prefix}_c{index}", target_pose,
                                   Mode(step.mode.value), None if pin is not None else anchor,
                                   configurations, still, pin)
            result.timings.append({"step": f"configure {step.name}",
                                   "seconds": round(monotonic() - begun, 4),
                                   "frames": configurations[-1].metadata.configure_frames})
            for snapshot in step.snapshots:
                if snapshot.repeat_of is not None and options.repeat_after_seconds > 0:
                    time.sleep(options.repeat_after_seconds)
                request = SnapshotRequest(
                    f"{prefix}_{snapshot.label}"[:64], snapshot.label, options.planes, snapshot.keep,
                    snapshot.compare, phase, options.limits.thresholds.pixel,
                    options.limits.thresholds.changed_fraction, options.limits.thresholds.mean,
                    options.dump, options.dump_crop, options.depth_ulps)
                acknowledgement, entry = timed(f"snapshot {snapshot.label}", request)
                if not isinstance(acknowledgement, SnapshotAck):
                    raise FastError("fast comparison: expected a snapshot acknowledgement")
                if acknowledgement.error is not None:
                    raise FastError(f"fast comparison: {snapshot.label}: {acknowledgement.error}")
                if acknowledgement.mode != step.mode.value:
                    raise FastError(f"fast comparison: {snapshot.label} rendered "
                                    f"{acknowledgement.mode}, not {step.mode.value}")
                entry.update(frames=acknowledgement.frames_waited,
                             gpu_ms=round(acknowledgement.gpu_ms, 2))
                result.snapshots.append({
                    "label": snapshot.label, "frame": acknowledgement.frame,
                    "eye_size": [acknowledgement.eye_width, acknowledgement.eye_height],
                    "frames_waited": acknowledgement.frames_waited,
                    "wait_ms": acknowledgement.wait_ms, "gpu_ms": acknowledgement.gpu_ms,
                    "generation": acknowledgement.visual.get("generation"),
                })
                gathered.append((snapshot, acknowledgement.comparisons))
    except EXPECTED_ERRORS as error:
        result.errors.append(str(error))
    finally:
        for name_, request in (("release", ReleaseRequest(f"{prefix}_release")),
                               ("end", Request(Command.END, f"{prefix}_end"))):
            try:
                runner.cancel_pending_request()
                acknowledgement, _ = timed(name_, request)
                if not isinstance(acknowledgement, (ReleaseAck, RestorationAck)):
                    raise FastError(f"fast comparison: unexpected {name_} acknowledgement")
            except EXPECTED_ERRORS as error:
                result.errors.append(f"{name_}: {error}")
    result.rows, result.floors = rows(name, gathered, options.limits, baseline)
    result.seconds = round(monotonic() - started, 3)
    return result


def _stage(runner, config: ExperimentConfig, prefix: str, options: FastOptions, timed) -> bool:
    """Stage the scene, reusing it when this session staged the same scene last. Returns whether the
    payload reused it."""
    key = stage_key(config.scene.stage, config.scene.environment)
    reuse = None if options.restage else reuse_distance(runner.config_dir, key)
    request = dataclasses.replace(
        config.scene.stage, id=f"{prefix}_stage", reuse_within_m=reuse,
        environment=config.scene.environment.request() if config.scene.environment else None)
    acknowledgement, entry = timed("stage", request)
    if not isinstance(acknowledgement, StageAck):
        raise FastError("fast comparison: expected the stage's acknowledgement")
    entry["reused"] = acknowledgement.reused
    record(runner.config_dir, key)
    LOG.info("fast: %s the stage (player %s m from the location)",
             "reused" if acknowledgement.reused else "staged", acknowledgement.distance_m)
    return acknowledgement.reused


def write_report(path: Path, scenes: list[SceneResult], extra: dict) -> None:
    value = dict(extra)
    value["scenes"] = [scene.to_wire() for scene in scenes]
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=1) + "\n")
    temporary.replace(path)
