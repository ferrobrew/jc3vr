# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///

import copy
import json
import struct
import unittest
from unittest.mock import patch

import numpy as np
from PIL import Image

if __package__:
    from tools import test_visual_compare as compare_tests
    from tools import visual_constants as constants
    from tools.visual_compare import Mode, load_sample
else:
    import test_visual_compare as compare_tests
    import visual_constants as constants
    from visual_compare import Mode, load_sample


def f32s(values):
    return {
        "values": list(values),
        "bits": [struct.unpack("<I", struct.pack("<f", value))[0] for value in values],
    }


def matrix(eye=0):
    return [
        f32s(
            [float(row == column) + (eye if column == 3 else 0) for column in range(4)]
        )
        for row in range(4)
    ]


def cb0(eye):
    return {
        "offset_view_projection": matrix(eye),
        "camera_position": f32s([float(eye), 2.0, 3.0, 1.0]),
    }


class NativeTests(unittest.TestCase):
    def test_bits_only_and_optional_values_are_native(self):
        value = f32s([0.0, -0.0, 1.0, -2.0])
        native = constants.F32s.from_wire(value, 4)
        self.assertEqual(native.bits, (0, 0x80000000, 0x3F800000, 0xC0000000))
        self.assertEqual(constants.F32s.from_wire({"bits": value["bits"]}, 4), native)
        self.assertEqual(native.values, (0.0, -0.0, 1.0, -2.0))
        value["values"][1] = 0.0
        with self.assertRaisesRegex(ValueError, "disagrees"):
            constants.F32s.from_wire(value, 4)

    def test_inconsistent_nonfinite_wrong_shape_and_non_u32_are_incompatible(self):
        for value in (
            {"bits": [True]},
            {"bits": [-1]},
            {"bits": [2**32]},
            {"bits": [0x7F800000]},
            {"bits": [0x7FC00001]},
            {"bits": [0], "values": [None]},
            {"bits": [0], "values": [1]},
            {"bits": [0], "values": [1e100]},
            {"bits": []},
        ):
            with self.subTest(value=value), self.assertRaises((ValueError, TypeError)):
                constants.F32s.from_wire(value, 1)

    def test_ordered_ulp_for_positive_negative_subnormal_and_signed_zero(self):
        for a, b in (
            (0x3F800000, 0x3F800001),
            (0xBF800000, 0xBF800001),
            (0, 1),
            (0x80000000, 0x80000001),
            (0, 0x80000000),
        ):
            self.assertEqual(
                abs(constants.ordered_f32(a) - constants.ordered_f32(b)), 1
            )


class ConstantsTests(unittest.TestCase):
    def setUp(self):
        self.fixture = compare_tests.VisualCompareTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.output = self.root / "constants-report"
        self.path = self.fixture.path
        self.manifest = self.fixture.manifest
        for mode in ("multipass", "single_pass"):
            for index in range(3):
                self.make_constants(mode, index)

    def metadata(self, mode, index):
        return self.root / self.fixture.entries(mode)[index]["metadata"]

    def update(self, mode, index, change):
        path = self.metadata(mode, index)
        value = json.loads(path.read_text())
        change(value)
        path.write_text(json.dumps(value))

    def make_constants(self, mode, index):
        metadata = json.loads(self.metadata(mode, index).read_text())
        records = []
        for pass_id in (47, 53):
            for eye in (0,) if mode == "single_pass" else (0, 1):
                context = {
                    "view": f32s(
                        [
                            float(row == column) + eye
                            for row in range(4)
                            for column in range(4)
                        ]
                    ),
                    "projection": f32s(
                        [
                            float(row == column)
                            for row in range(4)
                            for column in range(4)
                        ]
                    ),
                    "offset_view_projection": f32s(
                        [
                            float(row == column) + eye
                            for row in range(4)
                            for column in range(4)
                        ]
                    ),
                    "camera_position": f32s([float(eye), 2.0, 3.0]),
                }
                shader = (
                    {
                        "source": "single_pass_cb13",
                        "cb13_status": "uploaded",
                        "centered_cb0": cb0(10),
                        "live_cb13": {
                            "eyes": [cb0(0), cb0(1)],
                            "reprojection": [matrix(), matrix(1)],
                        },
                    }
                    if mode == "single_pass"
                    else {"source": "multipass_cb0", "cb0": cb0(eye)}
                )
                records.append(
                    {
                        "frame": metadata["predicted_display_time_ns"],
                        "generation": metadata["visual"]["generation"],
                        "mode": mode,
                        "physical_eye": "left" if eye == 0 else "right",
                        "pass": pass_id,
                        "render_context": context,
                        "constants": shader,
                    }
                )
        self.update(
            mode,
            index,
            lambda value: value.update(
                constants={"records": records, "unscoped_uploads": 0}
            ),
        )

    def change_record(self, mode, index, change, pass_id=47, eye="left"):
        def update(value):
            record = next(
                record
                for record in value["constants"]["records"]
                if record["pass"] == pass_id and record["physical_eye"] == eye
            )
            change(record)

        self.update(mode, index, update)

    def analyze(self, include_values=False):
        self.path.write_text(json.dumps(self.manifest))
        return constants.analyze_manifest(
            self.path, self.output, include_values=include_values
        )

    def comparison(self, report, pass_id=47, eye=constants.PhysicalEye.LEFT):
        return next(
            item
            for item in report.cases[0].comparisons
            if item.pass_id == pass_id and item.physical_eye == eye
        )

    def test_effective_actual_rows_match_both_eyes_despite_centered_context(self):
        report = self.analyze(include_values=True)
        self.assertEqual(report.status, "measured")
        self.assertEqual(len(report.cases[0].comparisons), 4)
        self.assertEqual(len(report.cases[0].repeatability), 8)
        for comparison in report.cases[0].comparisons:
            self.assertEqual(len(comparison.pairs), 9)
            for pair in comparison.pairs:
                fields = {item.field: item for item in pair.fields}
                for name, count in (
                    (constants.FieldName.EFFECTIVE_VP, 16),
                    (constants.FieldName.EFFECTIVE_POSITION, 4),
                ):
                    metric = fields[name]
                    self.assertEqual(
                        (metric.count, metric.unequal, metric.maxulp, metric.maxabs),
                        (count, 0, 0, 0),
                    )
                    self.assertIsNotNone(metric.a_values)
                self.assertNotIn(constants.FieldName.CENTERED_VP, fields)
                self.assertNotEqual(pair.a.frame, pair.b.frame)
                self.assertNotEqual(pair.a.generation, pair.b.generation)
                self.assertEqual(pair.b.record_eye, "left")
        right = self.comparison(report, eye=constants.PhysicalEye.RIGHT)
        self.assertTrue(
            any(
                metric.unequal
                for metric in right.pairs[0].fields
                if metric.field == constants.FieldName.CONTEXT_VIEW
            )
        )
        self.assertTrue(
            all(mode.final_image_pairs == 3 for mode in report.cases[0].modes)
        )
        wire = json.loads((self.output / "constants.json").read_text())
        self.assertEqual(wire, report.to_wire())
        self.assertNotIn(
            "pass",
            (self.output / "constants.json")
            .read_text()
            .split('"status": ')[1]
            .splitlines()[0],
        )

    def test_one_ulp_cross_difference_is_measured_without_threshold(self):
        for index in range(3):

            def change(record):
                row = record["constants"]["live_cb13"]["eyes"][0][
                    "offset_view_projection"
                ][0]
                row["bits"][0] += 1
                row["values"][0] = struct.unpack(
                    "<f", struct.pack("<I", row["bits"][0])
                )[0]

            self.change_record("single_pass", index, change)
        report = self.analyze()
        self.assertEqual(report.status, "measured")
        metric = self.comparison(report).pairs[0].fields[0]
        self.assertEqual((metric.count, metric.unequal, metric.maxulp), (16, 1, 1))
        self.assertEqual(metric.maxabs, 2**-23)
        self.assertIsNone(metric.a_values)

    def test_constant_repeatability_all_pairs_and_noise_are_separate(self):
        self.change_record(
            "multipass", 2, lambda record: record["constants"].update(cb0=cb0(2))
        )
        report = self.analyze()
        self.assertEqual(report.status, "measured")
        repeat = next(
            item
            for item in report.cases[0].repeatability
            if item.pass_id == 47
            and item.physical_eye == "left"
            and item.modes == (Mode.MULTIPASS,)
        )
        self.assertEqual([pair.fields[0].unequal for pair in repeat.pairs], [0, 4, 4])
        noise = self.comparison(report).within_mode[0]
        self.assertEqual(noise.unequal, 4)
        self.assertGreater(noise.maxabs, 0)

    def test_out_of_range_passes_are_inconclusive(self):
        for pass_id in (0, 46, 85):
            with self.subTest(pass_id=pass_id):
                self.make_constants("single_pass", 0)
                self.change_record(
                    "single_pass",
                    0,
                    lambda record, pass_id=pass_id: record.update({"pass": pass_id}),
                )
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                self.assertTrue(
                    all(not item.pairs for item in report.cases[0].comparisons)
                )

    def test_invalid_and_stale_records_are_inconclusive(self):
        changes = (
            lambda record: record.update(frame=-1),
            lambda record: record.update(generation=999),
            lambda record: record.update(mode="multipass"),
            lambda record: record.update(physical_eye="right"),
            lambda record: record.update(**{"pass": True}),
            lambda record: record.update(**{"pass": -1}),
            lambda record: record["constants"].update(live_cb13=None),
            lambda record: record["constants"].update(
                source="multipass_cb0", cb0=cb0(0)
            ),
            lambda record: record["render_context"]["view"]["bits"].__setitem__(
                0, 0x7FC00000
            ),
            lambda record: record["constants"]["live_cb13"]["eyes"][0][
                "camera_position"
            ]["values"].__setitem__(0, 100),
        )
        for change in changes:
            with self.subTest(change=change):
                self.make_constants("single_pass", 0)
                self.change_record("single_pass", 0, change)
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                self.assertTrue(
                    all(not item.pairs for item in report.cases[0].comparisons)
                )

    def test_missing_duplicate_records_and_missing_mode_are_inconclusive(self):
        changes = (
            lambda value: value.pop("constants"),
            lambda value: value["constants"].update(records=[]),
            lambda value: value["constants"]["records"].append(
                copy.deepcopy(value["constants"]["records"][0])
            ),
            lambda value: value["constants"]["records"].pop(),
        )
        for change in changes:
            with self.subTest(change=change):
                self.make_constants("single_pass", 0)
                self.update("single_pass", 0, change)
                self.assertEqual(self.analyze().status, "inconclusive")
        del self.manifest["cases"][0]["modes"]["single_pass"]
        self.assertEqual(self.analyze().status, "inconclusive")

    def test_unscoped_uploads_are_counted_without_invalidating_scoped_records(self):
        self.update(
            "multipass", 0, lambda value: value["constants"].update(unscoped_uploads=2)
        )
        self.update(
            "single_pass",
            0,
            lambda value: value["constants"].update(unscoped_uploads=1),
        )
        report = self.analyze()
        self.assertEqual(report.status, "measured")
        self.assertEqual(
            [mode.captures[0].unscoped_uploads for mode in report.cases[0].modes],
            [2, 1],
        )
        self.assertIn(
            "Ignored unscoped uploads", (self.output / "constants.html").read_text()
        )

    def test_missing_or_failed_cb13_upload_status_never_infers_uploaded_rows(self):
        for status in (None, "upload_unavailable", "live_rows_unavailable", "unknown"):
            with self.subTest(status=status):
                self.make_constants("single_pass", 0)
                if status is None:
                    self.change_record(
                        "single_pass",
                        0,
                        lambda record: record["constants"].pop("cb13_status"),
                    )
                else:
                    self.change_record(
                        "single_pass",
                        0,
                        lambda record, status=status: record["constants"].update(
                            cb13_status=status
                        ),
                    )
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                self.assertTrue(
                    all(not item.pairs for item in report.cases[0].comparisons)
                )

    def test_alignment_uses_pass_and_physical_eye_not_record_order(self):
        for index in range(3):
            self.update(
                "multipass",
                index,
                lambda value: value["constants"]["records"].reverse(),
            )
        self.assertEqual(self.analyze().status, "measured")
        for index in range(3):
            self.update(
                "single_pass",
                index,
                lambda value: value["constants"]["records"][1].update(**{"pass": 54}),
            )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertEqual(self.comparison(report, pass_id=53).status, "inconclusive")
        self.assertEqual(self.comparison(report, pass_id=47).status, "measured")

    def test_final_image_repeatability_blocks_constants_but_stable_cross_images_do_not(
        self,
    ):
        path = self.root / self.fixture.entries("single_pass")[2]["png"]
        with Image.open(path) as image:
            pixels = np.array(image)
        Image.fromarray(255 - pixels).save(path)
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertEqual(report.cases[0].modes[1].final_image_status, "inconclusive")
        for index in range(3):
            Image.fromarray(255 - pixels).save(
                self.root / self.fixture.entries("single_pass")[index]["png"]
            )
        self.assertEqual(self.analyze().status, "measured")

    def test_capture_provenance_and_input_mismatch_are_not_bypassed(self):
        self.update(
            "single_pass", 0, lambda value: value.update(fresh_eyes=[False, True])
        )
        self.assertEqual(self.analyze().status, "inconclusive")
        self.update(
            "single_pass", 0, lambda value: value.update(fresh_eyes=[True, True])
        )
        for index in range(3):
            self.update(
                "single_pass",
                index,
                lambda value: value["comparison_inputs"]["camera"][
                    "position"
                ].__setitem__(0, 3),
            )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertTrue(
            any(
                "comparison inputs" in error for error in self.comparison(report).errors
            )
        )

    def test_reused_capture_paths_ids_and_frames_are_not_independent(self):
        original = copy.deepcopy(self.fixture.entries("single_pass"))
        self.fixture.entries("single_pass")[2] = copy.deepcopy(original[0])
        self.assertEqual(self.analyze().status, "inconclusive")
        self.manifest["cases"][0]["modes"]["single_pass"] = original
        for field in ("id", "frame_ordinal", "predicted_display_time_ns"):
            before = self.metadata("single_pass", 2).read_text()
            value = json.loads(self.metadata("single_pass", 0).read_text())[field]
            self.update(
                "single_pass",
                2,
                lambda metadata, field=field, value=value: metadata.update(
                    {field: value}
                ),
            )
            self.assertEqual(self.analyze().status, "inconclusive")
            self.metadata("single_pass", 2).write_text(before)

    def test_reread_substitution_with_same_frame_is_rejected(self):
        mode = Mode.SINGLE_PASS
        self.path.write_text(json.dumps(self.manifest))
        manifest = constants.Manifest.from_wire(self.manifest)
        group = next(group for group in manifest.cases[0].modes if group.mode == mode)
        sample = load_sample(self.root, mode, 0, group.samples[0])
        self.update(
            "single_pass",
            0,
            lambda value: value["comparison_inputs"]["camera"]["position"].__setitem__(
                0, 1
            ),
        )
        with self.assertRaisesRegex(ValueError, "reread capture differs"):
            constants.load_constants(self.root, mode, group.samples[0], sample)

    def test_reverse_multipass_return_generation_is_measured(self):
        self.fixture.add_mode("reverse_multipass")
        for index in range(3):
            self.make_constants("reverse_multipass", index)
        report = self.analyze()
        self.assertEqual(report.status, "measured")
        self.assertEqual(len(report.cases[0].comparisons), 12)

    def test_reports_escape_html_and_do_not_change_primary_artifacts(self):
        self.manifest["cases"][0]["name"] = "<script>unsafe</script>"
        sentinel = self.output / "report.json"
        sentinel.parent.mkdir()
        sentinel.write_text("primary report")
        self.analyze()
        text = (self.output / "constants.html").read_text()
        self.assertNotIn("<script>unsafe</script>", text)
        self.assertIn("&lt;script&gt;unsafe&lt;/script&gt;", text)
        self.assertIn("center context", text)
        self.assertIn("not bitwise or visual equivalence", text)
        self.assertEqual(sentinel.read_text(), "primary report")

    def test_cli_success_inconclusive_invalid_manifest_and_output_failure(self):
        self.path.write_text(json.dumps(self.manifest))
        args = [str(self.path), "--output", str(self.output), "--include-values"]
        with patch("sys.stderr"):
            self.assertEqual(constants.main(args), 0)
            self.update("single_pass", 0, lambda value: value.pop("constants"))
            self.assertEqual(constants.main(args), 2)
            self.path.write_text("not JSON")
            self.assertEqual(constants.main(args), 2)
            with patch.object(
                constants, "write_reports", side_effect=OSError("unwritable")
            ):
                self.assertEqual(constants.main(args), 2)


if __name__ == "__main__":
    unittest.main()
