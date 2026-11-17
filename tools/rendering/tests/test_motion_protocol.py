import tomllib
import unittest

from tools.rendering.protocol import (
    Camera,
    Command,
    ConfigurationAck,
    Mode,
    OrientationOffset,
    PoseEvidence,
    PoseRequest,
    ProtocolError,
    Request,
    compose_pose,
    quaternion_normalize,
)
from tools.rendering.protocol.motion import (
    ExpectedMotion,
    MotionEvidence,
    MotionRequest,
    PhaseCapture,
)

ANCHOR = Camera((1.5, -2.0, 0.25), quaternion_normalize((0.1, 0.2, 0.3, 0.9)))
REQUEST = MotionRequest("motion", 120, 20.0, 10.0, 10.0)


def visual_wire():
    return {
        "generation": 1, "mode": "multipass", "profile": "controlled_v1",
        "ready": True, "paused": True, "frozen": True, "error": None,
        "warmup_frames": 30, "completed_frames": 30,
    }


def pose_wire(evidence):
    return {
        "case_id": evidence.case_id,
        "offset": evidence.offset.to_wire(),
        "anchor": {"position": list(evidence.anchor.position),
                   "orientation": list(evidence.anchor.orientation)},
        "applied": {"position": list(evidence.applied.position),
                    "orientation": list(evidence.applied.orientation)},
    }


def history_wire(phase, rendered_frame, anchor=ANCHOR, request=REQUEST):
    camera = compose_pose(anchor, request.offset_at(phase))
    return {"phase": phase, "rendered_frame": rendered_frame,
            "applied": {"position": list(camera.position),
                        "orientation": list(camera.orientation)}}


def evidence_wire(phase=15, cycle=2, rendered_frame=1000, anchor=ANCHOR,
                  request=REQUEST, history_length=8):
    return {
        "request": request.to_wire(), "phase": phase, "cycle": cycle,
        "rendered_frame": rendered_frame,
        "history": [history_wire((phase - history_length + index) % request.period_frames,
                                 900 + index, anchor, request)
                    for index in range(history_length)],
    }


def capture_pose(phase=15, anchor=ANCHOR, request=REQUEST):
    offset = request.offset_at(phase)
    return PoseEvidence(request.case_id, offset, anchor, compose_pose(anchor, offset))


class MotionProtocolTests(unittest.TestCase):
    def test_motion_request_wire_toml_and_trajectory(self):
        self.assertEqual(REQUEST.to_wire(), {
            "case_id": "motion", "period_frames": 120,
            "yaw_deg": 20.0, "pitch_deg": 10.0, "roll_deg": 10.0,
        })
        parsed = tomllib.loads(REQUEST.to_toml())
        self.assertEqual(parsed["motion"], REQUEST.to_wire())
        self.assertEqual(MotionRequest.from_wire(REQUEST.to_wire()), REQUEST)
        offset = REQUEST.offset_at(30)
        self.assertAlmostEqual(offset.yaw_deg, 20.0)
        self.assertAlmostEqual(offset.pitch_deg, 0.0)
        self.assertAlmostEqual(offset.roll_deg, 10.0)
        offset = REQUEST.offset_at(15)
        self.assertAlmostEqual(offset.yaw_deg, 20.0 * 2 ** 0.5 / 2)
        self.assertAlmostEqual(offset.pitch_deg, 10.0)
        for phase in (-1, 120, True, 1.5):
            with self.subTest(phase=phase), self.assertRaises(ProtocolError):
                REQUEST.offset_at(phase)

    def test_motion_request_bounds(self):
        for period in (31, 601, True, 120.0):
            with self.subTest(period=period), self.assertRaises(ProtocolError):
                MotionRequest("motion", period, 20.0, 10.0, 10.0)
        for amplitude in (float("nan"), float("inf"), -1, 46):
            with self.subTest(amplitude=amplitude), self.assertRaises(ProtocolError):
                MotionRequest("motion", 120, amplitude, 10.0, 10.0)
        for case_id in ("bad case", "é", "", "x" * 65):
            with self.subTest(case_id=case_id), self.assertRaises(ProtocolError):
                MotionRequest(case_id, 120, 20.0, 10.0, 10.0)
        self.assertEqual(MotionRequest("motion", 32, 0, 45, 0).yaw_deg, 0.0)
        self.assertEqual(MotionRequest("motion", 600, 0, 45, 0).pitch_deg, 45.0)

    def test_request_tables_and_legacy_wire(self):
        configure = Request(Command.CONFIGURE, "motion_config", Mode.MULTIPASS, 30, motion=REQUEST)
        parsed = tomllib.loads(configure.to_toml())
        self.assertEqual(parsed["motion"], REQUEST.to_wire())
        self.assertEqual(parsed["command"], "visual_configure")
        screenshot = Request(Command.SCREENSHOT, "shot", motion_capture=PhaseCapture(15))
        parsed = tomllib.loads(screenshot.to_toml())
        self.assertEqual(parsed["motion_capture"], {"phase": 15})
        self.assertEqual(PhaseCapture.from_wire({"phase": 15}), PhaseCapture(15))
        for phase in (-1, True, 1.5):
            with self.subTest(phase=phase), self.assertRaises(ProtocolError):
                PhaseCapture(phase)
        with self.assertRaises(ProtocolError):
            PhaseCapture(15).validate(15)
        PhaseCapture(14).validate(15)
        self.assertEqual(tomllib.loads(Request(Command.END, "end").to_toml()),
                         {"command": "visual_end", "id": "end"})
        self.assertEqual(Request(Command.CONFIGURE, "legacy", Mode.LEFT_ONLY, 30).to_wire(),
                         {"command": "visual_configure", "id": "legacy",
                          "mode": "left_only", "warmup_frames": 30})

    def test_request_variants_reject_motion_misuse(self):
        pose = PoseRequest("pose", OrientationOffset(0.0, 0.0, 0.0))
        for request in (
            lambda: Request(Command.CONFIGURE, "both", Mode.MULTIPASS, 30, pose, REQUEST),
            lambda: Request(Command.END, "end", motion=REQUEST),
            lambda: Request(Command.SCREENSHOT, "shot", motion=REQUEST),
            lambda: Request(Command.CONFIGURE, "motion", Mode.MULTIPASS, 30,
                            motion_capture=PhaseCapture(1)),
            lambda: Request(Command.END, "end", motion_capture=PhaseCapture(1)),
        ):
            with self.subTest(request=request), self.assertRaises(ProtocolError):
                request()

    def test_expected_motion_wire(self):
        expected = ExpectedMotion(REQUEST, 15)
        self.assertEqual(expected.to_wire(), {"request": REQUEST.to_wire(), "phase": 15})
        self.assertEqual(ExpectedMotion.from_wire(expected.to_wire()), expected)
        for phase in (-1, 120, True, 1.5):
            with self.subTest(phase=phase), self.assertRaises(ProtocolError):
                ExpectedMotion(REQUEST, phase)

    def test_configuration_ack_requires_matching_motion_and_pose(self):
        pose = capture_pose(phase=0)
        evidence = MotionEvidence.from_wire({
            "request": REQUEST.to_wire(), "phase": 0, "cycle": 0, "rendered_frame": 10,
            "history": [],
        })
        metadata = {**visual_wire(), "motion": evidence.to_wire(), "pose": pose_wire(pose)}
        ack = ConfigurationAck.from_wire({"id": "motion_config", "state": "ready", "metadata": metadata})
        self.assertEqual(ack.validate_motion(REQUEST), ANCHOR)
        with self.assertRaisesRegex(ProtocolError, "differs"):
            ack.validate_motion(MotionRequest("other", 120, 20.0, 10.0, 10.0))
        for change in (
            {"cycle": 1},
            {"history": [history_wire(119, 9)]},
            {"phase": 1},
        ):
            with self.subTest(change=change):
                wire = {**evidence.to_wire(), **change}
                changed = ConfigurationAck.from_wire({"id": "motion_config", "state": "ready", "metadata": {
                    **visual_wire(), "motion": wire, "pose": pose_wire(pose)}})
                with self.assertRaises(ProtocolError):
                    changed.validate_motion(REQUEST)
        missing = ConfigurationAck.from_wire({"id": "motion_config", "state": "ready", "metadata": {
            **visual_wire(), "pose": pose_wire(pose)}})
        with self.assertRaisesRegex(ProtocolError, "missing requested motion"):
            missing.validate_motion(REQUEST)
        unposed = ConfigurationAck.from_wire({"id": "motion_config", "state": "ready", "metadata": {
            **visual_wire(), "motion": evidence.to_wire()}})
        with self.assertRaisesRegex(ProtocolError, "no pose"):
            unposed.validate_motion(REQUEST)

    def test_capture_evidence_accepts_history_and_quaternion_sign(self):
        evidence = MotionEvidence.from_wire(evidence_wire())
        pose = capture_pose()
        camera = pose.applied
        self.assertEqual(evidence.validate_capture(REQUEST, 15, ANCHOR, pose, camera, 1000), ANCHOR)
        negated = Camera(camera.position, tuple(-item for item in camera.orientation))
        self.assertEqual(evidence.validate_capture(REQUEST, 15, ANCHOR, pose, negated, 1000), ANCHOR)
        history = list(evidence_wire()["history"])
        history[-1]["applied"]["orientation"] = [
            -item for item in history[-1]["applied"]["orientation"]]
        signed = MotionEvidence.from_wire({**evidence_wire(), "history": history})
        self.assertEqual(signed.validate_capture(REQUEST, 15, ANCHOR, pose, camera, 1000), ANCHOR)

    def test_capture_evidence_rejects_bad_history_phase_time_anchor_and_camera(self):
        pose = capture_pose()
        camera = pose.applied
        cases = (
            ("missing", {**evidence_wire(), "history": []}, "eight frames"),
            ("short", evidence_wire(history_length=7), "eight frames"),
            ("gap", {**evidence_wire(), "history": [
                {**entry, "phase": (entry["phase"] + 1) % 120}
                for entry in evidence_wire()["history"]]}, "contiguous"),
            ("stale", {**evidence_wire(), "history": [
                {**entry, "rendered_frame": 900 - index}
                for index, entry in enumerate(evidence_wire()["history"])]}, "strictly increasing"),
            ("future", {**evidence_wire(), "history": [
                {**entry, "rendered_frame": 1000 + index}
                for index, entry in enumerate(evidence_wire()["history"])]}, "before the capture"),
            ("geometry", {**evidence_wire(), "history": [
                {**entry, "applied": {"position": [9.0, 9.0, 9.0],
                                      "orientation": [0.0, 0.0, 0.0, 1.0]}}
                for entry in evidence_wire()["history"]]}, "trajectory and anchor"),
        )
        for name, wire, message in cases:
            with self.subTest(name=name):
                evidence = MotionEvidence.from_wire(wire)
                with self.assertRaisesRegex(ProtocolError, message):
                    evidence.validate_capture(REQUEST, 15, ANCHOR, pose, camera, 1000)
        for phase, message in ((14, "phase differs"), (True, "phase differs")):
            with self.subTest(phase=phase):
                evidence = MotionEvidence.from_wire(evidence_wire())
                with self.assertRaises(ProtocolError):
                    evidence.validate_capture(REQUEST, phase, ANCHOR, pose, camera, 1000)
        evidence = MotionEvidence.from_wire(evidence_wire())
        with self.assertRaisesRegex(ProtocolError, "predicted display time"):
            evidence.validate_capture(REQUEST, 15, ANCHOR, pose, camera, 1001)
        other_anchor = Camera((9.0, 9.0, 9.0), ANCHOR.orientation)
        with self.assertRaisesRegex(ProtocolError, "anchor"):
            evidence.validate_capture(REQUEST, 15, other_anchor, pose, camera, 1000)
        with self.assertRaisesRegex(ProtocolError, "applied camera"):
            evidence.validate_capture(REQUEST, 15, ANCHOR, pose,
                                      Camera((9.0, 9.0, 9.0), camera.orientation), 1000)

    def test_malformed_motion_evidence_is_rejected(self):
        valid = evidence_wire()
        for change in (
            {"phase": 120},
            {"cycle": -1},
            {"rendered_frame": -1},
            {"history": None},
            {"history": [{}] * 9},
            {"history": [{"phase": 120, "rendered_frame": 1,
                          "applied": {"position": [0, 0, 0], "orientation": [0, 0, 0, 1]}}]},
        ):
            with self.subTest(change=change), self.assertRaises(ProtocolError):
                MotionEvidence.from_wire({**valid, **change})
        for request in (
            {**REQUEST.to_wire(), "period_frames": 31},
            {**REQUEST.to_wire(), "yaw_deg": float("nan")},
            {**REQUEST.to_wire(), "case_id": "bad case"},
        ):
            with self.subTest(request=request), self.assertRaises(ProtocolError):
                MotionEvidence.from_wire({**valid, "request": request})
        with self.assertRaisesRegex(ProtocolError, "no completed cycle"):
            MotionEvidence.from_wire({**valid, "cycle": 0}).validate_capture(
                REQUEST, 15, ANCHOR, capture_pose(), capture_pose().applied, 1000)
