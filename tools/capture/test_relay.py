"""Tests for the capture relay's control protocol, window lookup, and region math.

    python3 -I -m unittest discover -s tools/capture
"""

import json
import os
import tempfile
import unittest

import relay


def start_line(**fields) -> bytes:
    message = {"protocol": "jc3vr-capture", "version": 1, "type": "start"}
    message.update(fields)
    return json.dumps(message).encode()


class ParseMessage(unittest.TestCase):
    def test_a_full_start_is_parsed(self):
        message = relay.parse_message(
            start_line(
                take="2026-11-01_10-00-00_000",
                folder="/home/user/sessions/s/recordings/r",
                window={"title": "Just Cause 3", "client_size": [1920, 1080]},
            )
            + b"\n"
        )
        self.assertEqual(
            message,
            relay.Start(
                take="2026-11-01_10-00-00_000",
                folder="/home/user/sessions/s/recordings/r",
                window=relay.WindowHint(title="Just Cause 3", client_size=(1920, 1080)),
            ),
        )

    def test_a_minimal_start_has_no_folder_and_no_hint(self):
        message = relay.parse_message(start_line())
        self.assertEqual(
            message,
            relay.Start(take=None, folder=None, window=relay.WindowHint(None, None)),
        )

    def test_stop_and_unknown_types(self):
        self.assertEqual(relay.parse_message(b'{"type": "stop"}'), relay.Stop())
        self.assertEqual(relay.parse_message(b'{"type": "ping"}'), relay.Unknown("ping"))

    def test_another_protocol_or_version_is_refused(self):
        for line in [
            start_line(version=2),
            start_line(protocol="other"),
            b'{"type": "start", "version": 1}',
        ]:
            with self.subTest(line=line):
                with self.assertRaises(relay.ProtocolError):
                    relay.parse_message(line)

    def test_malformed_lines_are_refused(self):
        for line in [b"not json", b"[1, 2]", b'{"kind": "start"}', b"\xff\xfe"]:
            with self.subTest(line=line):
                with self.assertRaises(relay.ProtocolError):
                    relay.parse_message(line)

    def test_unsafe_folders_and_takes_are_refused(self):
        for fields in [
            {"folder": "relative/path"},
            {"folder": "/a\u0000b"},
            {"folder": 3},
            {"take": "../escape"},
            {"take": ""},
        ]:
            with self.subTest(fields=fields):
                with self.assertRaises(relay.ProtocolError):
                    relay.parse_message(start_line(**fields))

    def test_bad_window_hints_are_refused(self):
        for window in [
            "Just Cause 3",
            {"title": 3},
            {"client_size": [1920]},
            {"client_size": [0, 1080]},
            {"client_size": [True, 1080]},
        ]:
            with self.subTest(window=window):
                with self.assertRaises(relay.ProtocolError):
                    relay.parse_message(start_line(window=window))


class LineReader(unittest.TestCase):
    def test_lines_split_across_reads_are_joined(self):
        reader = relay.LineReader()
        self.assertEqual(reader.feed(b'{"type":'), [])
        self.assertEqual(reader.feed(b'"stop"}\n{"a"'), [b'{"type":"stop"}'])
        self.assertEqual(reader.feed(b":1}\n"), [b'{"a":1}'])

    def test_an_overlong_line_is_refused(self):
        reader = relay.LineReader(limit=8)
        reader.feed(b"12345678")
        with self.assertRaises(relay.ProtocolError):
            reader.feed(b"9")


def window(id, title, app_id="steam_app_225540", workspace_id=1, size=(1920, 1080), **extra):
    layout = {
        "tile_pos_in_workspace_view": [0.0, 0.0],
        "window_offset_in_tile": [0.0, 0.0],
        "window_size": list(size),
    }
    layout.update(extra.pop("layout", {}))
    return {
        "id": id,
        "title": title,
        "app_id": app_id,
        "workspace_id": workspace_id,
        "is_focused": False,
        "is_floating": False,
        "layout": layout,
        **extra,
    }


class FindWindow(unittest.TestCase):
    def test_the_title_the_payload_sends_wins(self):
        windows = [window(1, "Steam", app_id="steam"), window(2, "Jeu")]
        hint = relay.WindowHint(title="Jeu", client_size=None)
        self.assertEqual(relay.find_window(windows, hint)["id"], 2)

    def test_without_a_title_the_default_title_then_the_app_id_match(self):
        none = relay.WindowHint(None, None)
        self.assertEqual(relay.find_window([window(4, "Just Cause 3", app_id="x")], none)["id"], 4)
        self.assertEqual(relay.find_window([window(5, "Renamed")], none)["id"], 5)

    def test_several_matches_narrow_by_app_id_then_size_then_focus(self):
        hint = relay.WindowHint(title="Just Cause 3", client_size=(1280, 720))
        by_app = [window(1, "Just Cause 3", app_id="browser"), window(2, "Just Cause 3")]
        self.assertEqual(relay.find_window(by_app, hint)["id"], 2)
        by_size = [window(1, "Just Cause 3"), window(2, "Just Cause 3", size=(1280, 720))]
        self.assertEqual(relay.find_window(by_size, hint)["id"], 2)
        by_focus = [window(1, "Just Cause 3"), window(2, "Just Cause 3", is_focused=True)]
        self.assertEqual(relay.find_window(by_focus, hint)["id"], 2)

    def test_no_match_and_ambiguity_are_errors(self):
        hint = relay.WindowHint(None, None)
        with self.assertRaisesRegex(relay.LocateError, "no window titled"):
            relay.find_window([window(1, "Steam", app_id="steam")], hint)
        with self.assertRaisesRegex(relay.LocateError, "several windows"):
            relay.find_window([window(1, "Just Cause 3"), window(2, "Just Cause 3")], hint)


OUTPUTS = {
    "DP-1": {
        "name": "DP-1",
        "logical": {"x": 0, "y": 0, "width": 3440, "height": 1440, "scale": 1.0},
    },
    "HDMI-A-2": {
        "name": "HDMI-A-2",
        "logical": {"x": 3440, "y": 0, "width": 2160, "height": 3840, "scale": 1.0},
    },
    "DP-3": {"name": "DP-3", "logical": None},
}
WORKSPACES = [
    {"id": 1, "idx": 1, "output": "DP-1", "is_active": True},
    {"id": 2, "idx": 2, "output": "DP-1", "is_active": False},
    {"id": 3, "idx": 1, "output": "HDMI-A-2", "is_active": True},
    {"id": 4, "idx": 1, "output": "DP-3", "is_active": True},
]


class WindowRegion(unittest.TestCase):
    def region(self, **fields):
        w = window(9, "Just Cause 3", **fields)
        return relay.window_region(w, WORKSPACES, OUTPUTS)

    def test_a_tiled_window_is_its_tile_position_plus_its_offset(self):
        region, output = self.region(
            size=(1141, 701),
            layout={"tile_pos_in_workspace_view": [4.0, 31.0], "window_offset_in_tile": [2.0, 3.0]},
        )
        self.assertEqual(region, relay.Region(x=6, y=34, width=1141, height=701))
        self.assertEqual(output["name"], "DP-1")
        self.assertEqual(region.gsr_arg(), "1141x701+6+34")

    def test_a_floating_window_on_a_second_output_is_offset_by_the_output(self):
        region, _ = self.region(
            workspace_id=3,
            size=(1074, 1265),
            is_floating=True,
            layout={"tile_pos_in_workspace_view": [1082.5, 2569.0]},
        )
        self.assertEqual(region, relay.Region(x=4522, y=2569, width=1074, height=1265))

    def test_a_window_off_screen_or_hidden_is_an_error(self):
        cases = [
            ({"layout": {"tile_pos_in_workspace_view": [-1141.0, 31.0]}}, "not wholly on"),
            ({"layout": {"tile_pos_in_workspace_view": [3000.0, 31.0]}}, "not wholly on"),
            ({"workspace_id": 2}, "not the active one"),
            ({"workspace_id": None}, "not on a workspace"),
            ({"workspace_id": 4}, "which is off"),
            ({"layout": {"tile_pos_in_workspace_view": None}}, "no on-screen position"),
        ]
        for fields, message in cases:
            with self.subTest(fields=fields):
                with self.assertRaisesRegex(relay.LocateError, message):
                    self.region(**fields)


class Region(unittest.TestCase):
    def test_the_slurp_form_round_trips(self):
        region = relay.parse_region("1920x1080+3440+0")
        self.assertEqual(region, relay.Region(x=3440, y=0, width=1920, height=1080))
        self.assertEqual(region.gsr_arg(), "1920x1080+3440+0")

    def test_malformed_and_empty_regions_are_refused(self):
        for text in ["1920x1080", "1920+0+0", "0x1080+0+0", "axb+0+0"]:
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    relay.parse_region(text)


class Takes(unittest.TestCase):
    def test_a_second_take_in_one_folder_gets_numbered_names(self):
        with tempfile.TemporaryDirectory() as folder:
            first = relay.take_paths(folder)
            self.assertEqual(first, (f"{folder}/video.mkv", f"{folder}/stream.json"))
            open(first[0], "wb").close()
            self.assertEqual(
                relay.take_paths(folder), (f"{folder}/video_1.mkv", f"{folder}/stream_1.json")
            )

    def test_the_command_records_the_region_with_audio_and_the_first_frame_time(self):
        region = relay.Region(x=4, y=31, width=1141, height=701)
        cbr = relay.Encoding(codec="hevc", bitrate_kbps=10_000, quality="high")
        command = relay.build_command("gsr", region, "/t/video.mkv", 60, cbr, "default_output", [])
        self.assertEqual(
            command,
            [
                "gsr", "-w", "1141x701+4+31", "-f", "60", "-k", "hevc", "-bm", "cbr", "-q", "10000",
                "-a", "default_output", "-write-first-frame-ts", "yes", "-o", "/t/video.mkv",
            ],
        )
        silent = relay.build_command("gsr", region, "/t/v.mkv", 30, cbr, "none", ["-cursor", "no"])
        self.assertNotIn("-a", silent)
        self.assertEqual(silent[-4:], ["-cursor", "no", "-o", "/t/v.mkv"])

    def test_without_a_bitrate_the_command_records_at_constant_quality(self):
        region = relay.Region(x=0, y=0, width=1920, height=1080)
        qp = relay.Encoding(codec="av1", bitrate_kbps=None, quality="very_high")
        command = relay.build_command("gsr", region, "/t/v.mkv", 60, qp, "none", [])
        self.assertEqual(command[5:11], ["-k", "av1", "-bm", "qp", "-q", "very_high"])

    def test_the_first_frame_time_is_read_from_the_timestamp_file(self):
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "video.mkv.ts")
            self.assertIsNone(relay.read_first_frame_ts(path))
            with open(path, "w") as f:
                f.write("monotonic_microsec\trealtime_microsec\n")
            self.assertIsNone(relay.read_first_frame_ts(path))
            with open(path, "a") as f:
                f.write("705275364165\t1793503069411417\n")
            self.assertEqual(relay.read_first_frame_ts(path), 1793503069411.417)


if __name__ == "__main__":
    unittest.main()
