# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy"]
# ///
"""Analyze a render trace's coordinate-frame events.

Reads `traces/<stamp>/trace.ndjson` and summarises its `CoordFrame` events: the render camera's
basis and the travel direction's agreement with -z_basis.

Usage: uv run tools/analysis/analyze_trace_frames.py <traces/stamp dir>
"""

import argparse
import json
import pathlib

import numpy as np


def load_records(path: pathlib.Path) -> list[dict]:
    """The trace's records, skipping any line that does not parse (a truncated final line)."""
    records = []
    with path.open() as f:
        for line in f:
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return records


def events(records: list[dict], kind: str) -> list[tuple[int, dict]]:
    """The `(frame, event)` pairs of one event kind, in trace order."""
    return [(r["frame"], r["event"]) for r in records if r.get("event", {}).get("ev") == kind]


# Per-frame travel below which a CoordFrame sample counts as standing still (matches the payload's
# summary threshold).
MOVING_THRESHOLD_M = 0.005


def summarise_coord_frame(records: list[dict]) -> None:
    samples = [e for _, e in events(records, "CoordFrame")]
    if not samples:
        print("no CoordFrame events in the trace")
        return
    print("=== coord frame ===")
    first = samples[0]
    print(f"{len(samples)} samples; first basis right={first['right']} up={first['up']}"
          f" z_basis={first['z_basis']}")
    moving = [s for s in samples if np.linalg.norm(s["delta"]) > MOVING_THRESHOLD_M]
    if not moving:
        print("camera stationary throughout; walk forward during the trace to test the frame")
        return
    for key in ("dot_neg_z", "dot_right", "dot_up"):
        v = np.array([s[key] for s in moving])
        print(f"  {key}: mean={v.mean():+.3f} min={v.min():+.3f} max={v.max():+.3f}"
              f" over {len(moving)} moving samples")
    print("  dot_neg_z near +1 while walking forward confirms forward = -Z")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("dir", type=pathlib.Path)
    args = ap.parse_args()
    summarise_coord_frame(load_records(args.dir / "trace.ndjson"))


if __name__ == "__main__":
    main()
