"""Scene staging, pinned weather, and moving-camera experiments."""

import dataclasses
import pathlib
import re
import tomllib
import unittest

import numpy as np

from tools.rendering.experiment import ExperimentConfig, ExperimentError, _check_observation
from tools.rendering.protocol import (
    Camera,
    Command,
    EnvironmentObservation,
    EnvironmentRequest,
    MotionRequest,
    OrientationOffset,
    Placement,
    SHIPPED_OVERRIDES,
    ProtocolError,
    StageAck,
    StageRequest,
    StageSpawn,
    TranslationOffset,
    VisualOverrides,
    WeatherControl,
    WeatherEvent,
    acknowledgement_from_wire,
    compose_pose,
    quaternion_normalize,
)
from tools.rendering.protocol.motion import MotionEvidence
from tools.rendering.tests.test_experiment import (
    ExperimentTestCase,
    FakeRunner,
    capture_factory,
)

MODES = "".join(f'\n[[configurations]]\nmode = "{mode}"\n'
                for mode in ("left_only", "right_only", "multipass", "single_pass"))
STAGE = """
[scene.stage]
settle_seconds = 15.0

[scene.stage.location]
position = [3106.1, 1045.5, 1234.0]
yaw_deg = 45.0

[[scene.stage.spawn]]
tags = "vehicle,ground,car"
position = [3112.0, 1045.0, 1240.0]
"""


def config_text(scene, extra=""):
    return f'name = "staged"\nwarmup_frames = 12\nsamples = 3\n{extra}\n{scene}\n{MODES}'


class StageProtocolTests(unittest.TestCase):
    def test_a_request_writes_the_payload_table(self):
        request = StageRequest("stage_1", 15, Placement((1, 2, 3), 90),
                               (StageSpawn("prop,barrel", Placement((4, 5, 6))),))
        table = tomllib.loads(request.to_toml())
        self.assertEqual(table["command"], "visual_stage")
        with_weather = dataclasses.replace(request, environment=EnvironmentRequest(22.5, WeatherEvent.RAIN))
        self.assertEqual(tomllib.loads(with_weather.to_toml())["stage"]["environment"],
                         {"hour": 22.5, "weather": "rain"})
        self.assertEqual(table["stage"], {
            "settle_seconds": 15.0,
            "location": {"position": [1.0, 2.0, 3.0], "yaw_deg": 90.0},
            "spawn": [{"tags": "prop,barrel", "position": [4.0, 5.0, 6.0], "yaw_deg": 0.0}],
        })

    def test_invalid_requests_are_rejected(self):
        for build in (
            lambda: StageRequest("stage_1", 15),
            lambda: StageRequest("stage_1", -1, Placement((0, 0, 0))),
            lambda: StageRequest("stage_1", 301, Placement((0, 0, 0))),
            lambda: StageRequest("bad id", 1, Placement((0, 0, 0))),
            lambda: Placement((0, float("nan"), 0)),
            lambda: StageSpawn("  ", Placement((0, 0, 0))),
            lambda: StageRequest.from_toml("stage_1", {"settle_seconds": 1, "weather": "rain",
                                                       "location": {"position": [0, 0, 0]}}),
            lambda: StageRequest.from_toml("stage_1", {"location": {"position": [0, 0, 0]}}),
        ):
            with self.assertRaises(ProtocolError):
                build()

    def test_the_acknowledgement_reports_where_the_player_landed(self):
        ack = acknowledgement_from_wire({
            "id": "stage_1", "state": "staged", "player_position": [1.0, 2.0, 3.5],
            "distance_m": 0.5, "spawns_submitted": 2, "spawns_present": 1,
        }, Command.STAGE, "stage_1")
        self.assertEqual(ack, StageAck("stage_1", (1.0, 2.0, 3.5), 0.5, 2, 1))
        failed = acknowledgement_from_wire({"id": "stage_1", "state": "failed", "error": "busy"},
                                           Command.STAGE, "stage_1")
        self.assertEqual(failed.error, "busy")
        with self.assertRaises(ProtocolError):
            acknowledgement_from_wire({"id": "stage_1", "state": "ready"}, Command.STAGE, "stage_1")


class StageExperimentTests(ExperimentTestCase):
    def load(self, text):
        path = self.root / "config.toml"
        path.write_text(text)
        return ExperimentConfig.load(path)

    def test_a_staged_config_stages_before_the_first_configuration(self):
        config = self.load(config_text(
            '[scene]\nposition = [1.0, 2.0, 3.0]\norientation = [0.0, 0.0, 0.0, 1.0]\n' + STAGE))
        self.assertEqual(config.scene.stage.location, Placement((3106.1, 1045.5, 1234.0), 45.0))

        class StagingRunner(FakeRunner):
            def request(self, value):
                if value.command == Command.STAGE:
                    self.commands.append(value.command)
                    self.stage = value
                    return StageAck(value.id, (3106.0, 1044.0, 1234.0), 1.5, 1, 0)
                return super().request(value)

        runner = StagingRunner(self.root / "config")
        report = self.run_config(config, runner=runner, capture=capture_factory())
        self.assertEqual(runner.commands[0], Command.STAGE)
        self.assertIsNone(runner.stage.environment)
        self.assertEqual(runner.commands[1], Command.CONFIGURE)
        self.assertEqual(report.errors, ())
        self.assertEqual(report.stage["spawns_submitted"], 1)

    def test_a_motion_config_takes_no_scene_pose(self):
        config = self.load(config_text(
            '[motion]\nperiod_frames = 120\nyaw_deg = 10.0\nforward_m = 2.0\nphase = 15\n'
            '[scene.environment]\nhour = 12.0\nweather = "rain"\n'))
        self.assertEqual(config.motion.request, MotionRequest("staged", 120, 10.0, 0.0, 0.0,
                                                              0.0, 0.0, 2.0))
        self.assertIsNone(config.scene.pose)
        for scene, extra in (
            ('[scene]\nposition = [1.0, 2.0, 3.0]\norientation = [0.0, 0.0, 0.0, 1.0]\n'
             '[motion]\nperiod_frames = 120\nphase = 0\n', ""),
            ('[scene.environment]\nhour = 12.0\nweather = "sunny"\n', ""),
            ('[motion]\nperiod_frames = 120\nphase = 120\n', ""),
            ('[motion]\nperiod_frames = 120\nphase = 0\n[diagnostics]\ndraw_census = true\n'
             'shader_identity = true\n', ""),
        ):
            with self.assertRaises(ExperimentError):
                self.load(config_text(scene, extra))

    def test_a_pinned_storm_must_be_observed(self):
        config = self.load(config_text(
            '[scene]\nposition = [1.0, 2.0, 3.0]\norientation = [0.0, 0.0, 0.0, 1.0]\n'
            '[scene.environment]\nhour = 12.0\nweather = "rain"\n'))
        for severity, control, failures in ((4.0, WeatherControl.RAIN, 0),
                                            (0.1, WeatherControl.RAIN, 1),
                                            (4.0, WeatherControl.SNOW, 1)):
            errors = []
            _check_observation(config, "capture", EnvironmentObservation(12.0, 0.0, severity, control),
                               errors)
            self.assertEqual(len(errors), failures, (severity, control, errors))


class MotionTranslationTests(unittest.TestCase):
    def test_the_translation_swings_with_the_phase(self):
        request = MotionRequest("surge", 40, 0.0, 0.0, 0.0, forward_m=2.0)
        self.assertEqual(request.translation_at(0), TranslationOffset())
        self.assertAlmostEqual(request.translation_at(10).forward, 2.0)
        self.assertEqual(request.pose_at(10).offset, OrientationOffset(0.0, 0.0, 0.0))
        self.assertIn("forward_m = 2.0", request.to_toml())
        self.assertEqual(MotionRequest.from_wire(request.to_wire()), request)
        self.assertNotIn("forward_m", MotionRequest("still", 40, 1.0, 0.0, 0.0).to_wire())
        with self.assertRaises(ProtocolError):
            MotionRequest("surge", 40, 0.0, 0.0, 0.0, right_m=21.0)


if __name__ == "__main__":
    unittest.main()


class MotionTranslationHistoryTests(unittest.TestCase):
    def test_a_translated_history_at_world_magnitudes_survives_float32(self):
        anchor = Camera((2091.0774, 1040.3096, 5004.919),
                        quaternion_normalize((-0.0058669983, -0.6749365, 0.008377615, 0.73780495)))
        request = MotionRequest("surge", 120, 0.0, 0.0, 0.0, right_m=1.0, forward_m=3.0)

        def history(phase, frame):
            camera = compose_pose(anchor, request.offset_at(phase), request.translation_at(phase))
            position = [float(np.float32(item)) for item in camera.position]
            return {"phase": phase, "rendered_frame": frame,
                    "applied": {"position": position, "orientation": list(camera.orientation)}}

        evidence = MotionEvidence.from_wire({
            "request": request.to_wire(), "phase": 15, "cycle": 1, "rendered_frame": 1000,
            "history": [history(7 + index, 900 + index) for index in range(8)],
        })
        evidence.validate_history(anchor)
        moved = Camera(tuple(item + 0.01 for item in anchor.position), anchor.orientation)
        with self.assertRaises(ProtocolError):
            evidence.validate_history(moved)


class AmbientOcclusionOverrideTests(unittest.TestCase):
    def test_ambient_occlusion_sources_round_trip_and_reject_other_values(self):
        overrides = VisualOverrides.from_wire({"ao_volumes": "disabled", "ssao": "disabled"})
        self.assertEqual(overrides.to_wire(), {"ssao": "disabled", "ao_volumes": "disabled"})
        self.assertEqual(tomllib.loads(overrides.to_toml())["visual_overrides"],
                         {"ssao": "disabled", "ao_volumes": "disabled"})
        with self.assertRaises(ProtocolError):
            VisualOverrides.from_wire({"ao_volumes": "skipped"})


class MotionBlurOverrideTests(unittest.TestCase):
    def test_motion_blur_is_not_an_override(self):
        # The motion blur never runs in VR, so there is nothing for an override to put back.
        with self.assertRaises(ProtocolError):
            VisualOverrides.from_wire({"motion_blur": "enabled"})


class ShippedOverrideTests(unittest.TestCase):
    def test_every_shipped_behaviour_round_trips_and_rejects_other_values(self):
        wire = {"fsr": "enabled", "foveation": "enabled", "auto_exposure": "enabled",
                "screen_fade": "enabled", "player_damage": "enabled"}
        overrides = VisualOverrides.from_wire(wire)
        self.assertEqual(overrides.to_wire(), wire)
        self.assertEqual(tomllib.loads(overrides.to_toml())["visual_overrides"], wire)
        for name in wire:
            with self.assertRaises(ProtocolError):
                VisualOverrides.from_wire({name: "disabled"})
        with self.assertRaises(ProtocolError):
            VisualOverrides.from_wire({"resolution": "enabled"})

    def test_the_shipped_list_matches_the_payload(self):
        source = (pathlib.Path(__file__).resolve().parents[3] /
                  "crates/jc3vr-render/src/testing/visual/overrides.rs").read_text()
        body = source.split("fn shipped(&self)", 1)[1].split("]", 1)[0]
        self.assertEqual(tuple(re.findall(r'\("(\w+)", self\.', body)), SHIPPED_OVERRIDES)
