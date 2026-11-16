# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "pillow"]
# ///
"""Synthetic comparison tests: uv run tools/test_visual_compare.py."""

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

if __package__:
    from tools.visual_compare import (
        ComparisonInputs,
        Mode,
        Thresholds,
        analyze_manifest,
        image_metrics,
        main,
        metadata_mismatches,
    )
else:
    from visual_compare import (
        ComparisonInputs,
        Mode,
        Thresholds,
        analyze_manifest,
        image_metrics,
        main,
        metadata_mismatches,
    )


class VisualCompareTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "report"
        self.path = self.root / "manifest.json"
        self.left = np.full((8, 12, 3), 30, dtype=np.uint8)
        self.right = np.full((8, 12, 3), 120, dtype=np.uint8)
        self.inputs = {
            "camera": {"position": [0, 0, 0], "orientation": [0, 0, 0, 1]},
            "eyes": [
                {
                    "offset": [-0.03, 0, 0],
                    "orientation_delta": [0, 0, 0, 1],
                    "projection_standard": np.eye(4).flatten().tolist(),
                    "projection_reverse_z": np.eye(4).flatten().tolist(),
                },
                {
                    "offset": [0.03, 0, 0],
                    "orientation_delta": [0, 0, 0, 1],
                    "projection_standard": np.eye(4).flatten().tolist(),
                    "projection_reverse_z": np.eye(4).flatten().tolist(),
                },
            ],
            "profile": "controlled_v1",
            "world_scale": 1.0,
            "build_stamp": "synthetic-build",
            "eye_width": 12,
            "eye_height": 8,
        }
        self.manifest = {"cases": [{"name": "synthetic", "modes": {}}]}
        for mode in ("left_only", "right_only", "multipass", "single_pass"):
            self.add_mode(mode)

    def add_mode(self, mode):
        isolated = mode in ("left_only", "right_only")
        eye = (0 if mode == "left_only" else 1) if isolated else None
        image = (
            (self.left if eye == 0 else self.right)
            if isolated
            else np.concatenate([self.left, self.right], axis=1)
        )
        samples = []
        for index in range(3):
            ordinal = 3 * len(self.manifest["cases"][0]["modes"]) + index + 1
            name = f"{mode}-{index}"
            png = f"{name}.png"
            metadata_path = f"{name}.json"
            Image.fromarray(image).save(self.root / png)
            metadata = {
                "id": name,
                "frame_ordinal": ordinal,
                "predicted_display_time_ns": ordinal * 11_111_111,
                "mode": mode,
                "visual": {
                    "generation": len(self.manifest["cases"][0]["modes"]) + 1,
                    "mode": mode,
                    "profile": "controlled_v1",
                    "ready": True,
                    "paused": True,
                    "frozen": True,
                    "error": None,
                    "warmup_frames": 3,
                    "completed_frames": 3,
                },
                "fresh_eyes": [eye in (None, 0), eye in (None, 1)],
                "width": image.shape[1],
                "height": image.shape[0],
                "eye_width": 12,
                "eye_height": 8,
                "selected_eye": eye,
                "rendered_eyes": [eye in (None, 0), eye in (None, 1)],
                "effective_config": {"mode": mode},
                "comparison_inputs": copy.deepcopy(self.inputs),
            }
            (self.root / metadata_path).write_text(json.dumps(metadata))
            samples.append({"png": png, "metadata": metadata_path})
        self.manifest["cases"][0]["modes"][mode] = samples

    def entries(self, mode):
        return self.manifest["cases"][0]["modes"][mode]

    def change_metadata(self, mode, index, change):
        path = self.root / self.entries(mode)[index]["metadata"]
        metadata = json.loads(path.read_text())
        change(metadata)
        path.write_text(json.dumps(metadata))

    def analyze(self, **kwargs):
        self.path.write_text(json.dumps(self.manifest))
        return analyze_manifest(
            self.path, self.output, tile_size=4, top_tiles=2, **kwargs
        )

    def mode_report(self, report, mode):
        return next(item for item in report.cases[0].modes if item.mode == Mode(mode))

    def change_images(self, mode, change, indices=range(3)):
        for index in indices:
            path = self.root / self.entries(mode)[index]["png"]
            with Image.open(path) as image:
                pixels = np.array(image)
            change(pixels, index)
            Image.fromarray(pixels).save(path)

    def test_stable_four_modes_pass_and_write_reports_and_crops(self):
        report = self.analyze()
        self.assertEqual(report.status, "pass")
        self.assertEqual(len(report.cases[0].comparisons), 6)
        self.assertEqual(
            json.loads((self.output / "report.json").read_text()), report.to_wire()
        )
        html = (self.output / "report.html").read_text()
        self.assertIn("Ranked tiles", html)
        for comparison in report.cases[0].comparisons:
            self.assertEqual(len(comparison.pairs), 9)
            self.assertEqual(comparison.worst_metrics.max, 0)
            for path in (
                comparison.artifacts.a,
                comparison.artifacts.b,
                comparison.artifacts.diff,
            ):
                self.assertTrue((self.output / path).is_file())
            for tile in comparison.ranked_tiles:
                self.assertEqual(tile.box[2] - tile.box[0], 4)
                for path in (tile.crops.a, tile.crops.b, tile.crops.diff):
                    self.assertTrue((self.output / path).is_file())

    def test_stable_local_difference_fails_and_ranks_affected_tile(self):
        self.change_images(
            "single_pass",
            lambda image, _: image.__setitem__((slice(0, 4), slice(4, 8)), 200),
        )
        report = self.analyze()
        self.assertEqual(report.status, "fail")
        self.assertEqual(self.mode_report(report, "single_pass").status, "pass")
        comparison = next(
            c
            for c in report.cases[0].comparisons
            if c.name == "multipass vs single_pass, eye 0"
        )
        self.assertEqual(comparison.status, "fail")
        self.assertEqual(comparison.ranked_tiles[0].box, (4, 0, 8, 4))
        self.assertEqual(comparison.worst_metrics.max, 170)
        self.assertAlmostEqual(comparison.worst_metrics.changed_fraction, 16 / 96)
        diff = np.array(Image.open(self.output / comparison.artifacts.diff))
        self.assertEqual(int(diff[0, 4, 0]), 170)

    def test_unstable_noise_is_inconclusive_not_a_visual_failure_or_pass(self):
        generator = np.random.default_rng(42)
        self.change_images(
            "single_pass",
            lambda image, _: image.__setitem__(
                slice(None), generator.integers(0, 256, image.shape, dtype=np.uint8)
            ),
        )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        mode = self.mode_report(report, "single_pass")
        self.assertEqual(mode.status, "inconclusive")
        self.assertTrue(any(c.status == "fail" for c in mode.repeatability))
        comparisons = [
            c for c in report.cases[0].comparisons if "single_pass" in c.name
        ]
        self.assertTrue(all(c.status == "inconclusive" for c in comparisons))
        self.assertTrue(all(c.artifacts is None for c in comparisons))

    def test_every_repeat_pair_is_checked_not_only_first_or_average(self):
        self.change_images(
            "single_pass",
            lambda image, index: image.__setitem__(slice(None), 120 + (index - 1) * 2),
        )
        report = self.analyze(thresholds=Thresholds(mean=3, changed_fraction=0.5))
        mode = self.mode_report(report, "single_pass")
        self.assertEqual(mode.status, "inconclusive")
        self.assertEqual(len(mode.repeatability[0].pairs), 3)
        self.assertTrue(
            any(pair.metrics.max == 4 for pair in mode.repeatability[0].pairs)
        )

    def test_separate_repeatability_threshold_and_overrides(self):
        self.change_images(
            "single_pass",
            lambda image, index: image.__setitem__(slice(None), 120 + index),
        )
        self.manifest["thresholds"] = {
            "mean": 255,
            "changed_fraction": 1,
            "repeatability": {"mean": 0, "pixel": 0, "changed_fraction": 0},
        }
        self.assertEqual(self.analyze().status, "inconclusive")
        self.assertEqual(
            self.analyze(repeatability=Thresholds(mean=255, changed_fraction=1)).status,
            "pass",
        )

    def test_optional_reverse_multipass_uses_logical_eye_order(self):
        self.add_mode("reverse_multipass")
        report = self.analyze()
        self.assertEqual(report.status, "pass")
        self.assertEqual(len(report.cases[0].comparisons), 12)
        self.change_images(
            "reverse_multipass",
            lambda image, _: image.__setitem__(
                slice(None), np.concatenate([self.right, self.left], axis=1)
            ),
        )
        self.assertEqual(self.analyze().status, "fail")

    def test_shape_mismatch_and_metadata_mismatch_are_explicit(self):
        path = self.root / self.entries("single_pass")[0]["png"]
        Image.fromarray(np.zeros((7, 24, 3), dtype=np.uint8)).save(path)
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn("shape", " ".join(self.mode_report(report, "single_pass").errors))
        self.change_metadata("left_only", 0, lambda metadata: metadata.update(width=24))
        report = self.analyze()
        self.assertIn(
            "metadata image shape",
            " ".join(self.mode_report(report, "left_only").errors),
        )

    def test_missing_mode_or_too_few_samples_never_pass(self):
        self.entries("right_only").pop()
        del self.manifest["cases"][0]["modes"]["single_pass"]
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        for mode in ("right_only", "single_pass"):
            self.assertIn("at least three", self.mode_report(report, mode).errors[0])

    def test_reused_png_is_not_three_independent_captures(self):
        self.entries("left_only")[1]["png"] = self.entries("left_only")[0]["png"]
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn(
            "independent sample", self.mode_report(report, "left_only").errors[0]
        )

    def test_controlled_v2_samples_pass_and_preserve_profile_in_reports(self):
        for mode in self.manifest["cases"][0]["modes"]:
            for index in range(3):
                self.change_metadata(
                    mode, index,
                    lambda metadata: (
                        metadata["visual"].update(profile="controlled_v2"),
                        metadata["comparison_inputs"].update(profile="controlled_v2"),
                    ),
                )
        report = self.analyze()
        self.assertEqual(report.status, "pass")
        wire = json.loads((self.output / "report.json").read_text())
        self.assertEqual(wire, report.to_wire())
        for mode in wire["cases"][0]["modes"]:
            for sample in mode["samples"]:
                self.assertEqual(sample["visual"]["profile"], "controlled_v2")
                self.assertEqual(sample["inputs"]["profile"], "controlled_v2")

    def test_mixed_controlled_profiles_are_inconclusive(self):
        for indices in ((0,), (0, 1, 2)):
            with self.subTest(indices=indices):
                for index in indices:
                    self.change_metadata(
                        "single_pass", index,
                        lambda metadata: (
                            metadata["visual"].update(profile="controlled_v2"),
                            metadata["comparison_inputs"].update(profile="controlled_v2"),
                        ),
                    )
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                mode = self.mode_report(report, "single_pass")
                if len(indices) == 1:
                    self.assertEqual(mode.status, "inconclusive")
                    comparison = mode.repeatability[0]
                else:
                    self.assertEqual(mode.status, "pass")
                    comparison = next(
                        item for item in report.cases[0].comparisons
                        if item.name == "multipass vs single_pass, eye 0"
                    )
                self.assertIn("comparison_inputs.profile", " ".join(comparison.errors))

    def test_visual_and_input_profiles_must_match(self):
        self.change_metadata(
            "left_only", 0,
            lambda metadata: metadata["visual"].update(profile="controlled_v2"),
        )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn(
            "differs from comparison_inputs.profile",
            self.mode_report(report, "left_only").errors[0],
        )

    def test_visual_readiness_requires_controlled_completed_frozen_capture(self):
        path = self.root / self.entries("left_only")[0]["metadata"]
        original = path.read_text()
        valid = json.loads(original)
        invalid = {
            "profile": ("native", "synthetic", "other-profile", None, "", 1, [], {}),
            "ready": (False, None, 0, 1, "true"),
            "paused": (False, None, 0, 1, "true"),
            "frozen": (False, None, 0, 1, "true"),
            "error": ("capture failed", "", False, 0, []),
            "generation": (0, -1, None, True, 1.0, "1"),
            "warmup_frames": (0, -1, None, True, 1.0, "1", 4, 301),
            "completed_frames": (0, -1, None, True, 1.0, "1", 2),
        }
        for field, values in invalid.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    data = copy.deepcopy(valid)
                    data["visual"][field] = value
                    if field == "profile":
                        data["comparison_inputs"][field] = value
                    path.write_text(json.dumps(data))
                    report = self.analyze()
                    self.assertEqual(report.status, "inconclusive")
                    self.assertIn(
                        field, self.mode_report(report, "left_only").errors[0]
                    )
                    if field == "profile":
                        self.assertIn(
                            "metadata.visual.profile",
                            self.mode_report(report, "left_only").errors[0],
                        )
            with self.subTest(field=field, missing=True):
                data = copy.deepcopy(valid)
                del data["visual"][field]
                path.write_text(json.dumps(data))
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                self.assertIn(field, self.mode_report(report, "left_only").errors[0])
        path.write_text(original)

    def test_inline_inputs_do_not_bypass_readiness(self):
        self.entries("left_only")[0]["comparison_inputs"] = self.inputs
        self.change_metadata(
            "left_only",
            0,
            lambda metadata: (
                metadata.pop("comparison_inputs"),
                metadata["visual"].update(ready=False),
            ),
        )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn("ready", self.mode_report(report, "left_only").errors[0])

    def test_frame_identity_is_typed_and_required(self):
        path = self.root / self.entries("left_only")[0]["metadata"]
        original = path.read_text()
        for field in ("frame_ordinal", "predicted_display_time_ns"):
            for value in (None, True, 1.5, "1"):
                with self.subTest(field=field, value=value):
                    data = json.loads(original)
                    data[field] = value
                    path.write_text(json.dumps(data))
                    report = self.analyze()
                    self.assertEqual(report.status, "inconclusive")
                    self.assertIn(
                        field, self.mode_report(report, "left_only").errors[0]
                    )
            data = json.loads(original)
            del data[field]
            path.write_text(json.dumps(data))
            self.assertEqual(self.analyze().status, "inconclusive")
        for ordinal in (0, -1):
            data = json.loads(original)
            data["frame_ordinal"] = ordinal
            path.write_text(json.dumps(data))
            self.assertEqual(self.analyze().status, "inconclusive")
        path.write_text(original)

    def test_renamed_frame_copies_are_not_repeatability_samples(self):
        first = json.loads(
            (self.root / self.entries("multipass")[0]["metadata"]).read_text()
        )
        path = self.root / self.entries("multipass")[1]["metadata"]
        original = path.read_text()
        for repeated in (
            ("frame_ordinal",),
            ("predicted_display_time_ns",),
            ("frame_ordinal", "predicted_display_time_ns"),
        ):
            for generation in (first["visual"]["generation"], 99):
                with self.subTest(repeated=repeated, generation=generation):
                    data = json.loads(original)
                    data.update({field: first[field] for field in repeated})
                    data["visual"]["generation"] = generation
                    path.write_text(json.dumps(data))
                    report = self.analyze()
                    self.assertEqual(report.status, "inconclusive")
                    mode = self.mode_report(report, "multipass")
                    self.assertIn("repeated frame", mode.errors[0])
                    self.assertFalse(mode.repeatability)
        path.write_text(original)

    def test_multipass_a_b_a_allows_new_generation_and_distinct_frames(self):
        self.change_metadata(
            "multipass",
            2,
            lambda metadata: metadata["visual"].update(
                generation=5, completed_frames=8
            ),
        )
        report = self.analyze()
        self.assertEqual(report.status, "pass")
        samples = self.mode_report(report, "multipass").samples
        self.assertEqual([sample.visual.generation for sample in samples], [3, 3, 5])
        self.assertEqual([sample.frame_ordinal for sample in samples], [7, 8, 9])
        self.assertEqual(
            [sample.predicted_display_time_ns for sample in samples],
            [ordinal * 11_111_111 for ordinal in (7, 8, 9)],
        )
        self.assertEqual(samples[2].visual.completed_frames, 8)
        recorded = json.loads((self.output / "report.json").read_text())
        mode = next(
            mode
            for mode in recorded["cases"][0]["modes"]
            if mode["mode"] == "multipass"
        )
        self.assertEqual(mode["samples"][2]["frame_ordinal"], 9)
        self.assertEqual(
            mode["samples"][2]["predicted_display_time_ns"], 9 * 11_111_111
        )
        self.assertEqual(mode["samples"][2]["visual"]["generation"], 5)
        self.assertNotIn("left", mode["samples"][2])
        self.assertNotIn("right", mode["samples"][2])

    def test_visual_warmup_accepts_one_through_300_with_no_completed_upper_bound(self):
        for warmup, completed in ((1, 1), (300, 300), (300, 301)):
            with self.subTest(warmup=warmup, completed=completed):
                for mode in self.manifest["cases"][0]["modes"]:
                    for index in range(3):
                        self.change_metadata(
                            mode,
                            index,
                            lambda metadata, warmup=warmup, completed=completed: (
                                metadata["visual"].update(
                                    warmup_frames=warmup, completed_frames=completed
                                )
                            ),
                        )
                self.assertEqual(self.analyze().status, "pass")

    def test_pose_projection_profile_build_and_resolution_mismatches(self):
        mutations = (
            (
                "camera.position",
                lambda data: data["camera"]["position"].__setitem__(0, 0.1),
            ),
            (
                "projection_standard",
                lambda data: data["eyes"][1]["projection_standard"].__setitem__(0, 2.0),
            ),
            ("profile", lambda data: data.update(profile="other-profile")),
            ("build_stamp", lambda data: data.update(build_stamp="other-build")),
            ("eye_width", lambda data: data.update(eye_width=13)),
        )
        for expected, mutate in mutations:
            with self.subTest(field=expected):
                for index in range(3):
                    inputs = copy.deepcopy(self.inputs)
                    mutate(inputs)
                    self.change_metadata(
                        "single_pass",
                        index,
                        lambda metadata, inputs=inputs: metadata.update(
                            comparison_inputs=inputs
                        ),
                    )
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                text = json.dumps(report.to_wire())
                self.assertIn(expected, text)
                if expected not in ("eye_width", "profile"):
                    self.assertEqual(
                        self.mode_report(report, "single_pass").status, "pass"
                    )
                    comparison = next(
                        c
                        for c in report.cases[0].comparisons
                        if c.name == "multipass vs single_pass, eye 0"
                    )
                    self.assertIn(expected, " ".join(comparison.errors))

    def test_inline_inputs_cannot_override_recorded_evidence(self):
        self.entries("single_pass")[0]["comparison_inputs"] = self.inputs | {
            "build_stamp": "different"
        }
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        self.assertIn(
            "disagree with metadata", self.mode_report(report, "single_pass").errors[0]
        )

    def test_looser_metadata_tolerance_is_not_qualifying(self):
        self.assertEqual(self.analyze(metadata_tolerance=1).status, "inconclusive")

    def test_explicit_repeatability_limits_survive_cross_mode_override(self):
        self.manifest["thresholds"] = {"repeatability": {"mean": 0.25}}
        report = self.analyze(thresholds=Thresholds(mean=255, changed_fraction=1))
        self.assertEqual(report.repeatability_thresholds, Thresholds())

    def test_metadata_file_reuse_is_not_independent(self):
        self.entries("left_only")[1]["metadata"] = self.entries("left_only")[0][
            "metadata"
        ]
        self.assertEqual(self.analyze().status, "inconclusive")

    def test_stale_eyes_null_profile_and_bad_matrix_are_inconclusive(self):
        for change in (
            lambda metadata: metadata.update(fresh_eyes=[False, False]),
            lambda metadata: metadata["comparison_inputs"].update(profile=None),
            lambda metadata: metadata["comparison_inputs"]["eyes"][0].update(
                projection_reverse_z=[1]
            ),
        ):
            path = self.root / self.entries("left_only")[0]["metadata"]
            original = path.read_text()
            self.change_metadata("left_only", 0, change)
            self.assertEqual(self.analyze().status, "inconclusive")
            path.write_text(original)

    def test_inline_inputs_supported_when_metadata_inputs_absent(self):
        for mode in self.manifest["cases"][0]["modes"]:
            for index, entry in enumerate(self.entries(mode)):
                entry["comparison_inputs"] = copy.deepcopy(self.inputs)
                self.change_metadata(
                    mode, index, lambda metadata: metadata.pop("comparison_inputs")
                )
        self.assertEqual(self.analyze().status, "pass")

    def test_float_tolerance_is_absolute_and_profile_is_exact(self):
        other = copy.deepcopy(self.inputs)
        other["eyes"][0]["offset"][0] += 0.000009
        self.assertEqual(
            metadata_mismatches(
                ComparisonInputs.from_wire(self.inputs),
                ComparisonInputs.from_wire(other),
                1e-5,
            ),
            [],
        )
        other["eyes"][0]["offset"][0] += 0.000002
        self.assertIn(
            "eyes[0].offset[0]",
            metadata_mismatches(
                ComparisonInputs.from_wire(self.inputs),
                ComparisonInputs.from_wire(other),
                1e-5,
            )[0],
        )
        other = copy.deepcopy(self.inputs)
        other["world_scale"] += 0.000001
        self.assertIn(
            "world_scale",
            metadata_mismatches(
                ComparisonInputs.from_wire(self.inputs),
                ComparisonInputs.from_wire(other),
                1e-5,
            )[0],
        )
        other = copy.deepcopy(self.inputs)
        other["camera"]["position"][0] = 1000000
        original = copy.deepcopy(other)
        other["camera"]["position"][0] += 0.001
        self.assertTrue(
            metadata_mismatches(
                ComparisonInputs.from_wire(original),
                ComparisonInputs.from_wire(other),
                1e-5,
            )
        )

    def test_same_mode_input_mismatch_blocks_repeatability(self):
        self.change_metadata(
            "multipass",
            1,
            lambda metadata: metadata["comparison_inputs"].update(
                build_stamp="different"
            ),
        )
        report = self.analyze()
        self.assertEqual(report.status, "inconclusive")
        mode = self.mode_report(report, "multipass")
        self.assertEqual(mode.status, "inconclusive")
        self.assertIn("build_stamp", mode.repeatability[0].errors[0])

    def test_invalid_mode_eye_and_generation_metadata(self):
        changes = (
            {"selected_eye": 1},
            {"rendered_eyes": [True, True]},
            {"mode": "multipass"},
            {"rendered_eyes": [1, 0]},
            {"eye_width": True},
        )
        for change in changes:
            with self.subTest(change=change):
                path = self.root / self.entries("left_only")[0]["metadata"]
                original = path.read_text()
                self.change_metadata(
                    "left_only",
                    0,
                    lambda metadata, change=change: metadata.update(change),
                )
                self.assertEqual(self.analyze().status, "inconclusive")
                path.write_text(original)

    def test_missing_corrupt_and_nonopaque_images_are_inconclusive(self):
        path = self.root / self.entries("left_only")[0]["png"]
        original = path.read_bytes()
        for data in (None, b"not an image"):
            if data is None:
                path.unlink()
            else:
                path.write_bytes(data)
            self.assertEqual(self.analyze().status, "inconclusive")
        rgba = np.full((8, 12, 4), 30, dtype=np.uint8)
        Image.fromarray(rgba).save(path)
        self.assertEqual(self.analyze().status, "inconclusive")
        path.write_bytes(original)
        rgba[:, :, :3] = self.left
        rgba[:, :, 3] = 255
        Image.fromarray(rgba).save(path)
        self.assertEqual(self.analyze().status, "pass")

    def test_metrics_channel_max_percentile_and_threshold_boundary(self):
        a = np.zeros((10, 10, 3), dtype=np.uint8)
        b = a.copy()
        b[0, 0] = [3, 0, 0]
        limits = Thresholds(pixel=3)
        metrics = image_metrics(a, b, limits)
        self.assertEqual(metrics.changed_fraction, 0)
        self.assertAlmostEqual(metrics.mean, 0.01)
        self.assertEqual(metrics.max, 3)
        b[0, 0, 0] = 4
        b[0, 1] = [10, 20, 30]
        metrics = image_metrics(a, b, limits)
        self.assertEqual(metrics.changed_fraction, 0.02)
        self.assertAlmostEqual(metrics.mean, 64 / 300)
        self.assertAlmostEqual(metrics.p99, 4.26)
        self.assertEqual(metrics.max, 30)

    def test_no_registration_or_resizing(self):
        self.left[0, 0] = 240
        for index in range(3):
            path = self.root / self.entries("left_only")[index]["png"]
            Image.fromarray(self.left).save(path)
        self.change_images(
            "single_pass", lambda image, _: image.__setitem__((0, 1), 240)
        )
        report = self.analyze()
        self.assertEqual(report.status, "fail")

    def test_invalid_thresholds_empty_inputs_and_nonfinite_inputs(self):
        for thresholds in (
            {"pixel": -1},
            {"changed_fraction": 2},
            {"mean": float("nan")},
            {"unknown": 1},
            {"pixel": True},
        ):
            with self.subTest(thresholds=thresholds):
                self.manifest["thresholds"] = thresholds
                report = self.analyze()
                self.assertEqual(report.status, "inconclusive")
                self.assertTrue(report.errors)
        self.manifest.pop("thresholds")
        for bad in (
            {},
            self.inputs
            | {
                "camera": {
                    "position": [float("nan"), 0, 0],
                    "orientation": [0, 0, 0, 1],
                }
            },
            self.inputs | {"eyes": []},
            self.inputs | {"camera": {}},
        ):
            self.entries("left_only")[0]["comparison_inputs"] = bad
            self.assertEqual(self.analyze().status, "inconclusive")

    def test_html_escapes_case_names(self):
        self.manifest["cases"][0]["name"] = "<script>alert('fixture')</script>"
        self.analyze()
        text = (self.output / "report.html").read_text()
        self.assertNotIn("<script>", text)
        self.assertIn("&lt;script&gt;", text)

    def test_cli_exit_codes_and_relative_manifest_paths(self):
        self.path.write_text(json.dumps(self.manifest))
        self.assertEqual(main([str(self.path), "--output", str(self.output)]), 0)
        self.change_images(
            "single_pass", lambda image, _: image.__setitem__(slice(None), 240)
        )
        self.assertEqual(main([str(self.path), "--output", str(self.output)]), 1)
        self.change_metadata(
            "single_pass", 0, lambda metadata: metadata.update(selected_eye=0)
        )
        self.assertEqual(main([str(self.path), "--output", str(self.output)]), 2)
        self.path.write_text("{bad json")
        self.assertEqual(main([str(self.path), "--output", str(self.output)]), 2)
        self.assertEqual(
            main([str(self.path), "--output", str(self.output), "--mean", "1"]), 2
        )
        self.assertEqual(
            json.loads((self.output / "report.json").read_text())["status"],
            "inconclusive",
        )


if __name__ == "__main__":
    unittest.main(argv=sys.argv)
