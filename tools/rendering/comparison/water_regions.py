from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from tools.rendering.comparison.stages import StageIdentity, StageSample


@dataclass(frozen=True)
class WaterMask:
    before: StageIdentity
    after: StageIdentity
    water: np.ndarray = field(repr=False)
    sky: np.ndarray = field(repr=False)


@dataclass(frozen=True)
class RegionError:
    pixels: int
    rgb_mae: float | None


@dataclass(frozen=True)
class WaterRegions:
    mask_a: tuple[str, str]
    mask_b: tuple[str, str]
    common_water: RegionError
    common_sky: RegionError
    water_disagreement_pixels: int
    sky_disagreement_pixels: int


def artifact_key(path: str) -> tuple[int, int]:
    stat = Path(path).stat()
    return stat.st_dev, stat.st_ino


def region_errors(
    a: StageSample, b: StageSample, ma: WaterMask, mb: WaterMask
) -> WaterRegions:
    if a.channels != ("r", "g", "b") or any(
        m.water.shape != a.values.shape[:2] for m in (ma, mb)
    ):
        raise ValueError("water regions: expected RGB on the depth pixel grid")
    delta = np.abs(a.values - b.values)

    def measure(mask: np.ndarray) -> RegionError:
        count = int(mask.sum())
        return RegionError(count, float(delta[mask].mean()) if count else None)

    return WaterRegions(
        (ma.before.raw, ma.after.raw),
        (mb.before.raw, mb.after.raw),
        measure(ma.water & mb.water),
        measure(ma.sky & mb.sky),
        int(np.count_nonzero(ma.water != mb.water)),
        int(np.count_nonzero(ma.sky != mb.sky)),
    )
