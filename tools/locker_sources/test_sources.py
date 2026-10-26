"""Tests for selective source inspection without a game installation."""

import struct
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import inspect_sources
from model_collection import parse_collection


IDENTITY = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)


def collection(names=(b"models/example.lod",), indices=(0,), translation=7.0):
    names_blob = b"\0".join(names) + b"\0"
    data = struct.pack("<4s7I", b"MDI\0", 12, len(names), len(indices), len(names_blob), 0, 0, 0)
    data += names_blob
    data += b"\0" * (-len(data) % 16)
    matrix = (*IDENTITY[:12], translation, 8, 9, 1)
    data += struct.pack("<16f", *matrix) * len(indices)
    data += struct.pack(f"<{len(indices)}H", *indices)
    data += struct.pack("<H", 3) * len(indices)
    data += struct.pack("<3f", 0, 0, 0) * len(indices)
    data += struct.pack("<f", -1) * len(indices)
    data += struct.pack("<6f", 6, 7, 8, 9, 10, 11) * len(indices)
    return data


class CollectionTests(unittest.TestCase):
    def test_preserves_file_indices_names_and_world(self):
        records = parse_collection(collection((b"a.lod", b"b.lod"), (1, 0)))
        self.assertEqual([record["collection_index"] for record in records], [0, 1])
        self.assertEqual([record["model"] for record in records], ["b.lod", "a.lod"])
        self.assertEqual(records[0]["world"][12:15], (7, 8, 9))
        self.assertEqual(records[0]["bounds"], (6, 7, 8, 9, 10, 11))
        self.assertEqual(records[0]["flags"], 3)
        self.assertEqual(records[0]["lod_factor"], -1)

    def test_rejects_truncated_header(self):
        with self.assertRaisesRegex(ValueError, "truncated header"):
            parse_collection(b"MDI\0")

    def test_rejects_unsupported_version(self):
        data = bytearray(collection())
        struct.pack_into("<I", data, 4, 13)
        with self.assertRaisesRegex(ValueError, "unsupported header"):
            parse_collection(data)

    def test_rejects_invalid_model_index(self):
        with self.assertRaisesRegex(ValueError, "invalid model index"):
            parse_collection(collection(indices=(1,)))

    def test_rejects_truncated_arrays(self):
        with self.assertRaisesRegex(ValueError, "size mismatch"):
            parse_collection(collection()[:-1])

    def test_rejects_nonfinite_transform(self):
        with self.assertRaisesRegex(ValueError, "non-finite world matrix"):
            parse_collection(collection(translation=float("nan")))

    def test_rejects_unterminated_name(self):
        data = bytearray(collection())
        name_size = struct.unpack_from("<I", data, 16)[0]
        data[32 + name_size - 1] = 65
        with self.assertRaisesRegex(ValueError, "unterminated name chunk"):
            parse_collection(data)

    def test_rejects_name_chunk_outside_buffer(self):
        data = bytearray(collection())
        struct.pack_into("<I", data, 16, len(data))
        with self.assertRaisesRegex(ValueError, "truncated name chunk"):
            parse_collection(data)

    def test_rejects_non_ascii_name(self):
        with self.assertRaisesRegex(ValueError, "non-ASCII model name"):
            parse_collection(collection(names=(b"\xff.lod",)))


class InspectorTests(unittest.TestCase):
    def test_standalone_model_dictionary_overrides_embedded_copy(self):
        class Archives:
            def read(self, path):
                return b"AAF\0" if path == "location.nl" else collection(translation=42)

            def locate(self, path):
                return SimpleNamespace(label="base" if path == "location.nl" else "patch")

        with patch.object(inspect_sources, "read_entity", return_value={"location.nl.mdic": collection()}):
            result = inspect_sources.inspect(Archives(), "location.nl")
        self.assertEqual(result["model_collections"][0]["archive"], "patch")
        self.assertEqual(result["model_collections"][0]["instances"][0]["world"][12], 42)

    def test_direct_dictionary_keeps_archive_provenance(self):
        class Archives:
            def read(self, path):
                return collection()

            def locate(self, path):
                return SimpleNamespace(label="patch")

        result = inspect_sources.inspect(Archives(), "location.nl.mdic")
        self.assertEqual(result["model_collections"][0]["archive"], "patch")

    def test_missing_path_has_actionable_error(self):
        archives = SimpleNamespace(read=lambda path: None, locate=lambda path: None)
        with self.assertRaisesRegex(FileNotFoundError, "not found in the base or patch archives"):
            inspect_sources.inspect(archives, "missing.nl")


if __name__ == "__main__":
    unittest.main()
