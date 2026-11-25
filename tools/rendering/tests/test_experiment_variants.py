"""Single-pass variant configuration, scheduling, verification, and reporting."""

import dataclasses
import json
import re
import tomllib
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np
from PIL import Image

from tools.rendering.comparison.images import Outcome
from tools.rendering.experiment import ExperimentConfig, ExperimentError, run_experiment
from tools.rendering.experiment_variants import (
    SinglePassVariant,
    VariantError,
    describe,
    verify_switches,
)
from tools.rendering.protocol import (
    SINGLE_PASS_SWITCHES,
    Capture,
    Command,
    Mode,
    ProtocolError,
    Request,
    ScreenshotAck,
    ShaderBuild,
    SunShadows,
    VisualOverrides,
)
from tools.rendering.tests.test_experiment import (
    ExperimentTestCase,
    FakeRunner,
    capture_wire,
    pose_evidence,
)

REPOSITORY = Path(__file__).resolve().parents[3]
CANONICAL = ("left_only", "right_only", "multipass", "single_pass")
# The creation-time switches the fake payload reports in its shader build.
REBUILDING = ("ssr_per_eye", "pixel_camera_per_eye", "static_lights_per_eye")
DEFAULTS = {name: True for name in SINGLE_PASS_SWITCHES}
CHANGED_PIXELS = 7


def variant_config_text(variants, *, configurations=CANONICAL, extra=""):
    lines = ['name = "variants"', "warmup_frames = 4", "samples = 3", extra, "",
             "[scene]", "position = [1.0, 2.0, 3.0]", "orientation = [0.0, 0.0, 0.0, 1.0]"]
    for mode in configurations:
        lines += ["", "[[configurations]]", f'mode = "{mode}"']
    return "\n".join(lines) + "\n" + variants


class VariantRunner(FakeRunner):
    """A fake payload that applies single-pass switch overrides the way the real one does."""

    def __init__(self, config_dir, *, stale_build=False):
        super().__init__(config_dir)
        self.overrides = None
        self.mode = None
        self.target_pose = None
        self.frame = 0
        self.stale_build = stale_build
        self.screenshots = []

    def request(self, request):
        if request.command == Command.CONFIGURE:
            ack = super().request(request)
            self.overrides = request.visual_overrides
            self.mode = request.mode
            self.target_pose = request.pose
            return dataclasses.replace(ack, metadata=dataclasses.replace(
                ack.metadata, warmup_frames=request.warmup_frames,
                visual_overrides=request.visual_overrides))
        if request.command != Command.SCREENSHOT:
            return super().request(request)
        self.commands.append(request.command)
        self.requests.append(request)
        self.screenshots.append((request, self.overrides))
        self.frame += 1
        evidence = pose_evidence(self.target_pose.case_id, self.anchor, self.target_pose.offset,
                                 self.target_pose.translation)
        wire = capture_wire(request.id, self.mode, self.frame, evidence, None)
        wire["visual"]["generation"] = self.generation
        effective = dict(DEFAULTS)
        if self.overrides is not None:
            wire["visual"]["visual_overrides"] = self.overrides.to_wire()
            effective.update(self.overrides.switches)
        built = {name: effective[name] for name in REBUILDING}
        if self.stale_build:
            built["ssr_per_eye"] = True
        single_pass = self.mode == Mode.SINGLE_PASS
        wire["visual"]["shader_build"] = {"active": single_pass, "collapse": single_pass,
                                          "switches": built}
        folder = self.config_dir / "captures"
        metadata = dict(wire, effective_config={"stereo": {"single_pass": effective}})
        (folder / f"{request.id}.capture.json").write_text(json.dumps(metadata))
        Image.fromarray(self.image(effective)).save(folder / f"{request.id}.capture.png")
        return ScreenshotAck(request.id, Capture.from_wire(wire))

    def image(self, effective):
        left = np.full((360, 320, 3), 30, dtype=np.uint8)
        right = np.full((360, 320, 3), 120, dtype=np.uint8)
        if self.mode == Mode.LEFT_ONLY:
            return left
        if self.mode == Mode.RIGHT_ONLY:
            return right
        if self.mode == Mode.SINGLE_PASS and not effective["ssr_per_eye"]:
            left[0, :CHANGED_PIXELS] = 200
        return np.concatenate([left, right], axis=1)


class VariantConfigTests(ExperimentTestCase):
    def load(self, text):
        path = self.root / "config.toml"
        path.write_text(text)
        return ExperimentConfig.load(path)

    def test_variants_load_with_switches_and_optional_diagnostics(self):
        config = self.load(variant_config_text(
            '[[single_pass_variants]]\nname = "ssr-off"\nswitches = { ssr_per_eye = false }\n'
            '[[single_pass_variants]]\nname = "repeat"\ndiagnostics = true\n'))
        self.assertEqual(config.single_pass_variants, (
            SinglePassVariant("ssr-off", (("ssr_per_eye", False),)),
            SinglePassVariant("repeat", (), True),
        ))

    def test_absent_variants_leave_the_default_schedule(self):
        config = self.load(variant_config_text(""))
        self.assertEqual(config.single_pass_variants, ())

    def test_invalid_variants_are_rejected_with_their_reason(self):
        for text, reason in (
            ('[[single_pass_variants]]\nname = "x"\nswitches = { ssr_per_eyes = false }\n',
             "unknown single-pass switches"),
            ('[[single_pass_variants]]\nname = "x"\nswitches = { enabled = false }\n',
             "unknown single-pass switches"),
            ('[[single_pass_variants]]\nname = "x"\nswitches = { ssr_per_eye = 0 }\n',
             "must be a boolean"),
            ('[[single_pass_variants]]\nname = "x"\nswitches = "ssr_per_eye"\n',
             "must be a table"),
            ('[[single_pass_variants]]\nname = "x"\nother = 1\n', "accepts only"),
            ('[[single_pass_variants]]\nswitches = {}\n', "needs a name"),
            ('[[single_pass_variants]]\nname = "default"\n', "other than"),
            ('[[single_pass_variants]]\nname = "has space"\n', "request ID"),
            ('[[single_pass_variants]]\nname = "x"\n[[single_pass_variants]]\nname = "x"\n',
             "unique"),
        ):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, reason):
                self.load(variant_config_text(text))
        with self.assertRaisesRegex(ValueError, "nonempty"):
            self.load(variant_config_text("", extra="single_pass_variants = []"))

    def test_variants_need_every_reference_mode_and_no_other_treatment(self):
        variant = '[[single_pass_variants]]\nname = "x"\nswitches = { ssr_per_eye = false }\n'
        with self.assertRaisesRegex(VariantError, "left_only"):
            self.load(variant_config_text(variant, configurations=("multipass", "single_pass")))
        with self.assertRaisesRegex(ExperimentError, "treatment"):
            self.load(variant_config_text(variant, extra='matched_treatment = "cascade_abba"'))

    def test_the_variant_overrides_compose_with_the_experiment_overrides(self):
        base = VisualOverrides(screen_space_reflections="enabled",
                               single_pass=(("glare_per_eye", False),))
        variant = SinglePassVariant("x", (("ssr_per_eye", False), ("glare_per_eye", True)))
        self.assertEqual(variant.overrides(base), VisualOverrides(
            screen_space_reflections="enabled",
            single_pass=(("glare_per_eye", True), ("ssr_per_eye", False))))
        self.assertEqual(variant.overrides(None), VisualOverrides(single_pass=variant.switches))
        self.assertIs(SinglePassVariant("repeat").overrides(base), base)
        self.assertIsNone(SinglePassVariant("repeat").overrides(None))

    def test_checked_in_production_configs_load_without_variants(self):
        for path in sorted((REPOSITORY / "tools/rendering/experiments").glob("*_production.toml")):
            with self.subTest(path=path.name):
                self.assertEqual(ExperimentConfig.load(path).single_pass_variants, ())


class VariantProtocolTests(unittest.TestCase):
    def test_switch_names_match_the_payload(self):
        source = (REPOSITORY / "payload/src/stereo/switches.rs").read_text()
        invocation = source[source.index("single_pass_switches! {"):]
        invocation = invocation[:invocation.index("\n}\n")]
        names = tuple(re.findall(r"^\s*\w+ => (\w+), (?:true|false);$", invocation, re.M))
        self.assertEqual(names, SINGLE_PASS_SWITCHES)

    def test_overrides_round_trip_through_the_request_file(self):
        overrides = VisualOverrides(SunShadows.DISABLED, "enabled",
                                    (("ssr_per_eye", False), ("glare_per_eye", True)))
        request = Request(Command.CONFIGURE, "cfg", Mode.SINGLE_PASS, 30, visual_overrides=overrides)
        table = tomllib.loads(request.to_toml())["visual_overrides"]
        self.assertEqual(table, {"sun_shadows": "disabled", "screen_space_reflections": "enabled",
                                 "single_pass": {"glare_per_eye": True, "ssr_per_eye": False}})
        self.assertEqual(VisualOverrides.from_wire(table), overrides)
        self.assertEqual(VisualOverrides.from_wire(overrides.to_wire()), overrides)
        only = VisualOverrides(single_pass=(("ssr_per_eye", False),))
        self.assertEqual(tomllib.loads(Request(Command.CONFIGURE, "cfg", Mode.SINGLE_PASS, 30,
                                               visual_overrides=only).to_toml())["visual_overrides"],
                         {"single_pass": {"ssr_per_eye": False}})

    def test_invalid_switch_overrides_are_rejected(self):
        for value in ({"single_pass": {}}, {"single_pass": {"unknown": False}},
                      {"single_pass": {"ssr_per_eye": 1}}, {"single_pass": []}):
            with self.subTest(value=value), self.assertRaises(ProtocolError):
                VisualOverrides.from_wire(value)
        with self.assertRaisesRegex(ProtocolError, "single_pass mode"):
            Request(Command.CONFIGURE, "cfg", Mode.MULTIPASS, 30,
                    visual_overrides=VisualOverrides(single_pass=(("ssr_per_eye", False),)))

    def test_shader_build_is_parsed_strictly(self):
        build = ShaderBuild.from_wire({"active": True, "collapse": True,
                                       "switches": {"ssr_per_eye": False}})
        self.assertEqual(build.switches, (("ssr_per_eye", False),))
        for value in ({"active": True, "collapse": True}, {"active": 1, "collapse": True, "switches": {}},
                      {"active": True, "collapse": True, "switches": {"nope": True}}):
            with self.subTest(value=value), self.assertRaises(ProtocolError):
                ShaderBuild.from_wire(value)


class VariantVerificationTests(unittest.TestCase):
    def metadata(self, effective, built, active=True):
        return {"effective_config": {"stereo": {"single_pass": effective}},
                "visual": {"shader_build": {"active": active, "collapse": active, "switches": built}}}

    def test_a_capture_must_carry_the_requested_switches_in_its_config_and_shaders(self):
        overrides = VisualOverrides(single_pass=(("ssr_per_eye", False),))
        verify_switches("shot", self.metadata({"ssr_per_eye": False, "glare_per_eye": True},
                                              {"ssr_per_eye": False, "glare_per_eye": True}), overrides)
        with self.assertRaisesRegex(VariantError, "without the requested"):
            verify_switches("shot", self.metadata({"ssr_per_eye": True}, {"ssr_per_eye": True}), overrides)
        with self.assertRaisesRegex(VariantError, "built with other values"):
            verify_switches("shot", self.metadata({"ssr_per_eye": False}, {"ssr_per_eye": True}), overrides)
        with self.assertRaisesRegex(VariantError, "built with other values"):
            verify_switches("shot", self.metadata({"ssr_per_eye": True, "glare_per_eye": True},
                                                  {"ssr_per_eye": True, "glare_per_eye": False}), None)
        with self.assertRaisesRegex(VariantError, "not built for single-pass"):
            verify_switches("shot", self.metadata({"ssr_per_eye": False}, {"ssr_per_eye": False},
                                                  active=False), overrides)
        with self.assertRaisesRegex(VariantError, "no switch evidence"):
            verify_switches("shot", {"effective_config": {}}, overrides)


class VariantRunTests(ExperimentTestCase):
    def run_variants(self, runner, variants):
        path = self.root / "config.toml"
        path.write_text(variant_config_text(variants))
        config = ExperimentConfig.load(path)
        (runner.config_dir / "captures").mkdir(parents=True, exist_ok=True)
        with patch("tools.rendering.experiment.validate_capture",
                   side_effect=lambda folder, identifier, capture, **_: capture):
            return run_experiment(runner, config, self.root / "out",
                                  analyze=lambda *_: SimpleNamespace(status=Outcome.FAIL))

    def test_variants_follow_the_default_schedule_in_their_own_cases(self):
        runner = VariantRunner(self.root / "config")
        report = self.run_variants(runner, (
            '[[single_pass_variants]]\nname = "ssr-off"\nswitches = { ssr_per_eye = false }\n'
            '[[single_pass_variants]]\nname = "repeat"\n'))
        self.assertEqual(report.errors, ())
        configure = [r for r in runner.requests if r.command == Command.CONFIGURE]
        self.assertEqual([r.mode for r in configure[1:]],
                         [Mode.LEFT_ONLY, Mode.RIGHT_ONLY, Mode.MULTIPASS, Mode.SINGLE_PASS,
                          Mode.SINGLE_PASS, Mode.SINGLE_PASS])
        self.assertEqual([r.visual_overrides for r in configure[1:]],
                         [None] * 4 + [VisualOverrides(single_pass=(("ssr_per_eye", False),))] + [None])
        self.assertEqual([c.condition for c in report.captures],
                         ["default"] * 12 + ["ssr-off"] * 3 + ["repeat"] * 3)
        cases = json.loads((self.root / "out" / "manifest.json").read_text())["cases"]
        self.assertEqual([c["name"] for c in cases],
                         ["variants", "variants-variant-ssr-off", "variants-variant-repeat"])
        for case in cases[1:]:
            for mode in ("left_only", "right_only", "multipass"):
                self.assertEqual(case["modes"][mode], cases[0]["modes"][mode])
            self.assertEqual(len(case["modes"]["single_pass"]), 3)
            self.assertFalse({s["png"] for s in case["modes"]["single_pass"]} &
                             {s["png"] for s in cases[0]["modes"]["single_pass"]})
        summaries = {s["variant"]: s for s in report.single_pass_variants}
        self.assertEqual(list(summaries), ["default", "ssr-off", "repeat"])
        self.assertEqual(summaries["ssr-off"]["switches"], {"ssr_per_eye": False})
        self.assertEqual([e["median"] for e in summaries["ssr-off"]["eyes"]], [CHANGED_PIXELS, 0])
        self.assertEqual(summaries["ssr-off"]["eyes"][0]["pairs"], [CHANGED_PIXELS] * 9)
        self.assertEqual([e["median"] for e in summaries["default"]["eyes"]], [0, 0])
        self.assertEqual([e["median"] for e in summaries["repeat"]["eyes"]], [0, 0])
        self.assertEqual(summaries["ssr-off"]["eyes"][0]["pixels"], 320 * 360)
        self.assertIn("ssr_per_eye=off", describe(summaries["ssr-off"]))
        written = json.loads((self.root / "out" / "report.json").read_text())
        self.assertEqual(len(written["single_pass_variants"]), 3)

    def test_variant_captures_skip_heavy_diagnostics_unless_asked(self):
        runner = VariantRunner(self.root / "config")
        path = self.root / "config.toml"
        path.write_text(variant_config_text(
            '[[single_pass_variants]]\nname = "light"\nswitches = { ssr_per_eye = false }\n'
            '[[single_pass_variants]]\nname = "heavy"\nswitches = { ssr_per_eye = false }\n'
            'diagnostics = true\n',
            extra="[diagnostics]\nlate_sky_stages = true\nshader_identity = false"))
        config = ExperimentConfig.load(path)
        seen = []

        def validate(folder, identifier, capture, **options):
            seen.append((identifier, options["late_sky_stages"]))
            return capture

        (runner.config_dir / "captures").mkdir(parents=True, exist_ok=True)
        with patch("tools.rendering.experiment.validate_capture", side_effect=validate):
            run_experiment(runner, config, self.root / "out",
                           analyze=lambda *_: SimpleNamespace(status=Outcome.FAIL))
        flags = [flag for identifier, flag in seen]
        self.assertEqual(flags, [True] * 12 + [False] * 3 + [True] * 3)

    def test_image_only_variants_are_exempt_from_the_resolve_content_check(self):
        runner = VariantRunner(self.root / "config")
        path = self.root / "config.toml"
        path.write_text(variant_config_text(
            '[[single_pass_variants]]\nname = "light"\nswitches = { ssr_per_eye = false }\n',
            extra="[diagnostics]\nresolve_inputs = true"))
        checked = []
        (runner.config_dir / "captures").mkdir(parents=True, exist_ok=True)
        with patch("tools.rendering.experiment.validate_capture",
                   side_effect=lambda folder, identifier, capture, **_: capture), \
             patch("tools.rendering.experiment._resolve_content_errors",
                   side_effect=lambda identifier, metadata: checked.append(identifier) or []):
            report = run_experiment(runner, ExperimentConfig.load(path), self.root / "out",
                                    analyze=lambda *_: SimpleNamespace(status=Outcome.FAIL))
        self.assertEqual(report.errors, ())
        self.assertEqual(len(checked), 12)
        self.assertFalse(any("_light_" in identifier for identifier in checked))

    def test_a_capture_whose_shaders_kept_the_old_switches_fails_the_run(self):
        runner = VariantRunner(self.root / "config", stale_build=True)
        report = self.run_variants(
            runner, '[[single_pass_variants]]\nname = "ssr-off"\nswitches = { ssr_per_eye = false }\n')
        self.assertEqual(report.outcome, Outcome.INCONCLUSIVE)
        self.assertTrue(any("built with other values" in error for error in report.errors))
        self.assertEqual(runner.commands[-1], Command.END)


if __name__ == "__main__":
    unittest.main()
