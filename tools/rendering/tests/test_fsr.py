"""FSR's harness tooling: the FSR variants and their schedule, the saved planes and FSR records of a
snapshot, the resolution-scale override, and the offline checks of the motion vectors and the image."""

import dataclasses
import unittest
from pathlib import Path

import numpy as np

from tools.rendering.comparison import fsr_jitter, fsr_motion, fsr_quality
from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.experiment import ExperimentConfig, ExperimentError, run_experiment
from tools.rendering.experiment_fsr import FsrVariant, FsrVariantError, parse_fsr_variants
from tools.rendering.fast.plan import label_mode, schedule
from tools.rendering.fast.cli import with_motion_phase
from tools.rendering.fast.plan import FastError
from tools.rendering.fast.run import runs_fsr, runs_temporal, step_planes
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
        jitter = FsrVariant("jitter-off", jitter=False).overrides(base)
        self.assertEqual((jitter.fsr, jitter.fsr_jitter), ("enabled", "disabled"))
        none = FsrVariant("no-aa", fsr=False, antialiasing=False).overrides(base)
        self.assertEqual((none.fsr, none.anti_aliasing), (None, "disabled"))
        self.assertIn('anti_aliasing = "disabled"', none.to_toml())
        self.assertEqual(VisualOverrides.from_wire(none.to_wire()), none)
        sharper = FsrVariant("sharp", sharpness=0.5).overrides(base)
        self.assertEqual((sharper.fsr, sharper.fsr_sharpness), ("enabled", 0.5))
        self.assertIn("fsr_sharpness = 0.5", sharper.to_toml())
        self.assertEqual(VisualOverrides.from_wire(sharper.to_wire()), sharper)
        self.assertEqual(FsrVariant("soft", sharpen=False).overrides(base).fsr_sharpen, "disabled")

    def test_a_taa_variant_runs_the_temporal_resolve_in_fsrs_place(self):
        base = VisualOverrides(screen_space_reflections="enabled", fsr="enabled")
        taa = FsrVariant("taa", taa=True).overrides(base)
        self.assertEqual((taa.fsr, taa.taa), (None, "enabled"))
        self.assertIn('taa = "enabled"', taa.to_toml())
        self.assertEqual(VisualOverrides.from_wire(taa.to_wire()), taa)
        config = ExperimentConfig.load(ROOT / "experiments/taa_quality_static.toml")
        with_taa = dataclasses.replace(config, visual_overrides=taa)
        self.assertTrue(runs_temporal(with_taa))
        self.assertFalse(runs_fsr(with_taa))
        self.assertEqual(step_planes((Plane.FINAL, Plane.FSR_MOTION), with_taa), (Plane.FINAL,))
        tuned = FsrVariant("tuned", taa=True, history_weight=0.85, clip_gamma_rest=2).overrides(base)
        self.assertEqual((tuned.taa_history_weight, tuned.taa_clip_gamma_rest), (0.85, 2.0))
        self.assertIn("taa_history_weight = 0.85", tuned.to_toml())
        self.assertEqual(VisualOverrides.from_wire(tuned.to_wire()), tuned)
        for value in ([{"name": "a", "taa": True, "fsr": False}],
                      [{"name": "a", "history_weight": 0.9}],
                      [{"name": "a", "taa": True, "clip_gamma_rest": 5}],
                      [{"name": "a", "taa": True, "sharpness": 0.5}], [{"name": "a", "taa": 1}]):
            with self.assertRaises(FsrVariantError, msg=str(value)):
                parse_fsr_variants(value)

    def test_the_taa_suite_loads(self):
        scenes = {scene.name: scene for scene in load_suite(ROOT / "suites/taa.toml")}
        self.assertIn("taa-meadow-slow", scenes)
        names = [v.name for v in scenes["taa-rotation"].config.fsr_variants]
        self.assertEqual(names[:4], ["taa", "fsr-off", "no-aa", "reference"])

    def test_the_motion_phase_can_be_swept(self):
        scene = next(scene for scene in load_suite(ROOT / "suites/taa.toml")
                     if scene.name == "taa-translation")
        self.assertEqual(with_motion_phase(scene, 18).config.motion.phase, 18)
        with self.assertRaises(FastError):
            with_motion_phase(scene, 500)
        still = next(scene for scene in load_suite(ROOT / "suites/taa.toml")
                     if scene.name == "taa-check")
        self.assertEqual(with_motion_phase(still, 18).config.motion.phase, 18)

    def test_invalid_variants_are_refused(self):
        for value in ([{"name": "no spaces"}], [{"name": "a", "fsr": False, "hybrid_velocity": False}],
                      [{"name": "a", "resolution_scale": 2.0}], [{"name": "a", "modes": ["sky"]}],
                      [{"name": "a", "fsr": False, "jitter": False}],
                      [{"name": "a", "antialiasing": False}], [{"name": "a", "jitter": 1}],
                      [{"name": "a", "sharpness": 1.5}], [{"name": "a", "fsr": False, "sharpness": 0.5}],
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
                         ["jitter-off", "fsr-off", "no-aa", "reference"])
        reference = quality.fsr_variants[-1]
        self.assertEqual((reference.fsr, reference.antialiasing, reference.resolution_scale),
                         (False, False, 1.414))
        self.assertIn("quality-meadow-slow", scenes)
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
        self.assertNotIn("taa", text)
        self.assertIn("taa = true", BenchmarkRequest("b1", "run", "single_pass", 5.0,
                                                     taa=True).to_toml())
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "run", "single_pass", 5.0, taa="yes")
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

    def test_a_still_jittered_camera_has_no_motion(self):
        # Two frames of one still camera through different jitter offsets: the pixels move by the
        # jitter's difference, which FSR must not be handed as motion.
        offsets = [[2e-4, -1e-4], [-3e-4, 2.5e-4]]
        jittered = [fsr_motion.jitter_translation(offset) @ projection() for offset in offsets]
        clip = fsr_motion.clip_positions(np.full((4, 8), 0.05))
        self.assertGreater(float(np.abs(fsr_motion.camera_motion_uv(
            clip, jittered[1] @ np.linalg.inv(jittered[0]))).max()), 1e-4)
        still = fsr_motion.jitter_free_motion_uv(clip, projection(), projection(), offsets[0])
        self.assertLess(float(np.abs(still).max()), 1e-9)


class JitterCheckTests(unittest.TestCase):
    def test_a_sub_pixel_shift_is_measured_and_explained(self):
        rng = np.random.default_rng(3)
        base = rng.uniform(0, 255, (24, 24))
        smooth = np.kron(base, np.ones((4, 4)))
        smooth = sum(np.roll(np.roll(smooth, dy, 0), dx, 1) for dy in range(-3, 4)
                     for dx in range(-3, 4)) / 49.0
        image = np.repeat(smooth[..., None], 3, axis=2)
        moved = fsr_jitter.shift_image(image, (0.3, -0.2))
        measured = fsr_jitter.estimate_shift(fsr_jitter.luma(image)[8:-8, 8:-8],
                                             fsr_jitter.luma(moved)[8:-8, 8:-8])
        self.assertAlmostEqual(measured[0], 0.3, delta=0.02)
        self.assertAlmostEqual(measured[1], -0.2, delta=0.02)
        self.assertEqual(fsr_jitter.blocks(np.ones((32, 48)), 16).shape, (2, 3))


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

    def test_scores_average_over_reports_and_snapshots(self):
        def result(psnr_a, psnr_b):
            return {"scene": "s", "reference": [
                {"config": "taa", "mode": "single_pass", "snapshot": "a", "psnr": psnr_a,
                 "sharpness": 1.0, "ghosting": 0.1},
                {"config": "taa", "mode": "single_pass", "snapshot": "b", "psnr": psnr_b,
                 "sharpness": 0.5, "ghosting": 0.3}],
                "flicker": [{"config": "taa", "mode": "single_pass", "mean": 2.0}]}
        rows = fsr_quality.average([result(30.0, 32.0), result(34.0, 36.0)])
        self.assertEqual(len(rows), 1)
        self.assertEqual((rows[0]["snapshots"], rows[0]["psnr"], rows[0]["sharpness"],
                          rows[0]["flicker"]), (4, 33.0, 0.75, 2.0))

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
