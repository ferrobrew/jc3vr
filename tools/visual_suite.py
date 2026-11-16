"""Orchestrate the unattended visual comparison suite."""

import json
import logging
import time
from pathlib import Path

if __package__:
    from .visual_protocol import (
        Case,
        Command,
        ConfigurationAck,
        Manifest,
        Mode,
        Request,
        RestorationAck,
        Sample,
        SuiteResult,
    )
else:
    from visual_protocol import (
        Case,
        Command,
        ConfigurationAck,
        Manifest,
        Mode,
        Request,
        RestorationAck,
        Sample,
        SuiteResult,
    )


LOG = logging.getLogger("visual_harness")


class SuiteError(RuntimeError):
    pass


def run_suite(runner) -> SuiteResult:
    smoke = runner.smoke()
    runner.settle(3)
    modes = [Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS]
    if runner.options.reverse_multipass:
        modes.append(Mode.REVERSE_MULTIPASS)
    sequence = [(mode.value, mode) for mode in modes] + [("multipass_return", Mode.MULTIPASS)]
    case = Case("gameplay", {})
    manifest = Manifest([case])
    path = runner.output / "suite.json"
    captures = []
    configurations = []
    failure = None
    try:
        for index, (group, mode) in enumerate(sequence):
            request = Request(Command.CONFIGURE, f"suite_config_{index}", mode,
                              runner.options.warmup_frames)
            acknowledgement = runner.request(request)
            if not isinstance(acknowledgement, ConfigurationAck):
                raise SuiteError("visual suite: expected a configuration acknowledgement")
            if acknowledgement.id != request.id or acknowledgement.mode != mode or acknowledgement.warmup_frames != request.warmup_frames:
                raise SuiteError("visual suite: configuration acknowledgement disagrees with its request")
            if configurations and acknowledgement.generation <= configurations[-1].generation:
                raise SuiteError("visual suite: configuration generation does not increase")
            configurations.append(acknowledgement)
            entries = case.modes.setdefault(mode, [])
            for sample in range(runner.options.samples):
                capture_id = f"suite_{group}_{sample + 1}"
                capture = runner.screenshot(capture_id, mode=mode, generation=acknowledgement.generation)
                if captures and (capture.frame_ordinal <= captures[-1].frame_ordinal or
                                 capture.predicted_display_time_ns <= captures[-1].predicted_display_time_ns):
                    raise SuiteError("visual suite: captures do not identify increasing rendered frames")
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
            # Restoration has a bounded cleanup budget even when the run deadline expires.
            runner.deadline = max(deadline, time.monotonic() + 35)
            runner.cancel_pending_request()
            restoration = runner.request(Request(Command.END, "suite_end"))
            if not isinstance(restoration, RestorationAck):
                raise SuiteError("visual suite: expected a restoration acknowledgement")
            if restoration.id != "suite_end":
                raise SuiteError("visual suite: restoration acknowledgement ID mismatch")
            if configurations and restoration.metadata.generation <= configurations[-1].generation:
                raise SuiteError("visual suite: restoration generation does not increase")
        except BaseException as error:
            runner.result.restoration_error = str(error)
            if failure is None:
                raise
            failure.add_note(f"visual suite: restoration failed: {error}")
            LOG.error("visual suite: restoration failed: %s", error)
        finally:
            runner.deadline = deadline
    comparison = runner.compare(path)
    return SuiteResult(smoke.smoke_succeeded, smoke.captures, suite_succeeded=True,
                      suite_manifest=str(path), suite_captures=tuple(captures),
                      configurations=tuple(configurations), comparison=comparison)


def write_manifest(path, manifest):
    temporary = Path(path).with_suffix(".tmp")
    temporary.write_text(json.dumps(manifest.to_wire(), indent=2) + "\n")
    temporary.replace(path)
