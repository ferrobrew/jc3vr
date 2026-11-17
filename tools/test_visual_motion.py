# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///
"""Motion provenance and sweep tests."""

import copy
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

if __package__:
    from tools.visual_compare import analyze_manifest
    from tools.visual_motion_protocol import ExpectedMotion, MotionRequest
    from tools.visual_protocol import (
        Camera,
        PoseEvidence,
        compose_pose,
        quaternion_normalize,
    )
else:
    from visual_compare import analyze_manifest
    from visual_motion_protocol import ExpectedMotion, MotionRequest
    from visual_protocol import (
        Camera,
        PoseEvidence,
        compose_pose,
        quaternion_normalize,
    )


REQUEST = MotionRequest("motion", 120, 20.0, 10.0, 10.0)
PHASE = 15
ANCHOR = Camera((1.5, -2.0, 0.25), quaternion_normalize((0.1, 0.2, 0.3, 0.9)))
MODES = ("left_only", "right_only", "multipass", "single_pass")


def camera_wire(camera):
    return {"position": list(camera.position), "orientation": list(camera.orientation)}


def pose_wire(evidence):
    return {
        "case_id": evidence.case_id,
        "offset": evidence.offset.to_wire(),
        "anchor": camera_wire(evidence.anchor),
        "applied": camera_wire(evidence.applied),
    }


def motion_wire(phase=PHASE, cycle=1, predicted=10_000, anchor=ANCHOR):
    return {
        "request": REQUEST.to_wire(), "phase": phase, "cycle": cycle,
        "rendered_frame": predicted,
        "history": [
            {"phase": (phase - 8 + index) % REQUEST.period_frames,
             "rendered_frame": predicted - 8 + index,
             "applied": camera_wire(compose_pose(anchor, REQUEST.offset_at(
                 (phase - 8 + index) % REQUEST.period_frames)))}
            for index in range(8)
        ],
    }


def pose_evidence(phase=PHASE, anchor=ANCHOR):
    offset = REQUEST.offset_at(phase)
    return PoseEvidence(REQUEST.case_id, offset, anchor, compose_pose(anchor, offset))


class MotionCompareTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "report"
        self.path = self.root / "manifest.json"
        self.left = np.full((8, 12, 3), 30, dtype=np.uint8)
        self.right = np.full((8, 12, 3), 120, dtype=np.uint8)
        self.inputs = {
            "camera": camera_wire(ANCHOR),
            "eyes": [
                {"offset": [-0.03, 0, 0], "orientation_delta": [0, 0, 0, 1],
                 "projection_standard": np.eye(4).flatten().tolist(),
                 "projection_reverse_z": np.eye(4).flatten().tolist()},
                {"offset": [0.03, 0, 0], "orientation_delta": [0, 0, 0, 1],
                 "projection_standard": np.eye(4).flatten().tolist(),
                 "projection_reverse_z": np.eye(4).flatten().tolist()},
            ],
            "profile": "controlled_v1", "world_scale": 1.0,
            "build_stamp": "synthetic-build", "eye_width": 12, "eye_height": 8,
        }
        self.manifest = {"cases": [{
            "name": "phase_015", "modes": {},
            "expected_motion": ExpectedMotion(REQUEST, PHASE).to_wire(),
        }]}
        for mode in MODES:
            self.add_mode(mode)

    def add_mode(self, mode):
        isolated = mode in ("left_only", "right_only")
        eye = (0 if mode == "left_only" else 1) if isolated else None
        image = ((self.left if eye == 0 else self.right) if isolated
                 else np.concatenate([self.left, self.right], axis=1))
        samples = []
        for index in range(3):
            ordinal = 3 * len(self.manifest["cases"][0]["modes"]) + index + 1
            predicted = 10_000 + ordinal * 100
            name = f"{mode}-{index}"
            png, metadata_path = f"{name}.png", f"{name}.json"
            Image.fromarray(image).save(self.root / png)
            evidence = pose_evidence()
            motion = motion_wire(cycle=index + 1, predicted=predicted)
            metadata = {
                "id": name, "frame_ordinal": ordinal,
                "predicted_display_time_ns": predicted, "mode": mode,
                "visual": {
                    "generation": len(self.manifest["cases"][0]["modes"]) + 1,
                    "mode": mode, "profile": "controlled_v1", "ready": True,
                    "paused": True, "frozen": True, "error": None,
                    "warmup_frames": 3, "completed_frames": 3,
                    "pose": pose_wire(evidence), "motion": motion,
                },
                "fresh_eyes": [eye in (None, 0), eye in (None, 1)],
                "width": image.shape[1], "height": image.shape[0],
                "eye_width": 12, "eye_height": 8, "selected_eye": eye,
                "rendered_eyes": [eye in (None, 0), eye in (None, 1)],
                "comparison_inputs": copy.deepcopy(self.inputs),
            }
            metadata["comparison_inputs"]["camera"] = camera_wire(evidence.applied)
            (self.root / metadata_path).write_text(json.dumps(metadata))
            samples.append({"png": png, "metadata": metadata_path})
        self.manifest["cases"][0]["modes"][mode] = samples

    def entry(self, mode="left_only", index=0):
        return self.manifest["cases"][0]["modes"][mode][index]

    def change_metadata(self, mode, index, change):
        path = self.root / self.entry(mode, index)["metadata"]
        metadata = json.loads(path.read_text())
        change(metadata)
        path.write_text(json.dumps(metadata))

    def analyze(self):
        self.path.write_text(json.dumps(self.manifest))
        return analyze_manifest(self.path, self.output, tile_size=4, top_tiles=2)

    def assert_inconclusive(self, text):
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn(text, json.dumps(report.to_wire()))
        return report

    def test_full_motion_provenance_passes_and_accepts_quaternion_sign(self):
        self.assertEqual(self.analyze().status, "pass")
        for index in range(3):
            def negate(metadata):
                pose = metadata["visual"]["pose"]["applied"]["orientation"]
                metadata["visual"]["pose"]["applied"]["orientation"] = [-item for item in pose]
                for entry in metadata["visual"]["motion"]["history"]:
                    entry["applied"]["orientation"] = [-item for item in entry["applied"]["orientation"]]
            self.change_metadata("left_only", index, negate)
        self.assertEqual(self.analyze().status, "pass")
        recorded = json.loads((self.output / "report.json").read_text())
        motion = recorded["cases"][0]["modes"][0]["samples"][0]["visual"]["motion"]
        self.assertEqual(motion["phase"], PHASE)
        self.assertEqual(len(motion["history"]), 8)
        self.assertNotIn("wall", json.dumps(motion))

    def test_missing_malformed_and_stale_history_are_inconclusive(self):
        cases = (
            ("missing", lambda data: data["visual"].pop("motion"), "no motion evidence"),
            ("short", lambda data: data["visual"]["motion"].update(
                history=data["visual"]["motion"]["history"][1:]), "eight frames"),
            ("gap", lambda data: data["visual"]["motion"]["history"][3].update(phase=1), "contiguous"),
            ("stale", lambda data: data["visual"]["motion"]["history"][1].update(
                rendered_frame=data["visual"]["motion"]["history"][0]["rendered_frame"]),
             "strictly increasing"),
            ("future", lambda data: data["visual"]["motion"]["history"][-1].update(
                rendered_frame=data["predicted_display_time_ns"]), "before the capture"),
        )
        for name, change, message in cases:
            with self.subTest(name=name):
                self.setUp()
                self.change_metadata("left_only", 0, change)
                self.assert_inconclusive(message)

    def test_wrong_predicted_time_and_request_are_inconclusive(self):
        self.change_metadata("left_only", 0, lambda data: data["visual"]["motion"].update(
            rendered_frame=data["predicted_display_time_ns"] + 1))
        self.assert_inconclusive("predicted display time")
        self.setUp()
        self.manifest["cases"][0]["expected_motion"]["request"]["case_id"] = "other"
        self.assert_inconclusive("request differs")

    def test_expected_motion_phase_and_anchor_drift_are_inconclusive(self):
        self.manifest["cases"][0]["expected_motion"]["phase"] = PHASE - 1
        self.assert_inconclusive("phase differs")
        self.setUp()
        other = Camera((9.0, 9.0, 9.0), ANCHOR.orientation)

        def move(data):
            evidence = pose_evidence(anchor=other)
            data["visual"]["pose"] = pose_wire(evidence)
            data["visual"]["motion"] = motion_wire(anchor=other)
            data["comparison_inputs"]["camera"] = camera_wire(evidence.applied)
        self.change_metadata("left_only", 0, move)
        self.assert_inconclusive("anchor")

    def test_motion_evidence_without_expected_motion_is_inconclusive(self):
        self.manifest["cases"][0].pop("expected_motion")
        self.assert_inconclusive("without expected_motion")

    def test_legacy_manifests_without_motion_still_pass(self):
        self.manifest["cases"][0].pop("expected_motion")
        for mode in MODES:
            for index in range(3):
                self.change_metadata(mode, index, lambda data: (
                    data["visual"].pop("motion", None),
                    data["visual"].pop("pose", None),
                    data["comparison_inputs"].update(camera=camera_wire(ANCHOR)),
                ))
        self.assertEqual(self.analyze().status, "pass")


if __name__ == "__main__":
    unittest.main()
