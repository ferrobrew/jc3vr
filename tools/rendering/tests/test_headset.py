"""The headset-rig override: parsing, parity with the payload's presets, and capture validation."""

import math
import re
import tomllib
import unittest
from pathlib import Path

from tools.rendering.protocol import (
    HEADSET_RIGS,
    Capture,
    Command,
    HeadsetGeometry,
    Mode,
    ProtocolError,
    Request,
    VisualOverrides,
)
from tools.rendering.protocol.headset import _rotate, _slerp_half
from tools.rendering.protocol.base import quaternion_conjugate, quaternion_multiply, quaternion_normalize
from tools.rendering.tests.test_experiment import capture_wire

REPOSITORY = Path(__file__).resolve().parents[3]


def geometry_wire(name):
    geometry = HEADSET_RIGS[name]
    return {"rig": geometry.rig, "ipd_m": geometry.ipd_m,
            "eyes": [{"fov_deg": list(eye.fov_deg), "yaw_deg": eye.yaw_deg, "pitch_deg": eye.pitch_deg,
                      "roll_deg": eye.roll_deg} for eye in geometry.eyes]}


def eyes_wire(name):
    """The per-eye camera inputs the payload records for a rig, built as the payload builds them."""
    geometry = HEADSET_RIGS[name]
    rotations = [eye.rotation() for eye in geometry.eyes]
    centre = _slerp_half(*rotations)
    eyes = []
    for index, eye in enumerate(geometry.eyes):
        tl, tr, tu, td = eye.tangents()
        projection = [0.0] * 16
        projection[0] = 2 / (tr - tl)
        projection[5] = 2 / (tu - td)
        projection[8] = (tl + tr) / (tr - tl)
        projection[9] = (td + tu) / (tu - td)
        delta = quaternion_normalize(quaternion_multiply(quaternion_conjugate(centre), rotations[index]))
        side = -0.5 if index == 0 else 0.5
        offset = _rotate(quaternion_conjugate(centre), (side * geometry.ipd_m, 0.0, 0.0))
        eyes.append({"offset": list(offset), "orientation_delta": list(delta),
                     "projection_standard": projection, "projection_reverse_z": [0.0] * 16})
    return eyes


def rig_capture(name, recorded=None, eyes=None):
    wire = capture_wire("shot", Mode.SINGLE_PASS, 3, None, None)
    if name is not None:
        wire["visual"]["visual_overrides"] = {"headset": name}
    if recorded is not None:
        wire["visual"]["headset"] = geometry_wire(recorded)
    if eyes is not None:
        wire["comparison_inputs"]["eyes"] = eyes_wire(eyes)
    return Capture.from_wire(wire)


class HeadsetOverrideTests(unittest.TestCase):
    def test_the_rig_round_trips_through_the_request_file(self):
        overrides = VisualOverrides(headset="index")
        request = Request(Command.CONFIGURE, "cfg", Mode.LEFT_ONLY, 30, visual_overrides=overrides)
        table = tomllib.loads(request.to_toml())["visual_overrides"]
        self.assertEqual(table, {"headset": "index"})
        self.assertEqual(VisualOverrides.from_wire(table), overrides)

    def test_the_stage_set_round_trips_and_is_closed(self):
        overrides = VisualOverrides(headset="index", stages="geometry")
        table = tomllib.loads(Request(Command.CONFIGURE, "cfg", Mode.LEFT_ONLY, 30,
                                      visual_overrides=overrides).to_toml())["visual_overrides"]
        self.assertEqual(table, {"headset": "index", "stages": "geometry"})
        self.assertEqual(VisualOverrides.from_wire(table), overrides)
        with self.assertRaises(ProtocolError):
            VisualOverrides(stages="all")

    def test_unknown_rigs_are_rejected(self):
        with self.assertRaisesRegex(ProtocolError, "unknown headset rig"):
            VisualOverrides(headset="vive")
        with self.assertRaises(ProtocolError):
            VisualOverrides.from_wire({"headset": "vive"})

    def test_presets_keep_the_eye_target_aspect(self):
        for name, geometry in HEADSET_RIGS.items():
            target = 1.0 if name == "beyond" else 320 / 360
            for eye in geometry.eyes:
                tl, tr, tu, td = eye.tangents()
                with self.subTest(rig=name):
                    self.assertAlmostEqual((tr - tl) / (tu - td), target, delta=0.01)

    def test_presets_match_the_payload(self):
        source = (REPOSITORY / "crates/jc3vr-capture/src/testing/visual/headset.rs").read_text()
        found = dict(re.findall(
            r"Self::(\w+) =>(?: \{\s*let mut geometry =)?\s*mirrored\(self, ([^)]*)\)", source))
        self.assertEqual(sorted(name.lower() for name in found), sorted(HEADSET_RIGS))
        for name, arguments in found.items():
            numbers = [float(item) for item in re.findall(r"-?\d+\.\d+", arguments)]
            geometry = HEADSET_RIGS[name.lower()]
            left = geometry.eyes[0]
            with self.subTest(rig=name):
                self.assertEqual(numbers, [geometry.ipd_m, *left.fov_deg, left.yaw_deg, left.pitch_deg,
                                           left.roll_deg])
        self.assertIn("geometry.eyes[1].pitch_deg = -geometry.eyes[0].pitch_deg", source)
        stress = HEADSET_RIGS["stress"].eyes
        self.assertEqual(stress[1].pitch_deg, -stress[0].pitch_deg)
        self.assertEqual(stress[1].roll_deg, -stress[0].roll_deg)

    def test_geometry_parses_strictly(self):
        self.assertTrue(HeadsetGeometry.from_wire(geometry_wire("quest")).matches(HEADSET_RIGS["quest"]))
        broken = geometry_wire("quest")
        broken["eyes"] = broken["eyes"][:1]
        for value in (broken, {**geometry_wire("quest"), "rig": "vive"},
                      {**geometry_wire("quest"), "ipd_m": -1.0}, {"rig": "quest"}):
            with self.subTest(value=value), self.assertRaises(ProtocolError):
                HeadsetGeometry.from_wire(value)


class HeadsetCaptureTests(unittest.TestCase):
    def test_a_capture_under_the_requested_rig_passes(self):
        for name in HEADSET_RIGS:
            with self.subTest(rig=name):
                rig_capture(name, recorded=name, eyes=name).validate_headset()

    def test_a_capture_without_the_rig_is_rejected(self):
        with self.assertRaisesRegex(ProtocolError, "not rendered under the index rig"):
            rig_capture("index").validate_headset()
        with self.assertRaisesRegex(ProtocolError, "not rendered under the index rig"):
            rig_capture("index", recorded="quest", eyes="quest").validate_headset()
        with self.assertRaisesRegex(ProtocolError, "no override requested"):
            rig_capture(None, recorded="quest").validate_headset()
        rig_capture(None).validate_headset()

    def test_camera_inputs_that_do_not_carry_the_rig_are_rejected(self):
        with self.assertRaisesRegex(ProtocolError, "projection"):
            rig_capture("index", recorded="index").validate_headset()
        with self.assertRaisesRegex(ProtocolError, "projection"):
            rig_capture("index", recorded="index", eyes="quest").validate_headset()
        capture = rig_capture("index", recorded="index", eyes="index")
        eyes = eyes_wire("index")
        eyes[0]["orientation_delta"] = [0.0, 0.0, 0.0, 1.0]
        wire = capture_wire("shot", Mode.SINGLE_PASS, 3, None, None)
        wire["visual"]["visual_overrides"] = {"headset": "index"}
        wire["visual"]["headset"] = geometry_wire("index")
        wire["comparison_inputs"]["eyes"] = eyes
        with self.assertRaisesRegex(ProtocolError, "orientation"):
            Capture.from_wire(wire).validate_headset()
        eyes = eyes_wire("index")
        eyes[1]["offset"] = [0.0315, 0.0, 0.0]
        wire["comparison_inputs"]["eyes"] = eyes
        with self.assertRaisesRegex(ProtocolError, "IPD"):
            Capture.from_wire(wire).validate_headset()
        capture.validate_headset()

    def test_the_index_rig_cants_each_eye_outward_by_five_degrees(self):
        eyes = eyes_wire("index")
        for index, sign in ((0, 1), (1, -1)):
            x, y, z, w = eyes[index]["orientation_delta"]
            self.assertAlmostEqual(2 * math.degrees(math.atan2(y, w)), sign * 5.0, places=4)


if __name__ == "__main__":
    unittest.main()
