"""The fast comparison: its schedule, its protocol, its verdicts, its suite files, and a run against
a fake session."""

import json
import tempfile
import unittest
from pathlib import Path

from tools.rendering.comparison.images import Mode as CompareMode
from tools.rendering.fast.plan import (
    FastError,
    Limits,
    SnapshotStep,
    Verdict,
    baseline_counts,
    failed,
    label_mode,
    rows,
    schedule,
    table,
)
from tools.rendering.fast.run import FastOptions, fast_config, holds_still, run_scene
from tools.rendering.fast.suite import load_config, load_suite
from tools.rendering.protocol import (
    Command,
    ProtocolError,
    StageAck,
    acknowledgement_from_wire,
)
from tools.rendering.protocol.snapshot import (
    Comparison,
    Plane,
    ReleaseAck,
    ReleaseRequest,
    SnapshotAck,
    SnapshotRequest,
)
from tools.rendering.session.errors import SessionError
from tools.rendering.session.reinject import PINNED_LINE, reinject
from tools.rendering.session.stage_record import record, reuse_distance
from tools.rendering.tests.test_experiment import FakeRunner, write_config

ALL = (CompareMode.LEFT_ONLY, CompareMode.RIGHT_ONLY, CompareMode.MULTIPASS, CompareMode.SINGLE_PASS)


def comparison_wire(against, plane, eye, differing, pixels=100_000, mean=0.0, grid=(2, 2)):
    heat = [0] * (grid[0] * grid[1])
    heat[0] = differing
    return {"against": against, "plane": plane, "eye": eye, "kind": "unorm8", "threshold": 3.0,
            "failed": False, "pixels": pixels, "differing": differing, "changed": differing,
            "mean": mean,
            "largest": 9.0 if differing else 0.0, "bbox": [1, 2, 3, 4] if differing else None,
            "grid": list(grid), "heat": heat, "dump": None}


def snapshot_wire(request_id, label, mode, comparisons, frame=1000):
    return {"id": request_id, "state": "completed", "error": None, "label": label, "mode": mode,
            "frame": frame, "eye_width": 10, "eye_height": 10,
            "planes": [{"plane": "final", "eye": 0, "width": 10, "height": 10, "format": 28,
                        "kind": "unorm8"}],
            "plane_errors": [], "comparisons": comparisons, "kept": True,
            "visual": {"generation": 3}, "timing": {"frames_waited": 2, "wait_ms": 5.0, "gpu_ms": 1.0}}


def comparison(against, plane, eye, differing, pixels=100, mean=0.0):
    return Comparison.from_wire(comparison_wire(against, plane, eye, differing, pixels, mean))


class ScheduleTests(unittest.TestCase):
    def test_references_come_first_and_are_kept(self):
        steps = schedule((CompareMode.SINGLE_PASS, CompareMode.MULTIPASS, CompareMode.LEFT_ONLY,
                          CompareMode.RIGHT_ONLY), ("ssr-off",))
        self.assertEqual([step.name for step in steps],
                         ["left_only", "right_only", "multipass", "single_pass", "ssr-off"])
        left, right, multipass, single, variant = steps
        self.assertEqual(left.snapshots[0], SnapshotStep("left_only_a", True, ()))
        self.assertEqual(left.snapshots[1], SnapshotStep("left_only_b", False, ("left_only_a",),
                                                         repeat_of="left_only"))
        self.assertEqual(multipass.snapshots[0].compare, ("left_only_a", "right_only_a"))
        self.assertEqual(single.snapshots[0].compare,
                         ("left_only_a", "right_only_a", "multipass_a"))
        self.assertTrue(single.snapshots[0].keep)
        self.assertEqual(variant.snapshots, (SnapshotStep(
            "v1_ssr_off", False, ("left_only_a", "right_only_a", "multipass_a")),))
        self.assertEqual(variant.mode, CompareMode.SINGLE_PASS)

    def test_without_repeats_only_references_are_kept(self):
        steps = schedule(ALL, (), repeat=False)
        self.assertEqual([len(step.snapshots) for step in steps], [1, 1, 1, 1])
        self.assertFalse(steps[-1].snapshots[0].keep)

    def test_unsupported_schedules_are_refused(self):
        with self.assertRaises(FastError):
            schedule(ALL + (CompareMode.REVERSE_MULTIPASS,), ())
        with self.assertRaises(FastError):
            schedule((CompareMode.LEFT_ONLY,), ("a",))
        with self.assertRaises(FastError):
            schedule((CompareMode.LEFT_ONLY, CompareMode.LEFT_ONLY), ())

    def test_labels_name_their_modes(self):
        self.assertEqual(label_mode("left_only_a"), "left_only")
        self.assertEqual(label_mode("single_pass_b"), "single_pass")
        self.assertEqual(label_mode("v2_clouds_off"), "single_pass")


class VerdictTests(unittest.TestCase):
    def results(self, single_left, left_noise=10, single_noise=20, pixels=100_000):
        steps = schedule(ALL, ())
        by_label = {snap.label: snap for step in steps for snap in step.snapshots}
        return [
            (by_label["left_only_b"], (comparison("left_only_a", "final", 0, left_noise, pixels),)),
            (by_label["single_pass_a"], (comparison("left_only_a", "final", 0, single_left, pixels),)),
            (by_label["single_pass_b"], (comparison("single_pass_a", "final", 0, single_noise,
                                                    pixels),)),
        ]

    def test_counts_within_thresholds_pass(self):
        result, floors = rows("town", self.results(single_left=100), Limits())
        self.assertEqual(floors[("single_pass", "final", 0)], 20)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0].floor, 20)
        self.assertEqual(result[0].verdict, Verdict.PASS)

    def test_counts_within_the_noise_allowance_are_noise(self):
        # Over the 0.1% threshold (100 of 100,000) but within twice the floor plus 0.02%.
        result, _ = rows("town", self.results(single_left=150, single_noise=70), Limits())
        self.assertEqual(result[0].verdict, Verdict.NOISE)

    def test_counts_beyond_the_noise_fail(self):
        result, _ = rows("town", self.results(single_left=1000), Limits())
        self.assertEqual(result[0].verdict, Verdict.FAIL)
        self.assertTrue(failed(result))

    def test_a_baseline_fails_only_regressions(self):
        baseline = {("town", "single_pass_a", "left_only_a", "final", 0): 990}
        result, _ = rows("town", self.results(single_left=1000), Limits(), baseline)
        self.assertEqual(result[0].verdict, Verdict.NOISE)
        self.assertEqual(result[0].baseline, 990)
        result, _ = rows("town", self.results(single_left=2000), Limits(), baseline)
        self.assertEqual(result[0].verdict, Verdict.REGRESS)

    def test_baseline_counts_read_a_report(self):
        result, _ = rows("town", self.results(single_left=1000), Limits())
        report = {"scenes": [{"rows": [row.to_wire() for row in result]}]}
        self.assertEqual(baseline_counts(json.loads(json.dumps(report))),
                         {("town", "single_pass_a", "left_only_a", "final", 0): 1000})

    def test_the_table_shows_counts_floors_and_verdicts(self):
        result, _ = rows("town", self.results(single_left=1000), Limits())
        text = table(result, ("final", "depth"))
        self.assertIn("1000/20!", text)
        self.assertIn("left_only", text)
        self.assertIn("fail", text)


class ProtocolTests(unittest.TestCase):
    def test_snapshot_requests_serialize(self):
        request = SnapshotRequest("f1_sp", "single_pass_a", ("final", "depth"), True,
                                  ("left_only_a",), 15)
        text = request.to_toml()
        self.assertIn('command = "snapshot"', text)
        self.assertIn("[motion_capture]\nphase = 15", text)
        self.assertIn('planes = ["final", "depth"]', text)
        self.assertIn('compare = ["left_only_a"]', text)
        self.assertIn('dump = "never"', text)
        self.assertIn("depth_ulps = 0", text)
        self.assertIn("depth_ulps = 64", SnapshotRequest("a", "b", ("final",), depth_ulps=64).to_toml())
        with self.assertRaises(ProtocolError):
            SnapshotRequest("a", "b", ("final",), depth_ulps=513)
        self.assertEqual(ReleaseRequest("f1_rel").to_toml(),
                         'command = "snapshot_release"\nid = "f1_rel"\n')

    def test_invalid_snapshot_requests_are_refused(self):
        with self.assertRaises(ProtocolError):
            SnapshotRequest("a", "b", ("depth",))
        with self.assertRaises(ProtocolError):
            SnapshotRequest("a", "b", ("final", "final"))
        with self.assertRaises(ProtocolError):
            SnapshotRequest("a", "no spaces", ("final",))
        with self.assertRaises(ValueError):
            SnapshotRequest("a", "b", ("sky",))

    def test_acknowledgements_parse(self):
        wire = snapshot_wire("f1_sp", "single_pass_a", "single_pass",
                             [comparison_wire("left_only_a", "final", 0, 7)])
        ack = acknowledgement_from_wire(wire, Command.SNAPSHOT, "f1_sp")
        self.assertIsInstance(ack, SnapshotAck)
        self.assertEqual(ack.comparisons[0].differing, 7)
        self.assertEqual(ack.comparisons[0].bbox, (1, 2, 3, 4))
        self.assertEqual(ack.planes, ((Plane.FINAL, 0),))
        release = acknowledgement_from_wire({"id": "r", "state": "released", "released": 3},
                                            Command.SNAPSHOT_RELEASE, "r")
        self.assertEqual(release, ReleaseAck("r", 3))
        failure = acknowledgement_from_wire({"id": "x", "state": "failed", "error": "snapshot: no"},
                                            Command.SNAPSHOT, "x")
        self.assertEqual(failure.error, "snapshot: no")

    def test_inconsistent_comparisons_are_refused(self):
        wire = comparison_wire("left_only_a", "final", 0, 7)
        wire["heat"][1] = 1
        with self.assertRaises(ProtocolError):
            Comparison.from_wire(wire)
        wire = comparison_wire("left_only_a", "final", 0, 7)
        wire["differing"] = 100_001
        with self.assertRaises(ProtocolError):
            Comparison.from_wire(wire)

    def test_stage_acknowledgements_report_reuse(self):
        wire = {"id": "s", "state": "staged", "player_position": [1.0, 2.0, 3.0],
                "distance_m": 1.5, "spawns_submitted": 0, "spawns_present": 2, "reused": True}
        self.assertTrue(StageAck.from_wire(wire).reused)
        del wire["reused"]
        self.assertFalse(StageAck.from_wire(wire).reused)


class FakeSnapshotRunner(FakeRunner):
    """A session whose snapshots compare by a table of counts keyed by (label, against)."""

    def __init__(self, config_dir, counts):
        super().__init__(config_dir)
        self.counts = counts
        self.kept = set()

    def request(self, value):
        if value.command == Command.SNAPSHOT:
            self.commands.append(value.command)
            self.requests.append(value)
            mode = value.label.rsplit("_", 1)[0] if not value.label.startswith("v") else "single_pass"
            comparisons = []
            for against in value.compare:
                if against not in self.kept:
                    raise AssertionError(f"{value.label} compares with unkept {against}")
                eyes = {"left_only": (0,), "right_only": (1,)}.get(against.rsplit("_", 1)[0], (0, 1))
                own = {"left_only": (0,), "right_only": (1,)}.get(mode, (0, 1))
                for eye in sorted(set(eyes) & set(own)):
                    for plane in value.planes:
                        comparisons.append(comparison_wire(
                            against, plane.value, eye, self.counts.get((value.label, against), 0)))
            if value.keep:
                self.kept.add(value.label)
            return SnapshotAck.from_wire(snapshot_wire(value.id, value.label, mode, comparisons))
        if value.command == Command.STAGE:
            self.commands.append(value.command)
            self.requests.append(value)
            reused = value.reuse_within_m is not None
            return StageAck(value.id, (1.0, 2.0, 3.0), 1.0, 0 if reused else len(value.spawns),
                            len(value.spawns) if reused else 0, reused)
        if value.command == Command.SNAPSHOT_RELEASE:
            self.commands.append(value.command)
            self.kept.clear()
            return ReleaseAck(value.id, 0)
        return super().request(value)


class RunTests(unittest.TestCase):
    def test_a_scene_runs_its_schedule_and_judges_its_rows(self):
        with tempfile.TemporaryDirectory() as root:
            config = write_config(root, [mode.value for mode in ALL])
            counts = {("left_only_b", "left_only_a"): 5, ("single_pass_b", "single_pass_a"): 5,
                      ("single_pass_a", "left_only_a"): 50_000}
            runner = FakeSnapshotRunner(Path(root) / "config", counts)
            result = run_scene(runner, config, "glint", FastOptions(planes=(Plane.FINAL,)))
        self.assertEqual(result.errors, [])
        self.assertEqual(runner.commands.count(Command.SNAPSHOT), 8)
        self.assertEqual(runner.commands[-2:], [Command.SNAPSHOT_RELEASE, Command.END])
        verdicts = {(row.test, row.against, row.eye): row.verdict for row in result.rows}
        self.assertEqual(verdicts[("single_pass_a", "left_only_a", 0)], Verdict.FAIL)
        self.assertEqual(verdicts[("single_pass_a", "right_only_a", 1)], Verdict.PASS)
        self.assertEqual(verdicts[("multipass_a", "left_only_a", 0)], Verdict.PASS)
        self.assertNotIn(("left_only_b", "left_only_a", 0), verdicts)
        self.assertEqual(result.floors[("left_only", "final", 0)], 5)
        steps = [timing["step"] for timing in result.timings]
        self.assertEqual(steps[0], "anchor")
        self.assertIn("snapshot single_pass_b", steps)

    def test_a_failed_snapshot_is_reported_and_the_session_released(self):
        class Failing(FakeSnapshotRunner):
            def request(self, value):
                if value.command == Command.SNAPSHOT and value.label == "multipass_a":
                    return acknowledgement_from_wire(
                        {"id": value.id, "state": "failed", "error": "snapshot: boom"},
                        Command.SNAPSHOT, value.id)
                return super().request(value)

        with tempfile.TemporaryDirectory() as root:
            config = write_config(root, [mode.value for mode in ALL])
            runner = Failing(Path(root) / "config", {})
            result = run_scene(runner, config, "glint", FastOptions(planes=(Plane.FINAL,)))
        self.assertTrue(result.errors)
        self.assertEqual(runner.commands[-2:], [Command.SNAPSHOT_RELEASE, Command.END])


    def test_a_repeated_stage_asks_to_be_reused(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "staged.toml"
            path.write_text(
                'name = "staged"\nwarmup_frames = 12\nsamples = 3\n'
                '[scene]\nposition = [1.0, 2.0, 3.0]\norientation = [0.0, 0.0, 0.0, 1.0]\n'
                '[scene.environment]\nhour = 13.0\nweather = "sunny"\n'
                '[scene.stage]\nsettle_seconds = 20.0\n'
                '[scene.stage.location]\nposition = [1.0, 2.0, 3.0]\n'
                + "".join(f'[[configurations]]\nmode = "{mode.value}"\n' for mode in ALL))
            config = load_config(path).config
            config_dir = Path(root) / "config"
            config_dir.mkdir()
            runner = FakeSnapshotRunner(config_dir, {})
            first = run_scene(runner, config, "staged", FastOptions(planes=(Plane.FINAL,)))
            second = run_scene(runner, config, "staged", FastOptions(planes=(Plane.FINAL,)))
            forced = run_scene(runner, config, "staged",
                               FastOptions(planes=(Plane.FINAL,), restage=True))
        self.assertEqual((first.stage_reused, second.stage_reused, forced.stage_reused),
                         (False, True, False))
        stages = [request for request in runner.requests if request.command == Command.STAGE]
        self.assertEqual([stage.reuse_within_m for stage in stages], [None, 5.0, None])
        self.assertIn("reuse_within_m = 5.0", stages[1].to_toml())


class FastConfigTests(unittest.TestCase):
    def test_a_still_motion_is_held_as_a_fixed_pose(self):
        scenes = {scene.name: scene for scene in load_suite(
            Path(__file__).resolve().parents[1] / "suites/standing.toml")}
        self.assertTrue(holds_still(scenes["town"].config, full_cycles=False))
        self.assertFalse(holds_still(scenes["town"].config, full_cycles=True))
        self.assertFalse(holds_still(scenes["town-rotation"].config, full_cycles=False))
        self.assertFalse(holds_still(scenes["shore"].config, full_cycles=False))
        self.assertEqual(fast_config(scenes["town"].config).interval_seconds, 0.0)
        self.assertFalse(fast_config(scenes["shore"].config).diagnostics.resolve_inputs)


class SuiteTests(unittest.TestCase):
    def test_the_standing_suite_loads(self):
        scenes = load_suite(Path(__file__).resolve().parents[1] / "suites/standing.toml")
        names = [scene.name for scene in scenes]
        self.assertIn("town", names)
        self.assertIn("cliff-dusk", names)
        beyond = next(scene for scene in scenes if scene.name == "town-beyond")
        self.assertEqual(beyond.config.visual_overrides.headset, "beyond")
        shore = next(scene for scene in scenes if scene.name == "shore")
        self.assertIsNotNone(shore.config.scene.stage)
        for scene in scenes:
            self.assertIsNone(scene.config.matched_treatment, scene.name)
            self.assertIsNotNone(scene.config.scene.stage, scene.name)

    def test_invalid_suites_are_refused(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "suite.toml"
            path.write_text('[[scene]]\nconfig = "coverage_town_midday.toml"\nheadset = "vive"\n')
            with self.assertRaises(FastError):
                load_suite(path)
            path.write_text('[[scene]]\nconfig = "coverage_town_midday.toml"\n'
                            '[scene.stage]\nsettle_seconds = 1.0\n'
                            'location = { position = [0.0, 0.0, 0.0] }\n')
            with self.assertRaises(FastError):
                load_suite(path)
            path.write_text('[[scene]]\nconfig = "coverage_town_midday.toml"\n'
                            '[[scene]]\nconfig = "coverage_town_midday.toml"\n')
            with self.assertRaises(FastError):
                load_suite(path)



class ReinjectTests(unittest.TestCase):
    def runner(self, root, log_text):
        sessions = Path(root) / "sessions"
        (sessions / "2026-01-01_00-00-00_000").mkdir(parents=True)
        (sessions / "2026-01-01_00-00-00_000/session.log").write_text("old\n")
        (sessions / "2026-01-02_00-00-00_000").mkdir()
        (sessions / "2026-01-02_00-00-00_000/session.log").write_text(log_text)
        runner = Path(root) / "runner.json"
        runner.write_text(json.dumps({"bus_name": "bus", "runtime_dir": str(root),
                                      "display_file": str(Path(root) / "display.json"),
                                      "sessions_dir": str(sessions)}))
        return runner

    def test_a_pinned_payload_is_not_ejected(self):
        with tempfile.TemporaryDirectory() as root:
            runner = self.runner(root, f"x\nWARN {PINNED_LINE}: an eject will be refused\n")
            with self.assertRaisesRegex(SessionError, "relaunch the session"):
                reinject(runner)

    def test_incomplete_runner_state_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            runner = Path(root) / "runner.json"
            runner.write_text(json.dumps({"bus_name": "bus"}))
            with self.assertRaisesRegex(SessionError, "runtime_dir"):
                reinject(runner)


class StageRecordTests(unittest.TestCase):
    def test_only_the_last_stage_is_reused(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertIsNone(reuse_distance(Path(root), "a"))
            record(Path(root), "a")
            self.assertEqual(reuse_distance(Path(root), "a"), 5.0)
            self.assertIsNone(reuse_distance(Path(root), "b"))
            (Path(root) / "last-stage.json").write_text("not json")
            self.assertIsNone(reuse_distance(Path(root), "a"))


if __name__ == "__main__":
    unittest.main()
