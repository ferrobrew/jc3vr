"""The benchmark's commands.

`benchmark --session N ...` runs the suite's scenes against a live session and writes a result
file per run; `benchmark summary <output>` combines the result files into `summary.json` and
`summary.md`; `benchmark charts <output>` renders the charts. See docs/mod/testing/benchmark.md."""

import argparse
import json
import logging
import shutil
import sys
import time
from pathlib import Path

from tools.rendering.benchmark.charts import render
from tools.rendering.benchmark.report import write_summary
from tools.rendering.benchmark.run import Mode, Options, run_scene
from tools.rendering.benchmark.suite import DEFAULT_SUITE, BenchmarkError, load_suite
from tools.rendering.protocol import ProtocolError
from tools.rendering.protocol.benchmark import BenchmarkMode
from tools.rendering.session.attach import AttachedSession
from tools.rendering.session.errors import SessionError
from tools.rendering.session.reinject import build, reinject

LOG = logging.getLogger("tools.rendering.benchmark")
SESSIONS = Path("target/harness-probe")
# The run stops before the disk would drop below this.
MIN_FREE_BYTES = 50 * 2**30


def main(argv=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s",
                        stream=sys.stderr)
    if argv and argv[0] in ("summary", "charts"):
        parser = argparse.ArgumentParser(prog=f"benchmark {argv[0]}")
        parser.add_argument("output", type=Path)
        args = parser.parse_args(argv[1:])
        try:
            if argv[0] == "summary":
                summary = write_summary(args.output)
                print((args.output / "summary.md").read_text(), end="")
                LOG.info("benchmark: %d groups summarized", len(summary["groups"]))
            else:
                for path in render(args.output):
                    print(path)
        except (OSError, ValueError, KeyError) as error:
            LOG.error("benchmark %s: %s", argv[0], error)
            return 2
        return 0
    return run_main(argv)


def run_main(argv) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--session", type=int, required=True, help="the session number (persistent-N)")
    parser.add_argument("--suite", type=Path, default=DEFAULT_SUITE)
    parser.add_argument("--only", nargs="+", help="run only the scenes with these names")
    parser.add_argument("--modes", nargs="+", default=["multipass", "single_pass"],
                        help="multipass, single_pass, and the suite's variant names")
    parser.add_argument("--scales", nargs="+", type=float,
                        help="vr.resolution_scale values to sweep (default: the session's)")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--record-seconds", type=float, default=10.0)
    parser.add_argument("--warmup-seconds", type=float, default=3.0)
    parser.add_argument("--no-gpu", action="store_true", help="skip the GPU timestamp queries")
    parser.add_argument("--detour-timing", action="store_true",
                        help="time the draw detours (inflates the cost it measures; a run of its own)")
    parser.add_argument("--no-draw-detours", action="store_true",
                        help="leave the draw detours uninstalled: in a fresh session, multipass then "
                             "runs without them (and its submissions are not counted)")
    parser.add_argument("--restage", action="store_true")
    parser.add_argument("--output", type=Path, help="the result directory (default: a new one "
                        "under target/harness-probe/benchmark)")
    parser.add_argument("--build", action="store_true", help="build and re-inject the payload first")
    parser.add_argument("--release", action="store_true",
                        help="with --build, the release payload (benchmarks should use it)")
    parser.add_argument("--timeout", type=float, default=4 * 3600.0)
    args = parser.parse_args(argv)
    runner = SESSIONS / f"persistent-{args.session}/runner.json"
    output = args.output or SESSIONS / "benchmark" / time.strftime("%Y%m%d-%H%M%S")
    try:
        suite = load_suite(args.suite)
        scenes = suite.scenes
        if args.only:
            unknown = set(args.only) - {scene.name for scene in scenes}
            if unknown:
                raise BenchmarkError(f"benchmark: no scenes named {sorted(unknown)}")
            scenes = tuple(scene for scene in scenes if scene.name in args.only)
        variants = {variant.name: variant for variant in suite.variants}
        modes = []
        for name in args.modes:
            if name in (BenchmarkMode.MULTIPASS, BenchmarkMode.SINGLE_PASS):
                modes.append(Mode.plain(BenchmarkMode(name)))
            elif name in variants:
                modes.append(Mode.variant(variants[name]))
            else:
                raise BenchmarkError(f"benchmark: unknown mode or variant {name!r}")
        if not 1 <= args.repeats <= 20:
            raise BenchmarkError("benchmark: --repeats must be within [1, 20]")
        options = Options(args.record_seconds, args.warmup_seconds, args.repeats,
                          tuple(args.scales) if args.scales else (None,), not args.no_gpu,
                          args.detour_timing, not args.no_draw_detours, args.restage,
                          _compositor_fps(args.session), args.session)
        output.mkdir(parents=True, exist_ok=True)
        if args.build:
            LOG.info("benchmark: built in %.1f s", build(args.release))
            result = reinject(runner, args.release)
            LOG.info("benchmark: re-injected; ready after %.1f s", result.ready_seconds)
        (output / "invocation.json").write_text(json.dumps({"argv": argv}, indent=1) + "\n")
        with AttachedSession.attach(runner, args.timeout) as session:
            for scene in scenes:
                if shutil.disk_usage(output).free < MIN_FREE_BYTES:
                    raise BenchmarkError("benchmark: less than 50 GiB free; stopping")
                LOG.info("benchmark: scene %s", scene.name)
                run_scene(session, scene, tuple(modes), options, output)
        write_summary(output)
    except (BenchmarkError, ProtocolError, SessionError, OSError, ValueError) as error:
        LOG.error("%s", error)
        if output.is_dir() and (output / "runs").is_dir():
            write_summary(output)
        return 2
    print((output / "summary.md").read_text(), end="")
    print(f"results: {output}")
    return 0


def _compositor_fps(session: int) -> int | None:
    """The pace session `session` was launched with, from its launch request (120 by default)."""
    request = SESSIONS / f"diagnostics-{session}/launch-request.json"
    try:
        argv = json.loads(request.read_text())["session"]
    except (OSError, ValueError, KeyError):
        return None
    if "--compositor-fps" in argv:
        return int(argv[argv.index("--compositor-fps") + 1])
    return 120
