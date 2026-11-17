"""JSON and HTML reports for repeated visual comparisons."""

import html
import json
from dataclasses import fields, is_dataclass


def to_wire(value):
    if is_dataclass(value):
        return {
            item.name: to_wire(getattr(value, item.name))
            for item in fields(value)
            if item.metadata.get("serialize", True)
        }
    if isinstance(value, (tuple, list)):
        return [to_wire(item) for item in value]
    return value


def image_html(parts, artifacts):
    if artifacts is None:
        return
    for item in fields(artifacts):
        path = getattr(artifacts, item.name)
        if path is not None:
            escaped = html.escape(path, quote=True)
            parts.append(
                f"<a href='{escaped}'><img src='{escaped}' alt='{item.name}' title='{item.name}'></a>"
            )


def append_comparison_html(parts, comparison):
    parts.append(
        f"<h4>{html.escape(comparison.name)}: <span class='{comparison.status}'>"
        f"{comparison.status}</span></h4>"
    )
    parts.extend(f"<pre>{html.escape(error)}</pre>" for error in comparison.errors)
    if comparison.worst_metrics is None:
        return
    parts.append(
        "<pre>Worst metrics: "
        + html.escape(json.dumps(to_wire(comparison.worst_metrics)))
        + "</pre>"
    )
    parts.append(
        "<details><summary>All sample pairs</summary><pre>"
        + html.escape(json.dumps(to_wire(comparison.pairs), indent=2))
        + "</pre></details>"
    )
    parts.append(
        "<p>Artifact pair: "
        + html.escape(json.dumps(to_wire(comparison.artifact_pair)))
        + "</p>"
    )
    image_html(parts, comparison.artifacts)
    parts.append(
        "<details><summary>Ranked tiles (mean error, then changed fraction)</summary>"
    )
    for tile in comparison.ranked_tiles:
        parts.append(
            f"<pre>Box: {tile.box}; metrics: {html.escape(json.dumps(to_wire(tile.metrics)))}</pre>"
        )
        image_html(parts, tile.crops)
    parts.append("</details>")


def write_reports(output, report):
    (output / "report.json").write_text(
        json.dumps(report.to_wire(), indent=2, allow_nan=False) + "\n", encoding="utf-8"
    )
    parts = [
        "<!doctype html><html lang='en'><meta charset='utf-8'>",
        (
            "<title>Visual comparison</title><style>body{font:14px sans-serif;margin:2em}"
            "img{max-width:30%;image-rendering:pixelated}pre{white-space:pre-wrap}"
            ".pass{color:#075}.fail{color:#b00}.inconclusive{color:#850}</style>"
        ),
        f"<h1>Visual comparison: {report.status}</h1>",
        (
            "<p>RGB values are compared without registration or resizing. Same-mode repeatability "
            "must pass before cross-mode comparison. Mean is absolute RGB channel error in [0,255]. "
            "Changed fraction counts pixels whose maximum channel error exceeds the pixel threshold. "
            "P99 is the 99th percentile of per-pixel maximum RGB error. Max is the maximum channel error. "
            "Worst metrics are independent maxima across sample pairs; artifacts show one worst pair.</p>"
        ),
        (
            f"<pre>Thresholds: {html.escape(str(report.thresholds))}\n"
            f"Repeatability: {html.escape(str(report.repeatability_thresholds))}\n"
            f"Metadata absolute tolerance: {report.metadata_tolerance}</pre>"
        ),
    ]
    parts.extend(f"<pre>{html.escape(error)}</pre>" for error in report.errors)
    for case in report.cases:
        parts.append(f"<h2>{html.escape(case.name)}: {case.status}</h2>")
        for mode in case.modes:
            parts.append(f"<h3>{mode.mode} repeatability: {mode.status}</h3>")
            parts.extend(f"<pre>{html.escape(error)}</pre>" for error in mode.errors)
            parts.append(
                "<details><summary>Capture provenance</summary><pre>"
                + html.escape(json.dumps(to_wire(mode.samples), indent=2))
                + "</pre></details>"
            )
            for comparison in mode.repeatability:
                append_comparison_html(parts, comparison)
        parts.append("<h3>Cross-mode comparisons</h3>")
        for comparison in case.comparisons:
            append_comparison_html(parts, comparison)
    parts.append("</html>")
    (output / "report.html").write_text("\n".join(parts) + "\n", encoding="utf-8")
