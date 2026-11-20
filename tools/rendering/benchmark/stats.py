"""Statistics over a benchmark run's frames, and across the repeats of a run.

Every number is named for what it measures, since the frame interval and the work inside it are
different things under a pacing compositor:

- `frame_ms`: the interval between successive game-thread frame starts. Under a compositor that
  paces below what the game reaches, this is the pacing, not the cost.
- `game_busy_ms`: the game thread's own work in the frame: the engine, the mod, and the OpenXR
  calls that do work, with every wait taken out.
- `draw_ms`: the draw task's CPU time (the engine's walk of the render passes that records the
  draws, with the mod's draw detours inside it, and the mod's post-draw work), summed over the
  frame's dispatches.
- `gpu_ms`: the summed GPU execution windows of the frame's dispatches, from timestamp queries. It
  includes any idle inside a dispatch, so it is an upper bound on the GPU's work; the GPU is the
  limit when it approaches `frame_ms` with `gpu_idle_ms` near zero.
- `wait_*`: the game thread blocked: on the compositor (`xrWaitFrame`), on the draw task (the
  drains), and on the frame tail's hold of the runtime.
- `pose_age_ms`: how old the head pose a submitted frame was rendered from is at its `xrEndFrame`,
  from the view location to the submit; only the rows in which a frame was submitted.
"""

import math
import statistics
from dataclasses import dataclass

FRAME_TIME_THRESHOLDS_FPS = (45, 60, 72, 90, 120, 144)


def percentile(values, q: float) -> float:
    """The `q`th percentile (0 to 100) by linear interpolation between closest ranks, as numpy's
    default."""
    if not values:
        raise ValueError("percentile: no values")
    if not 0 <= q <= 100:
        raise ValueError("percentile: q must be within [0, 100]")
    ordered = sorted(values)
    position = (len(ordered) - 1) * q / 100
    low = math.floor(position)
    high = math.ceil(position)
    if low == high:
        return float(ordered[low])
    return ordered[low] + (ordered[high] - ordered[low]) * (position - low)


@dataclass(frozen=True)
class Distribution:
    count: int
    mean: float
    median: float
    p99: float
    worst: float

    @classmethod
    def of(cls, values) -> "Distribution":
        values = list(values)
        if not values:
            return cls(0, math.nan, math.nan, math.nan, math.nan)
        return cls(len(values), statistics.fmean(values), percentile(values, 50),
                   percentile(values, 99), float(max(values)))

    def to_wire(self) -> dict:
        return {"count": self.count, "mean": _round(self.mean), "median": _round(self.median),
                "p99": _round(self.p99), "worst": _round(self.worst)}


def frame_series(ack: dict) -> dict[str, list[float]]:
    """The run's per-frame metrics in milliseconds (counts as counts), by name."""
    columns = ack["columns"]
    index = {name: i for i, name in enumerate(columns)}
    rows = ack["frames"]

    def col(name):
        return [row[index[name]] for row in rows]

    def ms(name):
        return [value / 1000 for value in col(name)]

    def total(*names):
        return [sum(row[index[name]] for name in names) / 1000 for row in rows]

    series = {
        "frame_ms": ms("frame_us"),
        "game_busy_ms": total("game_mod_us", "game_openxr_us", "game_engine_us"),
        "game_engine_ms": ms("game_engine_us"),
        "game_mod_ms": ms("game_mod_us"),
        "game_openxr_ms": ms("game_openxr_us"),
        "game_wait_ms": ms("game_wait_us"),
        "sim_ms": ms("sim_us"),
        "dispatch_ms": ms("dispatch_us"),
        "draw_ms": total("draw_task_us", "post_draw_us"),
        "draw_task_ms": ms("draw_task_us"),
        "post_draw_ms": ms("post_draw_us"),
        "tail_busy_ms": total("tail_mod_us", "tail_openxr_us"),
        "mod_ms": total("game_mod_us", "tail_mod_us", "other_mod_us"),
        "wait_compositor_ms": ms("vr_wait_frame_us"),
        "wait_draw_ms": ms("drain_us"),
        "wait_tail_lock_ms": ms("vr_lock_us"),
        "tail_drain_ms": ms("tail_drain_us"),
        "submit_ms": ms("submit_us"),
        "end_frame_ms": ms("vr_end_frame_us"),
        "engine_draws": col("engine_draws"),
        "d3d_submissions": col("d3d_submissions"),
        "draw_tasks": col("draw_tasks"),
        "detour_ms": ms("detour_us"),
        "forward_ms": ms("forward_us"),
        "detour_calls": col("detour_calls"),
        "forward_calls": col("forward_calls"),
    }
    if "pose_age_us" in index:
        series["pose_age_ms"] = [row[index["pose_age_us"]] / row[index["pose_ages"]] / 1000
                                 for row in rows if row[index["pose_ages"]]]
    return series


def gpu_frames(ack: dict) -> list[dict[str, float]]:
    """The run's GPU dispatches grouped into frames, in milliseconds: a dispatch of eye 0 opens a
    frame, and an eye 1 or far-field dispatch joins the open one. `gpu_ms` is the summed spans,
    `gpu_idle_ms` the measured idle before each dispatch, and each seam its summed time."""
    gpu = ack.get("gpu")
    if not gpu:
        return []
    index = {name: i for i, name in enumerate(gpu["columns"])}
    seams = gpu["seams"]
    frames: list[dict[str, float]] = []
    for row in gpu["dispatches"]:
        if row[index["lane"]] == 0 or not frames:
            frames.append({"gpu_ms": 0.0, "gpu_idle_ms": 0.0, "dispatches": 0,
                           **{f"gpu_{seam}_ms": 0.0 for seam in seams}})
        frame = frames[-1]
        frame["dispatches"] += 1
        frame["gpu_ms"] += row[index["span_us"]] / 1000
        idle = row[index["idle_before_us"]]
        if idle >= 0:
            frame["gpu_idle_ms"] += idle / 1000
        for seam in seams:
            value = row[index[f"{seam}_us"]]
            if value >= 0:
                frame[f"gpu_{seam}_ms"] += value / 1000
    # A frame cut short at either end of the recording would read low.
    return frames[1:-1] if len(frames) > 2 else frames


def summarize_run(ack: dict) -> dict:
    """The per-metric distributions of one run, and the mod's detour timing when it was asked
    for."""
    series = frame_series(ack)
    result = {name: Distribution.of(values).to_wire() for name, values in series.items()
              if not name.startswith(("detour", "forward"))}
    gpu = gpu_frames(ack)
    if gpu:
        for name in gpu[0]:
            result[name] = Distribution.of([frame[name] for frame in gpu]).to_wire()
    frames = series["frame_ms"]
    if frames:
        total_seconds = sum(frames) / 1000
        result["fps_mean"] = _round(len(frames) / total_seconds) if total_seconds else None
    if ack["request"].get("detour_timing"):
        result["detours"] = detour_summary(series, ack["environment"].get("clock_pair_ns", 0.0))
    return result


def detour_summary(series: dict, clock_pair_ns: float) -> dict:
    """The draw detours' own cost per frame and per draw: their whole time less the forwarding
    calls into the runtime, with the timing's own clock reads (one pair per detour and per forward)
    taken out."""
    frames = len(series["frame_ms"]) or 1
    detour_ns = sum(series["detour_ms"]) * 1e6
    forward_ns = sum(series["forward_ms"]) * 1e6
    detour_calls = sum(series["detour_calls"])
    forward_calls = sum(series["forward_calls"])
    timer_ns = (detour_calls + forward_calls) * clock_pair_ns
    overhead_ns = max(0.0, detour_ns - forward_ns - timer_ns)
    return {
        "detour_calls_per_frame": _round(detour_calls / frames),
        "forward_calls_per_frame": _round(forward_calls / frames),
        "detour_ms_per_frame": _round(detour_ns / frames / 1e6),
        "forward_ms_per_frame": _round(forward_ns / frames / 1e6),
        "overhead_ms_per_frame": _round(overhead_ns / frames / 1e6),
        "overhead_ns_per_draw": _round(overhead_ns / detour_calls) if detour_calls else None,
        "forward_ns_per_call": _round(forward_ns / forward_calls) if forward_calls else None,
        "clock_pair_ns": _round(clock_pair_ns),
    }


def aggregate(summaries: list[dict], metrics=None) -> dict:
    """The repeats of one run combined: for each metric, the median of the repeats' medians and
    p99s, the worst frame of any repeat, and the spread of the medians (smallest, largest)."""
    if not summaries:
        return {"repeats": 0}
    names = metrics or sorted({name for summary in summaries for name, value in summary.items()
                               if isinstance(value, dict) and "median" in value})
    result: dict = {"repeats": len(summaries)}
    for name in names:
        present = [summary[name] for summary in summaries if name in summary
                   and summary[name]["count"]]
        if not present:
            continue
        medians = [entry["median"] for entry in present]
        result[name] = {
            "median": _round(statistics.median(medians)),
            "p99": _round(statistics.median(entry["p99"] for entry in present)),
            "worst": _round(max(entry["worst"] for entry in present)),
            "spread": [_round(min(medians)), _round(max(medians))],
        }
    return result


def threshold_ms(fps: float) -> float:
    return 1000.0 / fps


def _round(value):
    if value is None or (isinstance(value, float) and math.isnan(value)):
        return None
    return round(float(value), 4)
