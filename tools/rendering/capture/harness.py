"""Launch-command orchestration for the visual capture harness."""

import logging
import signal
import sys
import time

from tools.rendering.capture.motion import run_motion_sweep
from tools.rendering.capture.probe import run_single_pass_probe
from tools.rendering.capture.suite import (
    SuiteError,
    run_orientation_sweep,
    run_suite,
)
from tools.rendering.capture.validation import validate_capture
from tools.rendering.protocol import (
    Command,
    FailedAck,
    ProtocolError,
    Request,
    ScreenshotAck,
    SmokeResult,
)
from tools.rendering.session.errors import SessionError
from tools.rendering.session.launch import RenderingSession
from tools.rendering.session.options import Options, parse_args

LOG = logging.getLogger("tools.rendering.capture.harness")
CALLBACKS = (("orientation_sweep", run_orientation_sweep), ("motion_sweep", run_motion_sweep),
             ("suite", run_suite), ("single_pass_probe", run_single_pass_probe))


def default_smoke(runner):
    runner.inject_until_ready()
    captures = []
    attempt = 0
    while len(captures) < 2:
        runner.assert_running()
        attempt += 1
        capture_id = f"smoke_{attempt}"
        result = runner.issue(Request(Command.SCREENSHOT, capture_id))
        if isinstance(result, FailedAck):
            if result.gameplay_ready is True:
                runner.mark_gameplay_ready()
            if result.gameplay_ready is False and (
                    "not_ready" in result.error or "not ready" in result.error or "waiting for gameplay" in result.error):
                runner.pump_menu()
                time.sleep(0.5)
                continue
            raise SessionError(f"session: screenshot {capture_id}: {result.error}")
        if not isinstance(result, ScreenshotAck):
            raise ProtocolError("visual protocol: expected a screenshot acknowledgement")
        if result.capture.gameplay_ready:
            runner.mark_gameplay_ready()
        captures.append(validate_capture(runner.config_dir / "captures", capture_id, result.capture))
    first, second = captures
    if second.frame_ordinal <= first.frame_ordinal or second.predicted_display_time_ns == first.predicted_display_time_ns:
        raise SessionError("session: sequential captures do not identify distinct rendered frames")
    return SmokeResult(True, tuple(captures))


def main(argv=None, keep_alive=False):
    description = ("Launch Just Cause 3 in a private headless VR session and keep it alive until "
                   "interrupted." if keep_alive else None)
    args = parse_args(argv, keep_alive=keep_alive, description=description)
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s", stream=sys.stderr)
    def interrupted(signum, _frame):
        raise KeyboardInterrupt(f"signal {signum}")
    for sig in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, interrupted)
    runner = RenderingSession(Options(**vars(args)), smoke=default_smoke)
    try:
        if args.check:
            runner.check()
            return 0
        callback = None if args.launch_only or args.keep_alive else next(
            (callback for name, callback in CALLBACKS if getattr(args, name)), default_smoke)
        runner.run(callback)
        return runner.result.exit_code
    except KeyboardInterrupt:
        LOG.error("session: interrupted")
        return 130
    except (SessionError, SuiteError, ProtocolError, OSError, ValueError) as error:
        LOG.error("%s", error)
        return 1
    finally:
        runner.close()
