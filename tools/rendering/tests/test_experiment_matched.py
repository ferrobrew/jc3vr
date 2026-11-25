import dataclasses
import json
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np
from PIL import Image

from tools.rendering.capture.validation import LAKE_SHADER, PACKED_LAKE_SHADER, require_lake_shader, validate_capture, validate_late_sky_stages
from tools.rendering.comparison.images import Outcome
from tools.rendering.comparison.stages import Stage
from tools.rendering.experiment import (
    ExperimentConfig,
    ExperimentError,
    MatchedTreatment,
    run_experiment,
)
from tools.rendering.experiment_matched import matched_cross_metrics
from tools.rendering.protocol import (
    Capture,
    CascadeScale,
    Case,
    Command,
    Mode,
    OrientationOffset,
    ProtocolError,
    Request,
    Sample,
    ScreenshotAck,
    TranslationOffset,
    WaterReflection,
)
from tools.rendering.tests.test_experiment import (
    HOUR,
    ExperimentTestCase,
    FakeRunner,
    capture_wire,
    pose_evidence,
    write_config,
)


class MatchedRunner(FakeRunner):
    def __init__(self, config_dir, *, changed=None, fail_at=None, fail_end=False):
        super().__init__(config_dir, fail_end=fail_end)
        self.changed = changed
        self.fail_at = fail_at
        self.screenshots = []
        self.target_pose = None

    def request(self, request):
        if request.command == Command.CONFIGURE:
            ack = super().request(request)
            ack = dataclasses.replace(ack, metadata=dataclasses.replace(
                ack.metadata, warmup_frames=request.warmup_frames, visual_overrides=request.visual_overrides))
            self.visual_overrides = request.visual_overrides
            if request.shader_identity:
                identity = {"requested": True, "status": "complete", "registered": 1,
                            "overflow": 0, "invalid": 0}
                ack = dataclasses.replace(ack, metadata=dataclasses.replace(ack.metadata, shader_identity=identity))
            self.mode = request.mode
            self.target_pose = request.pose
            return ack
        if request.command != Command.SCREENSHOT:
            return super().request(request)
        self.commands.append(request.command)
        self.requests.append(request)
        self.screenshots.append(request)
        index = len(self.screenshots)
        if index == self.fail_at:
            raise ExperimentError("experiment: synthetic screenshot failure")
        evidence = pose_evidence(self.target_pose.case_id, self.anchor, self.target_pose.offset,
                                 self.target_pose.translation)
        wire = capture_wire(request.id, self.mode, index, evidence, None)
        wire["visual"]["generation"] = self.generation
        if self.visual_overrides is not None:
            wire["visual"]["visual_overrides"] = self.visual_overrides.to_wire()
        wire["visual"]["shader_identity"] = {"requested": True, "status": "complete", "registered": 1,
                                                  "overflow": 0, "invalid": 0}
        if self.changed == "frame" and index == 2:
            wire["frame_ordinal"] = 1
            wire["predicted_display_time_ns"] = 1000
        if self.changed == "pose" and index == 2:
            wire["comparison_inputs"]["camera"]["position"][0] += 0.001
        if self.changed == "generation" and index == 2:
            wire["visual"]["generation"] += 1
        metadata = {"effective_config": {"test": 2 if self.changed == "config" and index == 2 else 1}}
        if self.visual_overrides and self.visual_overrides.screen_space_reflections == "enabled":
            metadata["effective_config"]["stereo"] = {"skip_ssr": False}
        (self.config_dir / "captures" / f"{request.id}.capture.json").write_text(json.dumps(metadata))
        return ScreenshotAck(request.id, Capture.from_wire(wire))


class MatchedExperimentTests(ExperimentTestCase):
    def config(self):
        path = Path(__file__).resolve().parents[1] / "experiments" / "shore_water_matched.toml"
        checked = ExperimentConfig.load(path)
        base = write_config(self.root, ["multipass"], environment=(HOUR, "sunny"))
        return dataclasses.replace(checked, scene=base.scene, interval_seconds=0.0)

    def run_matched(self, runner, config=None):
        (runner.config_dir / "captures").mkdir(parents=True, exist_ok=True)
        now = [0.0]
        def sleep(seconds):
            now[0] += seconds
        with patch("tools.rendering.experiment.validate_capture", side_effect=lambda folder, identifier, capture, **_: capture), \
             patch("tools.rendering.experiment.matched_cross_metrics", return_value=()):
            return run_experiment(runner, config or self.config(), self.root / "out",
                                  monotonic=lambda: now[0], sleep=sleep,
                                  analyze=lambda *_: SimpleNamespace(status=Outcome.INCONCLUSIVE))

    def test_abba_uses_one_target_configuration_and_separate_cases(self):
        runner = MatchedRunner(self.root / "config")
        report = self.run_matched(runner)
        self.assertEqual(report.errors, ())
        self.assertEqual(report.matched_treatment, MatchedTreatment.WATER_REFLECTION_ABBA)
        self.assertEqual(runner.commands.count(Command.CONFIGURE), 2)
        self.assertEqual(runner.commands[-1], Command.END)
        self.assertEqual([r.water_reflection for r in runner.screenshots],
                         [None, WaterReflection.ZERO_WEIGHT, WaterReflection.ZERO_WEIGHT, None] * 3)
        self.assertEqual([x.condition for x in report.captures], ["control", "zero_weight", "zero_weight", "control"] * 3)
        cases = json.loads((self.root / "out" / "manifest.json").read_text())["cases"]
        self.assertEqual([len(c["modes"]["multipass"]) for c in cases], [6, 6])
        ids = [{Path(s["metadata"]).name for s in c["modes"]["multipass"]} for c in cases]
        self.assertFalse(ids[0] & ids[1])

    def test_planar_input_abba_applies_to_every_physical_reference(self):
        path = Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_zero_planar.toml"
        config = dataclasses.replace(ExperimentConfig.load(path), scene=self.config().scene)
        runner = MatchedRunner(self.root / "config")
        report = self.run_matched(runner, config)
        self.assertEqual(report.errors, ())
        for mode in config.configurations:
            self.assertEqual([r.condition for r in report.captures if r.mode == mode],
                             ["control", "zero_planar", "zero_planar", "control"] * 3)
        with self.assertRaises(ExperimentError):
            dataclasses.replace(config, configurations=config.configurations[1:])
        with self.assertRaises(ProtocolError):
            Request(Command.SCREENSHOT, "shot", water_inputs=True, water_reflection=WaterReflection.ZERO_PLANAR)

    def test_identity_changes_fail_closed_and_preserve_evidence(self):
        for changed in ("pose", "config", "generation"):
            with self.subTest(changed=changed), self.subTest():
                root = self.root / changed
                root.mkdir()
                self.root = root
                runner = MatchedRunner(root / "config", changed=changed)
                report = self.run_matched(runner)
                self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
                self.assertTrue(report.errors)
                self.assertEqual(len(report.captures), 2)
                self.assertEqual(runner.commands[-1], Command.END)
                self.assertTrue((root / "out" / "manifest.json").exists())
                self.root = root.parent

    def test_capture_validation_rejects_treatment_wire_mismatch(self):
        anchor = self.config().scene.pose.camera()
        evidence = pose_evidence("target", anchor, OrientationOffset(0, 0, 0), TranslationOffset())
        wire = capture_wire("shot", Mode.MULTIPASS, 1, evidence, None)
        folder = self.root / "captures"
        folder.mkdir()
        Image.new("RGB", (640, 360)).save(folder / "shot.capture.png")
        (folder / "shot.capture.json").write_text(json.dumps(wire))
        with self.assertRaisesRegex(ProtocolError, "water reflection request mismatch"):
            validate_capture(folder, "shot", Capture.from_wire(wire), mode=Mode.MULTIPASS,
                             generation=1, water_reflection=WaterReflection.ZERO_WEIGHT)

    def test_nonincreasing_frame_is_preserved_before_failure(self):
        runner = MatchedRunner(self.root / "config", changed="frame")
        report = self.run_matched(runner)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertIsNone(report.comparison_outcome)
        self.assertIn("increasing rendered frames", report.errors[0])
        self.assertEqual(len(report.captures), 2)
        self.assertEqual(runner.commands[-1], Command.END)
        cases = json.loads((self.root / "out" / "manifest.json").read_text())["cases"]
        self.assertEqual([len(c["modes"]["multipass"]) for c in cases], [1, 1])

    def test_capture_and_restoration_failure_preserve_completed_samples(self):
        runner = MatchedRunner(self.root / "config", fail_at=3, fail_end=True)
        report = self.run_matched(runner)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(len(report.captures), 2)
        self.assertIn("synthetic screenshot failure", report.errors[0])
        self.assertIsNotNone(report.restoration_error)
        self.assertEqual(runner.commands[-1], Command.END)
        cases = json.loads((self.root / "out" / "manifest.json").read_text())["cases"]
        self.assertEqual(sum(len(c["modes"].get("multipass", [])) for c in cases), 2)

    def test_cross_metrics_are_descriptive_and_condition_separated(self):
        cases = {key: Case(key, {Mode.MULTIPASS: [Sample(f"{key}.png", f"{key}.json")]})
                 for key in ("control", "zero_weight")}
        def load(root, mode, index, spec):
            value = 0 if spec.png == "control.png" else 10
            image = np.full((2, 2, 3), value, dtype=np.uint8)
            return SimpleNamespace(capture_id=spec.png, image=lambda eye: image)
        with patch("tools.rendering.experiment_matched.load_sample", side_effect=load):
            metrics = matched_cross_metrics(self.root, cases)
        self.assertEqual(len(metrics), 2)
        self.assertEqual([entry["eye"] for entry in metrics], [0, 1])
        self.assertEqual(metrics[0]["metrics"]["mean"], 10)
        self.assertEqual(metrics[0]["metrics"]["changed_fraction"], 1)
        self.assertNotIn("status", metrics[0])

    def test_shared_references_do_not_enter_cross_condition_metrics(self):
        shared = [Sample('shared.png', 'shared.json')]
        cases = {key: Case(key, {Mode.MULTIPASS: shared.copy(),
                                 Mode.SINGLE_PASS: [Sample(f'{key}.png', f'{key}.json')]})
                 for key in ('base_center', 'base_eye')}
        def load(root, mode, index, spec):
            self.assertEqual(mode.value, 'single_pass')
            return SimpleNamespace(capture_id=spec.png, image=lambda eye: np.zeros((1, 1, 3), np.uint8))
        with patch('tools.rendering.experiment_matched.load_sample', side_effect=load):
            rows = matched_cross_metrics(self.root, cases)
        self.assertEqual([(row['base_center'], row['base_eye'], row['eye']) for row in rows],
                         [('base_center.png', 'base_eye.png', 0), ('base_center.png', 'base_eye.png', 1)])

    def test_cascade_requests_require_resolve_and_fixed_mp_configuration(self):
        path = Path(__file__).resolve().parents[1] / "experiments" / "shore_cascade.toml"
        config = ExperimentConfig.load(path)
        self.assertEqual(config.matched_treatment.conditions, ("shore_low", "shore_high"))
        for level in CascadeScale:
            with self.assertRaises(ProtocolError):
                Request(Command.SCREENSHOT, "shot", cascade_scale=level)
            request = Request(Command.SCREENSHOT, "shot", resolve_inputs=True, cascade_scale=level)
            self.assertIn(f'cascade_scale = "{level.value}"', request.to_toml())
        with self.assertRaises(ExperimentError):
            dataclasses.replace(config, diagnostics=dataclasses.replace(config.diagnostics, resolve_inputs=False))

    def test_late_sky_stages_require_matching_frame_eye_and_packed_width(self):
        for mode, eyes, width in ((Mode.RIGHT_ONLY, (1,), 2), (Mode.MULTIPASS, (0, 1), 2),
                                  (Mode.SINGLE_PASS, (0,), 4)):
            records = []
            for stage in (Stage.AFTER_APPLY_CLOUDS, Stage.BEFORE_FOG_VOLUME_APPLY, Stage.AFTER_FOG_VOLUME_APPLY):
                for eye in eyes:
                    name = f'shot.{stage}.eye{eye}.r11g11b10'
                    (self.root / name).write_bytes(bytes(width * 8))
                    records.append({'stage': stage, 'eye': eye, 'frame': 93, 'error': None, 'raw': name,
                                    'source': {'width': width, 'height': 2, 'format': 'DXGI_FORMAT(26)'}})
            metadata = {'stages': records, 'eye_width': 2, 'eye_height': 2, 'predicted_display_time_ns': 93}
            validate_late_sky_stages(self.root, 'shot', metadata, mode)
            for invalid in (records[:-1], records + records[:1], [dict(records[0], frame=92), *records[1:]],
                            [dict(records[0], raw=records[-1]['raw']), *records[1:]]):
                with self.assertRaises(ProtocolError):
                    validate_late_sky_stages(self.root, 'shot', dict(metadata, stages=invalid), mode)

    def test_surface_request_requires_an_untreated_selected_draw(self):
        path = Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_surface.toml"
        config = ExperimentConfig.load(path)
        self.assertTrue(config.diagnostics.water_surface_inputs)
        with self.assertRaises(ExperimentError):
            dataclasses.replace(config.diagnostics, water_tess_draw=None)
        with self.assertRaises(ProtocolError):
            Request(Command.SCREENSHOT, "shot", water_inputs=True, water_surface_inputs=True)
        with self.assertRaises(ProtocolError):
            Request(Command.SCREENSHOT, "shot", water_inputs=True, water_surface_inputs=True,
                    water_tess_draw=7, water_reflection=WaterReflection.ZERO_WEIGHT)

    def test_eye_width_pairs_only_the_packed_draw_with_the_producer(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_eye_width.toml")
        runner = MatchedRunner(self.root / "config")
        report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.water_reflection for r in runner.screenshots], [None] * 9 +
                         [None, WaterReflection.EYE_WIDTH_PLANAR, WaterReflection.EYE_WIDTH_PLANAR, None] * 3)
        self.assertTrue(all(r.water_surface_inputs and r.water_tess_draw == 7 for r in runner.screenshots))
        with self.assertRaises(ExperimentError):
            dataclasses.replace(config, diagnostics=dataclasses.replace(config.diagnostics, water_tess_draw=6))

    def test_sky_lighting_uses_traced_references_and_one_binding_change(self):
        for name, a, b in (("shore_lake_sky", WaterReflection.LAKE_LIGHTING, WaterReflection.LAKE_SKY),
                           ("shore_lake_sky_buffer", WaterReflection.LAKE_SKY, WaterReflection.LAKE_SKY_BUFFER)):
            with self.subTest(name=name):
                config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / f"{name}.toml")
                runner = MatchedRunner(self.root / name)
                report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
                self.assertEqual(report.errors, ())
                self.assertEqual([r.water_reflection for r in runner.screenshots], [WaterReflection.LAKE_LIGHTING] * 9 + [a, b, b, a] * 3)
                self.assertTrue(all(r.water_surface_inputs and r.water_tess_draw == 7 for r in runner.screenshots))

    def test_group_fade_holds_the_lake_and_atlas_treatments_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_group_fade.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.group_fade for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_card_atlas_holds_the_lake_and_fade_treatments_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_card_atlas.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.card_atlas for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_lake_ds_mvp_holds_physical_cards_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_ds_mvp.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.lake_ds_mvp for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_candidate_depth_holds_physical_mvp_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_candidate_edge.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.lake_depth_edge for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.lake_ds_mvp and r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_color_mirror_holds_depth_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_color_mirror.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.lake_color_mirror for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.lake_depth_edge and r.lake_ds_mvp and r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_owner_four_domain_holds_water_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_owner_domain.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.owner_four_domain for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.lake_color_mirror and r.lake_owner_four and r.lake_depth_edge and r.lake_ds_mvp and r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], group_cards=None)

    def test_patch63_hull_holds_domains_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_patch63_hull.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.patch63_hull for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.owner_four_domain and r.lake_color_mirror and r.lake_owner_four and r.lake_depth_edge and r.lake_ds_mvp and r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], owner_four_domain=False)

    def test_aa_color_edge_holds_geometry_fixed(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_aa_color_edge.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.aa_color_edge for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)
        for r in runner.screenshots[9:]:
            self.assertTrue(r.aa_weights and r.patch63_hull and r.owner_four_domain and r.lake_color_mirror and r.lake_owner_four and r.lake_depth_edge and r.lake_ds_mvp and r.card_atlas and r.lake_producer and r.group_fade and r.group_cards and r.lake_inner_edge and r.group_atlas_scale and r.writer_domain_camera)
        with self.assertRaises(ProtocolError):
            dataclasses.replace(runner.screenshots[-1], aa_weights=False)

    def test_march_coordinates_hold_physical_proxy_constant(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_march_eye.toml")
        runner = MatchedRunner(self.root / "config")
        with patch("tools.rendering.experiment._resolve_content_errors", return_value=[]):
            report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.proxy_eye for r in runner.screenshots], [None] * 9 + [True] * 12)
        self.assertEqual([r.march_eye for r in runner.screenshots], [None] * 9 + [False, True, True, False] * 3)

    def test_domain_camera_reuses_each_physical_reference_once(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_domain_camera.toml")
        runner = MatchedRunner(self.root / "config")
        report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        a, b = WaterReflection.LAKE_CAMERA_CONTROL, WaterReflection.LAKE_DOMAIN_CAMERA
        self.assertEqual([r.water_reflection for r in runner.screenshots], [a] * 9 + [a, b, b, a] * 3)

    def test_production_abba_observes_default_shader_and_requests_pristine_control(self):
        config = ExperimentConfig.load(Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_production_refraction.toml")
        runner = MatchedRunner(self.root / "config")
        report = self.run_matched(runner, dataclasses.replace(config, scene=self.config().scene))
        self.assertEqual(report.errors, ())
        self.assertEqual([r.water_reflection for r in runner.screenshots], [None] * 9 +
                         [WaterReflection.PRISTINE_LAKE, None, None, WaterReflection.PRISTINE_LAKE] * 3)
        for actual, expected in ((LAKE_SHADER, PACKED_LAKE_SHADER), (PACKED_LAKE_SHADER, LAKE_SHADER)):
            identity = {"status": "mapped", "identity": {"actual_sha256": actual, "pristine_sha256": LAKE_SHADER}}
            require_lake_shader(identity, actual)
            with self.assertRaises(ProtocolError):
                require_lake_shader(identity, expected)

    def test_planar_reference_includes_each_physical_eye(self):
        path = Path(__file__).resolve().parents[1] / "experiments" / "shore_lake_planar.toml"
        config = ExperimentConfig.load(path)
        self.assertEqual(tuple(mode.value for mode in config.configurations),
                         ("left_only", "right_only", "multipass", "single_pass"))
        with self.assertRaises(ExperimentError):
            dataclasses.replace(config, configurations=config.configurations[2:])

    def test_configuration_rejects_incompatible_matched_requests(self):
        config = self.config()
        for kwargs in ({"configurations": ()}, {"matched_treatment": "invalid"},
                       {"diagnostics": dataclasses.replace(config.diagnostics, shader_identity=False)}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ExperimentError):
                dataclasses.replace(config, **kwargs)
