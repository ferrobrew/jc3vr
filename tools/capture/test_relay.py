"""Tests for the capture relay's header parsing, path decoding, and command building.

    python3 -I -m unittest discover -s tools/capture
"""

import os
import tempfile
import unittest

import relay

PLAIN = b"YUV4MPEG2 W2 H2 F72:1 Ip A1:1 C420jpeg XYSCSS=420JPEG XCOLORRANGE=LIMITED"


class ParseHeader(unittest.TestCase):
    def test_a_header_without_the_tag_is_forwarded_unchanged(self):
        header = relay.parse_header(PLAIN + b"\n")
        self.assertIsNone(header.folder)
        self.assertEqual(header.forward, PLAIN + b"\n")

    def test_the_tag_is_extracted_and_removed(self):
        line = PLAIN + b" XJC3VR_DIR=/home/user/sessions/s/recordings/r\n"
        header = relay.parse_header(line)
        self.assertEqual(header.folder, "/home/user/sessions/s/recordings/r")
        self.assertEqual(header.forward, PLAIN + b"\n")

    def test_the_tag_may_sit_anywhere(self):
        line = b"YUV4MPEG2 W2 XJC3VR_DIR=/a H2 F72:1\n"
        header = relay.parse_header(line)
        self.assertEqual(header.folder, "/a")
        self.assertEqual(header.forward, b"YUV4MPEG2 W2 H2 F72:1\n")

    def test_other_x_parameters_are_kept(self):
        header = relay.parse_header(PLAIN + b" XOTHER=1\n")
        self.assertEqual(header.forward, PLAIN + b" XOTHER=1\n")

    def test_a_stream_that_is_not_y4m_is_refused(self):
        for line in [b"GET / HTTP/1.1\r\n", b"YUV4MPEG2W2\n", PLAIN]:
            with self.subTest(line=line):
                with self.assertRaises(relay.HeaderError):
                    relay.parse_header(line)


class DecodeFolder(unittest.TestCase):
    def test_percent_escapes_are_decoded(self):
        self.assertEqual(
            relay.decode_folder(b"/home/user/jc3vr%20sessions/100%25"),
            "/home/user/jc3vr sessions/100%",
        )

    def test_utf8_bytes_decode_to_the_filesystem_name(self):
        self.assertEqual(relay.decode_folder(b"/caf%C3%A9"), os.fsdecode(b"/caf\xc3\xa9"))

    def test_relative_and_nul_paths_are_refused(self):
        for value in [b"relative/path", b"", b"/a%00b"]:
            with self.subTest(value=value):
                with self.assertRaises(relay.HeaderError):
                    relay.decode_folder(value)


class Segments(unittest.TestCase):
    def test_numbering_continues_after_existing_segments(self):
        with tempfile.TemporaryDirectory() as folder:
            self.assertEqual(relay.next_segment_number(folder), 0)
            for name in ["video_000.mkv", "video_003.mkv", "frames.jsonl", "video_x.mkv"]:
                open(os.path.join(folder, name), "wb").close()
            self.assertEqual(relay.next_segment_number(folder), 4)

    def test_a_missing_folder_starts_at_zero(self):
        self.assertEqual(relay.next_segment_number("/nonexistent/jc3vr/relay/test"), 0)


class BuildCommand(unittest.TestCase):
    def test_the_output_token_becomes_the_start_number_and_pattern(self):
        command = relay.build_command(["ffmpeg", "-i", "pipe:0", "@OUTPUT@"], "/r", 2)
        self.assertEqual(
            command,
            ["ffmpeg", "-i", "pipe:0", "-segment_start_number", "2", "/r/video_%03d.mkv"],
        )

    def test_the_token_must_appear_once(self):
        for template in [["ffmpeg"], ["ffmpeg", "@OUTPUT@", "@OUTPUT@"]]:
            with self.subTest(template=template):
                with self.assertRaises(ValueError):
                    relay.build_command(template, "/r", 0)


if __name__ == "__main__":
    unittest.main()
