"""Combine a benchmark's result files into a summary: the repeats of each scene, mode, and render
size aggregated, written as `summary.json` and a Markdown table."""

import json
from collections import defaultdict
from pathlib import Path

from tools.rendering.benchmark.run import RUN_FORMAT, RUN_FORMAT_VERSION
from tools.rendering.benchmark.stats import aggregate

SUMMARY_FORMAT = "jc3vr-benchmark-summary"
SUMMARY_FORMAT_VERSION = 1

# The metrics the Markdown table shows, in order, with their column headings.
TABLE_METRICS = (
    ("frame_ms", "frame"),
    ("game_busy_ms", "game busy"),
    ("draw_ms", "draw task"),
    ("gpu_ms", "GPU"),
    ("gpu_idle_ms", "GPU idle"),
    ("wait_compositor_ms", "xrWaitFrame"),
    ("wait_draw_ms", "drain"),
    ("wait_tail_lock_ms", "tail lock"),
    ("pose_age_ms", "pose age"),
    ("mod_ms", "mod"),
    ("gpu_fsr_ms", "FSR (GPU)"),
    ("d3d_submissions", "submissions"),
    ("engine_draws", "engine draws"),
)


def load_runs(root: Path) -> list[dict]:
    runs = []
    for path in sorted((root / "runs").rglob("*.json")):
        value = json.loads(path.read_text())
        if value.get("format") != RUN_FORMAT or value.get("version") != RUN_FORMAT_VERSION:
            raise ValueError(f"benchmark summary: {path} is not a version "
                             f"{RUN_FORMAT_VERSION} run file")
        runs.append(value)
    return runs


def group_key(run: dict) -> tuple:
    payload = run["payload"]
    size = payload["environment"].get("engine_size") or [0, 0]
    double_wide = payload["environment"].get("double_wide_active")
    eye = [size[0] // 2, size[1]] if double_wide else list(size)
    return (run["run"]["scene"], run["run"]["mode"], run["run"]["resolution_scale"], tuple(eye),
            run["session"].get("compositor_fps"), bool(payload["request"].get("detour_timing")),
            bool(payload["environment"].get("com_detours_installed")))


def summarize(runs: list[dict]) -> dict:
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for run in runs:
        groups[group_key(run)].append(run)
    entries = []
    for (scene, mode, scale, eye, fps, detours, draw_detours), members in sorted(
            groups.items(), key=lambda item: str(item[0])):
        summaries = [member["summary"] for member in members]
        entry = {"scene": scene, "mode": mode, "resolution_scale": scale, "eye_size": list(eye),
                 "megapixels_per_eye": round(eye[0] * eye[1] / 1e6, 3), "compositor_fps": fps,
                 "detour_timing": detours, "draw_detours_installed": draw_detours,
                 **aggregate(summaries)}
        threads = [member["threads"] for member in members if member.get("threads")]
        if threads:
            names = sorted({name for t in threads for name in t["groups"]})
            entry["threads_busy"] = {
                name: round(sorted(t["groups"].get(name, {}).get("busy", 0.0)
                                   for t in threads)[len(threads) // 2], 3)
                for name in names}
        detour_runs = [s["detours"] for s in summaries if "detours" in s]
        if detour_runs:
            entry["detours"] = detour_runs[len(detour_runs) // 2]
        entries.append(entry)
    machine = runs[0]["machine"] if runs else None
    return {"format": SUMMARY_FORMAT, "version": SUMMARY_FORMAT_VERSION, "machine": machine,
            "groups": entries}


def markdown(summary: dict) -> str:
    header = ["scene", "mode", "eye size", "pace (fps)"] + [title for _, title in TABLE_METRICS]
    lines = ["Median per frame over the repeats, then the 1% low (99th percentile) and the worst "
             "frame of the frame interval; milliseconds unless a count. The spread is the "
             "smallest and largest repeat median.", "",
             "| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for entry in summary["groups"]:
        notes = (" (detour timing)" if entry["detour_timing"] else "") + (
            "" if entry["draw_detours_installed"] else " (no draw detours)")
        cells = [entry["scene"], entry["mode"] + notes,
                 f"{entry['eye_size'][0]}x{entry['eye_size'][1]}", str(entry["compositor_fps"])]
        for name, _ in TABLE_METRICS:
            metric = entry.get(name)
            if metric is None:
                cells.append("")
            elif name == "frame_ms":
                cells.append(f"{metric['median']:.2f} / {metric['p99']:.2f} / {metric['worst']:.1f} "
                             f"({metric['spread'][0]:.2f}-{metric['spread'][1]:.2f})")
            elif name in ("d3d_submissions", "engine_draws"):
                cells.append(f"{metric['median']:,.0f}")
            else:
                cells.append(f"{metric['median']:.2f}")
        lines.append("| " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def write_summary(root: Path) -> dict:
    summary = summarize(load_runs(root))
    (root / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    (root / "summary.md").write_text(markdown(summary))
    return summary
