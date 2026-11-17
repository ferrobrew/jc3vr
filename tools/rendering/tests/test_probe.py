import json
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from tools.rendering.capture.harness import main
from tools.rendering.session.errors import SessionError
from tools.rendering.session.launch import RenderingSession
from tools.rendering.session.options import Options, parse_args
from tools.rendering.capture.probe import PROBE_CASE, run_single_pass_probe
from tools.rendering.capture.suite import SuiteError
from tools.rendering.protocol import (
    Command,
    ConfigurationAck,
    Mode,
    ProbeResult,
    ProtocolError,
    RestorationAck,
    RunOutcome,
    RunReport,
    SmokeResult,
)
from tools.rendering.tests.test_suite import (
    capture_wire,
    capture_with_pose,
    pose_evidence,
    pose_wire,
)


class ProbeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.runner = RenderingSession(Options(single_pass_probe=True, probe_yaw=-60.0, probe_pitch=-10.0))
        self.runner.output = self.root
        self.runner.config_dir = self.root / "config"
        self.runner.config_dir.mkdir()
        self.runner.deadline = time.monotonic() + 20
        self.addCleanup(self.runner.close)

    def fake_run(self, capture_failure=None, restore_failure=None):
        commands = []
        poses = {}

        def request(value):
            commands.append(value)
            if value.command == Command.END:
                if restore_failure is not None:
                    raise restore_failure
                return RestorationAck.from_wire({"id": value.id, "state": "restored", "metadata": {
                    "generation": len(commands), "mode": None, "profile": "native", "ready": True,
                    "paused": False, "frozen": False, "error": None, "warmup_frames": 0,
                    "completed_frames": 0, "engine_dimensions": [2, 1]}})
            generation = len(commands)
            poses[generation] = value.pose
            metadata = {**capture_wire()["visual"], "mode": value.mode.value, "generation": generation,
                        "pose": pose_wire(pose_evidence(value.pose))}
            return ConfigurationAck.from_wire({"id": value.id, "state": "ready", "metadata": metadata})

        captures = []

        def screenshot(capture_id, mode, generation):
            if capture_failure is not None:
                raise capture_failure
            capture = capture_with_pose(mode, len(captures) + 1, generation, poses[generation], capture_id)
            captures.append(capture)
            return capture

        self.runner.smoke = Mock(return_value=SmokeResult(True))
        self.runner.settle = Mock()
        self.runner.request = Mock(side_effect=request)
        self.runner.screenshot = Mock(side_effect=screenshot)
        self.runner.compare = Mock()
        return commands

    def test_probe_configures_only_single_pass_and_captures_three_frames(self):
        commands = self.fake_run()
        result = run_single_pass_probe(self.runner)
        self.assertIsInstance(result, ProbeResult)
        self.assertEqual([request.command for request in commands], [Command.CONFIGURE, Command.END])
        configure = commands[0]
        self.assertEqual(configure.mode, Mode.SINGLE_PASS)
        self.assertEqual(configure.pose.case_id, PROBE_CASE)
        self.assertEqual(configure.pose.offset.yaw_deg, -60.0)
        self.assertEqual(configure.pose.offset.pitch_deg, -10.0)
        self.assertEqual(configure.pose.offset.roll_deg, 0.0)
        self.assertEqual(configure.pose.translation.right, 0.0)
        self.assertEqual(configure.pose.translation.up, 0.0)
        self.assertEqual(configure.pose.translation.forward, 0.0)
        self.runner.smoke.assert_called_once_with()
        self.runner.settle.assert_called_once_with(3)
        self.assertEqual(self.runner.screenshot.call_count, 3)
        for call in self.runner.screenshot.call_args_list:
            self.assertEqual(call.kwargs, {"mode": Mode.SINGLE_PASS, "generation": 1})
        self.assertEqual(len(result.probe_captures), 3)
        self.assertEqual(len(result.configurations), 1)
        self.runner.compare.assert_not_called()
        manifest = json.loads((self.root / "single_pass_probe.json").read_text())
        self.assertEqual([case["name"] for case in manifest["cases"]], [PROBE_CASE])
        self.assertEqual(manifest["cases"][0]["expected_pose"]["case_id"], PROBE_CASE)
        self.assertEqual(list(manifest["cases"][0]["modes"]), ["single_pass"])
        self.assertEqual(len(manifest["cases"][0]["modes"]["single_pass"]), 3)

    def test_restoration_failure_is_recorded_and_preserves_the_capture_error(self):
        self.fake_run(capture_failure=SessionError("capture failed"),
                      restore_failure=SessionError("restore failed"))
        with self.assertLogs("tools.rendering.capture.harness", level="ERROR"), \
                self.assertRaisesRegex(SessionError, "capture failed") as raised:
            run_single_pass_probe(self.runner)
        self.assertIn("restoration failed", raised.exception.__notes__[0])
        self.assertEqual(self.runner.result.restoration_error, "restore failed")
        self.assertEqual(self.runner.request.call_args_list[-1].args[0].id, "probe_end")

    def test_restoration_failure_alone_fails_the_probe(self):
        self.fake_run(restore_failure=SessionError("restore failed"))
        with self.assertRaisesRegex(SessionError, "restore failed"):
            run_single_pass_probe(self.runner)
        self.assertEqual(self.runner.result.restoration_error, "restore failed")

    def test_cli_probe_options_and_orientation_bounds(self):
        args = parse_args(["--single-pass-probe", "--probe-yaw", "-60", "--probe-pitch", "-10",
                           "--probe-right", "2", "--probe-up", "3", "--probe-forward", "4"])
        self.assertTrue(args.single_pass_probe)
        self.assertEqual((args.probe_yaw, args.probe_pitch), (-60.0, -10.0))
        self.assertEqual((args.probe_right, args.probe_up, args.probe_forward), (2.0, 3.0, 4.0))
        self.assertEqual(parse_args(["--single-pass-probe"]).timeout, 600)
        defaults = parse_args([])
        self.assertEqual((defaults.probe_yaw, defaults.probe_pitch), (0.0, 0.0))
        self.assertEqual((defaults.probe_right, defaults.probe_up, defaults.probe_forward),
                         (0.0, 0.0, 0.0))
        for yaw, pitch in ((181.0, 0.0), (0.0, -180.5), (float("nan"), 0.0), (0.0, float("inf"))):
            runner = RenderingSession(Options(single_pass_probe=True, probe_yaw=yaw, probe_pitch=pitch))
            with self.subTest(yaw=yaw, pitch=pitch), patch.object(runner, "reject_existing") as reject, \
                    self.assertRaises(SuiteError):
                runner.check()
            reject.assert_not_called()
        for translation in ((5000.5, 0.0, 0.0), (0.0, float("nan"), 0.0), (0.0, 0.0, -5000.5)):
            runner = RenderingSession(Options(single_pass_probe=True, probe_right=translation[0],
                                           probe_up=translation[1], probe_forward=translation[2]))
            with self.subTest(translation=translation), patch.object(runner, "reject_existing") as reject, \
                    self.assertRaises(SuiteError):
                runner.check()
            reject.assert_not_called()
        with patch.object(self.runner, "reject_existing") as reject, \
                self.assertRaisesRegex(SessionError, "mutually exclusive"):
            RenderingSession(Options(suite=True, single_pass_probe=True)).check()
        reject.assert_not_called()

    def test_probe_result_never_fabricates_a_pass(self):
        report = RunReport()
        result = ProbeResult(True, (), probe_manifest="single_pass_probe.json")
        report.complete(result)
        self.assertIs(report.result, result)
        self.assertEqual(report.status, RunOutcome.COMPLETED)
        self.assertEqual(report.exit_code, 2)
        wire = report.to_wire()
        self.assertEqual(wire["diagnostic"], "inconclusive")
        self.assertNotIn("visual_passed", wire)
        with patch.object(RenderingSession, "run", autospec=True) as run, \
                patch.object(RenderingSession, "close"), patch("tools.rendering.capture.harness.signal.signal"):
            def success(runner, callback):
                runner.result.complete(result)

            run.side_effect = success
            self.assertEqual(main(["--single-pass-probe"]), 2)
        with self.assertRaises(ProtocolError):
            report.complete(object())
