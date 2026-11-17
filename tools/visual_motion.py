"""Motion sweep orchestration for the visual harness."""

import logging
import time

if __package__:
    from .visual_motion_protocol import MotionRequest, PhaseCapture
    from .visual_protocol import (
        Capture,
        Case,
        Command,
        ConfigurationAck,
        ExpectedMotion,
        Manifest,
        Mode,
        Request,
        RestorationAck,
        Sample,
        SuiteResult,
    )
    from .visual_suite import SuiteError, write_manifest
else:
    from visual_motion_protocol import MotionRequest, PhaseCapture
    from visual_protocol import (
        Capture,
        Case,
        Command,
        ConfigurationAck,
        ExpectedMotion,
        Manifest,
        Mode,
        Request,
        RestorationAck,
        Sample,
        SuiteResult,
    )
    from visual_suite import SuiteError, write_manifest


LOG = logging.getLogger("visual_harness")
MOTION_SWEEP = MotionRequest("motion", 120, 20.0, 10.0, 10.0)
MOTION_PHASES = (0, 15, 30, 45, 60, 75, 90, 105)
MOTION_MODES = (
    ("left", Mode.LEFT_ONLY),
    ("right", Mode.RIGHT_ONLY),
    ("multipass", Mode.MULTIPASS),
    ("single_pass", Mode.SINGLE_PASS),
)
MOTION_RETURN = ("multipass_return", Mode.MULTIPASS)


def run_motion_sweep(runner) -> SuiteResult:
    smoke = runner.smoke()
    runner.settle(3)
    modes = list(MOTION_MODES)
    if runner.options.reverse_multipass:
        modes.append(("reverse_multipass", Mode.REVERSE_MULTIPASS))
    modes.append(MOTION_RETURN)
    cases = [Case(f"phase_{phase:03d}", {}, expected_motion=ExpectedMotion(MOTION_SWEEP, phase))
             for phase in MOTION_PHASES]
    by_phase = {case.expected_motion.phase: case for case in cases}
    manifest = Manifest(cases)
    path = runner.output / "motion_sweep.json"
    captures = []
    configurations = []
    anchor = None
    failure = None
    try:
        for index, (group, mode) in enumerate(modes):
            request = Request(Command.CONFIGURE, f"motion_config_{index}", mode,
                              runner.options.warmup_frames, motion=MOTION_SWEEP)
            acknowledgement = runner.request(request)
            if not isinstance(acknowledgement, ConfigurationAck):
                raise SuiteError("visual motion: expected a configuration acknowledgement")
            if (acknowledgement.id != request.id or acknowledgement.mode != mode or
                    acknowledgement.warmup_frames != request.warmup_frames):
                raise SuiteError("visual motion: configuration acknowledgement disagrees with its request")
            if configurations and acknowledgement.generation <= configurations[-1].generation:
                raise SuiteError("visual motion: configuration generation does not increase")
            anchor = acknowledgement.validate_motion(request, anchor)
            configurations.append(acknowledgement)
            for repeat in range(runner.options.samples):
                for phase in MOTION_PHASES:
                    capture_id = f"motion_{group}_{phase:03d}_c{repeat + 1}"
                    capture = runner.screenshot(capture_id, mode=mode, generation=acknowledgement.generation,
                                                motion_capture=PhaseCapture(phase))
                    if not isinstance(capture, Capture):
                        raise SuiteError("visual motion: expected a diagnostic capture")
                    if capture.visual.motion is None or capture.visual.pose is None:
                        raise SuiteError(f"visual motion: capture {capture_id} has no motion evidence")
                    anchor = capture.visual.motion.validate_capture(
                        MOTION_SWEEP, phase, anchor, capture.visual.pose,
                        capture.comparison_inputs.camera, capture.predicted_display_time_ns)
                    if captures and (capture.frame_ordinal <= captures[-1].frame_ordinal or
                                     capture.predicted_display_time_ns <= captures[-1].predicted_display_time_ns):
                        raise SuiteError("visual motion: captures do not identify increasing rendered frames")
                    captures.append(capture)
                    folder = runner.config_dir / "captures"
                    by_phase[phase].modes.setdefault(mode, []).append(Sample(
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
            restoration = runner.request(Request(Command.END, "motion_end"))
            if not isinstance(restoration, RestorationAck):
                raise SuiteError("visual motion: expected a restoration acknowledgement")
            if restoration.id != "motion_end":
                raise SuiteError("visual motion: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise SuiteError("visual motion: restoration generation does not increase")
        except BaseException as error:
            runner.result.restoration_error = str(error)
            if failure is None:
                raise
            failure.add_note(f"visual motion: restoration failed: {error}")
            LOG.error("visual motion: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
    comparison = runner.compare(path)
    return SuiteResult(smoke.smoke_succeeded, smoke.captures, suite_succeeded=True,
                       suite_manifest=str(path), suite_captures=tuple(captures),
                       configurations=tuple(configurations), comparison=comparison)
