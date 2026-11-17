"""Run a focused single-pass stereo probe at one orientation."""

import logging
import time

if __package__:
    from .visual_protocol import (
        Capture,
        Case,
        Command,
        ConfigurationAck,
        Manifest,
        Mode,
        OrientationOffset,
        PoseRequest,
        ProbeResult,
        ProtocolError,
        Request,
        RestorationAck,
        Sample,
        TranslationOffset,
    )
    from .visual_suite import SuiteError, write_manifest
else:
    from visual_protocol import (
        Capture,
        Case,
        Command,
        ConfigurationAck,
        Manifest,
        Mode,
        OrientationOffset,
        PoseRequest,
        ProbeResult,
        ProtocolError,
        Request,
        RestorationAck,
        Sample,
        TranslationOffset,
    )
    from visual_suite import SuiteError, write_manifest


LOG = logging.getLogger("visual_harness")

PROBE_CASE = "water_probe"
PROBE_MODE = Mode.SINGLE_PASS
PROBE_SAMPLES = 3
PROBE_MANIFEST = "single_pass_probe.json"


def probe_pose(options) -> PoseRequest:
    translation = TranslationOffset(options.probe_right, options.probe_up, options.probe_forward)
    return PoseRequest(PROBE_CASE,
                       OrientationOffset(options.probe_yaw, options.probe_pitch, 0.0),
                       translation)


def check_probe_options(options):
    try:
        probe_pose(options)
    except ProtocolError as error:
        raise SuiteError(f"visual probe: {error}") from error


def run_single_pass_probe(runner) -> ProbeResult:
    smoke = runner.smoke()
    runner.settle(3)
    pose = probe_pose(runner.options)
    case = Case(PROBE_CASE, {}, pose)
    manifest = Manifest([case])
    path = runner.output / PROBE_MANIFEST
    captures = []
    configurations = []
    anchor = None
    failure = None
    try:
        request = Request(Command.CONFIGURE, "probe_config", PROBE_MODE,
                          runner.options.warmup_frames, pose)
        acknowledgement = runner.request(request)
        if not isinstance(acknowledgement, ConfigurationAck):
            raise SuiteError("visual probe: expected a configuration acknowledgement")
        if (acknowledgement.id != request.id or acknowledgement.mode != PROBE_MODE or
                acknowledgement.warmup_frames != request.warmup_frames):
            raise SuiteError("visual probe: configuration acknowledgement disagrees with its request")
        anchor = acknowledgement.validate_pose(request, anchor)
        configurations.append(acknowledgement)
        entries = case.modes.setdefault(PROBE_MODE, [])
        for sample in range(PROBE_SAMPLES):
            capture_id = f"{PROBE_CASE}_{sample + 1}"
            capture = runner.screenshot(capture_id, mode=PROBE_MODE,
                                        generation=acknowledgement.generation)
            if not isinstance(capture, Capture):
                raise SuiteError("visual probe: expected a diagnostic capture")
            if capture.visual.pose is None:
                raise SuiteError(f"visual probe: capture {capture_id} has no pose evidence")
            anchor = capture.visual.pose.validate(pose, anchor)
            if captures and (capture.frame_ordinal <= captures[-1].frame_ordinal or
                             capture.predicted_display_time_ns <= captures[-1].predicted_display_time_ns):
                raise SuiteError("visual probe: captures do not identify increasing rendered frames")
            captures.append(capture)
            folder = runner.config_dir / "captures"
            entries.append(Sample(
                str((folder / f"{capture_id}.capture.png").relative_to(path.parent)),
                str((folder / f"{capture_id}.capture.json").relative_to(path.parent))))
            write_manifest(path, manifest)
    except BaseException as error:
        failure = error
        raise
    finally:
        deadline = runner.deadline
        try:
            runner.deadline = max(deadline, time.monotonic() + 35)
            runner.cancel_pending_request()
            restoration = runner.request(Request(Command.END, "probe_end"))
            if not isinstance(restoration, RestorationAck):
                raise SuiteError("visual probe: expected a restoration acknowledgement")
            if restoration.id != "probe_end":
                raise SuiteError("visual probe: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise SuiteError("visual probe: restoration generation does not increase")
        except BaseException as error:
            runner.result.restoration_error = str(error)
            if failure is None:
                raise
            failure.add_note(f"visual probe: restoration failed: {error}")
            LOG.error("visual probe: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
    return ProbeResult(smoke.smoke_succeeded, smoke.captures, probe_manifest=str(path),
                       probe_captures=tuple(captures), configurations=tuple(configurations))
