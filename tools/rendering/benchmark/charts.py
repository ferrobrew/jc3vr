"""Render a benchmark's charts as standalone SVG files, with no dependencies beyond the standard
library: the frame-time distribution of each scene and mode, and the frame time against the render
size for each mode, both against the frame budgets of the common headset refresh rates.

Run as `python3 -m tools.rendering benchmark charts <output>`; the files land in `<output>/charts/`."""

import math
from collections import defaultdict
from html import escape
from pathlib import Path

from tools.rendering.benchmark.report import load_runs
from tools.rendering.benchmark.stats import FRAME_TIME_THRESHOLDS_FPS, frame_series, percentile

# Categorical slots in fixed order (the reference palette's first slots), by mode name.
PALETTE = ("#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948")
TEXT = "#0b0b0b"
MUTED = "#52514e"
GRID = "#e4e3df"
SURFACE = "#fcfcfb"
FONT = "font-family='system-ui, sans-serif'"


def mode_colours(modes) -> dict[str, str]:
    """A colour per mode, multipass and single-pass first, so a mode keeps its colour in every
    chart."""
    ordered = sorted(set(modes), key=lambda m: (m not in ("multipass", "single_pass"), m))
    return {mode: PALETTE[i % len(PALETTE)] for i, mode in enumerate(ordered)}


def distribution_chart(rows: list[tuple[str, str, list[float]]], colours, title: str) -> str:
    """One row per (scene, mode): the 1st to 99th percentile as a line, the interquartile range as a
    bar, and the median as a tick, against the frame budgets."""
    width, row_h, left, right, top = 900, 26, 220, 30, 60
    height = top + row_h * len(rows) + 50
    top_ms = max([percentile(v, 99) for _, _, v in rows] + [22.2]) * 1.08
    scale = (width - left - right) / top_ms

    def x(ms):
        return left + ms * scale

    parts = [_header(width, height, title)]
    parts += _budget_lines(x, top - 10, top + row_h * len(rows), vertical=True)
    for i, (scene, mode, values) in enumerate(rows):
        y = top + i * row_h + row_h / 2
        p1, p25, p50, p75, p99 = (percentile(values, q) for q in (1, 25, 50, 75, 99))
        colour = colours[mode]
        parts.append(f"<text x='{left - 8}' y='{y + 4}' text-anchor='end' font-size='12' "
                     f"fill='{TEXT}' {FONT}>{escape(scene)} · {escape(mode)}</text>")
        parts.append(f"<line x1='{x(p1):.1f}' x2='{x(p99):.1f}' y1='{y}' y2='{y}' "
                     f"stroke='{colour}' stroke-width='2'/>")
        parts.append(f"<rect x='{x(p25):.1f}' y='{y - 7}' width='{max(x(p75) - x(p25), 2):.1f}' "
                     f"height='14' rx='4' fill='{colour}'/>")
        parts.append(f"<line x1='{x(p50):.1f}' x2='{x(p50):.1f}' y1='{y - 9}' y2='{y + 9}' "
                     f"stroke='{SURFACE}' stroke-width='2'/>")
        parts.append(f"<title>{escape(scene)} {escape(mode)}: median {p50:.2f} ms, "
                     f"p99 {p99:.2f} ms</title>")
    axis_y = top + row_h * len(rows) + 8
    for tick in _ticks(top_ms):
        parts.append(f"<text x='{x(tick):.1f}' y='{axis_y + 14}' text-anchor='middle' "
                     f"font-size='11' fill='{MUTED}' {FONT}>{tick:g}</text>")
    parts.append(f"<text x='{(left + width - right) / 2}' y='{axis_y + 32}' text-anchor='middle' "
                 f"font-size='12' fill='{MUTED}' {FONT}>frame time (ms): 1st-99th percentile, "
                 f"interquartile range, median</text>")
    parts.append("</svg>\n")
    return "\n".join(parts)


def resolution_chart(series: dict[str, list[tuple[float, float, float | None]]], colours,
                     title: str) -> str:
    """Median frame time (solid) and median GPU time (dashed) against megapixels per eye, one
    colour per mode, against the frame budgets."""
    width, height, left, right, top, bottom = 900, 520, 70, 160, 60, 60
    xs = [p[0] for points in series.values() for p in points]
    ys = [v for points in series.values() for p in points for v in p[1:] if v is not None]
    if not xs:
        return ""
    x_max = max(xs) * 1.05
    y_max = max(ys + [22.2]) * 1.1
    plot_w, plot_h = width - left - right, height - top - bottom

    def x(mp):
        return left + mp / x_max * plot_w

    def y(ms):
        return top + plot_h - ms / y_max * plot_h

    parts = [_header(width, height, title)]
    for tick in _ticks(y_max):
        parts.append(f"<line x1='{left}' x2='{left + plot_w}' y1='{y(tick):.1f}' y2='{y(tick):.1f}' "
                     f"stroke='{GRID}' stroke-width='1'/>")
        parts.append(f"<text x='{left - 6}' y='{y(tick) + 4:.1f}' text-anchor='end' font-size='11' "
                     f"fill='{MUTED}' {FONT}>{tick:g}</text>")
    for tick in _ticks(x_max):
        parts.append(f"<text x='{x(tick):.1f}' y='{top + plot_h + 16}' text-anchor='middle' "
                     f"font-size='11' fill='{MUTED}' {FONT}>{tick:g}</text>")
    for fps in FRAME_TIME_THRESHOLDS_FPS:
        ms = 1000 / fps
        if ms < y_max:
            parts.append(f"<line x1='{left}' x2='{left + plot_w}' y1='{y(ms):.1f}' y2='{y(ms):.1f}' "
                         f"stroke='{MUTED}' stroke-width='1' stroke-dasharray='2 4'/>")
            parts.append(f"<text x='{left + plot_w + 4}' y='{y(ms) + 4:.1f}' font-size='10' "
                         f"fill='{MUTED}' {FONT}>{fps} fps ({ms:.1f} ms)</text>")
    legend_y = top
    for mode, points in series.items():
        colour = colours[mode]
        points = sorted(points)
        frame = " ".join(f"{x(mp):.1f},{y(ms):.1f}" for mp, ms, _ in points)
        parts.append(f"<polyline points='{frame}' fill='none' stroke='{colour}' stroke-width='2'/>")
        gpu = " ".join(f"{x(mp):.1f},{y(g):.1f}" for mp, _, g in points if g is not None)
        if gpu:
            parts.append(f"<polyline points='{gpu}' fill='none' stroke='{colour}' stroke-width='2' "
                         f"stroke-dasharray='6 4'/>")
        for mp, ms, g in points:
            parts.append(f"<circle cx='{x(mp):.1f}' cy='{y(ms):.1f}' r='4' fill='{colour}' "
                         f"stroke='{SURFACE}' stroke-width='2'><title>{escape(mode)}: {mp:.2f} MP "
                         f"per eye, frame {ms:.2f} ms"
                         + (f", GPU {g:.2f} ms" if g is not None else "") + "</title></circle>")
        parts.append(f"<rect x='{width - right + 70}' y='{legend_y}' width='12' height='12' rx='3' "
                     f"fill='{colour}'/>")
        parts.append(f"<text x='{width - right + 88}' y='{legend_y + 11}' font-size='12' "
                     f"fill='{TEXT}' {FONT}>{escape(mode)}</text>")
        legend_y += 20
    parts.append(f"<text x='{width - right + 70}' y='{legend_y + 14}' font-size='11' fill='{MUTED}' "
                 f"{FONT}>solid: frame</text>")
    parts.append(f"<text x='{width - right + 70}' y='{legend_y + 30}' font-size='11' fill='{MUTED}' "
                 f"{FONT}>dashed: GPU</text>")
    parts.append(f"<text x='{left + plot_w / 2}' y='{height - 20}' text-anchor='middle' "
                 f"font-size='12' fill='{MUTED}' {FONT}>megapixels per eye</text>")
    parts.append(f"<text x='18' y='{top + plot_h / 2}' font-size='12' fill='{MUTED}' {FONT} "
                 f"transform='rotate(-90 18 {top + plot_h / 2})' text-anchor='middle'>median "
                 f"milliseconds per frame</text>")
    parts.append("</svg>\n")
    return "\n".join(parts)


# A run whose median frame exceeds this is left out of the charts: at that point the GPU has run out
# of memory and the frame measures paging, which would flatten every other point.
MAX_CHARTED_FRAME_MS = 100.0


def render(root: Path) -> list[Path]:
    runs = [run for run in load_runs(root) if not run["payload"]["request"].get("detour_timing")
            and run["summary"]["frame_ms"]["median"] <= MAX_CHARTED_FRAME_MS]
    out = root / "charts"
    out.mkdir(exist_ok=True)
    colours = mode_colours(run["run"]["mode"] for run in runs)
    written = []
    by_scale: dict[object, list[dict]] = defaultdict(list)
    for run in runs:
        by_scale[(run["run"]["resolution_scale"], run["session"].get("compositor_fps"))].append(run)
    for (scale, fps), members in sorted(by_scale.items(), key=lambda item: str(item[0])):
        pooled: dict[tuple[str, str], list[float]] = defaultdict(list)
        for run in members:
            pooled[(run["run"]["scene"], run["run"]["mode"])] += frame_series(run["payload"])[
                "frame_ms"]
        rows = [(scene, mode, values) for (scene, mode), values in sorted(pooled.items())
                if values]
        name = f"distribution-scale-{'native' if scale is None else f'{scale:g}'}-pace-{fps}"
        eye = _eye(members[0])
        path = out / f"{name}.svg"
        path.write_text(distribution_chart(rows, colours, f"Frame time per scene and mode, "
                                           f"{eye} per eye, compositor pace {fps} fps"))
        written.append(path)
    by_scene: dict[tuple, list[dict]] = defaultdict(list)
    for run in runs:
        by_scene[(run["run"]["scene"], run["session"].get("compositor_fps"))].append(run)
    for (scene, fps), members in sorted(by_scene.items()):
        points: dict[str, dict[float, list[tuple[float, float | None]]]] = defaultdict(
            lambda: defaultdict(list))
        for run in members:
            size = run["payload"]["environment"].get("engine_size") or [0, 0]
            wide = run["payload"]["environment"].get("double_wide_active")
            megapixels = (size[0] // (2 if wide else 1)) * size[1] / 1e6
            summary = run["summary"]
            points[run["run"]["mode"]][round(megapixels, 3)].append(
                (summary["frame_ms"]["median"], summary.get("gpu_ms", {}).get("median")))
        series = {}
        for mode, by_size in points.items():
            series[mode] = [(mp, _median([f for f, _ in values]),
                             _median([g for _, g in values if g is not None]))
                            for mp, values in by_size.items()]
        if sum(len(v) for v in series.values()) < 2 * len(series) or not any(
                len(v) > 1 for v in series.values()):
            continue
        path = out / f"resolution-{scene}-pace-{fps}.svg"
        path.write_text(resolution_chart(series, colours, f"Frame time against render size: "
                                         f"{scene}, compositor pace {fps} fps"))
        written.append(path)
    return written


def _eye(run: dict) -> str:
    size = run["payload"]["environment"].get("engine_size") or [0, 0]
    wide = run["payload"]["environment"].get("double_wide_active")
    return f"{size[0] // (2 if wide else 1)}x{size[1]}"


def _median(values):
    values = sorted(values)
    if not values:
        return None
    middle = len(values) // 2
    return values[middle] if len(values) % 2 else (values[middle - 1] + values[middle]) / 2


def _header(width, height, title) -> str:
    return (f"<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}' "
            f"viewBox='0 0 {width} {height}'>\n<rect width='100%' height='100%' fill='{SURFACE}'/>\n"
            f"<text x='20' y='30' font-size='16' fill='{TEXT}' {FONT}>{escape(title)}</text>")


def _budget_lines(x, y0, y1, vertical: bool) -> list[str]:
    parts = []
    for fps in FRAME_TIME_THRESHOLDS_FPS:
        ms = 1000 / fps
        parts.append(f"<line x1='{x(ms):.1f}' x2='{x(ms):.1f}' y1='{y0}' y2='{y1}' stroke='{MUTED}' "
                     f"stroke-width='1' stroke-dasharray='2 4'/>")
        parts.append(f"<text x='{x(ms):.1f}' y='{y0 - 4}' text-anchor='middle' font-size='10' "
                     f"fill='{MUTED}' {FONT}>{fps}</text>")
    return parts


def _ticks(maximum: float) -> list[float]:
    raw = maximum / 6
    magnitude = 10 ** math.floor(math.log10(raw)) if raw > 0 else 1
    step = next(m * magnitude for m in (1, 2, 2.5, 5, 10) if m * magnitude >= raw)
    return [i * step for i in range(int(maximum / step) + 1)]
