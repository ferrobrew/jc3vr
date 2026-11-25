"""The pinned anchor: the configuration request's `[anchor]` table and the session's record of the
anchors it pinned per stage."""

import tempfile
import tomllib
import unittest
from pathlib import Path

from tools.rendering.protocol import Camera, Command, Mode, ProtocolError, Request
from tools.rendering.session.stage_record import record_anchor, recorded_anchor


class AnchorRequestTest(unittest.TestCase):
    def test_a_configuration_carries_its_anchor(self):
        anchor = Camera((2091.0757, 1040.3412, 5004.901), (0.0074512903, -0.6945533, 0.0007524921, 0.7194024))
        request = Request(Command.CONFIGURE, "configure", Mode.LEFT_ONLY, 30, anchor=anchor)
        table = tomllib.loads(request.to_toml())
        self.assertEqual(table["anchor"]["position"], list(anchor.position))
        self.assertEqual(table["anchor"]["orientation"], list(anchor.orientation))

    def test_only_a_configuration_takes_a_finite_anchor(self):
        with self.assertRaises(ProtocolError):
            Request(Command.END, "end", anchor=Camera((0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 1.0)))
        with self.assertRaises(ProtocolError):
            Request(Command.CONFIGURE, "configure", Mode.LEFT_ONLY, 30,
                    anchor=Camera((0.0, float("nan"), 0.0), (0.0, 0.0, 0.0, 1.0)))


class AnchorRecordTest(unittest.TestCase):
    def test_anchors_are_recorded_per_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            config_dir = Path(directory)
            self.assertIsNone(recorded_anchor(config_dir, "town"))
            record_anchor(config_dir, "town", (1.0, 2.0, 3.0), (0.0, 0.0, 0.0, 1.0))
            record_anchor(config_dir, "shore", (4.0, 5.0, 6.0), (0.0, 1.0, 0.0, 0.0))
            self.assertEqual(recorded_anchor(config_dir, "town"), ((1.0, 2.0, 3.0), (0.0, 0.0, 0.0, 1.0)))
            self.assertEqual(recorded_anchor(config_dir, "shore"), ((4.0, 5.0, 6.0), (0.0, 1.0, 0.0, 0.0)))

    def test_a_malformed_record_pins_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            config_dir = Path(directory)
            (config_dir / "pinned-anchors.json").write_text('{"town": {"position": [1, 2]}}')
            self.assertIsNone(recorded_anchor(config_dir, "town"))


if __name__ == "__main__":
    unittest.main()
