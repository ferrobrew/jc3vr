import copy
import json
import struct
import unittest
from unittest.mock import patch

import numpy as np
from PIL import Image

if __package__:
    from tools import test_visual_compare as compare_tests
    from tools import visual_stages as stages
else:
    import test_visual_compare as compare_tests
    import visual_stages as stages


class FormatTests(unittest.TestCase):
    def decode(self, data, format_name, width=1):
        source = stages.Source(width, 1, stages.DxgiFormat.from_wire(format_name))
        return stages.decode_raw(data, source)

    def test_explicit_dxgi_strings_and_numeric_debug_format(self):
        for item in stages.DxgiFormat:
            with self.subTest(format=item):
                self.assertEqual(stages.DxgiFormat.from_wire(item.name), item)
                self.assertEqual(
                    stages.DxgiFormat.from_wire("DXGI_FORMAT_" + item.name), item
                )
                self.assertEqual(
                    stages.DxgiFormat.from_wire(f"DXGI_FORMAT({item.value})"), item
                )
        for value in (
            "DXGI_FORMAT(999)",
            "RGBA",
            "DXGI_FORMAT_R16G16_FLOAT",
            28,
            True,
            None,
        ):
            with self.subTest(value=value), self.assertRaises((ValueError, TypeError)):
                stages.DxgiFormat.from_wire(value)

    def test_rgba_bgra_preserve_code_values_and_alpha(self):
        for format_name, values in (
            ("R8G8B8A8_UNORM", [1, 2, 3, 4]),
            ("R8G8B8A8_TYPELESS", [1, 2, 3, 4]),
            ("R8G8B8A8_UNORM_SRGB", [1, 2, 3, 4]),
            ("B8G8R8A8_UNORM", [3, 2, 1, 4]),
            ("B8G8R8A8_UNORM_SRGB", [3, 2, 1, 4]),
        ):
            with self.subTest(format=format_name):
                bits, decoded, channels = self.decode(bytes([1, 2, 3, 4]), format_name)
                self.assertEqual(bits.tolist(), [[[1, 2, 3, 4]]])
                self.assertEqual(decoded.tolist(), [[values]])
                self.assertEqual(channels, ("r", "g", "b", "a"))

    def test_r10g10b10a2_is_not_four_byte_channels(self):
        packed = 1023 | (512 << 10) | (17 << 20) | (2 << 30)
        for format_name in (
            "R10G10B10A2_UNORM",
            "R10G10B10A2_TYPELESS",
            "R10G10B10A2_UINT",
        ):
            with self.subTest(format=format_name):
                _, values, _ = self.decode(struct.pack("<I", packed), format_name)
                self.assertEqual(values.tolist(), [[[1023, 512, 17, 2]]])

    def test_depth_is_eight_bytes_and_only_first_float_is_depth(self):
        for format_name in (
            "R32G8X24_TYPELESS",
            "D32_FLOAT_S8X24_UINT",
            "R32_FLOAT_X8X24_TYPELESS",
        ):
            with self.subTest(format=format_name):
                bits, values, channels = self.decode(
                    struct.pack("<fI", 0.75, 0xFFFFFFFF), format_name
                )
                self.assertEqual(bits.shape, (1, 1, 8))
                self.assertEqual(values.tolist(), [[[0.75]]])
                self.assertEqual(channels, ("depth",))

    def test_r11g11b10_normal_subnormal_and_zero(self):
        word = (15 << 6) | ((16 << 6) << 11) | ((14 << 5) << 22)
        _, values, _ = self.decode(struct.pack("<I", word), "R11G11B10_FLOAT")
        self.assertEqual(values.tolist(), [[[1.0, 2.0, 0.5]]])
        word = 1 | (1 << 11) | (1 << 22)
        _, values, _ = self.decode(struct.pack("<I", word), "R11G11B10_FLOAT")
        self.assertEqual(values.tolist(), [[[2.0**-20, 2.0**-20, 2.0**-19]]])
        _, values, _ = self.decode(bytes(4), "R11G11B10_FLOAT")
        self.assertEqual(values.tolist(), [[[0.0, 0.0, 0.0]]])

    def test_nonfinite_depth_or_packed_float_is_incompatible(self):
        for data, format_name in (
            (struct.pack("<fI", float("nan"), 0), "R32G8X24_TYPELESS"),
            (struct.pack("<fI", float("inf"), 0), "D32_FLOAT_S8X24_UINT"),
            (struct.pack("<I", 31 << 6), "R11G11B10_FLOAT"),
            (struct.pack("<I", (31 << 6) | 1), "R11G11B10_FLOAT"),
        ):
            with (
                self.subTest(format=format_name),
                self.assertRaisesRegex(ValueError, "nonfinite"),
            ):
                self.decode(data, format_name)

    def test_strict_length_rejects_short_long_and_row_padding(self):
        for format_name, length in (("R8G8B8A8_UNORM", 4), ("R32G8X24_TYPELESS", 8)):
            for size in (0, length - 1, length + 1, length + 4):
                with (
                    self.subTest(format=format_name, size=size),
                    self.assertRaisesRegex(ValueError, "exactly"),
                ):
                    self.decode(bytes(size), format_name)


class StageTests(unittest.TestCase):
    def setUp(self):
        self.fixture = compare_tests.VisualCompareTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.output = self.root / "stages"
        self.path = self.fixture.path
        self.manifest = self.fixture.manifest
        for mode in ("multipass", "single_pass"):
            for index in range(3):
                self.make_stages(mode, index)

    def metadata(self, mode, index):
        return self.root / self.fixture.entries(mode)[index]["metadata"]

    def update(self, mode, index, change):
        path = self.metadata(mode, index)
        value = json.loads(path.read_text())
        change(value)
        path.write_text(json.dumps(value))

    def pixels(self, stage, eye):
        if stage.depth:
            words = np.zeros((2, 3, 2), dtype="<u4")
            words[..., 0] = np.float32(0.25 + 0.25 * eye).view(np.uint32)
            return words.view(np.uint8).reshape(2, 3, 8).copy(), "DXGI_FORMAT(19)"
        if stage == stages.Stage.MAIN_COLOR:
            word = (
                ((15 + eye) << 6)
                | (((15 + eye) << 6) << 11)
                | (((15 + eye) << 5) << 22)
            )
            return np.full((2, 3), word, dtype="<u4").view(np.uint8).reshape(
                2, 3, 4
            ), "DXGI_FORMAT(26)"
        if stage == stages.Stage.GBUFFER1:
            word = (200 + eye) | ((300 + eye) << 10) | ((400 + eye) << 20) | (3 << 30)
            return np.full((2, 3), word, dtype="<u4").view(np.uint8).reshape(
                2, 3, 4
            ), "DXGI_FORMAT(23)"
        format_name = "DXGI_FORMAT(27)"
        return np.full((2, 3, 4), 20 + 50 * eye, dtype=np.uint8), format_name

    def stage_records(self, mode, index, stage, metadata):
        records = []
        for record_eye in (0,) if mode == "single_pass" else (0, 1):
            pixels, format_name = self.pixels(stage, record_eye)
            if mode == "single_pass":
                pixels = np.concatenate((pixels, self.pixels(stage, 1)[0]), axis=1)
            is_png = stage in (stages.Stage.PRE_AA, stages.Stage.POST_AA)
            filename = f"{mode}-{index}.{stage}.eye{record_eye}.{'png' if is_png else 'raw'}"
            if is_png:
                pixels[..., 3] = 255
                Image.fromarray(pixels).save(self.root / filename)
            else:
                (self.root / filename).write_bytes(pixels.tobytes())
            records.append(
                {
                    "stage": stage,
                    "eye": record_eye,
                    "frame": metadata["predicted_display_time_ns"],
                    "source": {
                        "width": pixels.shape[1],
                        "height": pixels.shape[0],
                        "format": format_name,
                    },
                    "raw": None if is_png else "C:\\captures\\" + filename,
                    "png": "C:\\captures\\" + filename if is_png else None,
                    "error": None,
                }
            )
        return records

    def make_stages(self, mode, index):
        metadata = json.loads(self.metadata(mode, index).read_text())
        records = []
        for stage in stages.Stage:
            if stage.optional:
                continue
            records.extend(self.stage_records(mode, index, stage, metadata))
        self.update(mode, index, lambda value: value.update(stages=records))

    def append_stage(self, mode, index, stage):
        metadata = json.loads(self.metadata(mode, index).read_text())
        records = self.stage_records(mode, index, stage, metadata)
        self.update(mode, index, lambda value: value["stages"].extend(records))

    def change_record(self, mode, index, change, stage=stages.Stage.GBUFFER0, eye=0):
        def update(value):
            record = next(
                record
                for record in value["stages"]
                if record["stage"] == stage and record["eye"] == eye
            )
            change(record)

        self.update(mode, index, update)

    def raw_path(self, mode, index, stage=stages.Stage.GBUFFER0, eye=0):
        metadata = self.metadata(mode, index)
        record = next(
            record
            for record in json.loads(metadata.read_text())["stages"]
            if record["stage"] == stage and record["eye"] == eye
        )
        return stages.artifact_path(metadata, record["raw"])

    def analyze(self):
        self.path.write_text(json.dumps(self.manifest))
        return stages.analyze_manifest(self.path, self.output)

    def comparison(self, report, stage=stages.Stage.GBUFFER0, eye=0):
        return next(
            check
            for check in report.cases[0].comparisons
            if check.stage == stage and check.eye == eye
        )

    def test_stable_all_stages_measure_whole_multipass_vs_sps_halves(self):
        report = self.analyze()
        self.assertEqual(report.status, stages.EvidenceStatus.MEASURED)
        self.assertEqual(len(report.cases[0].repeatability), 32)
        self.assertEqual(len(report.cases[0].comparisons), 16)
        for check in report.cases[0].comparisons:
            self.assertEqual(check.status, stages.EvidenceStatus.MEASURED)
            self.assertEqual(len(check.pairs), 9)
            self.assertEqual(check.within_mode_mae, 0)
            for pair in check.pairs:
                self.assertEqual(pair.metrics.pixels, 6)
                self.assertEqual(pair.metrics.exact_changed, 0)
                self.assertEqual(pair.metrics.mae, 0)
                self.assertEqual(pair.a.eye, check.eye)
                self.assertEqual(pair.a.record_eye, check.eye)
                self.assertEqual(pair.b.record_eye, 0)
                self.assertEqual(pair.a.source.width, 3)
                self.assertEqual(pair.b.source.width, 6)
        self.assertTrue((self.output / "stages.json").is_file())
        self.assertTrue((self.output / "stages.html").is_file())
        wire = json.loads((self.output / "stages.json").read_text())
        self.assertEqual(wire["status"], "measured")

    def test_water_depth_is_an_optional_diagnostic(self):
        report = self.analyze()
        self.assertEqual(report.status, stages.EvidenceStatus.MEASURED)
        self.assertFalse(
            any(
                check.stage.optional
                for check in report.cases[0].repeatability
                + report.cases[0].comparisons
            )
        )
        self.assertEqual(len(report.cases[0].repeatability), 32)
        self.assertEqual(len(report.cases[0].comparisons), 16)

        for mode in ("multipass", "single_pass"):
            for index in range(3):
                self.append_stage(mode, index, stages.Stage.WATER_DEPTH_BEFORE)
        report = self.analyze()
        self.assertEqual(report.status, stages.EvidenceStatus.MEASURED)
        self.assertEqual(
            len(
                [
                    check
                    for check in report.cases[0].repeatability
                    if check.stage == stages.Stage.WATER_DEPTH_BEFORE
                ]
            ),
            4,
        )
        depth_cross = [
            check
            for check in report.cases[0].comparisons
            if check.stage == stages.Stage.WATER_DEPTH_BEFORE
        ]
        self.assertEqual(len(depth_cross), 2)
        for check in depth_cross:
            self.assertEqual(check.status, stages.EvidenceStatus.MEASURED)
            self.assertEqual(len(check.pairs), 9)

    def test_water_depth_present_in_one_mode_only_skips_cross_mode(self):
        for index in range(3):
            self.append_stage(
                "multipass", index, stages.Stage.WATER_DEPTH_AFTER_LEFT
            )
        report = self.analyze()
        self.assertEqual(report.status, stages.EvidenceStatus.MEASURED)
        depth_repeats = [
            check
            for check in report.cases[0].repeatability
            if check.stage == stages.Stage.WATER_DEPTH_AFTER_LEFT
        ]
        self.assertEqual(len(depth_repeats), 2)
        self.assertTrue(
            all(check.modes == (stages.Mode.MULTIPASS,) for check in depth_repeats)
        )
        self.assertFalse(
            any(
                check.stage == stages.Stage.WATER_DEPTH_AFTER_LEFT
                for check in report.cases[0].comparisons
            )
        )

    def test_cross_difference_has_exact_native_channels_and_no_failure_threshold(self):
        for index in range(3):
            path = self.raw_path("single_pass", index)
            data = bytearray(path.read_bytes())
            data[0] += 4
            path.write_bytes(data)
        check = self.comparison(self.analyze())
        metric = check.pairs[0].metrics
        self.assertEqual(check.status, stages.EvidenceStatus.MEASURED)
        self.assertEqual(metric.exact_changed, 1)
        self.assertEqual(metric.value_changed, 1)
        self.assertEqual(metric.exact_changed_fraction, 1 / 6)
        self.assertEqual(metric.mae, 4 / 24)
        self.assertEqual(metric.maximum, 4)
        self.assertEqual(metric.channels[0].exact_changed, 1)
        self.assertEqual(metric.channels[0].mae, 4 / 6)
        self.assertEqual(metric.channels[1].maximum, 0)
        self.assertEqual(check.within_mode_mae, 0)
        self.assertEqual(check.cross_mae_minus_within_mode, metric.mae)

    def test_repeatability_checks_all_pairs_and_precedes_cross_mode(self):
        path = self.raw_path("single_pass", 2)
        data = bytearray(path.read_bytes())
        data[0] += 12
        path.write_bytes(data)
        report = self.analyze()
        repeat = next(
            check
            for check in report.cases[0].repeatability
            if check.stage == stages.Stage.GBUFFER0
            and check.eye == 0
            and check.modes == (stages.Mode.SINGLE_PASS,)
        )
        self.assertEqual([pair.metrics.maximum for pair in repeat.pairs], [0, 12, 12])
        cross = self.comparison(report)
        self.assertEqual(cross.within_mode_mae, 0.5)
        self.assertEqual(cross.within_mode_maximum, 12)
        self.assertEqual(cross.cross_mae_minus_within_mode, 0)
        page = (self.output / "stages.html").read_text()
        self.assertLess(
            page.index("Same-mode repeatability"), page.index("Cross-mode comparisons")
        )
        self.assertIn("not visual equivalence", page)

    def test_depth_retains_stencil_bits_separately_from_float_error(self):
        for index in range(3):
            path = self.raw_path("single_pass", index, stages.Stage.GEOMETRY_DEPTH)
            data = bytearray(path.read_bytes())
            data[4] = 9
            path.write_bytes(data)
        check = self.comparison(self.analyze(), stages.Stage.GEOMETRY_DEPTH)
        metric = check.pairs[0].metrics
        self.assertEqual(metric.exact_changed, 1)
        self.assertEqual(metric.value_changed, 0)
        self.assertEqual(metric.mae, 0)
        self.assertEqual(metric.depth.covered_a, 6)
        self.assertEqual(metric.depth.coverage_changed, 0)

    def test_depth_coverage_and_both_covered_error(self):
        for index in range(3):
            path = self.raw_path("single_pass", index, stages.Stage.GEOMETRY_DEPTH)
            data = bytearray(path.read_bytes())
            data[0:4] = struct.pack("<f", 0)
            data[8:12] = struct.pack("<f", 0.5)
            path.write_bytes(data)
        metric = (
            self.comparison(self.analyze(), stages.Stage.GEOMETRY_DEPTH)
            .pairs[0]
            .metrics
        )
        self.assertEqual(metric.mae, 0.5 / 6)
        self.assertEqual(metric.maximum, 0.25)
        self.assertEqual(metric.depth.covered_a, 6)
        self.assertEqual(metric.depth.covered_b, 5)
        self.assertEqual(metric.depth.coverage_changed, 1)
        self.assertEqual(metric.depth.both_covered, 5)
        self.assertEqual(metric.depth.covered_mae, 0.25 / 5)
        self.assertEqual(metric.depth.covered_maximum, 0.25)

    def test_invalid_capture_blocks_stage_loading(self):
        self.update("multipass", 0, lambda value: value["visual"].update(ready=False))
        original = stages.load_stage
        with patch.object(stages, "load_stage", wraps=original) as loader:
            report = self.analyze()
        self.assertFalse(
            any(call.args[1] == stages.Mode.MULTIPASS for call in loader.call_args_list)
        )
        check = self.comparison(report)
        self.assertEqual(check.status, stages.EvidenceStatus.INCONCLUSIVE)
        self.assertFalse(check.pairs)

    def test_incompatible_stage_records_are_inconclusive(self):
        for change, expected in (
            (lambda record: record.update(frame=1), "stage.frame"),
            (lambda record: record.update(eye=1), "expected one record"),
            (lambda record: record.update(eye=False), "expected one record"),
            (lambda record: record.update(error="readback failed"), "readback failed"),
            (
                lambda record: record.update(raw=None, png="only.png"),
                "unsupported stage",
            ),
            (lambda record: record["source"].update(width=5), "two equal halves"),
            (
                lambda record: record["source"].update(format="DXGI_FORMAT(99)"),
                "unsupported",
            ),
            (
                lambda record: record["source"].update(format="DXGI_FORMAT(19)"),
                "incompatible",
            ),
        ):
            with self.subTest(expected=expected):
                self.make_stages("single_pass", 0)
                self.change_record("single_pass", 0, change)
                report = self.analyze()
                self.assertEqual(
                    self.comparison(report).status, stages.EvidenceStatus.INCONCLUSIVE
                )
                errors = [
                    error
                    for check in report.cases[0].repeatability
                    for error in check.errors
                ]
                self.assertTrue(any(expected in error for error in errors), errors)

    def test_actual_dimensions_and_format_mismatch_are_not_resized_or_reinterpreted(
        self,
    ):
        self.change_record(
            "single_pass",
            0,
            lambda record: record["source"].update(format="DXGI_FORMAT(87)"),
        )
        report = self.analyze()
        self.assertEqual(
            self.comparison(report).status, stages.EvidenceStatus.INCONCLUSIVE
        )
        self.assertTrue(
            any(
                "formats differ" in error
                for check in report.cases[0].repeatability
                for error in check.errors
            )
        )
        self.make_stages("single_pass", 0)
        for index in range(3):
            self.change_record(
                "single_pass",
                index,
                lambda record: record["source"].update(width=12, height=1),
            )
        check = self.comparison(self.analyze())
        self.assertEqual(check.status, stages.EvidenceStatus.INCONCLUSIVE)
        self.assertTrue(any("dimensions differ" in error for error in check.errors))

    def test_metadata_input_mismatch_prevents_cross_mode_metrics(self):
        for index in range(3):
            self.update(
                "single_pass",
                index,
                lambda value: value["comparison_inputs"]["camera"][
                    "position"
                ].__setitem__(0, 1),
            )
        check = self.comparison(self.analyze())
        self.assertEqual(check.status, stages.EvidenceStatus.INCONCLUSIVE)
        self.assertFalse(check.pairs)
        self.assertTrue(any("camera.position" in error for error in check.errors))

    def test_missing_duplicate_and_short_artifacts_are_inconclusive(self):
        self.raw_path("multipass", 0).unlink()
        self.assertEqual(
            self.comparison(self.analyze()).status, stages.EvidenceStatus.INCONCLUSIVE
        )
        self.make_stages("multipass", 0)
        path = self.raw_path("multipass", 0)
        path.write_bytes(path.read_bytes() + b"x")
        self.assertEqual(
            self.comparison(self.analyze()).status, stages.EvidenceStatus.INCONCLUSIVE
        )
        self.make_stages("multipass", 0)
        self.update(
            "multipass",
            0,
            lambda value: value["stages"].append(copy.deepcopy(value["stages"][2])),
        )
        self.assertEqual(
            self.comparison(self.analyze()).status, stages.EvidenceStatus.INCONCLUSIVE
        )

    def test_missing_modes_stages_or_repeats_are_inconclusive(self):
        modes = self.manifest["cases"][0]["modes"]
        saved = modes.pop("single_pass")
        self.assertEqual(self.analyze().status, stages.EvidenceStatus.INCONCLUSIVE)
        modes["single_pass"] = saved[:2]
        self.assertEqual(self.analyze().status, stages.EvidenceStatus.INCONCLUSIVE)
        modes["single_pass"] = saved
        self.update("single_pass", 0, lambda value: value.pop("stages"))
        self.assertEqual(self.analyze().status, stages.EvidenceStatus.INCONCLUSIVE)

    def test_reused_capture_or_raw_path_is_not_repeatability_evidence(self):
        modes = self.manifest["cases"][0]["modes"]
        saved = modes["multipass"][2]
        modes["multipass"][2] = modes["multipass"][1]
        self.assertEqual(self.analyze().status, stages.EvidenceStatus.INCONCLUSIVE)
        modes["multipass"][2] = saved
        path = self.raw_path("multipass", 0)
        self.change_record("multipass", 1, lambda record: record.update(raw=str(path)))
        report = self.analyze()
        self.assertEqual(
            self.comparison(report).status, stages.EvidenceStatus.INCONCLUSIVE
        )
        self.assertTrue(
            any(
                "reused artifact" in error
                for check in report.cases[0].repeatability
                for error in check.errors
            )
        )

    def test_artifact_paths_use_local_basename_not_recorded_machine_path(self):
        metadata = self.root / "nested" / "capture.json"
        for recorded in (
            r"C:\game\captures\stage.raw",
            "/remote/game/stage.raw",
            "stage.raw",
        ):
            self.assertEqual(
                stages.artifact_path(metadata, recorded),
                self.root / "nested" / "stage.raw",
            )
        for recorded in (
            "",
            "..",
            "../stage.raw",
            "nested/../stage.raw",
            "stage.raw/",
            "stage.raw\\",
            "bad\x00raw",
            "C:",
        ):
            with self.subTest(recorded=recorded), self.assertRaises(ValueError):
                stages.artifact_path(metadata, recorded)
        local = self.root / "nested"
        local.mkdir()
        (local / "escape.raw").symlink_to(self.root / "outside.raw")
        with self.assertRaises(ValueError):
            stages.artifact_path(metadata, "escape.raw")

    def test_primary_report_and_manifest_remain_unchanged_and_html_is_escaped(self):
        sentinel = self.output / "report.json"
        self.output.mkdir()
        sentinel.write_text('{"status":"fail"}')
        self.manifest["cases"][0]["name"] = "<script>bad()</script>"
        self.path.write_text(json.dumps(self.manifest))
        before = self.path.read_bytes()
        self.analyze()
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(sentinel.read_text(), '{"status":"fail"}')
        page = (self.output / "stages.html").read_text()
        self.assertNotIn("<script>bad()", page)
        self.assertIn("&lt;script&gt;", page)

    def test_same_timestamp_metadata_substitution_cannot_supply_stage_evidence(self):
        mode = stages.Mode.MULTIPASS
        spec = stages.SampleSpec.from_wire(self.fixture.entries(mode)[0])
        sample = stages.load_sample(self.root, mode, 0, spec)
        path = self.metadata(mode, 0)
        original = json.loads(path.read_text())
        substitutions = (
            lambda value: value.update(id="substituted-capture"),
            lambda value: value.update(frame_ordinal=value["frame_ordinal"] + 100),
            lambda value: value["visual"].update(generation=99),
            lambda value: value["visual"].update(ready=False),
            lambda value: value["visual"].update(completed_frames=99),
            lambda value: value.update(fresh_eyes=[False, True]),
            lambda value: value.update(rendered_eyes=[True, False]),
            lambda value: value.update(selected_eye=0),
            lambda value: value.update(mode="single_pass"),
            lambda value: value["comparison_inputs"]["camera"]["position"].__setitem__(
                0, 0.000001
            ),
            lambda value: value.update(width=value["width"] + 2),
        )
        for index, substitute in enumerate(substitutions):
            with self.subTest(substitution=index):
                changed = copy.deepcopy(original)
                substitute(changed)
                self.assertEqual(
                    changed["predicted_display_time_ns"],
                    sample.predicted_display_time_ns,
                )
                path.write_text(json.dumps(changed))
                with self.assertRaises((ValueError, TypeError)):
                    stages.load_stage(
                        self.root, mode, spec, sample, stages.Stage.GBUFFER0, 0
                    )
        path.write_text(json.dumps(original))
        self.assertEqual(
            stages.load_stage(
                self.root, mode, spec, sample, stages.Stage.GBUFFER0, 0
            ).identity.capture_id,
            sample.capture_id,
        )

    def test_depth_meaningful_bytes_follow_the_descriptor_stencil_or_x8_layout(self):
        for format_id in (19, 20, 21):
            for byte_index in range(4, 8):
                with self.subTest(format_id=format_id, byte_index=byte_index):
                    for mode in ("multipass", "single_pass"):
                        for index in range(3):
                            self.make_stages(mode, index)
                            for eye in (0,) if mode == "single_pass" else (0, 1):
                                self.change_record(
                                    mode,
                                    index,
                                    lambda record, format_id=format_id: record[
                                        "source"
                                    ].update(format=f"DXGI_FORMAT({format_id})"),
                                    stage=stages.Stage.GEOMETRY_DEPTH,
                                    eye=eye,
                                )
                            if mode == "single_pass":
                                path = self.raw_path(
                                    mode, index, stages.Stage.GEOMETRY_DEPTH
                                )
                                data = bytearray(path.read_bytes())
                                data[byte_index] = 9
                                path.write_bytes(data)
                    metric = (
                        self.comparison(self.analyze(), stages.Stage.GEOMETRY_DEPTH)
                        .pairs[0]
                        .metrics
                    )
                    self.assertEqual(
                        metric.exact_changed, int(byte_index == 4 and format_id != 21)
                    )
                    self.assertEqual(metric.packed_byte_changed, 1)
                    self.assertEqual(metric.value_changed, 0)
                    self.assertEqual(metric.mae, 0)

    def test_depth_padding_is_not_meaningful_but_stored_byte_change_is_retained(self):
        for index in range(3):
            path = self.raw_path("single_pass", index, stages.Stage.GEOMETRY_DEPTH)
            data = bytearray(path.read_bytes())
            data[5:8] = bytes([7, 8, 9])
            path.write_bytes(data)
        metric = (
            self.comparison(self.analyze(), stages.Stage.GEOMETRY_DEPTH)
            .pairs[0]
            .metrics
        )
        self.assertEqual(metric.exact_changed, 0)
        self.assertEqual(metric.packed_byte_changed, 1)
        self.assertEqual(metric.value_changed, 0)
        self.assertEqual(metric.mae, 0)

    def test_png_typeless_descriptors_preserve_encoded_rgb_without_byte_swapping(self):
        for format_id in (27, 90):
            with self.subTest(format_id=format_id):
                for mode in ("multipass", "single_pass"):
                    for index in range(3):

                        def update(value, format_id=format_id, mode=mode, index=index):
                            for record in value["stages"]:
                                if record["stage"] in (
                                    stages.Stage.PRE_AA,
                                    stages.Stage.POST_AA,
                                ):
                                    record["source"]["format"] = (
                                        f"DXGI_FORMAT({format_id})"
                                    )
                                    path = stages.artifact_path(
                                        self.metadata(mode, index), record["png"]
                                    )
                                    with Image.open(path) as image:
                                        pixels = np.array(image)
                                    pixels[..., :3] = [7, 31, 109]
                                    Image.fromarray(pixels).save(path)

                        self.update(mode, index, update)
                report = self.analyze()
                self.assertEqual(report.status, stages.EvidenceStatus.MEASURED)
                for stage in (stages.Stage.PRE_AA, stages.Stage.POST_AA):
                    for eye in (0, 1):
                        check = self.comparison(report, stage, eye)
                        self.assertEqual(len(check.pairs), 9)
                        self.assertTrue(
                            all(pair.metrics.exact_changed == 0 for pair in check.pairs)
                        )
                        self.assertEqual(check.pairs[0].a.encoding, "png")
                        self.assertEqual(
                            check.pairs[0].a.source.format.value, format_id
                        )

    def test_png_stages_use_rgb_codes_and_reject_nonopaque_alpha_or_wrong_shape(self):
        metadata = self.metadata("single_pass", 0)
        record = next(
            record
            for record in json.loads(metadata.read_text())["stages"]
            if record["stage"] == stages.Stage.PRE_AA
        )
        path = stages.artifact_path(metadata, record["png"])
        with Image.open(path) as image:
            pixels = np.array(image)
        self.assertEqual(
            self.comparison(self.analyze(), stages.Stage.PRE_AA)
            .pairs[0]
            .metrics.channels[-1]
            .channel,
            "b",
        )
        pixels[0, 0, 3] = 1
        Image.fromarray(pixels).save(path)
        self.assertEqual(
            self.comparison(self.analyze(), stages.Stage.PRE_AA).status,
            stages.EvidenceStatus.INCONCLUSIVE,
        )
        pixels[0, 0, 3] = 255
        Image.fromarray(pixels[:, :-1]).save(path)
        self.assertEqual(
            self.comparison(self.analyze(), stages.Stage.PRE_AA).status,
            stages.EvidenceStatus.INCONCLUSIVE,
        )

    def test_final_image_repeatability_blocks_cross_stage_evidence_when_unstable(self):
        path = self.root / self.fixture.entries("multipass")[2]["png"]
        with Image.open(path) as image:
            pixels = np.array(image)
        pixels[:] = 255
        Image.fromarray(pixels).save(path)
        report = self.analyze()
        self.assertEqual(report.status, stages.EvidenceStatus.INCONCLUSIVE)
        self.assertFalse(self.comparison(report).pairs)
        self.assertTrue(
            any(
                "final-image repeatability is unstable" in error
                for check in report.cases[0].repeatability
                for error in check.errors
            )
        )

    def test_multipass_return_generation_is_included_in_all_pair_noise(self):
        original = list(self.fixture.entries("multipass"))
        for index, entry in enumerate(original):
            name = f"multipass-return-{index}"
            source = json.loads((self.root / entry["metadata"]).read_text())
            source.update(
                id=name,
                frame_ordinal=100 + index,
                predicted_display_time_ns=(100 + index) * 11_111_111,
            )
            source["visual"]["generation"] = 8
            (self.root / f"{name}.json").write_text(json.dumps(source))
            (self.root / f"{name}.png").write_bytes(
                (self.root / entry["png"]).read_bytes()
            )
            self.fixture.entries("multipass").append(
                {"png": f"{name}.png", "metadata": f"{name}.json"}
            )
            self.make_stages("multipass", 3 + index)
        report = self.analyze()
        check = self.comparison(report)
        self.assertEqual(check.status, stages.EvidenceStatus.MEASURED)
        self.assertEqual(len(check.pairs), 18)
        self.assertEqual({pair.a.generation for pair in check.pairs}, {3, 8})
        repeat = next(
            check
            for check in report.cases[0].repeatability
            if check.stage == stages.Stage.GBUFFER0
            and check.eye == 0
            and check.modes == (stages.Mode.MULTIPASS,)
        )
        self.assertEqual(len(repeat.pairs), 15)
        self.assertTrue(
            any(pair.a.generation != pair.b.generation for pair in repeat.pairs)
        )

    def test_cli_report_exit_status_and_invalid_manifest(self):
        self.path.write_text(json.dumps(self.manifest))
        self.assertEqual(stages.main([str(self.path), "--output", str(self.output)]), 0)
        self.path.write_text("{")
        self.assertEqual(stages.main([str(self.path), "--output", str(self.output)]), 2)
        self.assertIn(
            "visual stages:",
            json.loads((self.output / "stages.json").read_text())["errors"][0],
        )


if __name__ == "__main__":
    unittest.main()
