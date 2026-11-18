"""Identity and descriptive metrics for matched screenshot treatments."""

import dataclasses
from itertools import product
from pathlib import Path

from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import (
    SampleSpec,
    Thresholds,
    image_metrics,
    load_sample,
)
from tools.rendering.protocol import Capture, Case, Mode, ProtocolError


def matched_capture_identity(capture: Capture, metadata: dict, generation: int):
    if (capture.visual.generation != generation or capture.visual.pose is None or
            capture.comparison_inputs.camera is None or
            any(eye is None for eye in capture.comparison_inputs.eyes) or
            not isinstance(metadata.get("effective_config"), dict)):
        raise ProtocolError(f"experiment: matched capture {capture.id} has incomplete identity")
    return (capture.visual.generation, capture.mode, capture.visual.profile, capture.visual.pose,
            capture.comparison_inputs, metadata["effective_config"])


def matched_cross_metrics(root: Path, cases: dict[str, Case]) -> tuple[dict, ...]:
    samples = {}
    baseline, treated = cases
    mode = Mode.SINGLE_PASS if treated == "center_planar" else Mode.MULTIPASS
    for condition, case in cases.items():
        samples[condition] = [load_sample(root, CompareMode(mode.value), index,
                                         SampleSpec(sample.png, sample.metadata))
                              for index, sample in enumerate(case.modes.get(mode, []))]
    results = []
    for left, right in product(samples[baseline], samples[treated]):
        for eye in (0, 1):
            metrics = image_metrics(left.image(eye), right.image(eye), Thresholds())
            results.append({baseline: left.capture_id, treated: right.capture_id,
                            "eye": eye, "metrics": dataclasses.asdict(metrics)})
    return tuple(results)
