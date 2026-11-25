"""Command-line options for the private rendering session."""

import argparse
from dataclasses import dataclass
from pathlib import Path


# Mirrors the `vr.resolution_scale` clamp in `payload/src/vr/eye_resolution.rs` at the low end; the
# high end bounds the per-eye size to what a headset's recommended size reaches.
DEFAULT_RESOLUTION_SCALE = 0.5
MAX_RESOLUTION_SCALE = 4.0
# Each side of a seeded per-eye size, at most: a double-wide single-pass target is twice the width.
MAX_EYE_SIDE = 4096
DEFAULT_COMPOSITOR_FPS = 120
MAX_COMPOSITOR_FPS = 1000


@dataclass
class Options:
    no_build: bool = False
    check: bool = False
    gpu: str | None = None
    xdotool: str | None = None
    timeout: float = 180.0
    startup_timeout: float = 90.0
    build_timeout: float = 900.0
    copy_timeout: float = 180.0
    menu_interval: float = 3.0
    menu_timeout: float = 60.0
    output: Path | None = None
    release: bool = False
    launch_only: bool = False
    keep_alive: bool = False
    suite: bool = False
    orientation_sweep: bool = False
    motion_sweep: bool = False
    smoke: bool = False
    samples: int = 3
    warmup_frames: int = 30
    reverse_multipass: bool = False
    # The `vr.resolution_scale` the session seeds: the runtime's recommended per-eye size times this
    # is the eye size every capture is taken at. The default gives the 320 by 360 harness eye.
    resolution_scale: float = DEFAULT_RESOLUTION_SCALE
    # A per-eye size the session seeds in place of the runtime's recommended one (the simulated
    # headset's), as `(width, height)`; the scale then applies to it. `None` keeps the runtime's.
    eye_size: tuple[int, int] | None = None
    # The frame rate Monado's null compositor paces the session at. Every harness wait counts frames,
    # so a faster pace shortens them; the GPU bounds it at a large eye size.
    compositor_fps: int = DEFAULT_COMPOSITOR_FPS
    single_pass_probe: bool = False
    probe_yaw: float = 0.0
    probe_pitch: float = 0.0
    probe_right: float = 0.0
    probe_up: float = 0.0
    probe_forward: float = 0.0


def parse_eye_size(value: str) -> tuple[int, int]:
    """Parse `WIDTHxHEIGHT` into a per-eye size, each side within [64, MAX_EYE_SIDE]."""
    try:
        width, height = (int(side) for side in value.lower().split("x"))
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"eye size {value!r}: expected WIDTHxHEIGHT") from error
    if not (64 <= width <= MAX_EYE_SIDE and 64 <= height <= MAX_EYE_SIDE):
        raise argparse.ArgumentTypeError(f"eye size {value!r}: each side must be within [64, {MAX_EYE_SIDE}]")
    return width, height


def parse_args(argv=None, keep_alive=False, description=None):
    parser = argparse.ArgumentParser(
        description=description or "Run Just Cause 3 in a private headless VR session.")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--release", action="store_true")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--launch-only", action="store_true",
                      help="launch without claiming smoke success, bounded by --timeout")
    mode.add_argument("--keep-alive", action="store_true", default=keep_alive,
                      help="launch and inject, then serve until interrupted without an overall deadline")
    mode.add_argument("--smoke", action="store_true", help="run the default two-capture smoke check")
    mode.add_argument("--suite", action="store_true", help="run the four-mode visual comparison suite")
    mode.add_argument("--orientation-sweep", action="store_true",
                      help="run the frozen-anchor orientation sweep")
    mode.add_argument("--motion-sweep", action="store_true",
                      help="run the rendered-frame motion sweep")
    mode.add_argument("--single-pass-probe", action="store_true",
                      help="run a focused single-pass stereo probe at one orientation")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--warmup-frames", type=int, default=30)
    parser.add_argument("--probe-yaw", type=float, default=0.0)
    parser.add_argument("--probe-pitch", type=float, default=0.0)
    parser.add_argument("--probe-right", type=float, default=0.0,
                        help="probe translation right, in game world units")
    parser.add_argument("--probe-up", type=float, default=0.0,
                        help="probe translation up, in game world units")
    parser.add_argument("--probe-forward", type=float, default=0.0,
                        help="probe translation forward, in game world units")
    parser.add_argument("--reverse-multipass", action="store_true")
    parser.add_argument("--resolution-scale", type=float, default=DEFAULT_RESOLUTION_SCALE,
                        help="the vr.resolution_scale the session seeds, which sets the per-eye size")
    parser.add_argument("--eye-size", type=parse_eye_size, metavar="WIDTHxHEIGHT",
                        help="a per-eye size to seed in place of the runtime's recommended one")
    parser.add_argument("--compositor-fps", type=int, default=DEFAULT_COMPOSITOR_FPS,
                        help="the frame rate the null compositor paces the session at")
    parser.add_argument("--gpu", help="Vulkan PCI vendor:device, e.g. 10de:2b85")
    parser.add_argument("--xdotool")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--timeout", type=float)
    for name, default in (("startup-timeout", 90), ("build-timeout", 900),
                          ("copy-timeout", 180), ("menu-interval", 3), ("menu-timeout", 60)):
        parser.add_argument(f"--{name}", type=float, default=default)
    args = parser.parse_args(argv)
    if args.keep_alive and any((args.suite, args.orientation_sweep, args.motion_sweep,
                                args.smoke, args.single_pass_probe)):
        parser.error("--keep-alive is mutually exclusive with an execution mode")
    if not 1 <= args.compositor_fps <= MAX_COMPOSITOR_FPS:
        parser.error(f"--compositor-fps must be within [1, {MAX_COMPOSITOR_FPS}]")
    if not 0.1 <= args.resolution_scale <= MAX_RESOLUTION_SCALE:
        parser.error(f"--resolution-scale must be within [0.1, {MAX_RESOLUTION_SCALE}]")
    if args.timeout is None:
        args.timeout = (3600 if args.orientation_sweep else 2400 if args.motion_sweep
                        else 600 if args.suite or args.single_pass_probe else 180)
    return args
