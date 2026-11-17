"""Command-line parsing for the visual harness."""

import argparse
from pathlib import Path


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Run Just Cause 3 in a private headless VR session.")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--release", action="store_true")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--launch-only", action="store_true", help="launch without claiming smoke success")
    mode.add_argument("--smoke", action="store_true", help="run the default two-capture smoke check")
    mode.add_argument("--suite", action="store_true", help="run the four-mode visual comparison suite")
    mode.add_argument("--orientation-sweep", action="store_true",
                      help="run the frozen-anchor orientation sweep")
    mode.add_argument("--motion-sweep", action="store_true",
                      help="run the rendered-frame motion sweep")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--warmup-frames", type=int, default=30)
    parser.add_argument("--reverse-multipass", action="store_true")
    parser.add_argument("--gpu", help="Vulkan PCI vendor:device, e.g. 10de:2b85")
    parser.add_argument("--xdotool")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--timeout", type=float)
    for name, default in (("startup-timeout", 90), ("build-timeout", 900),
                          ("copy-timeout", 180), ("menu-interval", 3), ("menu-timeout", 60)):
        parser.add_argument(f"--{name}", type=float, default=default)
    args = parser.parse_args(argv)
    if args.timeout is None:
        args.timeout = (3600 if args.orientation_sweep else 2400 if args.motion_sweep
                        else 600 if args.suite else 180)
    return args
