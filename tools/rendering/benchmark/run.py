"""Run the benchmark against a live session: stage each scene, then for each render size, repeat,
and mode, ask the payload for a run while sampling the game's threads, and write the run's result
file."""

import dataclasses
import json
import logging
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

from tools.rendering.benchmark.machine import machine_info
from tools.rendering.benchmark.stats import summarize_run
from tools.rendering.benchmark.suite import BenchmarkError, BenchmarkScene, Variant
from tools.rendering.benchmark.threads import ThreadSampler, window
from tools.rendering.protocol import StageAck
from tools.rendering.protocol.benchmark import BenchmarkAck, BenchmarkMode, BenchmarkRequest
from tools.rendering.session.stage_record import record, reuse_distance, stage_key

LOG = logging.getLogger("tools.rendering.benchmark")

# The result files' format. Bump the version when a field changes meaning or goes away; adding a
# field keeps it.
RUN_FORMAT = "jc3vr-benchmark-run"
RUN_FORMAT_VERSION = 1


@dataclass(frozen=True)
class Mode:
    """A mode a run measures: multipass, single-pass, or a variant of either."""
    name: str
    mode: BenchmarkMode
    switches: tuple[tuple[str, bool], ...] = ()
    overlap_render_update: bool | None = None
    fsr: bool | None = None
    hybrid_velocity: bool | None = None

    @classmethod
    def plain(cls, mode: BenchmarkMode) -> "Mode":
        return cls(mode.value, mode)

    @classmethod
    def variant(cls, variant: Variant) -> "Mode":
        return cls(variant.name, variant.base, variant.switches, variant.overlap_render_update,
                   variant.fsr, variant.hybrid_velocity)


@dataclass(frozen=True)
class Options:
    record_seconds: float = 10.0
    warmup_seconds: float = 3.0
    repeats: int = 3
    scales: tuple[float | None, ...] = (None,)
    gpu: bool = True
    detour_timing: bool = False
    # Install the stereo draw path's detours in every run, so multipass's submissions are counted;
    # without it, a fresh session's multipass runs measure the game with no draw detours at all.
    install_draw_detours: bool = True
    restage: bool = False
    # Recorded in each result, since the payload cannot see it: the null compositor's pace.
    compositor_fps: int | None = None
    session: int | None = None


def schedule(modes: tuple[Mode, ...], repeats: int, scales) -> list[tuple[float | None, int, Mode]]:
    """The order of a scene's runs: each scale in turn, and within it the repeats, each running every
    mode once. The modes rotate from repeat to repeat, so a drift over the scene (streaming, the
    time of day) does not always land on the same mode."""
    order = []
    for scale in scales:
        for repeat in range(repeats):
            shift = repeat % len(modes)
            for mode in modes[shift:] + modes[:shift]:
                order.append((scale, repeat, mode))
    return order


def run_label(scene: str, mode: str, scale: float | None, repeat: int) -> str:
    """A run's label, which is also its result file's name: a request ID, so the scale's decimal
    point is written as `p`."""
    scale_name = "native" if scale is None else f"{scale:g}".replace(".", "p")
    return f"{scene}-{mode}-{scale_name}-r{repeat}"[:64]


def run_scene(session, scene: BenchmarkScene, modes: tuple[Mode, ...], options: Options,
              output: Path) -> list[Path]:
    """Run one scene's schedule; returns the result files written. The scene's last run puts the
    configuration back and releases the held camera, so the next scene stages with the camera
    following the player."""
    _stage(session, scene, options.restage)
    machine = machine_info()
    written = []
    steps = schedule(modes, options.repeats, options.scales)
    for index, (scale, repeat, mode) in enumerate(steps):
        label = run_label(scene.name, mode.name, scale, repeat)
        request = BenchmarkRequest(
            id=f"b{uuid.uuid4().hex[:12]}", label=label, mode=mode.mode,
            record_seconds=options.record_seconds, warmup_seconds=options.warmup_seconds,
            resolution_scale=scale, gpu=options.gpu, detour_timing=options.detour_timing,
            install_draw_detours=options.install_draw_detours,
            recapture_camera=index == 0, restore=index == len(steps) - 1,
            switches=mode.switches, overlap_render_update=mode.overlap_render_update,
            fsr=mode.fsr, fsr_hybrid_velocity=mode.hybrid_velocity, camera=scene.camera)
        LOG.info("benchmark: %s (%d of %d)", label, index + 1, len(steps))
        began = time.monotonic()
        with ThreadSampler(session.pid) as sampler:
            acknowledgement = session.request(request)
        if not isinstance(acknowledgement, BenchmarkAck):
            raise BenchmarkError(f"benchmark: {label}: expected a benchmark acknowledgement")
        ack = acknowledgement.value
        timing = ack["timing"]
        threads = window(sampler.samples, timing["epoch_start"], timing["epoch_end"], session.pid)
        result = {
            "format": RUN_FORMAT,
            "version": RUN_FORMAT_VERSION,
            "run": {"scene": scene.name, "config": scene.config.name, "mode": mode.name,
                    "base_mode": mode.mode.value, "switches": dict(mode.switches),
                    "overlap_render_update": mode.overlap_render_update,
                    "fsr": mode.fsr, "hybrid_velocity": mode.hybrid_velocity,
                    "resolution_scale": scale, "repeat": repeat, "label": label,
                    "camera": scene.camera.to_wire(), "seconds": round(time.monotonic() - began, 2)},
            "session": {"number": options.session, "compositor_fps": options.compositor_fps},
            "machine": machine,
            "payload": {key: value for key, value in ack.items() if key not in ("id", "state")},
            "threads": threads,
            "summary": summarize_run(ack),
        }
        path = output / "runs" / scene.name / f"{label}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(result, separators=(",", ":")) + "\n")
        temporary.replace(path)
        written.append(path)
        summary = result["summary"]
        LOG.info("benchmark: %s: frame %.2f ms median, %.2f p99; game busy %.2f; draw %.2f; GPU %s; "
                 "%s submissions", label, summary["frame_ms"]["median"], summary["frame_ms"]["p99"],
                 summary["game_busy_ms"]["median"], summary["draw_ms"]["median"],
                 summary.get("gpu_ms", {}).get("median"), summary["d3d_submissions"]["median"])
    return written


def _stage(session, scene: BenchmarkScene, restage: bool) -> None:
    key = stage_key(scene.stage, scene.environment)
    reuse = None if restage else reuse_distance(session.config_dir, key)
    request = dataclasses.replace(
        scene.stage, id=f"s{uuid.uuid4().hex[:12]}", reuse_within_m=reuse,
        environment=scene.environment.request() if scene.environment else None)
    acknowledgement = session.request(request)
    if not isinstance(acknowledgement, StageAck):
        raise BenchmarkError("benchmark: expected the stage's acknowledgement")
    record(session.config_dir, key)
    LOG.info("benchmark: %s the stage for %s", "reused" if acknowledgement.reused else "staged",
             scene.name)
