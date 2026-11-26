import dataclasses
import json
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np
from PIL import Image

from tools.rendering.capture.validation import (
    validate_resolve_inputs,
    validate_sun_inputs,
    validate_water_inputs,
    validate_water_treatment,
)
from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.comparison.images import Outcome, analyze_manifest
from tools.rendering.comparison.stages import Stage
from tools.rendering.experiment import (
    ExperimentConfig,
    ExperimentError,
    Feature,
    FeatureStatus,
    Mode,
    _resolve_content_errors,
    offset_for_target,
    run_experiment,
    sample_id,
)
from tools.rendering.protocol import (
    Camera,
    Capture,
    Command,
    ConfigurationAck,
    EnvironmentRequest,
    OrientationOffset,
    ProtocolError,
    RestorationAck,
    VisualOverrides,
    WeatherControl,
    WeatherEvent,
    cameras_match,
    compose_pose,
    euler_yxz_quaternion,
    quaternion_multiply,
    quaternion_normalize,
)
from tools.rendering.session.attach import SessionState, owning_process
from tools.rendering.session.errors import SessionError

ANCHOR = Camera((10.0, 12.0, 13.0), (0.0, 0.0, 0.0, 1.0))
TARGET = Camera((1.0, 2.0, 3.0), (0.0, 0.0, 0.0, 1.0))
HOUR = 9.5
LAKE_CONFIG = Path(__file__).resolve().parents[1] / "scenes/lake_glint.toml"
SCENE_DIRECTORY = Path(__file__).resolve().parents[1] / "scenes"
CANONICAL_MODES = (CompareMode.LEFT_ONLY, CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS,
                   CompareMode.SINGLE_PASS)
BASELINE_CAMERA = Camera((8257.701, 1486.9717, -4286.1167),
                         (0.00050330814, 0.80293703, 0.020165263, 0.59572256))
NEAR_VEGETATION_CAMERA = Camera((8257.683, 1486.9708, -4286.15),
                                (-0.04870652, 0.9686239, 0.20377125, 0.13368909))
LAKE_CAMERA = Camera((7586.867, 1514.6827, -4081.8762),
                     (-0.016829107, 0.97209185, 0.23272279, 0.024376998))


def world_yaw(camera, yaw_deg):
    return Camera(camera.position, quaternion_normalize(quaternion_multiply(
        euler_yxz_quaternion(OrientationOffset(yaw_deg, 0.0, 0.0)), camera.orientation)))


SCENES = {
    "lake_glint.toml": ("lake-glint", LAKE_CAMERA),
    "near_vegetation.toml": ("near-vegetation", NEAR_VEGETATION_CAMERA),
    "cliff_view.toml": ("cliff-view",
                        compose_pose(BASELINE_CAMERA, OrientationOffset(12.0, -16.0, 0.0))),
    "far_terrain.toml": ("far-terrain", BASELINE_CAMERA),
    "sky_clouds.toml": ("sky-clouds",
                        compose_pose(BASELINE_CAMERA, OrientationOffset(0.0, 32.0, 0.0))),
    "shore.toml": ("shore", world_yaw(LAKE_CAMERA, 45.0)),
}


def pose_evidence(case_id, anchor, offset, translation):
    applied = compose_pose(anchor, offset, translation)
    return {
        "case_id": case_id,
        "offset": offset.to_wire(),
        "translation": translation.to_wire(),
        "anchor": {"position": list(anchor.position),
                   "orientation": list(anchor.orientation)},
        "applied": {"position": list(applied.position),
                    "orientation": list(applied.orientation)},
    }


def observation(control=WeatherControl.SUNNY, hour=HOUR, severity=0.1):
    return {"hour": hour, "day_cycle_speed": 0.0, "weather_severity": severity,
            "weather_control": control.value}


def configuration_wire(request_id, mode, generation, evidence, environment):
    metadata = {
        "generation": generation,
        "mode": mode.value,
        "profile": "controlled_v2",
        "ready": True,
        "paused": True,
        "frozen": True,
        "error": None,
        "warmup_frames": 12,
        "completed_frames": 12,
        "engine_dimensions": [320, 360],
        "pose": evidence,
        "environment": environment,
    }
    return {"id": request_id, "state": "ready", "metadata": metadata}


def restoration_wire(request_id, generation, environment):
    metadata = {
        "generation": generation,
        "mode": None,
        "profile": "native",
        "ready": True,
        "paused": False,
        "frozen": False,
        "error": None,
        "warmup_frames": 0,
        "completed_frames": 0,
        "engine_dimensions": [320, 360],
        "environment": environment,
    }
    return {"id": request_id, "state": "restored", "metadata": metadata}


def eye_wire():
    return {"offset": [0.0, 0.0, 0.0], "orientation_delta": [0.0, 0.0, 0.0, 1.0],
            "projection_standard": [0.0] * 16, "projection_reverse_z": [0.0] * 16}


def capture_wire(identifier, mode, frame, evidence, environment):
    selected = 0 if mode == Mode.LEFT_ONLY else 1 if mode == Mode.RIGHT_ONLY else None
    rendered = (True, True) if selected is None else (selected == 0, selected == 1)
    width = 640 if selected is None else 320
    return {
        "id": identifier,
        "frame_ordinal": frame,
        "predicted_display_time_ns": frame * 1000,
        "source": "eye_textures" if selected is None else "isolated_eye",
        "width": width,
        "height": 360,
        "eye_width": 320,
        "eye_height": 360,
        "fresh_eyes": [True, True],
        "gameplay_ready": True,
        "xr_session_running": True,
        "mode": mode.value,
        "selected_eye": selected,
        "rendered_eyes": list(rendered),
        "visual": configuration_wire(identifier, mode, 1, evidence, environment)["metadata"],
        "comparison_inputs": {
            "build_stamp": "test",
            "profile": "controlled_v2",
            "camera": {"position": list(TARGET.position),
                       "orientation": list(TARGET.orientation)},
            "eyes": [eye_wire(), eye_wire()],
            "eye_width": 320,
            "eye_height": 360,
            "world_scale": 1.0,
        },
    }


class FakeRunner:
    def __init__(self, config_dir, end_environment="ambient", fail_end=False):
        self.config_dir = Path(config_dir)
        self.deadline = time.monotonic() + 60
        self.pending_request = None
        self.generation = 0
        self.anchor = None
        self.commands = []
        self.requests = []
        self.environment_requested = False
        self.end_environment = end_environment
        self.fail_end = fail_end
        self.assert_running_calls = 0

    def assert_running(self):
        self.assert_running_calls += 1

    def request(self, value):
        self.commands.append(value.command)
        self.requests.append(value)
        if value.command == Command.CONFIGURE:
            if value.environment is not None:
                self.environment_requested = True
            if self.anchor is None:
                self.anchor = ANCHOR
            evidence = pose_evidence(value.pose.case_id, self.anchor, value.pose.offset,
                                     value.pose.translation)
            self.generation += 1
            environment = observation() if value.environment is not None else None
            return ConfigurationAck.from_wire(configuration_wire(
                value.id, value.mode, self.generation, evidence, environment))
        if value.command == Command.END:
            if self.fail_end:
                environment = observation(WeatherControl.SUNNY)
            elif not self.environment_requested or self.end_environment == "missing":
                environment = None
            else:
                environment = observation(WeatherControl.AMBIENT)
            return RestorationAck.from_wire(restoration_wire(
                value.id, self.generation + 1, environment))
        raise AssertionError(f"unexpected request {value.command}")

    def cancel_pending_request(self):
        return False


def write_config(root, configurations, *, feature=None, environment=None,
                 name="synthetic-glint", samples=3, warmup_frames=12,
                 interval_seconds=None):
    lines = [
        f'name = "{name}"',
        f"warmup_frames = {warmup_frames}",
        f"samples = {samples}",
    ]
    if interval_seconds is not None:
        lines.append(f"interval_seconds = {interval_seconds}")
    lines += [
        "",
        "[scene]",
        "position = [1.0, 2.0, 3.0]",
        "orientation = [0.0, 0.0, 0.0, 1.0]",
    ]
    if environment is not None:
        lines += ["", "[scene.environment]", f"hour = {environment[0]}",
                  f'weather = "{environment[1]}"']
    for mode in configurations:
        lines += ["", "[[configurations]]", f'mode = "{mode}"']
    if feature is not None:
        lines += [
            "",
            "[feature]",
            f'stage = "{feature["stage"]}"',
            f"roi = {json.dumps(feature['roi'])}",
            f"luminance = {feature['luminance']}",
            f"min_fraction = {feature['min_fraction']}",
            f"reference_modes = {json.dumps(feature['reference_modes'])}",
            f"target_modes = {json.dumps(feature['target_modes'])}",
        ]
    path = Path(root) / "config.toml"
    path.write_text("\n".join(lines) + "\n")
    return ExperimentConfig.load(path)


def synthetic_feature():
    return {
        "stage": "main_color",
        "roi": [0.0, 0.0, 1.0, 1.0],
        "luminance": 4.0,
        "min_fraction": 0.01,
        "reference_modes": ["multipass"],
        "target_modes": ["single_pass"],
    }


def capture_factory(environment=None):
    frames = iter(range(1, 1000))

    def capture(runner, config, pose, anchor, mode, generation, identifier):
        return Capture.from_wire(capture_wire(
            identifier, mode, next(frames),
            pose_evidence(identifier, anchor, pose.offset, pose.translation),
            environment))

    return capture


class ExperimentTestCase(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def run_config(self, config, *, capture=None, analyze=None, runner=None,
                   monotonic=None, sleep=None):
        runner = runner or FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True, exist_ok=True)
        analyze = analyze or (lambda *_: SimpleNamespace(status=Outcome.PASS))
        return run_experiment(runner, config, self.root / "out",
                              capture=capture or capture_factory(), analyze=analyze,
                              monotonic=monotonic, sleep=sleep)


class ConfigTests(ExperimentTestCase):
    def test_lake_config_loads_ordered_configurations_and_feature(self):
        config = ExperimentConfig.load(LAKE_CONFIG)
        self.assertEqual(config.name, "lake-glint")
        self.assertEqual(config.samples, 3)
        self.assertEqual(config.configurations, CANONICAL_MODES)
        self.assertEqual(config.scene.pose.position, (7586.867, 1514.6827, -4081.8762))
        self.assertEqual(config.scene.environment.hour, 13.226033)
        self.assertEqual(config.scene.environment.weather, WeatherEvent.SUNNY)
        self.assertEqual(config.feature.stage, Stage.MAIN_COLOR)
        self.assertEqual(config.feature.reference_modes, (CompareMode.MULTIPASS,))
        self.assertEqual(config.feature.target_modes, (CompareMode.SINGLE_PASS,))
        self.assertEqual(config.interval_seconds, 0.0)

    def test_checked_in_interval_experiment_configs_load(self):
        root = Path(__file__).resolve().parents[1] / "experiments"
        interval = ExperimentConfig.load(root / "cliff_multipass_interval4.toml")
        control = ExperimentConfig.load(root / "cliff_multipass_control.toml")
        cold = ExperimentConfig.load(root / "cliff_multipass_cold_slice.toml")
        self.assertEqual(interval.interval_seconds, 4.0)
        self.assertEqual(control.interval_seconds, 0.0)
        self.assertEqual(cold.interval_seconds, 4.0)
        self.assertNotEqual(cold.scene.environment.hour, interval.scene.environment.hour)
        for config in (interval, control, cold):
            self.assertEqual(config.samples, 3)
            self.assertEqual(config.configurations,
                             (CompareMode.MULTIPASS, CompareMode.MULTIPASS))
        self.assertEqual(interval.scene.environment.hour, 13.226033)
        self.assertEqual(cold.scene.environment.hour, 9.0)

    def test_checked_in_water_input_experiment(self):
        root = Path(__file__).resolve().parents[1] / "experiments"
        config = ExperimentConfig.load(root / "shore_water_inputs.toml")
        self.assertTrue(config.diagnostics.water_inputs)
        self.assertEqual(config.configurations, CANONICAL_MODES)
        self.assertEqual(config.scene.pose, ExperimentConfig.load(SCENE_DIRECTORY / "shore.toml").scene.pose)
        self.assertEqual(config.samples, 3)
        self.assertEqual(config.scene.environment.hour, 13.226033)

    def test_checked_in_sun_input_experiment(self):
        root = Path(__file__).resolve().parents[1] / "experiments"
        config = ExperimentConfig.load(root / "sky_sun_inputs.toml")
        self.assertTrue(config.diagnostics.sun_inputs)
        self.assertFalse(config.diagnostics.water_inputs)
        self.assertEqual(config.configurations, CANONICAL_MODES)
        self.assertEqual(config.samples, 3)
        self.assertEqual(config.scene.environment.hour, 13.226033)

    def test_identity_and_depth_clip_experiments_are_explicit(self):
        root = Path(__file__).resolve().parents[1] / "experiments"
        water = ExperimentConfig.load(root / "shore_water_identity.toml")
        sun = ExperimentConfig.load(root / "sky_sun_depth_clip.toml")
        self.assertTrue(water.diagnostics.water_inputs)
        self.assertTrue(water.diagnostics.shader_identity)
        self.assertTrue(sun.diagnostics.sun_inputs)
        self.assertEqual(sun.diagnostics.sun_depth_clip.value, "disabled_right")
        self.assertEqual(sun.configurations, CANONICAL_MODES)

    def test_water_treatment_experiments_are_matched(self):
        root = Path(__file__).resolve().parents[1] / "experiments"
        control = ExperimentConfig.load(root / "shore_water_control.toml")
        treated = ExperimentConfig.load(root / "shore_water_zero_weight.toml")
        self.assertIsNone(control.diagnostics.water_reflection)
        self.assertEqual(treated.diagnostics.water_reflection.value, "zero_weight")
        self.assertEqual(control.scene, treated.scene)
        self.assertEqual(control.configurations, treated.configurations)
        self.assertEqual((control.samples, control.interval_seconds), (treated.samples, treated.interval_seconds))

    def test_diagnostics_default_and_type_validation(self):
        config = write_config(self.root, ["multipass"])
        self.assertFalse(config.diagnostics.water_inputs)
        self.assertFalse(config.diagnostics.sun_inputs)
        path = self.root / "config.toml"
        original = path.read_text()
        for value in ("1", '"true"', "[]"):
            path.write_text(original + f"\n[diagnostics]\nwater_inputs = {value}\n")
            with self.subTest(value=value), self.assertRaises(ExperimentError):
                ExperimentConfig.load(path)
        path.write_text(original + "\n[diagnostics]\nunknown = true\n")
        with self.assertRaises(ExperimentError):
            ExperimentConfig.load(path)
        for diagnostic in ('water_reflection = "zero_weight"',
                           'water_reflection = "zero_weight"\nwater_inputs = true',
                           'water_reflection = "other"\nwater_inputs = true\nshader_identity = true'):
            path.write_text(original + f"\n[diagnostics]\n{diagnostic}\n")
            with self.subTest(diagnostic=diagnostic), self.assertRaises(ExperimentError):
                ExperimentConfig.load(path)

    def test_interval_seconds_loads_and_must_be_finite_nonnegative(self):
        config = write_config(self.root, ["multipass"], interval_seconds=2.5)
        self.assertEqual(config.interval_seconds, 2.5)
        for value in ("-1", "inf", "true", '"2.5"'):
            path = self.root / "config.toml"
            path.write_text(
                'name = "synthetic-glint"\n'
                "warmup_frames = 12\n"
                "samples = 3\n"
                f"interval_seconds = {value}\n"
                "\n[scene]\n"
                "position = [1.0, 2.0, 3.0]\n"
                "orientation = [0.0, 0.0, 0.0, 1.0]\n"
                "\n[[configurations]]\n"
                'mode = "multipass"\n'
            )
            with self.assertRaises(ExperimentError):
                ExperimentConfig.load(path)

    def test_scene_configs_load_canonical_modes_and_verified_poses(self):
        self.assertEqual(sorted(path.name for path in SCENE_DIRECTORY.glob("*.toml")),
                         sorted(SCENES))
        poses = []
        for filename, (name, camera) in SCENES.items():
            with self.subTest(scene=filename):
                config = ExperimentConfig.load(SCENE_DIRECTORY / filename)
                self.assertEqual(config.name, name)
                self.assertEqual(config.warmup_frames, 30)
                self.assertEqual(config.samples, 3)
                self.assertEqual(config.configurations, CANONICAL_MODES)
                self.assertEqual(config.scene.environment.hour, 13.226033)
                self.assertEqual(config.scene.environment.weather, WeatherEvent.SUNNY)
                self.assertTrue(cameras_match(camera, config.scene.pose.camera()))
                poses.append(config.scene.pose)
        self.assertEqual(len(set(poses)), len(SCENES))

    def test_synthetic_config_accepts_arbitrary_order_and_repeats(self):
        config = write_config(self.root, ["left_only", "multipass", "right_only",
                                          "single_pass", "multipass"])
        self.assertEqual(config.configurations, (CompareMode.LEFT_ONLY,
                                                 CompareMode.MULTIPASS,
                                                 CompareMode.RIGHT_ONLY,
                                                 CompareMode.SINGLE_PASS,
                                                 CompareMode.MULTIPASS))
        self.assertIsNone(config.feature)
        self.assertIsNone(config.scene.environment)

    def test_optional_environment_and_feature_load(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature(),
                              environment=(HOUR, "sunny"))
        self.assertEqual(config.scene.environment.hour, HOUR)
        self.assertEqual(config.scene.environment.weather, WeatherEvent.SUNNY)
        self.assertIsNotNone(config.feature)

    def test_feature_reference_after_target_is_rejected(self):
        with self.assertRaises(ExperimentError):
            write_config(self.root, ["single_pass", "multipass"],
                         feature=synthetic_feature())

    def test_feature_mode_must_be_configured(self):
        with self.assertRaises(ExperimentError):
            write_config(self.root, ["multipass"], feature=synthetic_feature())

    def test_unknown_configuration_mode_is_rejected(self):
        with self.assertRaises(ExperimentError):
            write_config(self.root, ["quadpass"])

    def test_feature_must_be_main_color(self):
        feature = synthetic_feature()
        feature["stage"] = "pre_aa"
        with self.assertRaises(ExperimentError):
            write_config(self.root, ["multipass", "single_pass"], feature=feature)


class WaterInputValidationTests(ExperimentTestCase):
    def test_resolve_content_errors_preserve_failed_status(self):
        metadata = {"resolve_inputs": {"eyes": [
            {"status": "observed", "observation": {"constant_buffers": [
                {"slot": 0, "content": {"status": "unbound"}},
                {"slot": 3, "content": {"status": "captured"}},
            ]}}, {"status": "not_required"}]}}
        self.assertEqual(_resolve_content_errors("shot", metadata),
                         ["resolve inputs: shot: eye 0 cb0: unbound"])
        metadata["resolve_inputs"]["eyes"][0]["observation"]["constant_buffers"][0]["content"] = {"status": "captured"}
        self.assertEqual(_resolve_content_errors("shot", metadata), [])

    def test_water_treatment_checks_draws_and_zero_artifacts(self):
        records = []
        for eye in (0, 1):
            raw = f"shot.weight{eye}.raw"
            (self.root / raw).write_bytes(bytes(4))
            records.extend([
                {"stage": "water_reflection_treatment", "eye": eye, "frame": 42, "error": None,
                 "water_input": {"capture_id": "shot", "treatment_draws": 1}},
                {"stage": "water_input_reflection_alpha", "eye": eye, "raw": raw,
                 "source": {"width": 1, "height": 1, "bytes_per_pixel": 4, "format_id": 41}},
            ])
        metadata = {"predicted_display_time_ns": 42, "stages": records}
        validate_water_treatment(self.root, "shot", metadata, Mode.MULTIPASS)
        records[0]["water_input"]["treatment_draws"] = 0
        with self.assertRaises(ProtocolError):
            validate_water_treatment(self.root, "shot", metadata, Mode.MULTIPASS)
        records[0]["water_input"]["treatment_draws"] = 1
        (self.root / "shot.weight0.raw").write_bytes(b"1234")
        with self.assertRaises(ProtocolError):
            validate_water_treatment(self.root, "shot", metadata, Mode.MULTIPASS)

    def test_resolve_validation_checks_bindings_and_ranges(self):
        shader = {"status": "mapped", "identity": {"pristine_sha256": "a" * 64, "actual_sha256": "b" * 64}}
        observation = {"frame_counters": {"counter": 1, "frame_index": 2, "ring_index": 3},
                       "pixel_shader": {"pointer": 1, "identity": shader},
                       "constant_buffers": [{"slot": i, "pointer": 0, "byte_width": 0,
                                             "first_constant": 0, "num_constants": 0} for i in range(4)],
                       "resources": [{"slot": i, "view": 0, "resource": 0, "format": 0,
                                      "dimension": 0, "texture": None} for i in (*range(15), *range(16, 32), 44, 45)]}
        for slot in (0, 3):
            observation["constant_buffers"][slot]["content"] = {"status": "unbound"}
        metadata = {"predicted_display_time_ns": 42, "visual": {"generation": 7},
                    "resolve_inputs": {"frame": 42, "generation": 7,
                                       "eyes": [{"status": "observed", "observation": observation},
                                                {"status": "not_required"}]}}
        validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        buffer = observation["constant_buffers"][0]
        buffer.update(pointer=1, byte_width=32, first_constant=0, num_constants=2,
                      content={"status": "captured", "raw": "shot.resolve_cb0.eye0.raw"})
        raw = self.root / "shot.resolve_cb0.eye0.raw"
        raw.write_bytes(bytes(32))
        validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        raw.write_bytes(bytes(16))
        with self.assertRaises(ProtocolError):
            validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        buffer.update(pointer=0, byte_width=0, first_constant=0, num_constants=0,
                      content={"status": "unbound"})
        for status in ("unknown_range", "empty_range", "oversize", "invalid_range", "read_error"):
            buffer["content"] = {"status": status, "error": "read failed"} if status == "read_error" else {"status": status}
            validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        buffer["content"] = {"status": "read_error"}
        with self.assertRaises(ProtocolError):
            validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        buffer["content"] = {"status": "unbound"}
        observation["constant_buffers"][0]["num_constants"] = -1
        with self.assertRaises(ProtocolError):
            validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)
        observation["constant_buffers"][0]["num_constants"] = 0
        observation["resources"][0]["slot"] = 44
        with self.assertRaises(ProtocolError):
            validate_resolve_inputs(self.root, "shot", metadata, Mode.LEFT_ONLY, True)

    def test_requested_sun_inputs_require_both_stages_and_matching_identity(self):
        records = []
        for stage in ("pre_sun_color", "pre_sun_depth"):
            name = f"shot.{stage}.raw"
            (self.root / name).write_bytes(bytes(16))
            records.append({"stage": stage, "eye": 0, "error": None, "raw": name,
                            "source": {"width": 2, "height": 2, "bytes_per_pixel": 4,
                                       "format_id": 26 if stage == "pre_sun_color" else 39},
                            "sun_input": {"capture_id": "shot", "view": 1, "resource": 2,
                                          "view_format": 26 if stage == "pre_sun_color" else 40,
                                          "view_dimension": 3, "mip": 0}})
        validate_sun_inputs(self.root, "shot", {"stages": records}, Mode.LEFT_ONLY)
        with self.assertRaises(ProtocolError):
            validate_sun_inputs(self.root, "shot", {"stages": records[:1]}, Mode.LEFT_ONLY)
        records[1]["sun_input"]["capture_id"] = "wrong"
        with self.assertRaises(ProtocolError):
            validate_sun_inputs(self.root, "shot", {"stages": records}, Mode.LEFT_ONLY)

    def test_all_requested_stages_are_validated(self):
        records = []
        for eye in (0, 1):
            for stage, slot, index in (("water_input_reflection", "reflection", 1),
                                       ("water_input_reflection_alpha", "reflection_alpha", 12),
                                       ("water_input_refraction", "refraction", 3)):
                name = f"shot.{stage}.eye{eye}.raw"
                (self.root / name).write_bytes(bytes(16))
                records.append({"stage": stage, "eye": eye, "error": None, "raw": name,
                                "source": {"width": 2, "height": 2, "bytes_per_pixel": 4, "format_id": 26},
                                "water_input": {"capture_id": "shot", "slot": slot,
                                                "shader_slot": index, "pixel_shader": 10,
                                                "draw_index": 3, "tessellated": True,
                                                "view_format": 26, "view_dimension": 4,
                                                "most_detailed_mip": 0, "mip_levels": 1,
                                                "resource": 20, "resource_format": 26}})
        validate_water_inputs(self.root, "shot", {"stages": records}, Mode.MULTIPASS)
        with self.assertRaises(ProtocolError):
            validate_water_inputs(self.root, "shot", {"stages": records[:-1]}, Mode.MULTIPASS)
        with self.assertRaises(ProtocolError):
            validate_water_inputs(self.root, "shot", {"stages": records + records[:1]}, Mode.MULTIPASS)
        records[0]["water_input"]["capture_id"] = "wrong"
        with self.assertRaises(ProtocolError):
            validate_water_inputs(self.root, "shot", {"stages": records}, Mode.MULTIPASS)


class SampleIdTests(ExperimentTestCase):
    def test_repeated_modes_use_a_global_per_mode_counter(self):
        config = write_config(self.root, ["multipass", "single_pass", "multipass"],
                              samples=3)
        identifiers = []

        def capture(runner, config, pose, anchor, mode, generation, identifier):
            identifiers.append(identifier)
            return Capture.from_wire(capture_wire(
                identifier, mode, len(identifiers),
                pose_evidence(identifier, anchor, pose.offset, pose.translation), None))

        report = self.run_config(config, capture=capture)
        self.assertEqual(report.outcome, Outcome.PASS)
        self.assertEqual([item.capture_id.rsplit("_", 1)[1] for item in report.captures],
                         ["1", "2", "3", "1", "2", "3", "4", "5", "6"])
        self.assertEqual(len(set(identifiers)), len(identifiers))
        multipass = [item for item in identifiers if "_multipass_" in item]
        single = [item for item in identifiers if "_single_pass_" in item]
        self.assertTrue(multipass[0].endswith("_multipass_1"))
        self.assertTrue(multipass[-1].endswith("_multipass_6"))
        self.assertTrue(single[0].endswith("_single_pass_1"))
        self.assertTrue(single[-1].endswith("_single_pass_3"))

    def test_interval_seconds_spaces_completed_captures_across_configurations(self):
        config = write_config(self.root, ["multipass", "multipass"], samples=3,
                              interval_seconds=4.0)
        base = capture_factory()
        now = [0.0]
        sleeps = []
        captures = []

        def monotonic():
            return now[0]

        def sleep(seconds):
            sleeps.append(seconds)
            now[0] += seconds

        def capture(*args, **kwargs):
            now[0] += 0.25
            captures.append(args)
            return base(*args, **kwargs)

        self.run_config(config, capture=capture, monotonic=monotonic, sleep=sleep)
        self.assertEqual(len(captures), 6)
        self.assertAlmostEqual(sum(sleeps), 5 * 4.0)
        self.assertTrue(all(0 < gap <= 0.2 for gap in sleeps))

    def test_interval_seconds_stops_at_the_deadline_without_another_capture(self):
        config = write_config(self.root, ["multipass"], samples=3, interval_seconds=4.0)
        base = capture_factory()
        captures = []
        now = [0.0]
        sleeps = []

        def monotonic():
            return now[0]

        def sleep(seconds):
            sleeps.append(seconds)
            now[0] += seconds

        def capture(*args, **kwargs):
            now[0] += 0.25
            captures.append(args)
            return base(*args, **kwargs)

        runner = FakeRunner(self.root / "config")
        runner.deadline = 2.0
        report = self.run_config(config, capture=capture, runner=runner,
                                 monotonic=monotonic, sleep=sleep)
        self.assertEqual(len(captures), 1)
        self.assertLessEqual(now[0], 2.0)
        self.assertIn("deadline", " ".join(report.errors))
        self.assertGreater(runner.assert_running_calls, 1)

    def test_zero_interval_seconds_never_sleeps(self):
        config = write_config(self.root, ["multipass"], samples=3)
        sleeps = []
        self.run_config(config, sleep=sleeps.append)
        self.assertEqual(sleeps, [])

    def test_pair_stem_is_shared_between_modes(self):
        prefix = "synthetic_ab12cd34"
        single = sample_id(prefix, CompareMode.SINGLE_PASS, 1)
        multipass = sample_id(prefix, CompareMode.MULTIPASS, 1)
        self.assertEqual(single.replace("_single_pass_", "_"),
                         multipass.replace("_multipass_", "_"))


class RunTests(ExperimentTestCase):
    def test_resolve_read_error_retains_captures_and_comparison_outcome(self):
        config = write_config(self.root, ["multipass"])
        config = dataclasses.replace(config, diagnostics=dataclasses.replace(
            config.diagnostics, resolve_inputs=True))
        base = capture_factory()

        def capture(runner, config, pose, anchor, mode, generation, identifier):
            result = base(runner, config, pose, anchor, mode, generation, identifier)
            folder = runner.config_dir / "captures"
            eyes = []
            for eye in range(2):
                buffers = []
                for slot in range(4):
                    buffer = {"slot": slot, "pointer": 1, "byte_width": 16,
                              "first_constant": 0, "num_constants": 1}
                    if slot in (0, 3):
                        if eye == 0 and slot == 3:
                            buffer["content"] = {"status": "read_error", "error": "map failed"}
                        else:
                            raw = f"{identifier}.resolve_cb{slot}.eye{eye}.raw"
                            (folder / raw).write_bytes(bytes(16))
                            buffer["content"] = {"status": "captured", "raw": raw}
                    buffers.append(buffer)
                eyes.append({"status": "observed", "observation": {
                    "frame_counters": {"counter": 1, "frame_index": 0, "ring_index": 0},
                    "pixel_shader": {"pointer": 1, "identity": {"status": "unarmed"}},
                    "constant_buffers": buffers,
                    "resources": [{"slot": slot, "view": 0, "resource": 0,
                                   "format": 0, "dimension": 0, "texture": None}
                                  for slot in (*range(15), *range(16, 32), 44, 45)],
                }})
            metadata = {"predicted_display_time_ns": result.predicted_display_time_ns,
                        "visual": {"generation": generation},
                        "resolve_inputs": {"frame": result.predicted_display_time_ns,
                                           "generation": generation, "eyes": eyes}}
            validate_resolve_inputs(folder, identifier, metadata, mode, False)
            (folder / f"{identifier}.capture.json").write_text(json.dumps(metadata))
            return result

        report = self.run_config(config, capture=capture)
        self.assertEqual(report.comparison_outcome, Outcome.PASS)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(len(report.captures), 3)
        manifest = json.loads((self.root / "out" / "manifest.json").read_text())
        self.assertEqual(len(manifest["cases"][0]["modes"]["multipass"]), 3)
        self.assertEqual(len(report.errors), 3)
        self.assertTrue(all("eye 0 cb3: read_error" in error for error in report.errors))
        self.assertIsNone(report.restoration_error)

    def test_anchor_uses_the_first_configured_mode(self):
        config = write_config(self.root, ["left_only", "multipass", "right_only",
                                          "single_pass"])
        runner = FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True)
        report = self.run_config(config, runner=runner)
        configure = [request for request in runner.requests
                     if request.command == Command.CONFIGURE]
        self.assertEqual(configure[0].mode, Mode.LEFT_ONLY)
        self.assertEqual([request.mode for request in configure[1:]],
                         [Mode.LEFT_ONLY, Mode.MULTIPASS, Mode.RIGHT_ONLY,
                          Mode.SINGLE_PASS])
        self.assertEqual(report.outcome, Outcome.PASS)
        self.assertEqual(report.comparison_outcome, Outcome.PASS)

    def test_analysis_runs_once_and_manifest_is_written(self):
        config = write_config(self.root, ["multipass", "single_pass"])
        calls = []

        def analyze(manifest, output):
            calls.append((manifest, output))
            return SimpleNamespace(status=Outcome.PASS)

        report = self.run_config(config, analyze=analyze)
        self.assertEqual(len(calls), 1)
        self.assertTrue((self.root / "out" / "manifest.json").is_file())
        self.assertEqual(report.comparison_outcome, Outcome.PASS)
        self.assertEqual(report.comparison_report,
                         str(self.root / "out" / "comparison" / "report.json"))

    def test_missing_isolated_modes_are_inconclusive_even_with_bright_feature(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())

        def analyze(manifest, output):
            return SimpleNamespace(status=Outcome.INCONCLUSIVE)

        def fractions(feature, root, capture, index):
            return (0.05, 0.05) if capture.mode == Mode.MULTIPASS else (0.05, 0.0)

        with patch("tools.rendering.experiment._bright_fractions", side_effect=fractions):
            report = self.run_config(config, analyze=analyze)
        self.assertEqual(report.comparison_outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.feature.status, FeatureStatus.DIFFERENCE)

    def test_strict_analyzer_reports_inconclusive_for_only_paired_modes(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())
        runner = FakeRunner(self.root / "config")
        folder = runner.config_dir / "captures"
        folder.mkdir(parents=True)
        frames = iter(range(1, 1000))

        def capture(runner, config, pose, anchor, mode, generation, identifier):
            frame = next(frames)
            wire = capture_wire(identifier, mode, frame,
                                pose_evidence(identifier, anchor, pose.offset,
                                              pose.translation), None)
            (folder / f"{identifier}.capture.json").write_text(json.dumps(wire))
            Image.new("RGB", (wire["width"], wire["height"]), (10, 10, 10)).save(
                folder / f"{identifier}.capture.png")
            return Capture.from_wire(wire)

        def fractions(feature, root, capture, index):
            return (0.05, 0.05) if capture.mode == Mode.MULTIPASS else (0.05, 0.0)

        with patch("tools.rendering.experiment._bright_fractions", side_effect=fractions):
            report = run_experiment(runner, config, self.root / "out", capture=capture)
        self.assertEqual(report.comparison_outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.feature.status, FeatureStatus.DIFFERENCE)
        self.assertTrue((self.root / "out" / "comparison" / "report.json").is_file())

    def test_dim_reference_stops_before_target_configurations(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())
        runner = FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True)
        with patch("tools.rendering.experiment._bright_fractions",
                   return_value=(0.0, 0.0)):
            report = self.run_config(config, runner=runner)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.comparison_outcome, Outcome.PASS)
        self.assertEqual(len(report.captures), 3)
        self.assertTrue(all(item.mode == CompareMode.MULTIPASS
                            for item in report.captures))
        self.assertEqual(runner.commands.count(Command.CONFIGURE), 2)
        self.assertEqual(runner.commands.count(Command.END), 1)
        self.assertEqual(report.feature.status, FeatureStatus.INCONCLUSIVE)

    def test_feature_ineligible_cannot_make_comparison_pass(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())
        with patch("tools.rendering.experiment._bright_fractions",
                   return_value=(0.0, 0.0)):
            report = self.run_config(config)
        self.assertEqual(report.comparison_outcome, Outcome.PASS)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertEqual(report.feature.status, FeatureStatus.INCONCLUSIVE)
        self.assertTrue(report.feature.errors)

    def test_target_retaining_brightness_is_reported_separately(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())

        def analyze(manifest, output):
            return SimpleNamespace(status=Outcome.FAIL)

        with patch("tools.rendering.experiment._bright_fractions",
                   return_value=(0.05, 0.05)):
            report = self.run_config(config, analyze=analyze)
        self.assertEqual(report.comparison_outcome, Outcome.FAIL)
        self.assertEqual(report.outcome, Outcome.FAIL)
        self.assertEqual(report.feature.status, FeatureStatus.MATCH)

    def test_optional_environment_does_not_require_an_observation(self):
        config = write_config(self.root, ["multipass", "single_pass"])
        runner = FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True)
        report = self.run_config(config, runner=runner)
        configure = [request for request in runner.requests
                     if request.command == Command.CONFIGURE]
        self.assertTrue(all(request.environment is None for request in configure))
        self.assertEqual(report.environment_restoration, "not_requested")
        self.assertIsNone(report.restoration_error)
        self.assertEqual(report.outcome, Outcome.PASS)

    def test_configured_environment_is_requested_and_restored(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              environment=(HOUR, "sunny"))
        runner = FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True)
        report = self.run_config(config, runner=runner)
        configure = [request for request in runner.requests
                     if request.command == Command.CONFIGURE]
        self.assertTrue(all(request.environment == EnvironmentRequest(HOUR, WeatherEvent.SUNNY)
                            for request in configure))
        self.assertEqual(report.environment_restoration, "ambient")
        self.assertIsNone(report.restoration_error)

    def test_capture_failure_still_ends_and_preserves_the_failure(self):
        config = write_config(self.root, ["multipass", "single_pass"])
        runner = FakeRunner(self.root / "config")
        (runner.config_dir / "captures").mkdir(parents=True)
        analysis = []

        def capture(*_):
            raise ExperimentError("capture failed")

        report = self.run_config(
            config, runner=runner, capture=capture,
            analyze=lambda *_: analysis.append(True) or SimpleNamespace(status=Outcome.PASS))
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertIsNone(report.comparison_outcome)
        self.assertEqual(report.errors, ("capture failed",))
        self.assertEqual(runner.commands[-1], Command.END)
        self.assertEqual(runner.commands.count(Command.END), 1)
        self.assertEqual(analysis, [])
        self.assertEqual(json.loads((self.root / "out" / "report.json").read_text())["outcome"],
                         "inconclusive")

    def test_end_failure_does_not_mask_the_capture_failure(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              environment=(HOUR, "sunny"))
        runner = FakeRunner(self.root / "config", fail_end=True)
        (runner.config_dir / "captures").mkdir(parents=True)

        def capture(*_):
            raise ExperimentError("capture failed")

        report = self.run_config(config, runner=runner, capture=capture)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertIn("capture failed", report.errors)
        self.assertIn("not released to ambient", report.restoration_error)
        self.assertTrue(any("restoration" in error for error in report.errors))
        self.assertEqual(runner.commands.count(Command.END), 1)

    def test_restoration_failure_keeps_the_raw_comparison_outcome(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              environment=(HOUR, "sunny"))
        runner = FakeRunner(self.root / "config", fail_end=True)
        (runner.config_dir / "captures").mkdir(parents=True)
        report = self.run_config(config, runner=runner)
        self.assertEqual(report.comparison_outcome, Outcome.PASS)
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertIn("not released to ambient", report.restoration_error)

    def test_report_wire_is_json_serializable(self):
        config = write_config(self.root, ["multipass", "single_pass"],
                              feature=synthetic_feature())
        with patch("tools.rendering.experiment._bright_fractions",
                   return_value=(0.05, 0.05)):
            report = self.run_config(config)
        wire = json.loads(json.dumps(report.to_wire()))
        self.assertEqual(wire["feature"]["status"], "match")
        self.assertEqual(wire["outcome"], "pass")
        self.assertEqual(wire["interval_seconds"], 0.0)


class FeatureTests(unittest.TestCase):
    def feature(self, roi):
        return Feature(Stage.MAIN_COLOR, roi, 4.0, 0.01,
                       (CompareMode.MULTIPASS,), (CompareMode.SINGLE_PASS,))

    def test_bright_fraction_selects_the_configured_roi(self):
        feature = self.feature((0.0, 0.0, 1.0, 1.0))
        bright = np.full((10, 10, 3), 10.0)
        dim = np.full((10, 10, 3), 0.1)
        self.assertEqual(feature.bright_fraction(bright), 1.0)
        self.assertEqual(feature.bright_fraction(dim), 0.0)

    def test_bright_fraction_ignores_pixels_outside_the_roi(self):
        feature = self.feature((0.0, 0.0, 0.5, 1.0))
        values = np.zeros((10, 10, 3))
        values[:, 5:] = 10.0
        self.assertEqual(feature.bright_fraction(values), 0.0)

    def test_feature_rejects_non_main_color_stage(self):
        with self.assertRaises(ExperimentError):
            Feature(Stage.PRE_AA, (0.0, 0.0, 1.0, 1.0), 4.0, 0.01,
                    (CompareMode.MULTIPASS,), (CompareMode.SINGLE_PASS,))


class StrictAnalyzerTests(ExperimentTestCase):
    def test_missing_isolated_modes_are_inconclusive(self):
        manifest = self.root / "manifest.json"
        manifest.write_text(json.dumps({"cases": [{"name": "synthetic", "modes": {}}]}))
        report = analyze_manifest(manifest, self.root / "comparison")
        self.assertEqual(report.status, Outcome.INCONCLUSIVE)
        self.assertTrue(report.cases)
        self.assertTrue(report.cases[0].modes)


class OwnershipTests(unittest.TestCase):
    def test_only_the_game_process_owns_the_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary)
            config_dir = proc / "config"
            config_dir.mkdir()
            state = SessionState("abc123", config_dir)
            for pid, comm, argv in ((10, "justcause3.exe", ["Z:\\JustCause3.exe"]),
                                     (11, "wineserver", ["wineserver"]),
                                     (12, "monado-service", ["monado-service"]),
                                     (13, "python3", ["python3", "--spawn", "justcause3.exe"])):
                entry = proc / str(pid)
                entry.mkdir()
                (entry / "comm").write_text(comm + "\n")
                (entry / "cmdline").write_bytes(b"\0".join(item.encode() for item in argv) + b"\0")
                (entry / "environ").write_bytes(
                    f"JC3VR_HARNESS_TOKEN=abc123\0JC3VR_CONFIG_DIR={config_dir}\0".encode())
            self.assertEqual(owning_process(state, proc), 10)


class AttachTests(unittest.TestCase):
    def test_runner_requires_a_token_and_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "runner.json"
            path.write_text(json.dumps({"token": "abc123", "config_dir": temporary}))
            state = SessionState.load(path)
            self.assertEqual(state.token, "abc123")
            path.write_text(json.dumps({"token": "abc123", "config_dir": "/does/not/exist"}))
            with self.assertRaises(SessionError):
                SessionState.load(path)


class CompositionTests(unittest.TestCase):
    def test_composition_reaches_the_synthetic_target(self):
        offset, translation = offset_for_target(ANCHOR, TARGET)
        applied = compose_pose(ANCHOR, offset, translation)
        self.assertLess(max(abs(a - b) for a, b in zip(applied.position, TARGET.position)), 1e-4)
        self.assertLess(max(abs(a - b) for a, b in
                            zip(quaternion_normalize(applied.orientation),
                                quaternion_normalize(TARGET.orientation))), 1e-6)


if __name__ == "__main__":
    unittest.main()
