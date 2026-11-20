"""The fast comparison's commands.

`fast` runs a list of scenes against a live session and prints a table of the GPU comparisons.
`iterate` first builds the payload incrementally and re-injects it into the session's game, then
runs the scenes: the inner loop of single-pass work, with no checks beyond the build."""

import argparse
import dataclasses
import json
import logging
import sys
import time
from pathlib import Path

from tools.rendering.comparison.images import Thresholds
from tools.rendering.fast.plan import (
    DEFAULT_DEPTH_ULPS,
    FastError,
    Limits,
    baseline_counts,
    failed,
    table,
)
from tools.rendering.fast.run import FastOptions, run_scene, write_report
from tools.rendering.fast.suite import load_config, load_suite
from tools.rendering.protocol import ProtocolError
from tools.rendering.protocol.snapshot import DumpPolicy, Plane
from tools.rendering.session.attach import AttachedSession
from tools.rendering.session.errors import SessionError
from tools.rendering.session.reinject import build, reinject

LOG = logging.getLogger("tools.rendering.fast")
SESSIONS = Path("target/harness-probe")
DEFAULT_SUITE = Path(__file__).resolve().parents[1] / "suites/standing.toml"


def main(argv=None, iterate=False) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--session", type=int, help="the harness session number (persistent-N)")
    target.add_argument("--runner", type=Path, help="runner.json of a live session")
    scenes = parser.add_mutually_exclusive_group()
    scenes.add_argument("--configs", type=Path, nargs="+", help="experiment configs to run")
    scenes.add_argument("--suite", type=Path, help=f"a suite file (default {DEFAULT_SUITE.name})")
    parser.add_argument("--only", nargs="+", help="run only the suite's scenes with these names")
    parser.add_argument("--output", type=Path, help="the report directory (default: a new one "
                        "under target/harness-probe/fast)")
    parser.add_argument("--velocity", action="store_true", help="also compare the velocity buffer")
    parser.add_argument("--planes", nargs="+", choices=[plane.value for plane in Plane],
                        help="the planes to compare (default final, depth, albedo, normals)")
    parser.add_argument("--dump", choices=[policy.value for policy in DumpPolicy],
                        default=DumpPolicy.NEVER.value)
    parser.add_argument("--no-crop", action="store_true", help="dump whole eyes, not the differing region")
    parser.add_argument("--save", action="store_true", help="write every snapshot's planes whole, "
                        "for offline analysis (`fsr-motion`, `fsr-quality`)")
    parser.add_argument("--no-repeat", action="store_true", help="skip the noise-floor repeats")
    parser.add_argument("--repeat-after-seconds", type=float, default=0.0,
                        help="wait this long before each repeat snapshot, so the floor also "
                             "measures what changes with time at a frozen camera")
    parser.add_argument("--next-frame-repeat", action="store_true",
                        help="under a motion, take each repeat on the frame after its first "
                             "snapshot's phase, not at the same phase a cycle later")
    parser.add_argument("--jitter-phase", type=int, default=0,
                        help="at a still camera, the FSR jitter index each first snapshot waits "
                             "for (its repeat takes the next), while the jitter runs")
    parser.add_argument("--full-cycles", action="store_true",
                        help="run a motion that never moves the camera as configured, not as a "
                             "fixed pose")
    parser.add_argument("--restage", action="store_true", help="stage afresh even where a stage "
                        "could be reused")
    parser.add_argument("--baseline", type=Path, help="a previous fast-report.json: fail only on "
                        "regressions against it")
    parser.add_argument("--noise-factor", type=float, default=Limits().noise_factor)
    parser.add_argument("--warmup-frames", type=int, help="override every scene's warm-up frames "
                        "(1 to 300); the configs ask for 30")
    parser.add_argument("--settle-seconds", type=float, help="override every stage's settle "
                        "(0 to 300 seconds); the configs ask for 20")
    parser.add_argument("--depth-ulps", type=int, default=DEFAULT_DEPTH_ULPS,
                        help="how many representable floats apart two depths may be and still match "
                             f"(default {DEFAULT_DEPTH_ULPS}; 0 counts any changed bit)")
    parser.add_argument("--timeout", type=float, default=1800.0)
    if iterate:
        parser.add_argument("--no-build", action="store_true")
        parser.add_argument("--no-reinject", action="store_true")
        parser.add_argument("--release", action="store_true")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s",
                        stream=sys.stderr)
    runner = args.runner or SESSIONS / f"persistent-{args.session}/runner.json"
    try:
        if args.configs:
            scene_list = [load_config(path) for path in args.configs]
        else:
            scene_list = load_suite(args.suite or DEFAULT_SUITE)
        if args.warmup_frames is not None:
            if not 1 <= args.warmup_frames <= 300:
                raise FastError("fast: --warmup-frames must be within [1, 300]")
            scene_list = [dataclasses.replace(scene, config=dataclasses.replace(
                scene.config, warmup_frames=args.warmup_frames)) for scene in scene_list]
        if args.settle_seconds is not None:
            scene_list = [_with_settle(scene, args.settle_seconds) for scene in scene_list]
        if args.only:
            unknown = set(args.only) - {scene.name for scene in scene_list}
            if unknown:
                raise FastError(f"fast: no scenes named {sorted(unknown)}")
            scene_list = [scene for scene in scene_list if scene.name in args.only]
        planes = tuple(Plane(plane) for plane in (args.planes or
                                                  ("final", "depth", "albedo", "normals")))
        if args.velocity and Plane.VELOCITY not in planes:
            planes += (Plane.VELOCITY,)
        if Plane.FINAL not in planes:
            planes = (Plane.FINAL,) + planes
        options = FastOptions(planes, DumpPolicy(args.dump), not args.no_crop,
                              Limits(Thresholds(), args.noise_factor), args.restage,
                              not args.no_repeat, args.full_cycles, args.depth_ulps,
                              args.repeat_after_seconds, args.save, args.next_frame_repeat,
                              args.jitter_phase)
        baseline = (baseline_counts(json.loads(args.baseline.read_text()))
                    if args.baseline else None)
        output = args.output or SESSIONS / "fast" / time.strftime("%Y%m%d-%H%M%S")
        output.mkdir(parents=True, exist_ok=True)
        timings = {}
        if iterate and not args.no_build:
            timings["build_seconds"] = round(build(args.release), 2)
            LOG.info("fast: built in %.1f s", timings["build_seconds"])
        if iterate and not args.no_reinject:
            result = reinject(runner, args.release)
            timings["injector_seconds"] = round(result.injector_seconds, 2)
            timings["reinject_seconds"] = round(result.ready_seconds, 2)
            LOG.info("fast: re-injected; the payload was ready after %.1f s (%s)",
                     result.ready_seconds, result.session_log)
        results = []
        started = time.monotonic()
        with AttachedSession.attach(runner, args.timeout) as session:
            for scene in scene_list:
                LOG.info("fast: %s (%s)", scene.name, scene.path)
                outcome = run_scene(session, scene.config, scene.name, options, baseline)
                results.append(outcome)
                LOG.info("fast: %s took %.1f s%s", scene.name, outcome.seconds,
                         "" if not outcome.errors else f"; errors: {outcome.errors}")
                write_report(output / "fast-report.json", results, timings)
        timings["scenes_seconds"] = round(time.monotonic() - started, 2)
        write_report(output / "fast-report.json", results, timings)
    except (FastError, ProtocolError, SessionError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 2
    rows = [row for result in results for row in result.rows]
    print(table(rows, tuple(plane.value for plane in planes)))
    for result in results:
        stage = {None: "", True: ", stage reused", False: ", staged"}[result.stage_reused]
        print(f"{result.name}: {result.seconds:.1f} s{stage}"
              + "".join(f"\n  error: {error}" for error in result.errors))
    print("timings: " + ", ".join(f"{key} {value}" for key, value in timings.items())
          + f"; report {output / 'fast-report.json'}")
    if any(result.errors for result in results):
        return 2
    return 1 if failed(rows) else 0


def _with_settle(scene, seconds):
    stage = scene.config.scene.stage
    if stage is None:
        return scene
    try:
        stage = dataclasses.replace(stage, settle_seconds=seconds)
    except ProtocolError as error:
        raise FastError(f"fast: --settle-seconds: {error}") from error
    config = dataclasses.replace(scene.config, scene=dataclasses.replace(scene.config.scene,
                                                                        stage=stage))
    return dataclasses.replace(scene, config=config)


def iterate_main(argv=None) -> int:
    return main(argv, iterate=True)
