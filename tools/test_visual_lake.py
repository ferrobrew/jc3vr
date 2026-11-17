import json
import tempfile
import time
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np

from tools.visual_lake import (
    DEFAULT_CONFIG,
    CaptureEvidence,
    Gate,
    LakeConfig,
    LakeError,
    LakeOutcome,
    Mode,
    RunnerState,
    _multipass_gate,
    classify,
    offset_for_target,
    owning_process,
    run_experiment,
    sample_id,
)
from tools.visual_protocol import (
    Camera,
    Capture,
    Command,
    ConfigurationAck,
    EnvironmentObservation,
    EnvironmentRequest,
    ProtocolError,
    Request,
    RestorationAck,
    WeatherControl,
    WeatherEvent,
    compose_pose,
    quaternion_normalize,
)
from tools.visual_protocol import (
    Mode as ProtocolMode,
)

ANCHOR = Camera((8257.701, 1486.9717, -4286.1167),
                (0.00050330814, 0.80293703, 0.020165263, 0.59572256))
TARGET = Camera((7586.867, 1514.6827, -4081.8762),
                (-0.016829107, 0.97209185, 0.23272279, 0.024376998))
HOUR = 13.226033


def pose_evidence(case_id, anchor, offset, translation):
    applied = compose_pose(anchor, offset, translation)
    return {
        "case_id": case_id,
        "offset": offset.to_wire(),
        "translation": translation.to_wire(),
        "anchor": {"position": list(anchor.position),
                   "orientation": list(anchor.orientation)},
        "applied": {"position": list(applied.position),
                    "orientation": list(applied.orientation)},
    }


def observation(control=WeatherControl.SUNNY, hour=HOUR, severity=0.1):
    return {"hour": hour, "day_cycle_speed": 0.0, "weather_severity": severity,
            "weather_control": control.value}


def configuration_wire(request_id, mode, generation, evidence, environment):
    metadata = {
        "generation": generation,
        "mode": mode.value,
        "profile": "controlled_v2",
        "ready": True,
        "paused": True,
        "frozen": True,
        "error": None,
        "warmup_frames": 30,
        "completed_frames": 30,
        "engine_dimensions": [320, 360],
        "pose": evidence,
        "environment": environment,
    }
    return {"id": request_id, "state": "ready", "metadata": metadata}


def restoration_wire(request_id, generation, environment):
    metadata = {
        "generation": generation,
        "mode": None,
        "profile": "native",
        "ready": True,
        "paused": False,
        "frozen": False,
        "error": None,
        "warmup_frames": 0,
        "completed_frames": 0,
        "engine_dimensions": [320, 360],
        "environment": environment,
    }
    return {"id": request_id, "state": "restored", "metadata": metadata}


def eye_wire():
    return {"offset": [0.0, 0.0, 0.0], "orientation_delta": [0.0, 0.0, 0.0, 1.0],
            "projection_standard": [0.0] * 16, "projection_reverse_z": [0.0] * 16}


def capture_wire(identifier, mode, frame, evidence, environment):
    return {
        "id": identifier,
        "frame_ordinal": frame,
        "predicted_display_time_ns": frame * 1000,
        "source": "eye_textures",
        "width": 640,
        "height": 360,
        "eye_width": 320,
        "eye_height": 360,
        "fresh_eyes": [True, True],
        "gameplay_ready": True,
        "xr_session_running": True,
        "mode": mode.value,
        "selected_eye": None,
        "rendered_eyes": [True, True],
        "visual": configuration_wire(identifier, mode, 1, evidence, environment)["metadata"],
        "comparison_inputs": {
            "build_stamp": "test",
            "profile": "controlled_v2",
            "camera": {"position": list(TARGET.position),
                       "orientation": list(TARGET.orientation)},
            "eyes": [eye_wire(), eye_wire()],
            "eye_width": 320,
            "eye_height": 360,
            "world_scale": 1.0,
        },
    }


class FakeRunner:
    def __init__(self, config_dir, end_environment="ambient", fail_end=False):
        self.config_dir = Path(config_dir)
        self.deadline = time.monotonic() + 60
        self.pending_request = None
        self.generation = 0
        self.anchor = None
        self.commands = []
        self.end_environment = end_environment
        self.fail_end = fail_end

    def request(self, value):
        self.commands.append(value.command)
        if value.command == Command.CONFIGURE:
            if self.anchor is None:
                self.anchor = ANCHOR
            evidence = pose_evidence(value.pose.case_id, self.anchor, value.pose.offset,
                                     value.pose.translation)
            self.generation += 1
            return ConfigurationAck.from_wire(configuration_wire(
                value.id, value.mode, self.generation, evidence, observation()))
        if value.command == Command.END:
            if self.fail_end:
                environment = observation(WeatherControl.SUNNY)
            elif self.end_environment == "missing":
                environment = None
            else:
                environment = observation(WeatherControl.AMBIENT)
            return RestorationAck.from_wire(restoration_wire(
                value.id, self.generation + 1, environment))
        raise AssertionError(f"unexpected request {value.command}")

    def cancel_pending_request(self):
        return False


class ProtocolTests(unittest.TestCase):
    def test_environment_hour_is_exclusive(self):
        self.assertEqual(EnvironmentRequest(23.5, WeatherEvent.SUNNY).hour, 23.5)
        with self.assertRaises(ProtocolError):
            EnvironmentRequest(24.0, WeatherEvent.SUNNY)
        with self.assertRaises(ProtocolError):
            EnvironmentRequest(-0.1, WeatherEvent.SUNNY)

    def test_environment_observation_tolerates_missing_control(self):
        observation = EnvironmentObservation.from_wire(
            {"hour": HOUR, "day_cycle_speed": 0.0, "weather_severity": 0.1})
        self.assertIsNone(observation.weather_control)

    def test_environment_is_configuration_only(self):
        request = Request(Command.CONFIGURE, "lake_env", ProtocolMode.SINGLE_PASS, 30,
                          environment=EnvironmentRequest(HOUR, WeatherEvent.SUNNY))
        self.assertEqual(tomllib.loads(request.to_toml())["environment"],
                         {"hour": HOUR, "weather": "sunny"})
        with self.assertRaises(ProtocolError):
            Request(Command.SCREENSHOT, "lake_env",
                    environment=EnvironmentRequest(HOUR, WeatherEvent.SUNNY))

    def test_composition_reaches_reference_target(self):
        offset, translation = offset_for_target(ANCHOR, TARGET)
        applied = compose_pose(ANCHOR, offset, translation)
        self.assertAlmostEqual(offset.yaw_deg, 70.0, places=3)
        self.assertAlmostEqual(offset.pitch_deg, -25.0, places=3)
        self.assertAlmostEqual(offset.roll_deg, 0.0, places=3)
        self.assertAlmostEqual(translation.up, 50.0, places=3)
        self.assertAlmostEqual(translation.forward, 700.0, places=3)
        self.assertLess(max(abs(a - b) for a, b in zip(applied.position, TARGET.position)), 1e-4)
        self.assertLess(max(abs(a - b) for a, b in
                            zip(quaternion_normalize(applied.orientation),
                                quaternion_normalize(TARGET.orientation))), 1e-6)

    def test_checked_config_loads_reference_values(self):
        config = LakeConfig.load(DEFAULT_CONFIG)
        self.assertEqual(config.name, "lake-glint")
        self.assertEqual(config.samples, 3)
        self.assertEqual(config.pose.position, TARGET.position)
        self.assertEqual(config.environment.hour, HOUR)
        self.assertEqual(config.environment.weather, WeatherEvent.SUNNY)
        self.assertGreater(config.gate.min_fraction, 0.0)


class GateTests(unittest.TestCase):
    def test_bright_fraction_selects_the_configured_roi(self):
        gate = Gate((0.0, 0.0, 1.0, 1.0), 4.0, 0.01, 0.05, 0.5)
        bright = np.full((10, 10, 3), 10.0)
        dim = np.full((10, 10, 3), 0.1)
        self.assertEqual(gate.bright_fraction(bright), 1.0)
        self.assertEqual(gate.bright_fraction(dim), 0.0)

    def test_bright_fraction_ignores_pixels_outside_the_roi(self):
        gate = Gate((0.0, 0.0, 0.5, 1.0), 4.0, 0.01, 0.05, 0.5)
        values = np.zeros((10, 10, 3))
        values[:, 5:] = 10.0
        self.assertEqual(gate.bright_fraction(values), 0.0)


class ClassifyTests(unittest.TestCase):
    def setUp(self):
        self.config = LakeConfig.load(DEFAULT_CONFIG)

    def item(self, mode, identifier, left, right, control=WeatherControl.SUNNY):
        return CaptureEvidence(mode, identifier, (left, right), HOUR, 0.1, control)

    def test_missing_right_eye_is_reproduced(self):
        evidence = (
            self.item(Mode.MULTIPASS, "m1", 0.05, 0.05),
            self.item(Mode.MULTIPASS, "m2", 0.05, 0.05),
            self.item(Mode.SINGLE_PASS, "s1", 0.05, 0.0),
            self.item(Mode.SINGLE_PASS, "s2", 0.05, 0.0),
        )
        outcome, findings, errors = classify(self.config, evidence, ())
        self.assertEqual(outcome, LakeOutcome.REPRODUCED)
        self.assertEqual(errors, ())
        self.assertTrue(findings)

    def test_dim_multipass_reference_is_inconclusive(self):
        evidence = (
            self.item(Mode.MULTIPASS, "m1", 0.0, 0.0),
            self.item(Mode.SINGLE_PASS, "s1", 0.0, 0.0),
        )
        outcome, findings, _ = classify(self.config, evidence, ())
        self.assertEqual(outcome, LakeOutcome.INCONCLUSIVE)
        self.assertTrue(any("multipass eye" in finding for finding in findings))

    def test_bright_right_eye_is_not_reproduced(self):
        evidence = (
            self.item(Mode.MULTIPASS, "m1", 0.05, 0.05),
            self.item(Mode.SINGLE_PASS, "s1", 0.05, 0.05),
        )
        outcome, _, _ = classify(self.config, evidence, ())
        self.assertEqual(outcome, LakeOutcome.NOT_REPRODUCED)

    def test_mixed_right_eye_is_inconclusive(self):
        evidence = (
            self.item(Mode.MULTIPASS, "m1", 0.05, 0.05),
            self.item(Mode.SINGLE_PASS, "s1", 0.05, 0.0),
            self.item(Mode.SINGLE_PASS, "s2", 0.05, 0.05),
            self.item(Mode.SINGLE_PASS, "s3", 0.05, 0.0),
        )
        outcome, findings, errors = classify(self.config, evidence, ())
        self.assertEqual(outcome, LakeOutcome.INCONCLUSIVE)
        self.assertIn("right-eye reflection is unstable across samples", findings)
        self.assertEqual(errors, ())

    def test_unobserved_environment_is_inconclusive(self):
        evidence = (
            self.item(Mode.MULTIPASS, "m1", 0.05, 0.05),
            self.item(Mode.SINGLE_PASS, "s1", 0.05, 0.0),
        )
        outcome, _, errors = classify(self.config, evidence, ("m1: environment observation is unavailable",))
        self.assertEqual(outcome, LakeOutcome.INCONCLUSIVE)
        self.assertTrue(errors)


class MultipassGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = LakeConfig.load(DEFAULT_CONFIG)
        offset, translation = offset_for_target(ANCHOR, TARGET)
        evidence = pose_evidence("lake_target", ANCHOR, offset, translation)
        self.capture = Capture.from_wire(capture_wire("m1", Mode.MULTIPASS, 1, evidence,
                                                      observation()))

    def test_dim_multipass_eye_is_rejected(self):
        with patch("tools.visual_lake._bright_fractions", return_value=(0.05, 0.0)):
            errors, evidence = _multipass_gate(self.config, self.root, [self.capture])
        self.assertTrue(any("eye 1" in error for error in errors))
        self.assertEqual(len(evidence), 1)

    def test_bright_multipass_eyes_pass(self):
        with patch("tools.visual_lake._bright_fractions", return_value=(0.05, 0.05)):
            errors, _ = _multipass_gate(self.config, self.root, [self.capture])
        self.assertEqual(errors, ())


class EndTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = LakeConfig.load(DEFAULT_CONFIG)

    def capture_failure(self, *_):
        raise LakeError("capture failed")

    def test_capture_failure_still_ends_and_preserves_the_failure(self):
        runner = FakeRunner(self.root / "config")
        (self.root / "config" / "captures").mkdir(parents=True)
        with self.assertRaises(LakeError) as raised:
            run_experiment(runner, self.config, self.root / "out", capture=self.capture_failure,
                           analyze=lambda *_: (LakeOutcome.INCONCLUSIVE, (), (), ()))
        self.assertEqual(str(raised.exception), "capture failed")
        self.assertEqual(runner.commands[-1], Command.END)
        self.assertEqual(runner.commands.count(Command.END), 1)
        report = json.loads((self.root / "out" / "report.json").read_text())
        self.assertEqual(report["outcome"], "inconclusive")
        self.assertEqual(report["errors"], ["capture failed"])

    def test_dim_multipass_aborts_before_single_pass(self):
        runner = FakeRunner(self.root / "config")
        (self.root / "config" / "captures").mkdir(parents=True)
        offset, translation = offset_for_target(ANCHOR, TARGET)
        evidence = pose_evidence("lake_target", ANCHOR, offset, translation)
        frames = iter(range(1, 10))

        def capture(*arguments):
            identifier = arguments[-1]
            frame = next(frames)
            return Capture.from_wire(capture_wire(identifier, Mode.MULTIPASS, frame, evidence,
                                                  observation()))

        def analyze(*_):
            raise AssertionError("analysis must not run after the multipass gate")

        with patch("tools.visual_lake._multipass_gate", return_value=(("m1: dim",), ())):
            report = run_experiment(runner, self.config, self.root / "out", capture=capture,
                                    analyze=analyze)
        self.assertEqual(report.outcome, LakeOutcome.INCONCLUSIVE)
        self.assertEqual(report.errors, ("m1: dim",))
        self.assertEqual(runner.commands[-1], Command.END)
        self.assertEqual(runner.commands.count(Command.CONFIGURE), 2)
        self.assertEqual(json.loads((self.root / "out" / "report.json").read_text())["outcome"],
                         "inconclusive")

    def test_end_failure_does_not_mask_the_capture_failure(self):
        runner = FakeRunner(self.root / "config", fail_end=True)
        (self.root / "config" / "captures").mkdir(parents=True)
        with self.assertRaises(LakeError) as raised:
            run_experiment(runner, self.config, self.root / "out", capture=self.capture_failure,
                           analyze=lambda *_: (LakeOutcome.INCONCLUSIVE, (), (), ()))
        self.assertEqual(str(raised.exception), "capture failed")
        self.assertTrue(any("restoration failed" in note for note in raised.exception.__notes__))
        report = json.loads((self.root / "out" / "report.json").read_text())
        self.assertIn("not released to ambient", report["restoration_error"])

    def test_missing_restoration_observation_is_a_restoration_failure(self):
        runner = FakeRunner(self.root / "config", end_environment="missing")
        (self.root / "config" / "captures").mkdir(parents=True)
        with self.assertRaises(LakeError):
            run_experiment(runner, self.config, self.root / "out", capture=self.capture_failure,
                           analyze=lambda *_: (LakeOutcome.INCONCLUSIVE, (), (), ()))
        report = json.loads((self.root / "out" / "report.json").read_text())
        self.assertIn("unobserved", report["restoration_error"])

    def test_sample_ids_share_a_pair_stem_and_are_unique(self):
        prefix = "lake_glint_ab12cd34"
        identifiers = [sample_id(prefix, mode, index)
                       for mode in (Mode.SINGLE_PASS, Mode.MULTIPASS) for index in (1, 2, 3)]
        self.assertEqual(len(set(identifiers)), len(identifiers))
        single = sample_id(prefix, Mode.SINGLE_PASS, 1)
        multipass = sample_id(prefix, Mode.MULTIPASS, 1)
        self.assertEqual(single.replace("_single_pass_", "_"), multipass.replace("_multipass_", "_"))


class OwnershipTests(unittest.TestCase):
    def test_only_the_game_process_owns_the_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary)
            config_dir = proc / "config"
            config_dir.mkdir()
            state = RunnerState("abc123", config_dir)
            for pid, comm, argv in ((10, "justcause3.exe", ["Z:\\JustCause3.exe"]),
                                     (11, "wineserver", ["wineserver"]),
                                     (12, "monado-service", ["monado-service"]),
                                     (13, "python3", ["python3", "--spawn", "justcause3.exe"])):
                entry = proc / str(pid)
                entry.mkdir()
                (entry / "comm").write_text(comm + "\n")
                (entry / "cmdline").write_bytes(b"\0".join(item.encode() for item in argv) + b"\0")
                (entry / "environ").write_bytes(
                    f"JC3VR_HARNESS_TOKEN=abc123\0JC3VR_CONFIG_DIR={config_dir}\0".encode())
            self.assertEqual(owning_process(state, proc), 10)


class AttachTests(unittest.TestCase):
    def test_runner_requires_a_token_and_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "runner.json"
            path.write_text(json.dumps({"token": "abc123", "config_dir": temporary}))
            state = RunnerState.load(path)
            self.assertEqual(state.token, "abc123")
            path.write_text(json.dumps({"token": "abc123", "config_dir": "/does/not/exist"}))
            with self.assertRaises(LakeError):
                RunnerState.load(path)


if __name__ == "__main__":
    unittest.main()
