"""Check that the image FSR is handed moves by the sub-pixel jitter FSR is told.

Reads a fast report of the jitter scenes (`jitter-shift` in `tools/rendering/suites/fsr.toml`, run
with `--planes final pre_aa --save`), whose every mode took two snapshots at a still camera, each
with the FSR records of the eyes it resolved (`fsr::DispatchRecord`, whose `jitter_pixels` is the
offset FSR was told, in the eye's pixels, x right and y down). For every mode and eye it estimates
the shift between the two snapshots' `pre_aa` planes (the tone-mapped image the anti-aliasing reads)
and compares it with the change in the jitter FSR was told between them: the content moves by the
offset, so the second image is the first moved by the difference.

The shift is the least-squares solution of `b - a = -grad . d` over the eye, the gradient taken as
the mean of the two images' (exact to first order for a shift under a pixel), refined once by
resampling the first image by the estimate. What remains once the first image is moved by the
expected shift is the change the jitter does not explain. At full resolution it is mostly the
aliasing a sub-pixel shift changes (a thin edge that covers a pixel in one frame and misses it in
the next), which no resampling reproduces; averaged over 16 by 16 pixel blocks that vanishes, and
what is left is a change of the shading itself, which is where a scene-side flicker the jitter
excites (a shadow term that changes with it) would show as a blob. Both are reported: the pixels
more than three codes apart before and after the move, and the mean and the largest change of the
block averages, in codes.

    python3 -m tools.rendering fsr-jitter --session N target/harness-probe/fast/<stamp>/fast-report.json
"""

import argparse
import gzip
import json
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np

SESSIONS = Path("target/harness-probe")
THRESHOLD = 3.0
TILE = 64


class JitterCheckError(ValueError):
    pass


@dataclass(frozen=True)
class EyeShift:
    scene: str
    config: str
    mode: str
    eye: int
    told: tuple[float, float]
    measured: tuple[float, float]
    # The pixels more than three codes apart before and after moving the first image by the shift
    # FSR was told, and the largest mean residual of a tile, in codes.
    changed: int
    residual: int
    worst_tile: float
    # The block averages' mean and largest change once the first image is moved, in codes of luma.
    block_mean: float
    block_worst: float

    @property
    def error(self) -> float:
        return float(np.hypot(self.measured[0] - self.told[0], self.measured[1] - self.told[1]))


def load_rgba8(captures: Path, item: dict) -> np.ndarray:
    """A saved RGBA8 plane as float luma-weighted RGB in codes."""
    if item.get("error"):
        raise JitterCheckError(f"fsr-jitter: {item['stem']} was not saved: {item['error']}")
    data = gzip.decompress((captures / item["file"]).read_bytes())
    width, height = item["width"], item["height"]
    if len(data) != width * height * 4:
        raise JitterCheckError(f"fsr-jitter: {item['stem']} is not a {width}x{height} RGBA8 plane")
    return np.frombuffer(data, dtype=np.uint8).reshape(height, width, 4)[..., :3].astype(np.float64)


def luma(image: np.ndarray) -> np.ndarray:
    return image @ np.array([0.2126, 0.7152, 0.0722])


def shift_image(image: np.ndarray, shift: tuple[float, float]) -> np.ndarray:
    """`image` with its content moved by `shift` pixels (x right, y down), bilinearly, the border
    clamped."""
    height, width = image.shape[:2]
    ys, xs = np.mgrid[0:height, 0:width].astype(np.float64)
    sx = np.clip(xs - shift[0], 0, width - 1)
    sy = np.clip(ys - shift[1], 0, height - 1)
    x0 = np.floor(sx).astype(int)
    y0 = np.floor(sy).astype(int)
    x1 = np.minimum(x0 + 1, width - 1)
    y1 = np.minimum(y0 + 1, height - 1)
    fx = (sx - x0)[..., None] if image.ndim == 3 else sx - x0
    fy = (sy - y0)[..., None] if image.ndim == 3 else sy - y0
    top = image[y0, x0] * (1 - fx) + image[y0, x1] * fx
    bottom = image[y1, x0] * (1 - fx) + image[y1, x1] * fx
    return top * (1 - fy) + bottom * fy


def estimate_shift(a: np.ndarray, b: np.ndarray) -> tuple[float, float]:
    """The shift (x right, y down) that moves luma `a` onto luma `b`, by least squares on the
    gradients, refined once."""
    def solve(first: np.ndarray) -> np.ndarray:
        gy_a, gx_a = np.gradient(first)
        gy_b, gx_b = np.gradient(b)
        gx = 0.5 * (gx_a + gx_b)[2:-2, 2:-2].ravel()
        gy = 0.5 * (gy_a + gy_b)[2:-2, 2:-2].ravel()
        dt = (b - first)[2:-2, 2:-2].ravel()
        # Leave out the pixels whose change a sub-pixel shift cannot make (occlusion, flicker).
        keep = np.abs(dt) <= 0.75 * (np.abs(gx) + np.abs(gy)) + 1.0
        system = np.array([[np.dot(gx[keep], gx[keep]), np.dot(gx[keep], gy[keep])],
                           [np.dot(gx[keep], gy[keep]), np.dot(gy[keep], gy[keep])]])
        right = -np.array([np.dot(gx[keep], dt[keep]), np.dot(gy[keep], dt[keep])])
        return np.linalg.solve(system, right)

    first = solve(a)
    second = solve(shift_image(a, (first[0], first[1])))
    total = first + second
    return float(total[0]), float(total[1])


def blocks(image: np.ndarray, size: int = 16) -> np.ndarray:
    height, width = image.shape
    rows, columns = height // size, width // size
    return image[:rows * size, :columns * size].reshape(rows, size, columns, size).mean(axis=(1, 3))


def tile_worst(residual: np.ndarray) -> float:
    height, width = residual.shape
    rows, columns = height // TILE, width // TILE
    tiles = residual[:rows * TILE, :columns * TILE].reshape(rows, TILE, columns, TILE)
    return float(tiles.mean(axis=(1, 3)).max())


def check_report(report: dict, captures: Path) -> list[EyeShift]:
    checks = []
    for scene in report.get("scenes", []):
        snapshots = {snapshot["label"]: snapshot for snapshot in scene.get("snapshots", [])}
        for label, first in snapshots.items():
            if not label.endswith("_a"):
                continue
            second = snapshots.get(label[:-1] + "b")
            if second is None:
                continue
            step = first.get("step", "")
            config, _, mode = step.rpartition(" ")
            jitter = {record["eye"]: record["jitter_pixels"] for record in first.get("fsr", [])}
            jitter_b = {record["eye"]: record["jitter_pixels"] for record in second.get("fsr", [])}
            for item in first.get("saved", []):
                _, plane, eye_name = item["stem"].rsplit(".", 2)
                if plane != "pre_aa":
                    continue
                eye = int(eye_name.removeprefix("eye"))
                other = next((x for x in second.get("saved", [])
                              if x["stem"].rsplit(".", 2)[1:] == ["pre_aa", eye_name]), None)
                if other is None or eye not in jitter or eye not in jitter_b:
                    continue
                a = load_rgba8(captures, item)
                b = load_rgba8(captures, other)
                told = (jitter_b[eye][0] - jitter[eye][0], jitter_b[eye][1] - jitter[eye][1])
                measured = estimate_shift(luma(a), luma(b))
                moved = shift_image(a, told)
                residual = np.abs(b - moved).max(axis=2)[2:-2, 2:-2]
                changed = int(np.count_nonzero(np.abs(b - a).max(axis=2) > THRESHOLD))
                block_change = np.abs(blocks(luma(b)[2:-2, 2:-2]) - blocks(luma(moved)[2:-2, 2:-2]))
                checks.append(EyeShift(scene["name"], config or "fsr", mode, eye, told, measured,
                                       changed, int(np.count_nonzero(residual > THRESHOLD)),
                                       tile_worst(residual), float(block_change.mean()),
                                       float(block_change.max())))
    return checks


def describe(check: EyeShift) -> str:
    return (f"{check.scene:20} {check.config:10} {check.mode:11} eye {check.eye}  told "
            f"({check.told[0]:+.4f}, {check.told[1]:+.4f}) px  measured ({check.measured[0]:+.4f}, "
            f"{check.measured[1]:+.4f}) px  error {check.error:.4f} px  changed {check.changed:>9}  "
            f"unexplained {check.residual:>9}  worst tile {check.worst_tile:.2f}  blocks mean "
            f"{check.block_mean:.3f} worst {check.block_worst:.2f}")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path, help="a fast-report.json")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--session", type=int, help="the session whose captures the report saved")
    target.add_argument("--captures", type=Path, help="the captures folder the report saved into")
    parser.add_argument("--json", type=Path, help="also write the measurements here")
    args = parser.parse_args(argv)
    captures = args.captures or SESSIONS / f"persistent-{args.session}/config/captures"
    try:
        checks = check_report(json.loads(args.report.read_text()), captures)
    except (OSError, ValueError, KeyError) as error:
        print(f"fsr-jitter: {error}", file=sys.stderr)
        return 2
    for check in checks:
        print(describe(check))
    if args.json:
        args.json.write_text(json.dumps([asdict(check) | {"error": check.error} for check in checks],
                                        indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
