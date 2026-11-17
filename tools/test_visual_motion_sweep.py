import json
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

if __package__:
    from tools.visual_harness_cli import parse_args
    from tools.visual_motion import MOTION_PHASES, MOTION_SWEEP, run_motion_sweep
    from tools.visual_protocol import (
        Camera,
        Capture,
        Command,
        ComparisonReport,
        ConfigurationAck,
        Mode,
        Outcome,
        PoseEvidence,
        RestorationAck,
        RunReport,
        SmokeResult,
        compose_pose,
        quaternion_normalize,
    )
else:
    from visual_harness_cli import parse_args
    from visual_motion import MOTION_PHASES, MOTION_SWEEP, run_motion_sweep
    from visual_protocol import (
        Camera,
        Capture,
        Command,
        ComparisonReport,
        ConfigurationAck,
        Mode,
        Outcome,
        PoseEvidence,
        RestorationAck,
        RunReport,
        SmokeResult,
        compose_pose,
        quaternion_normalize,
    )

ANCHOR = Camera((1.5, -2.0, 0.25), quaternion_normalize((0.1, 0.2, 0.3, 0.9)))


def camera_wire(camera):
    return {"position": list(camera.position), "orientation": list(camera.orientation)}


def pose_wire(evidence):
    return {"case_id": evidence.case_id, "offset": evidence.offset.to_wire(),
            "anchor": camera_wire(evidence.anchor), "applied": camera_wire(evidence.applied)}


def motion_wire(request, phase, cycle, predicted, anchor=ANCHOR):
    return {
        "request": request.to_wire(), "phase": phase, "cycle": cycle,
        "rendered_frame": predicted,
        "history": [
            {"phase": (phase - 8 + index) % request.period_frames,
             "rendered_frame": predicted - 8 + index,
             "applied": camera_wire(compose_pose(anchor, request.offset_at(
                 (phase - 8 + index) % request.period_frames)))}
            for index in range(8)
        ],
    }


def capture_wire(mode, ordinal, generation, request, phase, cycle, anchor=ANCHOR):
    predicted = 1000 + ordinal
    offset = request.offset_at(phase)
    pose = PoseEvidence(request.case_id, offset, anchor, compose_pose(anchor, offset))
    selected = mode.selected_eye
    return {
        "id": f"motion_{mode.value}_{phase}_{cycle}", "frame_ordinal": ordinal,
        "predicted_display_time_ns": predicted,
        "source": "isolated_eye" if selected is not None else "eye_textures",
        "width": 1 if selected is not None else 2, "height": 1, "eye_width": 1, "eye_height": 1,
        "fresh_eyes": list(mode.rendered_eyes), "rendered_eyes": list(mode.rendered_eyes),
        "selected_eye": selected, "gameplay_ready": True, "xr_session_running": True,
        "mode": mode.value,
        "visual": {
            "generation": generation, "mode": mode.value, "profile": "controlled_v1",
            "ready": True, "paused": True, "frozen": True, "error": None,
            "warmup_frames": 30, "completed_frames": 30,
            "pose": pose_wire(pose), "motion": motion_wire(request, phase, cycle, predicted, anchor),
        },
        "comparison_inputs": {
            "build_stamp": "fixture", "profile": "controlled_v1", "eye_width": 1, "eye_height": 1,
            "world_scale": 1.0, "camera": camera_wire(pose.applied),
            "eyes": [{"offset": [0, 0, 0], "orientation_delta": [0, 0, 0, 1],
                      "projection_standard": [0] * 16, "projection_reverse_z": [0] * 16}
                     for _ in range(2)],
        },
    }


def visual_wire(mode, generation, warmup):
    return {"generation": generation, "mode": mode.value, "profile": "controlled_v1",
            "ready": True, "paused": True, "frozen": True, "error": None,
            "warmup_frames": warmup, "completed_frames": warmup}


class FakeRunner:
    def __init__(self, root, reverse=False):
        self.options = SimpleNamespace(samples=3, warmup_frames=30, reverse_multipass=reverse)
        self.output = root
        self.config_dir = root / "config"
        self.config_dir.mkdir(exist_ok=True)
        self.deadline = time.monotonic() + 30
        self.result = RunReport()
        self.commands = []
        self.calls = []
        self.cycles = {}
        self.frames = 0
        self.generation = 0
        self.compared = False

    def smoke(self):
        return SmokeResult(True)

    def settle(self, _seconds):
        return None

    def cancel_pending_request(self):
        return False

    def request(self, request):
        self.commands.append(request)
        if request.command == Command.END:
            return RestorationAck.from_wire({"id": request.id, "state": "restored", "metadata": {
                "generation": self.generation + 1, "mode": None, "profile": "native",
                "ready": True, "paused": False, "frozen": False, "error": None,
                "warmup_frames": 0, "completed_frames": 0, "engine_dimensions": [2, 1]}})
        self.generation += 1
        pose = PoseEvidence(request.motion.case_id, request.motion.offset_at(0), ANCHOR,
                            compose_pose(ANCHOR, request.motion.offset_at(0)))
        metadata = {**visual_wire(request.mode, self.generation, request.warmup_frames),
                    "pose": pose_wire(pose),
                    "motion": {"request": request.motion.to_wire(), "phase": 0, "cycle": 0,
                               "rendered_frame": 0, "history": []}}
        return ConfigurationAck.from_wire({"id": request.id, "state": "ready", "metadata": metadata})

    def screenshot(self, capture_id, mode=None, generation=None, motion_capture=None):
        self.calls.append((capture_id, mode, generation, motion_capture))
        self.frames += 1
        key = (mode, motion_capture.phase)
        self.cycles[key] = self.cycles.get(key, 0) + 1
        wire = capture_wire(mode, self.frames, generation, MOTION_SWEEP,
                            motion_capture.phase, self.cycles[key])
        wire["id"] = capture_id
        return Capture.from_wire(wire)

    def compare(self, path):
        self.path = path
        self.compared = True
        return ComparisonReport(Outcome.PASS)


class MotionSweepTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.runner = FakeRunner(self.root)

    def test_modes_configure_once_and_repeat_phases_in_order(self):
        result = run_motion_sweep(self.runner)
        self.assertTrue(result.suite_succeeded)
        configure = [request for request in self.runner.commands if request.command == Command.CONFIGURE]
        self.assertEqual([request.mode for request in configure], [
            Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS, Mode.MULTIPASS])
        self.assertTrue(all(request.motion == MOTION_SWEEP and request.pose is None for request in configure))
        self.assertEqual(self.runner.commands[-1].command, Command.END)
        self.assertEqual(len(self.runner.calls), 5 * 3 * len(MOTION_PHASES))
        for index in range(5):
            block = self.runner.calls[index * 24:(index + 1) * 24]
            self.assertEqual([capture.phase for _, _, _, capture in block],
                             list(MOTION_PHASES) * 3)
        ids = [capture_id for capture_id, _, _, _ in self.runner.calls]
        self.assertEqual(len(ids), len(set(ids)))
        for capture_id in ids:
            self.assertTrue(capture_id.isascii())
            self.assertRegex(capture_id, r"^[A-Za-z0-9_-]+$")
            self.assertLessEqual(len(capture_id), 64)
        manifest = json.loads((self.root / "motion_sweep.json").read_text())
        self.assertEqual([case["name"] for case in manifest["cases"]],
                         [f"phase_{phase:03d}" for phase in MOTION_PHASES])
        for case, phase in zip(manifest["cases"], MOTION_PHASES):
            self.assertEqual(case["expected_motion"], {
                "request": MOTION_SWEEP.to_wire(), "phase": phase})
            self.assertNotIn("expected_pose", case)
            self.assertEqual({mode: len(samples) for mode, samples in case["modes"].items()}, {
                "left_only": 3, "right_only": 3, "multipass": 6, "single_pass": 3})
        self.assertEqual(self.runner.path, self.root / "motion_sweep.json")
        self.assertEqual(result.comparison, ComparisonReport(Outcome.PASS))

    def test_reverse_multipass_precedes_return(self):
        self.runner = FakeRunner(self.root, reverse=True)
        run_motion_sweep(self.runner)
        configure = [request for request in self.runner.commands if request.command == Command.CONFIGURE]
        self.assertEqual([request.mode for request in configure], [
            Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS,
            Mode.REVERSE_MULTIPASS, Mode.MULTIPASS])

    def test_anchor_drift_fails_before_comparison(self):
        original = self.runner.screenshot
        def screenshot(*args, **kwargs):
            capture = original(*args, **kwargs)
            if self.runner.frames == 1:
                return capture
            wire = capture_wire(capture.mode, self.runner.frames, capture.visual.generation,
                                MOTION_SWEEP, capture.visual.motion.phase,
                                capture.visual.motion.cycle, Camera((9.0, 9.0, 9.0), ANCHOR.orientation))
            wire["id"] = capture.id
            return Capture.from_wire(wire)
        self.runner.screenshot = screenshot
        with self.assertRaisesRegex(Exception, "anchor"):
            run_motion_sweep(self.runner)
        self.assertFalse(self.runner.compared)

    def test_cli_motion_sweep_is_exclusive_with_motion_timeout(self):
        options = parse_args(["--motion-sweep"])
        self.assertTrue(options.motion_sweep)
        self.assertGreaterEqual(options.timeout, 1200)
        with self.assertRaises(SystemExit), patch("sys.stderr"):
            parse_args(["--suite", "--motion-sweep"])


if __name__ == "__main__":
    unittest.main()
