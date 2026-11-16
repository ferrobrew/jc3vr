import json
import os
import struct
import subprocess
import tempfile
import time
import tomllib
import unittest
import zlib
from dataclasses import replace
from pathlib import Path
from unittest.mock import Mock, patch

from tools.visual_harness import (
    HarnessError,
    Options,
    VisualHarness,
    default_smoke,
    main,
    validate_capture,
)
from tools.visual_protocol import (
    AckOutcome,
    Capture,
    Command,
    ComparisonReport,
    ConfigurationAck,
    DiagnosticProfile,
    FailedAck,
    Mode,
    NativeCapture,
    Outcome,
    ProtocolError,
    Request,
    RestorationAck,
    ScreenshotAck,
    SmokeResult,
    acknowledgement_from_wire,
)
from tools.visual_suite import SuiteError, run_suite


def capture_wire(mode=Mode.MULTIPASS, ordinal=1, generation=1):
    selected = mode.selected_eye
    return {
        "id": "sample", "frame_ordinal": ordinal, "predicted_display_time_ns": ordinal * 100,
        "source": "isolated_eye" if selected is not None else "eye_textures",
        "width": 1 if selected is not None else 2, "height": 1, "eye_width": 1, "eye_height": 1,
        "fresh_eyes": list(mode.rendered_eyes), "rendered_eyes": list(mode.rendered_eyes),
        "selected_eye": selected, "gameplay_ready": True, "xr_session_running": True,
        "mode": mode.value, "visual": {"generation": generation, "mode": mode.value,
                                        "profile": "controlled_v1", "ready": True, "paused": True, "frozen": True,
                                        "error": None, "warmup_frames": 30, "completed_frames": 30,
                                        "engine_dimensions": [2, 1]},
        "comparison_inputs": {
            "build_stamp": "fixture", "profile": "controlled_v1", "eye_width": 1, "eye_height": 1,
            "world_scale": 1.0, "camera": {"position": [0, 0, 0], "orientation": [0, 0, 0, 1]},
            "eyes": [{"offset": [0, 0, 0], "orientation_delta": [0, 0, 0, 1],
                      "projection_standard": [0] * 16, "projection_reverse_z": [0] * 16} for _ in range(2)],
        },
    }


def native_capture_wire(ordinal=1):
    wire = capture_wire(ordinal=ordinal, generation=0)
    wire.update(id=f"smoke_{ordinal}", state="captured", mode="native", xr_frame_rendering=True,
                build_stamp="fixture", png="Z:\\captures\\sample.capture.png",
                metadata="Z:\\captures\\sample.capture.json", format="DXGI_FORMAT(28)",
                effective_config={"testing": {"enabled": True, "request_file": True}}, single_pass=None)
    wire["visual"].update(mode=None, profile="native", paused=False, frozen=False,
                          warmup_frames=0, completed_frames=0, engine_dimensions=None)
    inputs = wire["comparison_inputs"]
    inputs.update(profile="native", camera=None)
    for index, eye in enumerate(inputs["eyes"]):
        eye.update(offset=[-0.0315 if index == 0 else 0.0315, 0.0, 0.0],
                   projection_standard=[1.09, 0, 0, 0, 0, 1.01, 0, 0, 0, 0, -1.0000005, -1, 0, 0, -0.02, 0],
                   projection_reverse_z=[1.09, 0, 0, 0, 0, 1.01, 0, 0, 0, 0, 0.0000005, -1, 0, 0, 0.02, 0])
    return wire


def png(width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress((b"\0" + b"\xff" * width * 4) * height)) + chunk(b"IEND", b""))


class SuiteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.runner = VisualHarness(Options(suite=True))
        self.runner.output = self.root
        self.runner.config_dir = self.root / "config"
        self.runner.config_dir.mkdir()
        self.runner.deadline = time.monotonic() + 20
        self.addCleanup(self.runner.close)

    def fake_run(self, failure=None):
        commands = []
        def request(value):
            commands.append(value)
            if value.command == Command.END:
                return RestorationAck.from_wire({"id": value.id, "state": "restored", "metadata": {
                    "generation": len(commands), "mode": None, "profile": "native", "ready": True,
                    "paused": False, "frozen": False, "error": None, "warmup_frames": 0,
                    "completed_frames": 0, "engine_dimensions": [2, 1]}})
            return ConfigurationAck.from_wire({"id": value.id, "state": "ready", "metadata": {"generation": len(commands),
                    "mode": value.mode, "profile": "controlled_v1", "ready": True,
                    "paused": True, "frozen": True, "warmup_frames": value.warmup_frames,
                    "completed_frames": value.warmup_frames, "error": None, "engine_dimensions": [2, 1]}})
        frames = []
        def screenshot(capture_id, mode, generation):
            if failure:
                raise failure
            capture = replace(Capture.from_wire(capture_wire(mode, len(frames) + 1, generation)), id=capture_id)
            frames.append(capture)
            return capture
        self.runner.smoke = Mock(return_value=SmokeResult(True))
        self.runner.settle = Mock()
        self.runner.request = Mock(side_effect=request)
        self.runner.screenshot = Mock(side_effect=screenshot)
        self.runner.compare = Mock(return_value=ComparisonReport(Outcome.PASS))
        return commands

    def test_sequence_samples_manifest_and_aba_return(self):
        commands = self.fake_run()
        result = run_suite(self.runner)
        self.assertTrue(result.suite_succeeded)
        self.runner.smoke.assert_called_once_with()
        self.runner.settle.assert_called_once_with(3)
        self.assertEqual([request.mode for request in commands[:-1]], [
            Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS, Mode.MULTIPASS])
        self.assertEqual(commands[-1].command, Command.END)
        manifest = json.loads((self.root / "suite.json").read_text())
        modes = manifest["cases"][0]["modes"]
        self.assertEqual({mode: len(samples) for mode, samples in modes.items()}, {
            "left_only": 3, "right_only": 3, "multipass": 6, "single_pass": 3})
        for samples in modes.values():
            for sample in samples:
                self.assertFalse(Path(sample["png"]).is_absolute())
                self.assertTrue(sample["metadata"].endswith(".capture.json"))
        self.runner.compare.assert_called_once_with(self.root / "suite.json")

    def test_suite_report_keeps_the_json_contract(self):
        self.fake_run()
        with patch.object(self.runner, "start", return_value=self.runner):
            result = self.runner.run(run_suite)
        self.assertIs(self.runner.result.result, result)
        report = json.loads((self.root / "result.json").read_text())
        self.assertEqual(set(report), {"status", "smoke_succeeded", "captures", "suite_succeeded",
                                      "suite_manifest", "suite_captures", "configurations", "comparison",
                                      "cleanup_errors", "visual_passed"})
        self.assertEqual(report["status"], "completed")
        self.assertEqual(report["comparison"], {"status": "pass"})
        self.assertTrue(report["visual_passed"])
        self.assertEqual(report["captures"], [])
        self.assertEqual(len(report["suite_captures"]), 15)
        self.assertEqual(set(report["configurations"][0]), {"id", "metadata"})
        self.assertEqual(report["configurations"][0]["metadata"]["mode"], "left_only")
        self.assertEqual(report["cleanup_errors"], [])

    def test_nonpass_suite_retains_captures_and_controls_the_cli_exit(self):
        for outcome, exit_code in ((Outcome.FAIL, 1), (Outcome.INCONCLUSIVE, 2)):
            with self.subTest(outcome=outcome):
                self.runner = VisualHarness(Options(suite=True))
                self.runner.output = self.root
                self.runner.config_dir = self.root / "config"
                self.runner.deadline = time.monotonic() + 20
                self.fake_run()
                self.runner.compare.return_value = ComparisonReport(outcome)
                with patch.object(self.runner, "start", return_value=self.runner), \
                        patch("tools.visual_harness.VisualHarness", return_value=self.runner), \
                        patch("tools.visual_harness.signal.signal"):
                    self.assertEqual(main(["--suite"]), exit_code)
                result = self.runner.result.result
                self.assertTrue(result.suite_succeeded)
                self.assertFalse(result.visual_passed)
                self.assertEqual(result.comparison.status, outcome)
                self.assertEqual(self.runner.result.exit_code, exit_code)
                report = json.loads((self.root / "result.json").read_text())
                self.assertEqual(report["status"], "completed")
                self.assertTrue(report["smoke_succeeded"])
                self.assertTrue(report["suite_succeeded"])
                self.assertFalse(report["visual_passed"])
                self.assertEqual(report["comparison"], {"status": outcome.value})
                self.assertEqual(len(report["configurations"]), 5)
                self.assertEqual(len(report["suite_captures"]), 15)
                self.assertNotIn("error", report)

    def test_failed_acknowledgements_are_typed(self):
        screenshot = acknowledgement_from_wire({
            "id": "sample", "state": "failed", "error": "not_ready", "gameplay_ready": False,
        }, Command.SCREENSHOT, "sample")
        self.assertEqual(screenshot, FailedAck("sample", "not_ready", False))
        self.assertIs(screenshot.outcome, AckOutcome.FAILED)
        configuration = acknowledgement_from_wire({
            "id": "configure", "state": "failed", "metadata": {"error": "warmup failed"},
        }, Command.CONFIGURE, "configure")
        self.assertEqual(configuration, FailedAck("configure", "warmup failed", None))
        for state in ("unknown", "completed"):
            with self.subTest(state=state), self.assertRaises(ProtocolError):
                acknowledgement_from_wire({"id": "configure", "state": state}, Command.CONFIGURE, "configure")

    def test_native_payload_shape_completes_the_default_smoke(self):
        folder = self.runner.config_dir / "captures"
        folder.mkdir()
        published = []
        def publish(_):
            path = self.runner.config_dir / "test_request.toml"
            if not path.exists():
                return
            request = tomllib.loads(path.read_text())
            path.unlink()
            wire = native_capture_wire(len(published) + 1)
            self.assertEqual(request["id"], wire["id"])
            capture_id = wire["id"]
            (folder / f"{capture_id}.capture.json").write_text(json.dumps(wire))
            (folder / f"{capture_id}.capture.png").write_bytes(png(2, 1))
            acknowledgement = {**wire, "state": "completed"}
            parsed = acknowledgement_from_wire(acknowledgement, Command.SCREENSHOT, capture_id)
            self.assertIsInstance(parsed, ScreenshotAck)
            self.assertIs(type(parsed.capture), NativeCapture)
            (folder / f"{capture_id}.json").write_text(json.dumps(acknowledgement))
            published.append(capture_id)
        with patch.object(self.runner, "inject", return_value=subprocess.CompletedProcess([], 0, "")), \
                patch.object(self.runner, "pump_menu"), \
                patch("tools.visual_harness.time.sleep", side_effect=publish):
            result = default_smoke(self.runner)
        self.assertTrue(result.smoke_succeeded)
        self.assertEqual(len(result.captures), 2)
        self.assertEqual(result.captures[1].frame_ordinal, 2)
        self.assertTrue(all(type(capture) is NativeCapture for capture in result.captures))

    def test_optional_reverse_mode_precedes_return(self):
        self.runner.options.reverse_multipass = True
        commands = self.fake_run()
        run_suite(self.runner)
        self.assertEqual([request.mode for request in commands[-3:-1]], [Mode.REVERSE_MULTIPASS, Mode.MULTIPASS])

    def test_restoration_requires_ready_error_free_dimensions(self):
        metadata = {**capture_wire()["visual"], "mode": None, "profile": "native"}
        for change in ({"ready": False}, {"error": "resize failed"}, {"engine_dimensions": None},
                       {"engine_dimensions": [0, 1]}, {"engine_dimensions": [True, 1]}):
            with self.subTest(change=change), self.assertRaises(ProtocolError):
                RestorationAck.from_wire({"id": "end", "state": "restored", "metadata": {**metadata, **change}})
        restoration = RestorationAck.from_wire({"id": "end", "state": "restored", "metadata": metadata})
        self.assertTrue(restoration.metadata.paused)
        self.assertTrue(restoration.metadata.frozen)

    def test_warmup_boundaries_match_the_payload(self):
        for count in (1, 300):
            with self.subTest(count=count):
                self.assertEqual(Request(Command.CONFIGURE, "configure", Mode.LEFT_ONLY, count).warmup_frames, count)
        wire = {"id": "configure", "state": "ready", "metadata": {
            **capture_wire()["visual"], "warmup_frames": 301, "completed_frames": 301}}
        with self.assertRaisesRegex(ProtocolError, "warmup"):
            ConfigurationAck.from_wire(wire)

    def test_stale_restoration_generation_prevents_comparison(self):
        self.fake_run()
        original = self.runner.request.side_effect
        def request(value):
            acknowledgement = original(value)
            if value.command == Command.END:
                return replace(acknowledgement, metadata=replace(acknowledgement.metadata, generation=5))
            return acknowledgement
        self.runner.request.side_effect = request
        with self.assertRaisesRegex(SuiteError, "restoration generation"):
            run_suite(self.runner)
        self.runner.compare.assert_not_called()

    def test_expired_suite_cancels_owned_request_before_restoration(self):
        real_request = self.runner.request
        self.fake_run()
        original = self.runner.request.side_effect
        pending = self.runner.config_dir / "test_request.toml"
        deadline = self.runner.deadline
        def request(value):
            if value.command == Command.CONFIGURE:
                with patch.object(self.runner, "assert_running"), patch.object(self.runner, "pump_menu"), \
                        patch("tools.visual_harness.time.monotonic", side_effect=[0, 76]):
                    return real_request(value)
            self.assertFalse(pending.exists())
            self.assertGreaterEqual(self.runner.deadline, time.monotonic() + 34)
            return original(value)
        self.runner.request.side_effect = request
        with self.assertRaisesRegex(HarnessError, "timed out waiting"):
            run_suite(self.runner)
        self.assertEqual(self.runner.deadline, deadline)
        cancelled = list(self.runner.config_dir.glob("test_request.toml.*.cancelled"))
        self.assertEqual(len(cancelled), 1)
        self.assertEqual(cancelled[0].read_text(), Request(
            Command.CONFIGURE, "suite_config_0", Mode.LEFT_ONLY, 30).to_toml())
        self.runner.compare.assert_not_called()

    def test_pending_cancellation_preserves_unowned_requests(self):
        owned = Request(Command.CONFIGURE, "owned", Mode.LEFT_ONLY, 30)
        pending = self.runner.config_dir / "test_request.toml"
        self.runner.pending_request = owned
        for contents in (Request(Command.END, "other").to_toml(), owned.to_toml() + "\n"):
            pending.write_text(contents)
            self.assertFalse(self.runner.cancel_pending_request())
            self.assertEqual(pending.read_text(), contents)
        pending.unlink()
        self.assertFalse(self.runner.cancel_pending_request())
        self.assertIsNone(self.runner.pending_request)
        self.assertEqual(list(self.runner.config_dir.glob("*.cancelled")), [])

    def test_pending_cancellation_preserves_a_replacement_during_rename(self):
        owned = Request(Command.CONFIGURE, "owned", Mode.LEFT_ONLY, 30)
        other = Request(Command.END, "other").to_toml()
        pending = self.runner.config_dir / "test_request.toml"
        pending.write_text(owned.to_toml())
        self.runner.pending_request = owned
        rename = Path.rename
        def replace_during_rename(source, destination):
            source.write_text(other)
            return rename(source, destination)
        with patch.object(Path, "rename", replace_during_rename), self.assertRaisesRegex(HarnessError, "changed during cancellation"):
            self.runner.cancel_pending_request()
        self.assertEqual(pending.read_text(), other)

    def test_capture_failure_restores_before_propagating(self):
        commands = self.fake_run(HarnessError("capture failed"))
        deadline = self.runner.deadline
        with self.assertRaisesRegex(HarnessError, "capture failed"):
            run_suite(self.runner)
        self.assertEqual(commands[-1].command, Command.END)
        self.assertEqual(self.runner.deadline, deadline)
        self.runner.compare.assert_not_called()

    def test_configuration_failure_restores(self):
        self.fake_run()
        self.runner.request.side_effect = [ProtocolError("configuration failed"), ProtocolError("restore failed")]
        with self.assertLogs("visual_harness", level="ERROR"), self.assertRaises(ProtocolError):
            run_suite(self.runner)
        self.assertEqual(self.runner.request.call_args.args[0].command, Command.END)

    def test_restore_failure_prevents_success_and_comparison(self):
        self.fake_run()
        original = self.runner.request.side_effect
        def request(value):
            if value.command == Command.END:
                raise HarnessError("restore failed")
            return original(value)
        self.runner.request.side_effect = request
        with self.assertRaisesRegex(HarnessError, "restore failed"):
            run_suite(self.runner)
        self.assertEqual(self.runner.result.restoration_error, "restore failed")
        self.runner.compare.assert_not_called()

    def test_repeated_frame_rejected(self):
        self.fake_run()
        self.runner.screenshot.side_effect = None
        self.runner.screenshot.return_value = Capture.from_wire(capture_wire())
        with self.assertRaisesRegex(SuiteError, "increasing"):
            run_suite(self.runner)
        self.assertEqual(self.runner.request.call_args.args[0].command, Command.END)

    def test_controlled_profile_versions_are_accepted_and_mixing_is_rejected(self):
        for profile in (DiagnosticProfile.CONTROLLED_V1, DiagnosticProfile.CONTROLLED_V2):
            with self.subTest(profile=profile):
                wire = capture_wire()
                wire["visual"]["profile"] = profile.value
                wire["comparison_inputs"]["profile"] = profile.value
                ack = ConfigurationAck.from_wire({"id": "configure", "state": "ready", "metadata": wire["visual"]})
                self.assertIs(ack.metadata.profile, profile)
                capture = Capture.from_wire(wire)
                self.assertIs(capture.comparison_inputs.profile, profile)
                capture.validate("sample", Mode.MULTIPASS, 1, (2, 1))
                other = (DiagnosticProfile.CONTROLLED_V2 if profile == DiagnosticProfile.CONTROLLED_V1
                         else DiagnosticProfile.CONTROLLED_V1)
                wire["comparison_inputs"]["profile"] = other.value
                with self.assertRaisesRegex(ProtocolError, "profile"):
                    Capture.from_wire(wire).validate("sample", Mode.MULTIPASS, 1, (2, 1))

    def test_capture_single_and_pair_validation(self):
        for mode in Mode:
            with self.subTest(mode=mode):
                wire = capture_wire(mode)
                (self.root / "sample.capture.json").write_text(json.dumps(wire))
                (self.root / "sample.capture.png").write_bytes(png(wire["width"], 1))
                capture = validate_capture(self.root, "sample", Capture.from_wire(wire), mode=mode, generation=1)
                self.assertIsInstance(capture, Capture)
                self.assertEqual(capture.selected_eye, mode.selected_eye)
                with self.assertRaisesRegex(ProtocolError, "generation"):
                    validate_capture(self.root, "sample", Capture.from_wire(wire), mode=mode, generation=2)

    def test_single_eye_cannot_satisfy_native_smoke(self):
        wire = capture_wire(Mode.LEFT_ONLY)
        (self.root / "sample.capture.json").write_text(json.dumps(wire))
        with self.assertRaisesRegex(HarnessError, "fresh gameplay XR pair"):
            validate_capture(self.root, "sample", Capture.from_wire(wire))

    def test_malformed_capture_inputs_and_eye_state_rejected(self):
        for key, value in (("selected_eye", True), ("rendered_eyes", [1, 1]), ("frame_ordinal", True)):
            wire = capture_wire()
            wire[key] = value
            with self.subTest(key=key), self.assertRaises(ProtocolError):
                Capture.from_wire(wire)
        wire = capture_wire()
        wire["comparison_inputs"]["camera"]["position"][0] = float("nan")
        with self.assertRaisesRegex(ProtocolError, "numeric vector"):
            Capture.from_wire(wire)

    def test_ack_wait_request_wire_and_states(self):
        seen = []
        def publish(_):
            path = self.runner.config_dir / "test_request.toml"
            if not path.exists():
                return
            value = tomllib.loads(path.read_text())
            path.unlink()
            seen.append(value)
            folder = self.runner.config_dir / "visual"
            folder.mkdir(exist_ok=True)
            state = "restored" if value["command"] == "visual_end" else "ready"
            metadata = capture_wire()["visual"]
            if state == "restored":
                metadata.update(mode=None, profile="native", ready=True)
            else:
                metadata["mode"] = value["mode"]
            (folder / f"{value['id']}.json").write_text(json.dumps({
                "id": value["id"], "state": state, "metadata": metadata}))
        with patch.object(self.runner, "pump_menu"), patch("tools.visual_harness.time.sleep", side_effect=publish):
            self.runner.request(Request(Command.CONFIGURE, "configure", Mode.LEFT_ONLY, 30))
            self.runner.request(Request(Command.END, "end"))
        self.assertEqual(seen[0], {"command": "visual_configure", "id": "configure",
                                  "mode": "left_only", "warmup_frames": 30})

    def test_stale_ack_and_outstanding_request_rejected(self):
        folder = self.runner.config_dir / "visual"
        folder.mkdir()
        (folder / "stale.json").write_text("{}")
        with self.assertRaisesRegex(HarnessError, "stale"):
            self.runner.request(Request(Command.END, "stale"))
        (self.runner.config_dir / "test_request.toml").write_text("outstanding")
        with self.assertRaisesRegex(HarnessError, "outstanding"):
            self.runner.request(Request(Command.END, "new"))

    def test_cli_smoke_default_and_suite_deadline(self):
        for arguments, suite, timeout in (([], False, 180), (["--smoke"], False, 180),
                                           (["--suite"], True, 600), (["--suite", "--timeout", "42"], True, 42)):
            with self.subTest(arguments=arguments), patch.object(VisualHarness, "run", autospec=True) as run, \
                    patch.object(VisualHarness, "close"), patch("tools.visual_harness.signal.signal"):
                def success(runner, suite=suite, timeout=timeout):
                    self.assertEqual(runner.options.suite, suite)
                    self.assertEqual(runner.options.timeout, timeout)
                    runner.result.complete(SmokeResult(True))
                run.side_effect = success
                self.assertEqual(main(arguments), 0)

    def test_suite_options_rejected_without_host_checks(self):
        for name, value in (("samples", 2), ("samples", True), ("warmup_frames", 0), ("warmup_frames", 301)):
            setattr(self.runner.options, name, value)
            with self.subTest(name=name), patch.object(self.runner, "reject_existing") as reject:
                with self.assertRaises(HarnessError):
                    self.runner.check()
                reject.assert_not_called()
            self.runner.options = Options(suite=True)

    def test_request_timeout_and_wrong_id_are_bounded(self):
        with patch.object(self.runner, "pump_menu"), \
                patch("tools.visual_harness.time.monotonic", side_effect=[0, 36]), \
                patch.object(self.runner, "assert_running"), \
                self.assertRaisesRegex(HarnessError, "timed out waiting"):
            self.runner.request(Request(Command.END, "timeout"))
        (self.runner.config_dir / "test_request.toml").unlink()
        def publish(_):
            path = self.runner.config_dir / "test_request.toml"
            if path.exists():
                path.unlink()
                folder = self.runner.config_dir / "visual"
                folder.mkdir(exist_ok=True)
                (folder / "wrong.json").write_text('{"id":"other","state":"restored"}')
        with patch.object(self.runner, "pump_menu"), patch("tools.visual_harness.time.sleep", side_effect=publish), \
                self.assertRaisesRegex(HarnessError, "ID mismatch"):
            self.runner.request(Request(Command.END, "wrong"))

    def test_wrong_single_eye_dimensions_and_stale_eye_fail(self):
        for change, message in (({"width": 2}, "dimensions"), ({"fresh_eyes": [False, False]}, "stale"),
                                ({"rendered_eyes": [True, True]}, "selection")):
            wire = {**capture_wire(Mode.LEFT_ONLY), **change}
            capture = Capture.from_wire(wire)
            with self.subTest(change=change), self.assertRaisesRegex(ProtocolError, message):
                capture.validate("sample", Mode.LEFT_ONLY, 1, (wire["width"], 1))

    def test_secondary_restoration_failure_preserves_capture_error(self):
        self.fake_run(HarnessError("capture failed"))
        original = self.runner.request.side_effect
        def request(value):
            if value.command == Command.END:
                raise HarnessError("restore failed")
            return original(value)
        self.runner.request.side_effect = request
        with self.assertLogs("visual_harness", level="ERROR"), self.assertRaisesRegex(HarnessError, "capture failed") as raised:
            run_suite(self.runner)
        self.assertIn("restoration failed", raised.exception.__notes__[0])

    def test_request_variants_enforce_protocol_fields(self):
        for command, request_id, mode, frames in (
                (Command.CONFIGURE, "valid", None, 30), (Command.CONFIGURE, "valid", Mode.LEFT_ONLY, True),
                (Command.CONFIGURE, "valid", Mode.LEFT_ONLY, 301),
                (Command.END, "valid", Mode.MULTIPASS, 30), (Command.END, "../escape", None, None),
                (Command.END, "x" * 65, None, None)):
            with self.subTest(command=command, request_id=request_id), self.assertRaises(ProtocolError):
                Request(command, request_id, mode, frames)

    def test_comparison_drops_only_inherited_python_environment(self):
        folder = self.root / "comparison"
        folder.mkdir()
        (folder / "report.json").write_text('{"status":"pass"}')
        inherited = {"PATH": "/bin", "PYTHONPATH": "/nix/python3.13/site-packages",
                     "PYTHONHOME": "/nix/python3.13", "DISPLAY": ":42"}
        for configured in (None, inherited.copy()):
            with self.subTest(configured=configured is not None), \
                    patch.dict("os.environ", inherited, clear=True), \
                    patch.object(self.runner, "env", configured), \
                    patch.object(self.runner, "_command", return_value=subprocess.CompletedProcess([], 0)) as command:
                self.runner.compare(self.root / "suite.json")
                self.assertEqual(command.call_args.kwargs["env"], {"PATH": "/bin", "DISPLAY": ":42"})
                self.assertEqual(dict(os.environ), inherited)
                if configured is not None:
                    self.assertEqual(configured, inherited)

    def test_comparison_invocation_and_nonpass_report(self):
        folder = self.root / "comparison"
        folder.mkdir()
        report = folder / "report.json"
        report.write_text('{"status":"pass"}')
        with patch.object(self.runner, "_command", return_value=subprocess.CompletedProcess([], 0)) as command:
            self.assertEqual(self.runner.compare(self.root / "suite.json"), ComparisonReport(Outcome.PASS))
        self.assertEqual(command.call_args.args[0][:2], ["uv", "run"])
        self.assertEqual(command.call_args.args[0][-2:], ["--output", str(folder)])
        for outcome, exit_code in ((Outcome.FAIL, 1), (Outcome.INCONCLUSIVE, 2)):
            report.write_text(json.dumps({"status": outcome.value}))
            with self.subTest(outcome=outcome), \
                    patch.object(self.runner, "_command", return_value=subprocess.CompletedProcess([], exit_code)), \
                    self.assertLogs("visual_harness", level="WARNING"):
                self.assertEqual(self.runner.compare(self.root / "suite.json"), ComparisonReport(outcome))
        for outcome, exit_code in ((Outcome.PASS, 1), (Outcome.FAIL, 0), (Outcome.INCONCLUSIVE, 137)):
            report.write_text(json.dumps({"status": outcome.value}))
            with self.subTest(outcome=outcome, exit_code=exit_code), \
                    patch.object(self.runner, "_command", return_value=subprocess.CompletedProcess([], exit_code)), \
                    self.assertRaisesRegex(HarnessError, "disagrees with process status"):
                self.runner.compare(self.root / "suite.json")


if __name__ == "__main__":
    unittest.main()
