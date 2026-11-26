"""Check the motion vectors FSR is handed against the camera's motion computed offline.

Reads a fast report whose snapshots saved the `fsr_motion`, `fsr_depth`, and `velocity` planes
(`fast --planes final velocity fsr_motion fsr_depth --save`) and the FSR records each snapshot
carries (`fsr::DispatchRecord`: the eye's current and previous view-projections, the decode's
reprojection, its sign convention, and FSR's motion-vector scale). For every eye it reconstructs
each pixel from its depth, reprojects it through the eye's previous view-projection in double
precision, and compares the motion that gives, in FSR's convention (the previous position less the
current one, in UV with Y down, scaled to pixels), with the decoded vector FSR reads (the decode's
output times FSR's scale over the render size). The pixels are split by how the decode treats them:
`camera` (the velocity pass left the clear value, or its motion saturated the encoding's clamp,
so the hybrid decode gives them the camera's motion) and `engine` (the engine's own motion, kept).

The recorded view-projections carry no jitter; the record gives the clip-space offset the eye
rasterized with on top of them (`jitter_ndc`). FSR wants motion without it, so the expected motion is
taken from the definition rather than from the decode's construction: the pixel's point (through the
jittered current matrix, where the render put it) in the previous frame less the same point in the
unjittered current one.

It also checks the engine's own vectors independently of the decode: where the velocity pass drew,
the engine's motion (current less previous, against the centre camera's previous view-projection)
is compared with the same reconstruction through the centre camera's matrices, which validates the
recorded matrices and the encoding's convention against the game's own shaders.

    python3 -m tools.rendering fsr-motion --session N target/harness-probe/fast/<stamp>/fast-report.json
"""

import argparse
import gzip
import json
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np

SESSIONS = Path("target/harness-probe")
# The velocity target's clear value, as bytes: what a pixel the velocity pass never drew holds
# (`UNDRAWN` in crates/jc3vr-render/shaders/velocity_decode.hlsl).
UNDRAWN = (127, 127, 255, 0)
# DXGI formats the saved planes come in.
R32G8X24_TYPELESS = 19
R16G16_TYPELESS = 33
R16G16_FLOAT = 34
R32_TYPELESS = 39
R32_FLOAT = 41
R8G8B8A8_TYPELESS = 27
B8G8R8A8_TYPELESS = 90


class MotionCheckError(ValueError):
    pass


@dataclass(frozen=True)
class Stats:
    """The error magnitudes of one pixel class, in pixels."""
    pixels: int
    mean: float
    p99: float
    max: float

    @classmethod
    def of(cls, error: np.ndarray) -> "Stats":
        if error.size == 0:
            return cls(0, 0.0, 0.0, 0.0)
        return cls(int(error.size), float(error.mean()), float(np.percentile(error, 99)),
                   float(error.max()))


@dataclass(frozen=True)
class EyeCheck:
    scene: str
    label: str
    step: str
    eye: int
    hybrid: bool
    correct_parallax: bool
    side_by_side: bool
    # The analytic camera motion's own size, for scale: its mean and largest magnitude in pixels.
    motion_mean: float
    motion_max: float
    # The decoded vectors FSR reads against the analytic camera motion, by pixel class.
    camera: Stats
    engine: Stats
    all: Stats
    # The decoded vectors' magnitude at every pixel, which a still camera needs to be zero.
    decoded_max: float
    decoded_nonzero: int
    # The engine's own vectors (where it drew) against their reconstruction through the centre
    # camera's previous view-projection.
    engine_raw: Stats
    # The same through the previous view-projection the decode takes the engine to have encoded
    # against (`encoded_previous`).
    engine_own: Stats
    # How far the decode's recorded reprojection is from the one the recorded view-projections give,
    # as the largest change in a reprojected NDC coordinate over a grid of test points.
    reprojection_mismatch: float


def matrix(values) -> np.ndarray:
    """A recorded column-major matrix, in double precision. The payload records single-precision
    values in their shortest decimal form, so they are rounded back to single precision first: the
    decimal is not the stored value, and the reprojection of near geometry amplifies the difference
    by the near plane's reciprocal."""
    if not isinstance(values, list) or len(values) != 16:
        raise MotionCheckError("fsr-motion: a recorded matrix is not sixteen numbers")
    return np.asarray(values, dtype=np.float32).astype(np.float64).reshape(4, 4).T


def load_plane(captures: Path, record: dict) -> np.ndarray:
    """A saved raw plane, decoded to its values: (H, W) for a depth, (H, W, 2) for motion vectors,
    (H, W, 4) bytes for an 8-bit plane."""
    if record.get("error"):
        raise MotionCheckError(f"fsr-motion: {record['stem']} was not saved: {record['error']}")
    if record.get("encoding") != "raw.gz":
        raise MotionCheckError(f"fsr-motion: {record['stem']} is not a raw plane")
    width, height = record["width"], record["height"]
    data = gzip.decompress((captures / record["file"]).read_bytes())
    pixel_bytes = record["pixel_bytes"]
    if len(data) != width * height * pixel_bytes:
        raise MotionCheckError(f"fsr-motion: {record['stem']} holds {len(data)} bytes, not "
                               f"{width * height * pixel_bytes}")
    fmt = record["format"]
    if fmt == R32G8X24_TYPELESS:
        return np.frombuffer(data, dtype="<f4").reshape(height, width, 2)[..., 0]
    if fmt in (R32_TYPELESS, R32_FLOAT):
        return np.frombuffer(data, dtype="<f4").reshape(height, width)
    if fmt in (R16G16_TYPELESS, R16G16_FLOAT):
        return np.frombuffer(data, dtype="<f2").reshape(height, width, 2).astype(np.float32)
    if fmt == R8G8B8A8_TYPELESS:
        return np.frombuffer(data, dtype=np.uint8).reshape(height, width, 4)
    if fmt == B8G8R8A8_TYPELESS:
        return np.frombuffer(data, dtype=np.uint8).reshape(height, width, 4)[..., [2, 1, 0, 3]]
    raise MotionCheckError(f"fsr-motion: {record['stem']} has unhandled format {fmt}")


def clip_positions(depth: np.ndarray) -> np.ndarray:
    """Each pixel's clip position (NDC x and y at the pixel centre, the raster depth, w of one), as
    the decode reconstructs it: (H, W, 4)."""
    height, width = depth.shape
    x = (np.arange(width, dtype=np.float64) + 0.5) / width * 2.0 - 1.0
    y = 1.0 - (np.arange(height, dtype=np.float64) + 0.5) / height * 2.0
    clip = np.empty((height, width, 4), dtype=np.float64)
    clip[..., 0] = x[None, :]
    clip[..., 1] = y[:, None]
    clip[..., 2] = depth
    clip[..., 3] = 1.0
    return clip


def camera_motion_uv(clip: np.ndarray, reprojection: np.ndarray) -> np.ndarray:
    """The previous position less the current one, in UV with Y down, for each pixel reprojected
    through `reprojection` (current clip to previous clip): FSR's convention, before its scale."""
    previous = clip @ reprojection.T
    previous_ndc = previous[..., :2] / previous[..., 3:4]
    delta = previous_ndc - clip[..., :2]
    return delta * np.array([0.5, -0.5])


def jitter_translation(offset) -> np.ndarray:
    """The clip-space translation by a jitter offset (`x += offset.x * w`, `y += offset.y * w`)."""
    result = np.eye(4)
    result[0, 3], result[1, 3] = offset
    return result


def jitter_free_motion_uv(clip: np.ndarray, current: np.ndarray, previous: np.ndarray,
                          jitter) -> np.ndarray:
    """FSR's motion for each pixel of a render through `current` jittered by the clip-space offset
    `jitter` (None for none), in UV with Y down: where the pixel's point lies in the previous frame
    less where it lies in the unjittered current one. Without a jitter it is the plain
    reprojection."""
    offset = np.zeros(2) if jitter is None else np.asarray(jitter, np.float64)
    point = clip @ np.linalg.inv(jitter_translation(offset) @ current).T
    previous_clip = point @ previous.T
    previous_ndc = previous_clip[..., :2] / previous_clip[..., 3:4]
    current_ndc = clip[..., :2] - offset
    return (previous_ndc - current_ndc) * np.array([0.5, -0.5])


def check_eye(scene: str, snapshot: dict, record: dict, planes: dict[str, np.ndarray]) -> EyeCheck:
    views = record.get("views")
    if views is None:
        raise MotionCheckError(f"fsr-motion: {snapshot['label']} eye {record['eye']} has no "
                               "view-projection history")
    motion = planes["fsr_motion"]
    depth = planes["fsr_depth"]
    velocity = planes["velocity"]
    height, width = depth.shape
    if motion.shape[:2] != (height, width) or velocity.shape[:2] != (height, width):
        raise MotionCheckError(f"fsr-motion: {snapshot['label']} eye {record['eye']}'s planes "
                               "differ in size")
    if record["render_size"] != [width, height]:
        raise MotionCheckError(f"fsr-motion: {snapshot['label']} eye {record['eye']} was resolved "
                               f"at {record['render_size']}, not {width}x{height}")
    jitter = record.get("jitter_ndc")
    offset = np.zeros(2) if jitter is None else np.asarray(jitter, np.float64)
    # The matrix the eye rasterized with, jitter and all.
    current_eye = jitter_translation(offset) @ matrix(views["current_eye"])
    # The decode's jitter-free construction, for comparing with what it recorded.
    reproject_eye = (jitter_translation(offset) @ matrix(views["previous_eye"])
                     @ np.linalg.inv(current_eye))
    reproject_center = matrix(views["previous_center"]) @ np.linalg.inv(current_eye)
    clip = clip_positions(depth.astype(np.float64))
    size = np.array([width, height], dtype=np.float64)
    analytic = jitter_free_motion_uv(clip, matrix(views["current_eye"]),
                                     matrix(views["previous_eye"]), jitter) * size
    # FSR scales the decode's output by its motion-vector scale over the render size.
    scale = np.asarray(record["motion_vector_scale"], dtype=np.float64) / size
    decoded = motion.astype(np.float64) * scale * size
    undrawn = np.all(velocity == np.array(UNDRAWN, dtype=np.uint8), axis=-1)
    saturated = np.any((velocity[..., :2] == 0) | (velocity[..., :2] == 255), axis=-1)
    camera_class = undrawn | saturated
    error = np.linalg.norm(decoded - analytic, axis=-1)
    analytic_size = np.linalg.norm(analytic, axis=-1)
    decoded_size = np.linalg.norm(decoded, axis=-1)
    # The engine encodes the current position less the previous one against the centre camera's
    # previous view-projection, in UV with Y down: the negative of FSR's convention.
    drawn = ~undrawn & ~saturated
    engine_uv = (velocity[..., :2].astype(np.float64) / 255.0 - 0.5) * 0.25
    engine_expected = -camera_motion_uv(clip, reproject_center)
    engine_error = np.linalg.norm((engine_uv - engine_expected) * size, axis=-1)
    engine_own = Stats(0, 0.0, 0.0, 0.0)
    if record.get("encoded_previous") is not None:
        # The same check through the previous view-projection the payload takes the engine to have
        # encoded against (the render camera's own, through the single-pass rewrite's map).
        own = matrix(record["encoded_previous"]) @ np.linalg.inv(current_eye)
        own_error = np.linalg.norm((engine_uv + camera_motion_uv(clip, own)) * size, axis=-1)
        engine_own = Stats.of(own_error[drawn])
    mismatch = 0.0
    if record.get("reprojection") is not None:
        recorded = matrix(record["reprojection"]["eye"])
        grid = np.array([[x, y, d, 1.0] for x in (-0.9, 0.0, 0.9) for y in (-0.9, 0.0, 0.9)
                         for d in (0.0, 1e-4, 0.05)])
        a = grid @ recorded.T
        b = grid @ reproject_eye.T
        mismatch = float(np.max(np.abs(a[:, :2] / a[:, 3:4] - b[:, :2] / b[:, 3:4])))
    return EyeCheck(
        scene, snapshot["label"], snapshot.get("step", ""), record["eye"], record["hybrid"],
        record["correct_parallax"], record["side_by_side"], float(analytic_size.mean()),
        float(analytic_size.max()), Stats.of(error[camera_class]), Stats.of(error[~camera_class]),
        Stats.of(error.ravel()), float(decoded_size.max()), int(np.count_nonzero(decoded_size)),
        Stats.of(engine_error[drawn]), engine_own, mismatch)


def check_report(report: dict, captures: Path) -> list[EyeCheck]:
    checks = []
    for scene in report.get("scenes", []):
        for snapshot in scene.get("snapshots", []):
            saved = {item["stem"]: item for item in snapshot.get("saved", [])}
            for record in snapshot.get("fsr", []):
                eye = record["eye"]
                stems = {plane: f"{snapshot['label']}.{plane}.eye{eye}"
                         for plane in ("fsr_motion", "fsr_depth", "velocity")}
                if not all(stem in saved for stem in stems.values()):
                    continue
                planes = {plane: load_plane(captures, saved[stem]) for plane, stem in stems.items()}
                checks.append(check_eye(scene["name"], snapshot, record, planes))
    return checks


def describe(check: EyeCheck) -> str:
    def stats(value: Stats) -> str:
        return f"{value.mean:.3f}/{value.p99:.3f}/{value.max:.2f} ({value.pixels})"
    return (f"{check.scene:20} {check.step:26} eye {check.eye} hybrid={int(check.hybrid)} "
            f"motion {check.motion_mean:.2f}/{check.motion_max:.1f} px; error mean/p99/max (pixels): "
            f"camera {stats(check.camera)}, engine {stats(check.engine)}, all {stats(check.all)}; "
            f"engine vs centre {stats(check.engine_raw)}, vs encoded {stats(check.engine_own)}; decoded max {check.decoded_max:.3g} at "
            f"{check.decoded_nonzero} nonzero; reprojection mismatch {check.reprojection_mismatch:.2g}")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path, help="a fast-report.json")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--session", type=int, help="the session whose captures the report saved")
    target.add_argument("--captures", type=Path, help="the captures folder the report saved into")
    parser.add_argument("--json", type=Path, help="also write the checks here")
    args = parser.parse_args(argv)
    captures = args.captures or SESSIONS / f"persistent-{args.session}/config/captures"
    try:
        checks = check_report(json.loads(args.report.read_text()), captures)
    except (OSError, ValueError, KeyError) as error:
        print(f"fsr-motion: {error}", file=sys.stderr)
        return 2
    for check in checks:
        print(describe(check))
    if args.json:
        args.json.write_text(json.dumps([asdict(check) for check in checks], indent=1) + "\n")
    return 0 if checks else 2


if __name__ == "__main__":
    sys.exit(main())
