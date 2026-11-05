#!/usr/bin/env python3
"""Reframe a recorded take into a flat video: crop to a chosen field of view about the optical axis,
and optionally stabilise the head rotation from the pose sidecar.

The video is the desktop mirror window, which shows one eye's render filled into the window, so its
projection is off-axis (the optical axis is not at the image centre) and wider or narrower than a
flat viewer wants. The take's sidecar (`frames.jsonl`) carries every rendered frame's wall clock,
eye fields of view, and located poses, and a line for each mirror layout giving where the eye image
lands in the window; `stream.json` carries the wall clock of the video's first frame. Together they
let this tool reproject each video frame exactly rather than guess: the output is a symmetric
pinhole camera of the requested horizontal field of view and aspect, pointed along the smoothed head
orientation, and each frame is warped by the homography between the two cameras.

    reframe.py VIDEO.mkv OUT.mp4 [--sidecar FRAMES.jsonl] [--stream STREAM.json] [--hfov 75]
               [--aspect 16:9] [--width 1920] [--smooth 0.5] [--dry-run]

The sidecar and stream metadata default to the files beside the video (`stream.json` for
`video.mkv`, `stream_N.json` for `video_N.mkv`). Each video frame takes the pose of the last frame
the game rendered at or before the video frame's wall-clock time, and the mirror layout in effect at
that frame. The mirror must have shown a single eye; the stereo pair is refused.

Rotation-only stabilisation is a homography, so no depth is needed and nothing is estimated. The
smoothing is a symmetric Gaussian window over the sampled orientations, `--smooth` seconds wide
(standard deviation); `0` disables it and the tool only crops. The eye image's margin outside the
crop is what the stabilised view pans into; where the smoothed view reaches past the window, the
output shows black, so a wider field of view or a smaller `--smooth` trades one for the other.

Take markers made during the take are sidecar lines with a `marker` object. The tool lists them
with their time in the video before the warp (and in the dry run), so a take can be found without
scrubbing.

Requires ffmpeg and ffprobe on PATH, and numpy and OpenCV (`opencv-python`) for the warp. Audio is
copied from the video unchanged.
"""

import argparse
import bisect
import json
import math
import os
import re
import subprocess
import sys

import numpy as np

try:
    import cv2
except ImportError:  # The dry run does not need it.
    cv2 = None


def read_sidecar(path):
    """The sidecar's frame lines, take-marker lines, and mirror-layout lines, each in file order."""
    frames, markers, mirrors = [], [], []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            record = json.loads(line)
            if "marker" in record:
                markers.append(record)
            elif "mirror" in record:
                mirrors.append(record)
            elif "present" in record:
                # The mirror's present counter, which the frame stamp draws into the video.
                continue
            else:
                frames.append(record)
    return frames, markers, mirrors


def default_stream_path(video):
    """`stream.json` for `video.mkv`, `stream_N.json` for `video_N.mkv`, beside the video."""
    folder, name = os.path.split(video)
    match = re.fullmatch(r"video(_\d+)?\.mkv", name)
    suffix = (match.group(1) or "") if match else ""
    return os.path.join(folder, f"stream{suffix}.json")


def frame_at(frames, times, unix_ms):
    """The last rendered frame at or before `unix_ms` (the first frame before the take's first)."""
    index = bisect.bisect_right(times, unix_ms) - 1
    return frames[max(index, 0)]


def mirror_at(mirrors, frame):
    """The mirror layout in effect at sidecar frame `frame`."""
    current = None
    for record in mirrors:
        if record["frame"] <= frame:
            current = record["mirror"]
        else:
            break
    return current or (mirrors[0]["mirror"] if mirrors else None)


def marker_times(markers, video_start_ms):
    """The markers as (seconds into the video, marker)."""
    return [((m["marker"]["unix_ms"] - video_start_ms) / 1000.0, m["marker"]) for m in markers]


def print_markers(markers, video_start_ms):
    found = marker_times(markers, video_start_ms)
    if not found:
        return
    print("take markers:", file=sys.stderr)
    for seconds, marker in found:
        label = marker.get("label") or "(unlabelled)"
        print(f"  #{marker['index']} at {seconds:.2f} s: {label}", file=sys.stderr)


def intrinsics_in_window(fov, rect, scale):
    """The pinhole matrix of an eye image with OpenXR fov angles (left, right, up, down) drawn into
    the window rectangle `rect` (x, y, width, height in window pixels), in video pixels."""
    left, right, up, down = (math.tan(a) for a in fov)
    x, y, width, height = rect
    fx = width / (right - left)
    fy = height / (up - down)
    cx = x - left * fx
    cy = y + up * fy
    sx, sy = scale
    return np.array([[fx * sx, 0, cx * sx], [0, fy * sy, cy * sy], [0, 0, 1]], dtype=np.float64)


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


def probe(video):
    out = subprocess.check_output(
        [
            "ffprobe", "-v", "error", "-select_streams", "v:0",
            "-show_entries", "stream=width,height,r_frame_rate:format=duration",
            "-of", "json", video,
        ]
    )
    info = json.loads(out)
    stream = info["streams"][0]
    num, den = stream["r_frame_rate"].split("/")
    return int(stream["width"]), int(stream["height"]), float(num) / float(den), float(info["format"]["duration"])


class Framing:
    """Each video frame's input camera and head orientation, from the sidecar."""

    def __init__(self, frames, mirrors, video_start_ms, rate, video_size):
        self.frames = frames
        self.times = [f["unix_ms"] for f in frames]
        self.mirrors = mirrors
        self.video_start_ms = video_start_ms
        self.rate = rate
        self.video_size = video_size

    def sample(self, index):
        """(k_in, orientation) for video frame `index`, or None where the frame cannot be framed."""
        unix_ms = self.video_start_ms + index * 1000.0 / self.rate
        frame = frame_at(self.frames, self.times, unix_ms)
        mirror = mirror_at(self.mirrors, frame["frame"])
        if mirror is None or not frame.get("views"):
            return None
        if len(mirror["draws"]) != 1:
            raise ValueError("the mirror showed the stereo pair; reframing needs a single eye")
        draw = mirror["draws"][0]
        view = frame["views"][draw["eye"]]
        window_w, window_h = mirror["window"]
        scale = (self.video_size[0] / window_w, self.video_size[1] / window_h)
        return intrinsics_in_window(view["fov"], draw["rect"], scale), view["orientation"]


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("video")
    parser.add_argument("output")
    parser.add_argument("--sidecar", help="the take's frames.jsonl (default: beside the video)")
    parser.add_argument("--stream", help="the video's stream.json (default: beside the video)")
    parser.add_argument("--hfov", type=float, default=75.0, help="output horizontal field of view in degrees")
    parser.add_argument("--aspect", default="16:9", help="output aspect as W:H")
    parser.add_argument("--width", type=int, default=1920, help="output width in pixels")
    parser.add_argument("--smooth", type=float, default=0.5, help="stabilisation window (seconds, standard deviation); 0 crops only")
    parser.add_argument("--encoder", default="libx264")
    parser.add_argument("--crf", default="16")
    parser.add_argument("--dry-run", action="store_true", help="print the framing and exit")
    args = parser.parse_args(argv[1:])

    sidecar = args.sidecar or os.path.join(os.path.dirname(args.video), "frames.jsonl")
    stream_path = args.stream or default_stream_path(args.video)
    with open(stream_path) as f:
        stream = json.load(f)
    video_start_ms = stream.get("video_start_unix_ms")
    if video_start_ms is None:
        print(f"{stream_path} has no video_start_unix_ms; the video cannot be aligned", file=sys.stderr)
        return 1

    aw, ah = (int(v) for v in args.aspect.split(":"))
    out_w = args.width & ~1
    out_h = int(round(out_w * ah / aw)) & ~1
    in_w, in_h, rate, duration = probe(args.video)
    frames, markers, mirrors = read_sidecar(sidecar)
    if not frames:
        print("the sidecar has no frames", file=sys.stderr)
        return 1
    if not mirrors:
        print("the sidecar records no mirror layout (was the mirror on?)", file=sys.stderr)
        return 1
    framing = Framing(frames, mirrors, video_start_ms, rate, (in_w, in_h))
    count = int(math.ceil(duration * rate))
    try:
        samples = [framing.sample(i) for i in range(count)]
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1
    if not any(samples):
        print("no video frame has located views (was the recording made with a headset running?)", file=sys.stderr)
        return 1
    # Frames without views (before the first located frame) borrow the nearest framed one.
    first = next(s for s in samples if s is not None)
    previous = first
    for i, sample in enumerate(samples):
        if sample is None:
            samples[i] = previous
        else:
            previous = sample
    quats = np.array([s[1] for s in samples], dtype=np.float64)
    targets = smooth_orientations(quats, args.smooth * rate)
    k_out = intrinsics_symmetric(args.hfov, out_w, out_h)
    print_markers(markers, video_start_ms)

    if args.dry_run:
        k_in = samples[0][0]
        src_hfov = math.degrees(2 * math.atan(in_w / 2 / k_in[0, 0]))
        print(f"video {in_w}x{in_h} at {rate:.2f} fps, {count} frames, first frame at unix ms {video_start_ms:.1f}")
        print(f"sidecar {len(frames)} frames, {len(mirrors)} mirror layouts, {len(markers)} markers")
        print(f"first frame's horizontal field of view across the video about {src_hfov:.1f} deg")
        print(f"output {out_w}x{out_h}, hfov {args.hfov} deg, smoothing sigma {args.smooth * rate:.1f} frames")
        h = homography(k_in, k_out, quat_to_matrix(quats[0]), quat_to_matrix(targets[0]))
        corners = np.array([[0, 0, 1], [out_w, 0, 1], [0, out_h, 1], [out_w, out_h, 1]], dtype=np.float64).T
        mapped = h @ corners
        mapped /= mapped[2]
        print("first frame's output corners in video pixels:")
        for x, y in mapped[:2].T:
            print(f"  ({x:8.1f}, {y:8.1f})")
        return 0

    if cv2 is None:
        print("OpenCV (opencv-python) is needed for the warp", file=sys.stderr)
        return 1

    # Decode at the constant rate the frame times above assume (gpu-screen-recorder writes a
    # variable frame rate by default).
    decode = subprocess.Popen(
        ["ffmpeg", "-v", "error", "-i", args.video, "-vf", f"fps={rate}", "-f", "rawvideo", "-pix_fmt", "bgr24", "-"],
        stdout=subprocess.PIPE,
    )
    encode = subprocess.Popen(
        [
            "ffmpeg", "-v", "error", "-y",
            "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{out_w}x{out_h}", "-r", f"{rate}", "-i", "-",
            "-i", args.video,
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
    index = 0
    while True:
        raw = decode.stdout.read(frame_bytes)
        if len(raw) < frame_bytes:
            break
        image = np.frombuffer(raw, dtype=np.uint8).reshape(in_h, in_w, 3)
        i = min(index, count - 1)
        h = homography(samples[i][0], k_out, quat_to_matrix(quats[i]), quat_to_matrix(targets[i]))
        warped = cv2.warpPerspective(
            image, h, (out_w, out_h), flags=cv2.INTER_LINEAR | cv2.WARP_INVERSE_MAP, borderMode=cv2.BORDER_CONSTANT
        )
        encode.stdin.write(warped.tobytes())
        index += 1
    encode.stdin.close()
    decode.wait()
    encode.wait()
    print(f"wrote {index} frames to {args.output}", file=sys.stderr)
    return 0 if encode.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
