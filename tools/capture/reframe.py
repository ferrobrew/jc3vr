#!/usr/bin/env python3
"""Reframe a recorded eye segment into a flat video: crop to a chosen field of view about the
optical axis, and optionally stabilise the head rotation from the pose sidecar.

The recorder streams the whole eye render, whose projection is off-axis (the optical axis is not
at the image centre, and the field of view is wider than a flat viewer wants). The sidecar
(`frames.jsonl`, one line per stream frame) carries the eye's field of view and located pose for
every frame, so this tool can reproject each frame exactly rather than guess: the output is a
symmetric pinhole camera of the requested horizontal field of view and aspect, pointed along the
smoothed head orientation, and each frame is warped by the homography between the two cameras.

    reframe.py SEGMENT.mkv FRAMES.jsonl OUT.mp4 [--hfov 75] [--aspect 16:9] [--width 1920]
               [--smooth 0.5] [--offset N] [--dry-run]

Rotation-only stabilisation is a homography, so no depth is needed and nothing is estimated. The
smoothing is a symmetric Gaussian window over the recorded orientations, `--smooth` seconds wide
(standard deviation); `0` disables it and the tool only crops. The source's margin outside the
crop is what the stabilised view pans into; where the smoothed view reaches past the source, the
output shows black, so a wider field of view or a smaller `--smooth` trades one for the other.

`--offset` is the stream frame the segment starts at, for segments after the first: the sidecar
covers the whole recording, and the segment muxer cuts it into equal lengths, so pass the sum of
the frame counts of the earlier segments (or use the sidecar's `stream_frame` of the segment's
first frame). Segments are cut at a fixed duration, so for a constant-rate stream the offset is
`segment index * segment seconds * frame rate`.

Take markers made during the recording are lines of the same sidecar with a `marker` object and
the `stream_frame` they fall at. The tool lists the ones from the segment's start on, with their
time in the output, before the warp (and in the dry run), so a take can be found without scrubbing.

Requires ffmpeg on PATH, and numpy and OpenCV (`opencv-python`) for the warp. Audio is copied
from the segment unchanged.
"""

import argparse
import json
import math
import subprocess
import sys

import numpy as np

try:
    import cv2
except ImportError:  # The dry run does not need it.
    cv2 = None


def read_sidecar(path):
    """The sidecar's frame lines (one per stream frame, in order) and its take-marker lines."""
    frames = []
    markers = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                record = json.loads(line)
                (markers if "marker" in record else frames).append(record)
    return frames, markers


def segment_markers(markers, offset, rate):
    """The markers at or after the segment's first frame, as (segment frame, seconds, marker)."""
    out = []
    for record in markers:
        frame = record["stream_frame"] - offset
        if frame >= 0:
            out.append((frame, frame / rate, record["marker"]))
    return out


def print_markers(markers, offset, rate):
    found = segment_markers(markers, offset, rate)
    if not found:
        return
    print("take markers from the segment's start (ones past its end belong to later segments):", file=sys.stderr)
    for frame, seconds, marker in found:
        label = marker.get("label") or "(unlabelled)"
        print(f"  #{marker['index']} frame {frame} at {seconds:.2f} s: {label}", file=sys.stderr)


def intrinsics_from_fov(fov, width, height):
    """The pinhole matrix of an eye image with OpenXR fov angles (left, right, up, down)."""
    left, right, up, down = (math.tan(a) for a in fov)
    fx = width / (right - left)
    fy = height / (up - down)
    cx = -left * fx
    cy = up * fy
    return np.array([[fx, 0, cx], [0, fy, cy], [0, 0, 1]], dtype=np.float64)


def intrinsics_symmetric(hfov_deg, width, height):
    fx = (width / 2) / math.tan(math.radians(hfov_deg) / 2)
    return np.array([[fx, 0, width / 2], [0, fx, height / 2], [0, 0, 1]], dtype=np.float64)


def quat_to_matrix(q):
    x, y, z, w = q
    return np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
        ],
        dtype=np.float64,
    )


def smooth_orientations(quats, sigma_frames):
    """A Gaussian-window average of unit quaternions, sign-aligned to the running mean."""
    if sigma_frames <= 0:
        return quats
    radius = int(math.ceil(3 * sigma_frames))
    weights = np.exp(-0.5 * (np.arange(-radius, radius + 1) / sigma_frames) ** 2)
    out = np.empty_like(quats)
    n = len(quats)
    for i in range(n):
        lo, hi = max(0, i - radius), min(n, i + radius + 1)
        window = quats[lo:hi].copy()
        w = weights[lo - i + radius : hi - i + radius]
        # Align signs to the centre quaternion so antipodal pairs do not cancel.
        flip = np.sum(window * quats[i], axis=1) < 0
        window[flip] *= -1
        mean = np.sum(window * w[:, None], axis=0)
        out[i] = mean / np.linalg.norm(mean)
    return out


# Camera convention (x right, y down, z forward) to OpenXR view convention (x right, y up, z back)
# and back; it is its own inverse.
FLIP = np.diag([1.0, -1.0, -1.0])


def homography(k_in, k_out, r_frame, r_target):
    """The map from output pixels to input pixels, for warping with an inverse map."""
    rotation = FLIP @ r_frame.T @ r_target @ FLIP
    return k_in @ rotation @ np.linalg.inv(k_out)


def probe(segment):
    out = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate,nb_frames",
            "-of",
            "json",
            segment,
        ]
    )
    stream = json.loads(out)["streams"][0]
    num, den = stream["r_frame_rate"].split("/")
    return int(stream["width"]), int(stream["height"]), float(num) / float(den)


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("segment")
    parser.add_argument("sidecar")
    parser.add_argument("output")
    parser.add_argument("--hfov", type=float, default=75.0, help="output horizontal field of view in degrees")
    parser.add_argument("--aspect", default="16:9", help="output aspect as W:H")
    parser.add_argument("--width", type=int, default=1920, help="output width in pixels")
    parser.add_argument("--smooth", type=float, default=0.5, help="stabilisation window (seconds, standard deviation); 0 crops only")
    parser.add_argument("--offset", type=int, default=0, help="stream frame index of the segment's first frame")
    parser.add_argument("--encoder", default="libx264")
    parser.add_argument("--crf", default="16")
    parser.add_argument("--dry-run", action="store_true", help="print the framing and exit")
    args = parser.parse_args(argv[1:])

    aw, ah = (int(v) for v in args.aspect.split(":"))
    out_w = args.width & ~1
    out_h = int(round(out_w * ah / aw)) & ~1
    in_w, in_h, rate = probe(args.segment)
    frames, markers = read_sidecar(args.sidecar)
    if not frames:
        print("the sidecar is empty", file=sys.stderr)
        return 1
    eye = frames[0]["eye"]
    fov = frames[0]["views"][eye]["fov"] if frames[0].get("views") else None
    if fov is None:
        print("the sidecar has no located views (was the recording made with a headset running?)", file=sys.stderr)
        return 1
    k_in = intrinsics_from_fov(fov, in_w, in_h)
    k_out = intrinsics_symmetric(args.hfov, out_w, out_h)

    quats = np.array(
        [f["views"][f["eye"]]["orientation"] if f.get("views") else [0, 0, 0, 1] for f in frames],
        dtype=np.float64,
    )
    targets = smooth_orientations(quats, args.smooth * rate)
    print_markers(markers, args.offset, rate)

    if args.dry_run:
        src_hfov = math.degrees(math.atan(math.tan(fov[1])) - math.atan(math.tan(fov[0])))
        src_vfov = math.degrees(fov[2] - fov[3])
        print(f"source {in_w}x{in_h} at {rate:.2f} fps, eye {eye}, fov {src_hfov:.1f}x{src_vfov:.1f} deg")
        print(f"output {out_w}x{out_h}, hfov {args.hfov} deg, smoothing sigma {args.smooth * rate:.1f} frames")
        h = homography(k_in, k_out, quat_to_matrix(quats[0]), quat_to_matrix(targets[0]))
        corners = np.array([[0, 0, 1], [out_w, 0, 1], [0, out_h, 1], [out_w, out_h, 1]], dtype=np.float64).T
        mapped = h @ corners
        mapped /= mapped[2]
        print("first frame's output corners in source pixels:")
        for x, y in mapped[:2].T:
            print(f"  ({x:8.1f}, {y:8.1f})")
        return 0

    if cv2 is None:
        print("OpenCV (opencv-python) is needed for the warp", file=sys.stderr)
        return 1

    decode = subprocess.Popen(
        ["ffmpeg", "-v", "error", "-i", args.segment, "-f", "rawvideo", "-pix_fmt", "bgr24", "-"],
        stdout=subprocess.PIPE,
    )
    encode = subprocess.Popen(
        [
            "ffmpeg", "-v", "error", "-y",
            "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{out_w}x{out_h}", "-r", f"{rate}", "-i", "-",
            "-i", args.segment,
            "-map", "0:v", "-map", "1:a?",
            "-c:v", args.encoder, "-crf", args.crf, "-pix_fmt", "yuv420p",
            "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709",
            "-c:a", "copy",
            "-shortest",
            args.output,
        ],
        stdin=subprocess.PIPE,
    )
    frame_bytes = in_w * in_h * 3
    index = args.offset
    written = 0
    while True:
        raw = decode.stdout.read(frame_bytes)
        if len(raw) < frame_bytes:
            break
        image = np.frombuffer(raw, dtype=np.uint8).reshape(in_h, in_w, 3)
        i = min(index, len(frames) - 1)
        h = homography(k_in, k_out, quat_to_matrix(quats[i]), quat_to_matrix(targets[i]))
        warped = cv2.warpPerspective(
            image, h, (out_w, out_h), flags=cv2.INTER_LINEAR | cv2.WARP_INVERSE_MAP, borderMode=cv2.BORDER_CONSTANT
        )
        encode.stdin.write(warped.tobytes())
        index += 1
        written += 1
    encode.stdin.close()
    decode.wait()
    encode.wait()
    print(f"wrote {written} frames to {args.output}", file=sys.stderr)
    return 0 if encode.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
