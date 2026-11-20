"""FSR's harness tooling: the FSR variants and their schedule, the saved planes and FSR records of a
snapshot, the resolution-scale override, and the offline checks of the motion vectors and the image."""

import dataclasses
import unittest
from pathlib import Path

import numpy as np

from tools.rendering.comparison import fsr_motion, fsr_quality
from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.experiment import ExperimentConfig, ExperimentError, run_experiment
from tools.rendering.experiment_fsr import FsrVariant, FsrVariantError, parse_fsr_variants
from tools.rendering.fast.plan import label_mode, schedule
from tools.rendering.fast.run import step_planes
from tools.rendering.fast.suite import load_suite
from tools.rendering.protocol import (
    Command,
    ProtocolError,
    VisualOverrides,
    acknowledgement_from_wire,
)
from tools.rendering.protocol.benchmark import BenchmarkRequest
from tools.rendering.protocol.snapshot import Plane, SnapshotAck, SnapshotRequest

ROOT = Path(__file__).resolve().parents[1]


class VariantTests(unittest.TestCase):
    def test_overrides_put_fsr_and_the_scale_in_place(self):
        base = VisualOverrides(screen_space_reflections="enabled", fsr="enabled")
        off = FsrVariant("fsr-off", fsr=False).overrides(base)
        self.assertIsNone(off.fsr)
        self.assertEqual(off.screen_space_reflections, "enabled")
        hybrid = FsrVariant("hybrid-off", hybrid_velocity=False).overrides(base)
        self.assertEqual((hybrid.fsr, hybrid.fsr_hybrid_velocity), ("enabled", "disabled"))
        reference = FsrVariant("reference", fsr=False, resolution_scale=1.414).overrides(base)
        self.assertEqual(reference.resolution_scale, 1.414)
        # A base naming FSR alone leaves nothing once the variant takes FSR out.
        self.assertIsNone(FsrVariant("off", fsr=False).overrides(VisualOverrides(fsr="enabled")))
        self.assertEqual(FsrVariant("on").overrides(None), VisualOverrides(fsr="enabled"))

    def test_invalid_variants_are_refused(self):
        for value in ([{"name": "no spaces"}], [{"name": "a", "fsr": False, "hybrid_velocity": False}],
                      [{"name": "a", "resolution_scale": 2.0}], [{"name": "a", "modes": ["sky"]}],
                      [{"name": "a"}, {"name": "a"}], [{"name": "a", "extra": 1}], []):
            with self.assertRaises(FsrVariantError, msg=str(value)):
                parse_fsr_variants(value)

    def test_variants_render_only_configured_modes(self):
        configured = (CompareMode.LEFT_ONLY, CompareMode.MULTIPASS)
        variant = FsrVariant("reference", modes=("multipass",))
        self.assertEqual(variant.modes_in(configured), (CompareMode.MULTIPASS,))
        self.assertEqual(FsrVariant("all").modes_in(configured), configured)
        with self.assertRaises(FsrVariantError):
            FsrVariant("x", modes=("single_pass",)).modes_in(configured)

    def test_the_fsr_suite_loads_and_the_original_path_refuses_it(self):
        scenes = {scene.name: scene for scene in load_suite(ROOT / "suites/fsr.toml")}
        quality = scenes["quality-rotation"].config
        self.assertEqual([v.name for v in quality.fsr_variants],
                         ["hybrid-off", "fsr-off", "reference", "reduced"])
        self.assertEqual(scenes["motion-index"].config.visual_overrides.headset, "index")
        with self.assertRaises(ExperimentError):
            run_experiment(None, quality, Path("/nonexistent"))

    def test_a_config_names_only_its_own_modes(self):
        config = ExperimentConfig.load(ROOT / "experiments/fsr_quality_static.toml")
        self.assertEqual(config.warmup_frames, 120)
        self.assertTrue(config.fsr_variants)


class ScheduleTests(unittest.TestCase):
    def test_fsr_variants_run_last_with_their_own_repeats(self):
        variants = (FsrVariant("hybrid-off", hybrid_velocity=False),
                    FsrVariant("reference", fsr=False, resolution_scale=1.414, modes=("left_only",)))
        steps = schedule((CompareMode.LEFT_ONLY, CompareMode.SINGLE_PASS), (), True, variants)
        names = [step.name for step in steps]
        self.assertEqual(names, ["left_only", "single_pass", "hybrid-off left_only",
                                 "hybrid-off single_pass", "reference left_only"])
        hybrid = steps[2]
        self.assertEqual([s.label for s in hybrid.snapshots],
                         ["f1_hybrid_off_left_only_a", "f1_hybrid_off_left_only_b"])
        self.assertEqual(hybrid.snapshots[0].compare, ("left_only_a",))
        self.assertEqual(hybrid.snapshots[1].repeat_of, "f1_hybrid_off_left_only")
        self.assertEqual(label_mode(hybrid.snapshots[1].label), "f1_hybrid_off_left_only")
        # A scaled variant renders another eye size, which the GPU comparison cannot pair.
        self.assertEqual(steps[4].snapshots[0].compare, ())

    def test_fsr_planes_are_taken_only_where_fsr_runs(self):
        planes = (Plane.FINAL, Plane.FSR_MOTION, Plane.FSR_DEPTH, Plane.VELOCITY)
        config = ExperimentConfig.load(ROOT / "experiments/fsr_motion_static.toml")
        self.assertEqual(step_planes(planes, config), planes)
        off = FsrVariant("off", fsr=False)
        without = dataclasses.replace(config, visual_overrides=off.overrides(config.visual_overrides))
        self.assertEqual(step_planes(planes, without), (Plane.FINAL, Plane.VELOCITY))


class ProtocolTests(unittest.TestCase):
    def test_a_saving_snapshot_serializes(self):
        text = SnapshotRequest("a", "b", ("final", "fsr_motion"), save=True).to_toml()
        self.assertIn("save = true", text)
        self.assertIn('"fsr_motion"', text)
        with self.assertRaises(ProtocolError):
            SnapshotRequest("a", "b", ("final",), save=1)

    def test_saved_planes_and_fsr_records_parse(self):
        wire = {"id": "s", "state": "completed", "label": "b", "mode": "multipass", "frame": 4,
                "eye_width": 8, "eye_height": 4, "planes": [{"plane": "final", "eye": 0}],
                "comparisons": [], "plane_errors": [], "error": None, "visual": {},
                "timing": {"frames_waited": 1, "wait_ms": 1.0, "gpu_ms": 1.0},
                "saved": [{"stem": "b.final.eye0", "file": "s.dump/b.final.eye0.png"}],
                "fsr": [{"eye": 0, "hybrid": True}]}
        ack = acknowledgement_from_wire(wire, Command.SNAPSHOT, "s")
        self.assertIsInstance(ack, SnapshotAck)
        self.assertEqual(ack.saved[0]["stem"], "b.final.eye0")
        self.assertEqual(ack.fsr[0]["hybrid"], True)

    def test_the_resolution_scale_override_round_trips(self):
        overrides = VisualOverrides(resolution_scale=1.4142)
        self.assertEqual(overrides.resolution_scale, 1.414)
        self.assertEqual(VisualOverrides.from_wire(overrides.to_wire()), overrides)
        self.assertIn("resolution_scale = 1.414", overrides.to_toml())
        with self.assertRaises(ProtocolError):
            VisualOverrides(resolution_scale=2.0)

    def test_a_benchmark_request_carries_fsr_settings(self):
        request = BenchmarkRequest("b1", "run", "single_pass", 5.0, fsr=False,
                                   fsr_hybrid_velocity=True)
        text = request.to_toml()
        self.assertIn("fsr = false", text)
        self.assertIn("fsr_hybrid_velocity = true", text)
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "run", "single_pass", 5.0, fsr=1)


def column_major(matrix: np.ndarray) -> list[float]:
    return [float(v) for v in np.asarray(matrix, dtype=np.float32).T.ravel()]


def projection(near: float = 0.1) -> np.ndarray:
    """An infinite reverse-Z projection, as the game's."""
    return np.array([[1.0, 0, 0, 0], [0, 1.0, 0, 0], [0, 0, 0, near], [0, 0, -1.0, 0]])


def translation(x: float) -> np.ndarray:
    matrix = np.eye(4)
    matrix[0, 3] = x
    return matrix


class MotionCheckTests(unittest.TestCase):
    def test_a_still_camera_reprojects_onto_itself(self):
        clip = fsr_motion.clip_positions(np.full((4, 8), 0.01))
        self.assertEqual(float(np.abs(fsr_motion.camera_motion_uv(clip, np.eye(4))).max()), 0.0)

    def test_fsr_motion_points_from_the_current_position_to_the_previous_one(self):
        # The camera moved right by 0.1 m since the last frame (the view moved left by it), so a
        # point ahead appears further left now than it did: its previous position is to the right.
        current = projection() @ translation(-0.1)
        previous = projection()
        reprojection = previous @ np.linalg.inv(current)
        clip = fsr_motion.clip_positions(np.full((4, 8), 0.1))
        motion = fsr_motion.camera_motion_uv(clip, reprojection)
        self.assertTrue(np.all(motion[..., 0] > 0))
        self.assertTrue(np.allclose(motion[..., 1], 0))

    def test_a_decode_matching_the_camera_has_no_error(self):
        height, width = 4, 8
        current = projection() @ translation(-0.05)
        previous = projection()
        depth = np.full((height, width), 0.02, dtype=np.float32)
        reprojection = np.asarray(previous, dtype=np.float32).astype(np.float64) @ np.linalg.inv(
            np.asarray(current, dtype=np.float32).astype(np.float64))
        motion = fsr_motion.camera_motion_uv(fsr_motion.clip_positions(depth.astype(np.float64)),
                                             reprojection).astype(np.float32)
        velocity = np.zeros((height, width, 4), dtype=np.uint8)
        velocity[:] = fsr_motion.UNDRAWN
        record = {"eye": 0, "hybrid": True, "correct_parallax": True, "side_by_side": False,
                  "render_size": [width, height], "motion_vector_scale": [width, height],
                  "views": {"current_eye": column_major(current), "previous_eye": column_major(previous),
                            "current_center": column_major(current),
                            "previous_center": column_major(previous)}}
        check = fsr_motion.check_eye("s", {"label": "a", "step": "multipass"}, record,
                                     {"fsr_motion": motion, "fsr_depth": depth, "velocity": velocity})
        self.assertLess(check.camera.max, 1e-3)
        self.assertEqual(check.engine.pixels, 0)
        self.assertGreater(check.motion_mean, 0.01)


class QualityTests(unittest.TestCase):
    def test_images_are_keyed_by_configuration_mode_eye_and_letter(self):
        scene = {"snapshots": [
            {"label": "multipass_a", "step": "multipass",
             "saved": [{"stem": "multipass_a.final.eye1", "file": "x"}]},
            {"label": "f2_fsr_off_multipass_b", "step": "fsr-off multipass",
             "saved": [{"stem": "f2_fsr_off_multipass_b.final.eye0", "file": "y"},
                       {"stem": "f2_fsr_off_multipass_b.depth.eye0", "file": "z"}]}]}
        images = fsr_quality.scene_images(scene)
        self.assertEqual(set(images), {("fsr", "multipass", 1, "a"), ("fsr-off", "multipass", 0, "b")})

    def test_a_sharper_image_scores_above_one_and_blur_below(self):
        rng = np.random.default_rng(1)
        reference = np.repeat(rng.uniform(0, 255, (32, 32, 1)).astype(np.float32), 3, axis=2)
        blurred = (reference + np.roll(reference, 1, 0) + np.roll(reference, 1, 1)) / 3
        gradient = fsr_quality.gradient(fsr_quality.luma(reference))
        edges = gradient >= np.percentile(gradient, 90)
        flat = gradient <= np.percentile(gradient, 50)
        same = fsr_quality.against_reference(reference, reference, gradient, edges, flat)
        self.assertEqual((same["sharpness"], same["ghosting"]), (1.0, 0.0))
        self.assertLess(fsr_quality.against_reference(blurred, reference, gradient, edges,
                                                      flat)["sharpness"], 1.0)
        self.assertEqual(fsr_quality.difference(reference, reference + 4)["differing"], 32 * 32)


if __name__ == "__main__":
    unittest.main()
