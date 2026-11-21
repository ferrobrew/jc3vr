"""The frame-time benchmark: its statistics, its protocol and acknowledgement parsing, its thread
accounting, its schedule, and its summary and charts over result files."""

import json
import math
import tempfile
import tomllib
import unittest
from pathlib import Path

from tools.rendering.benchmark.charts import mode_colours, render
from tools.rendering.benchmark.machine import cpu_model
from tools.rendering.benchmark.report import markdown, summarize, write_summary
from tools.rendering.benchmark.run import (
    RUN_FORMAT,
    RUN_FORMAT_VERSION,
    Mode,
    run_label,
    schedule,
)
from tools.rendering.benchmark.stats import (
    Distribution,
    aggregate,
    detour_summary,
    frame_series,
    gpu_frames,
    percentile,
    summarize_run,
)
from tools.rendering.benchmark.suite import BenchmarkError, load_suite
from tools.rendering.benchmark.threads import Sample, parse_stat, window
from tools.rendering.protocol import Command, ProtocolError, acknowledgement_from_wire
from tools.rendering.protocol.benchmark import (
    BenchmarkAck,
    BenchmarkMode,
    BenchmarkRequest,
    CameraPath,
)

# The payload's frame columns (`payload/src/testing/benchmark/sample.rs`).
COLUMNS = ["frame_us", "sim_ticks", "game_mod_us", "game_openxr_us", "game_engine_us",
           "game_wait_us", "tail_mod_us", "tail_openxr_us", "tail_wait_us", "other_mod_us",
           "other_engine_us", "other_wait_us", "draw_task_us", "draw_tasks", "post_draw_us",
           "dispatch_us", "drain_us", "tail_drain_us", "vr_wait_frame_us", "vr_begin_frame_us",
           "vr_end_frame_us", "vr_swapchain_wait_us", "vr_lock_us", "submit_us", "sim_us",
           "engine_update_us", "engine_update_render_us", "debug_ui_us", "engine_draws",
           "engine_dispatches", "d3d_submissions", "detour_us", "detour_calls", "forward_us",
           "forward_calls", "pose_age_us", "pose_ages"]
SEAMS = ["pre_draw", "gbuffer", "scene", "post_effects", "post_draw", "fsr"]


def frame(frame_us, **values):
    row = [0] * len(COLUMNS)
    row[COLUMNS.index("frame_us")] = frame_us
    for name, value in values.items():
        row[COLUMNS.index(name)] = value
    return row


def ack(frames, dispatches=(), detour_timing=False, engine_size=(7168, 3561), double_wide=True):
    return {
        "id": "b1", "state": "completed", "error": None, "label": "town-single_pass-1-r0",
        "request": {"mode": "single_pass", "detour_timing": detour_timing},
        "environment": {"engine_size": list(engine_size), "double_wide_active": double_wide,
                        "com_detours_installed": True, "clock_pair_ns": 20.0},
        "timing": {"epoch_start": 100.0, "epoch_end": 110.0},
        "columns": COLUMNS, "frames": [list(f) for f in frames],
        "gpu": {"seams": SEAMS,
                "columns": ["lane", "span_us", "idle_before_us", "submit_us"]
                + [f"{s}_us" for s in SEAMS],
                "dispatches": [list(d) for d in dispatches], "dropped": 0},
        "slots": [],
    }


def gpu_row(lane, span, idle=-1, fsr=-1):
    return [lane, span, idle, 0, -1, -1, -1, -1, -1, fsr]


class StatisticsTests(unittest.TestCase):
    def test_percentile_interpolates_between_ranks(self):
        values = [1, 2, 3, 4]
        self.assertEqual(percentile(values, 0), 1)
        self.assertEqual(percentile(values, 100), 4)
        self.assertAlmostEqual(percentile(values, 50), 2.5)
        self.assertAlmostEqual(percentile(values, 99), 3.97)
        self.assertEqual(percentile([7], 99), 7)
        with self.assertRaises(ValueError):
            percentile([], 50)
        with self.assertRaises(ValueError):
            percentile([1], 101)

    def test_a_distribution_reports_the_median_the_one_percent_low_and_the_worst(self):
        values = [10.0] * 99 + [40.0]
        distribution = Distribution.of(values)
        self.assertEqual(distribution.count, 100)
        self.assertEqual(distribution.median, 10.0)
        self.assertAlmostEqual(distribution.p99, 10.3)
        self.assertEqual(distribution.worst, 40.0)
        self.assertTrue(math.isnan(Distribution.of([]).median))

    def test_frame_series_separate_work_from_waits(self):
        series = frame_series(ack([frame(14000, game_mod_us=500, game_openxr_us=100,
                                         game_engine_us=4400, game_wait_us=8000,
                                         draw_task_us=6000, post_draw_us=250,
                                         d3d_submissions=6000)]))
        self.assertEqual(series["frame_ms"], [14.0])
        self.assertEqual(series["game_busy_ms"], [5.0])
        self.assertEqual(series["game_wait_ms"], [8.0])
        self.assertEqual(series["draw_ms"], [6.25])
        self.assertEqual(series["d3d_submissions"], [6000])

    def test_the_pose_age_is_averaged_over_the_frames_each_row_submitted(self):
        series = frame_series(ack([frame(14000, pose_age_us=16000, pose_ages=1),
                                   frame(14000),
                                   frame(28000, pose_age_us=30000, pose_ages=2)]))
        self.assertEqual(series["pose_age_ms"], [16.0, 15.0])

    def test_gpu_dispatches_are_grouped_into_frames_by_the_first_eye(self):
        rows = [gpu_row(1, 3000),  # The tail of a frame begun before the recording.
                gpu_row(0, 4000, idle=500, fsr=900), gpu_row(1, 4000, idle=200, fsr=1000),
                gpu_row(0, 5000, idle=100), gpu_row(1, 5000),
                gpu_row(0, 1000)]  # Cut short at the end.
        frames = gpu_frames(ack([], rows))
        self.assertEqual(len(frames), 2)
        self.assertAlmostEqual(frames[0]["gpu_ms"], 8.0)
        self.assertAlmostEqual(frames[0]["gpu_idle_ms"], 0.7)
        self.assertAlmostEqual(frames[0]["gpu_fsr_ms"], 1.9)
        self.assertEqual(frames[0]["dispatches"], 2)
        self.assertAlmostEqual(frames[1]["gpu_ms"], 10.0)

    def test_a_run_summary_carries_every_metric_and_the_frame_rate(self):
        summary = summarize_run(ack([frame(10000), frame(10000), frame(20000)],
                                    [gpu_row(0, 1000)] * 4))
        self.assertEqual(summary["frame_ms"]["median"], 10.0)
        self.assertEqual(summary["frame_ms"]["worst"], 20.0)
        self.assertAlmostEqual(summary["fps_mean"], 75.0)
        self.assertEqual(summary["gpu_ms"]["median"], 1.0)
        self.assertNotIn("detours", summary)

    def test_the_detour_overhead_takes_out_the_forwards_and_the_clock_reads(self):
        series = frame_series(ack([frame(10000, detour_us=3000, detour_calls=5000,
                                         forward_us=1500, forward_calls=6000)] * 2))
        result = detour_summary(series, clock_pair_ns=20.0)
        # (3.0 ms - 1.5 ms) less 11,000 pairs of 20 ns, per frame.
        self.assertAlmostEqual(result["overhead_ms_per_frame"], 1.28)
        self.assertAlmostEqual(result["overhead_ns_per_draw"], 256.0)
        self.assertAlmostEqual(result["forward_ns_per_call"], 250.0)
        self.assertEqual(result["detour_calls_per_frame"], 5000)

    def test_repeats_aggregate_to_the_median_of_medians_with_their_spread(self):
        summaries = [{"frame_ms": Distribution.of(v).to_wire()}
                     for v in ([10, 10, 30], [12, 12, 12], [11, 11, 50])]
        result = aggregate(summaries)
        self.assertEqual(result["repeats"], 3)
        self.assertEqual(result["frame_ms"]["median"], 11)
        self.assertEqual(result["frame_ms"]["spread"], [10, 12])
        self.assertEqual(result["frame_ms"]["worst"], 50)
        self.assertEqual(aggregate([]), {"repeats": 0})


class ProtocolTests(unittest.TestCase):
    def test_a_request_writes_its_table(self):
        request = BenchmarkRequest("b1", "town", "single_pass", 10.0, resolution_scale=1.5,
                                   switches={"fsr_per_eye": False}.items(),
                                   camera=CameraPath(yaw_deg=20.0, forward_m=5.0))
        value = tomllib.loads(request.to_toml())
        self.assertEqual(value["command"], "benchmark")
        table = value["benchmark"]
        self.assertEqual(table["mode"], "single_pass")
        self.assertEqual(table["resolution_scale"], 1.5)
        self.assertEqual(table["single_pass"], {"fsr_per_eye": False})
        self.assertEqual(table["camera"]["yaw_deg"], 20.0)
        self.assertTrue(table["install_draw_detours"])
        still = tomllib.loads(BenchmarkRequest("b2", "town", "multipass", 1.0).to_toml())
        self.assertNotIn("camera", still["benchmark"])
        self.assertNotIn("resolution_scale", still["benchmark"])
        self.assertNotIn("overlap_render_update", still["benchmark"])
        self.assertNotIn("gpu_passes", still["benchmark"])
        passes = BenchmarkRequest("b4", "town", "single_pass", 1.0, gpu_passes=True)
        self.assertIs(tomllib.loads(passes.to_toml())["benchmark"]["gpu_passes"], True)
        serial = BenchmarkRequest("b3", "town", "multipass", 1.0, overlap_render_update=False)
        self.assertIs(tomllib.loads(serial.to_toml())["benchmark"]["overlap_render_update"], False)

    def test_invalid_requests_are_refused(self):
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "town", "multipass", 1.0, switches=(("fsr_per_eye", False),))
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "town", "multipass", 0.0)
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "a b", "multipass", 1.0)
        with self.assertRaises(ProtocolError):
            BenchmarkRequest("b1", "town", "multipass", 1.0, gpu=False, gpu_passes=True)
        with self.assertRaises(ProtocolError):
            CameraPath(period_seconds=0)

    def test_an_acknowledgement_is_parsed_and_checked(self):
        value = ack([frame(10000)], [gpu_row(0, 100)])
        parsed = acknowledgement_from_wire(value, Command.BENCHMARK, "b1")
        self.assertIsInstance(parsed, BenchmarkAck)
        self.assertEqual(parsed.label, "town-single_pass-1-r0")
        broken = ack([frame(10000)[:-1]])
        with self.assertRaises(ProtocolError):
            acknowledgement_from_wire(broken, Command.BENCHMARK, "b1")
        failed = {"id": "b1", "state": "failed", "error": "benchmark: busy"}
        self.assertEqual(acknowledgement_from_wire(failed, Command.BENCHMARK, "b1").error,
                         "benchmark: busy")


class ThreadTests(unittest.TestCase):
    def test_a_stat_line_with_an_awkward_name_parses(self):
        line = "123 (dxvk (cs) x) R 1 1 1 0 -1 4194304 10 0 0 0 250 50 0 0 20 0 1 0 1 0 0"
        self.assertEqual(parse_stat(line, 100), ("dxvk (cs) x", 3.0))

    def test_the_window_turns_cpu_time_into_busy_fractions_by_group(self):
        samples = [Sample(99.0, {10: ("JustCause3.exe", 5.0), 11: ("dxvk-cs", 1.0),
                                 12: ("CpuFragmentThre", 0.0)}),
                   Sample(111.0, {10: ("JustCause3.exe", 11.0), 11: ("dxvk-cs", 10.6),
                                  12: ("CpuFragmentThre", 3.0), 13: ("new", 1.0)})]
        result = window(samples, 100.0, 110.0, pid=10)
        self.assertEqual(result["window_seconds"], 12.0)
        self.assertAlmostEqual(result["groups"]["game"]["busy"], 0.5)
        self.assertAlmostEqual(result["groups"]["dxvk_cs"]["busy"], 0.8)
        self.assertAlmostEqual(result["groups"]["fragment_workers"]["busy"], 0.25)
        self.assertEqual(result["top"][0]["name"], "dxvk-cs")
        self.assertIsNone(window(samples, 98.0, 110.0))


class ScheduleTests(unittest.TestCase):
    def test_modes_rotate_between_repeats_within_each_scale(self):
        modes = (Mode.plain(BenchmarkMode.MULTIPASS), Mode.plain(BenchmarkMode.SINGLE_PASS))
        order = schedule(modes, 2, (0.5, 1.0))
        self.assertEqual([(scale, repeat, mode.name) for scale, repeat, mode in order], [
            (0.5, 0, "multipass"), (0.5, 0, "single_pass"),
            (0.5, 1, "single_pass"), (0.5, 1, "multipass"),
            (1.0, 0, "multipass"), (1.0, 0, "single_pass"),
            (1.0, 1, "single_pass"), (1.0, 1, "multipass")])

    def test_a_label_is_a_request_id_at_any_scale(self):
        self.assertEqual(run_label("town", "single_pass", 1.4142, 2), "town-single_pass-1p4142-r2")
        self.assertEqual(run_label("town", "multipass", None, 0), "town-multipass-native-r0")
        BenchmarkRequest("b1", run_label("capital", "fsr-per-eye-off", 0.7071, 1), "single_pass", 1.0)

    def test_the_shipped_suite_loads(self):
        suite = load_suite(Path(__file__).resolve().parents[1] / "suites/benchmark.toml")
        self.assertIn("capital", [scene.name for scene in suite.scenes])
        self.assertTrue(all(variant.switches or variant.overlap_render_update is not None
                            or variant.fsr is not None or variant.hybrid_velocity is not None
                            or variant.large_cache_cores is not None or variant.jitter is not None
                            or variant.foveation is not None
                            for variant in suite.variants))
        jitter_off = {variant.name: variant for variant in suite.variants}["fsr-jitter-off"]
        self.assertIs(Mode.variant(jitter_off).jitter, False)
        self.assertIn("fsr_jitter = false",
                      BenchmarkRequest("b1", "x", "single_pass", 1.0, fsr_jitter=False).to_toml())
        placement = {variant.name: variant for variant in suite.variants}["multipass-large-cache-off"]
        self.assertEqual((placement.base, placement.large_cache_cores),
                         (BenchmarkMode.MULTIPASS, False))
        fsr_off = {variant.name: variant for variant in suite.variants}["fsr-off"]
        self.assertIs(Mode.variant(fsr_off).fsr, False)
        serial = {variant.name: variant for variant in suite.variants}["multipass-serial"]
        self.assertEqual(serial.base, BenchmarkMode.MULTIPASS)
        self.assertIs(Mode.variant(serial).overlap_render_update, False)
        moving = next(scene for scene in suite.scenes if scene.name == "town-moving")
        self.assertFalse(moving.camera.still)

    def test_a_multipass_variant_with_single_pass_switches_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "suite.toml"
            path.write_text("[[scene]]\nname = 'capital'\nconfig = 'coverage_capital_midday.toml'\n"
                            "[[variant]]\nname = 'x'\nbase = 'multipass'\n"
                            "switches = { fsr_per_eye = false }\n")
            with self.assertRaises(BenchmarkError):
                load_suite(path)

    def test_a_suite_without_a_stage_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "suite.toml"
            path.write_text("[[scene]]\nname = 'shore'\nconfig = 'shore_lake_production.toml'\n")
            with self.assertRaises(BenchmarkError):
                load_suite(path)


def run_file(scene, mode, repeat, frames, engine_size=(3584, 3561), double_wide=False):
    payload = ack(frames, [gpu_row(0, 9000)] * 5, engine_size=engine_size,
                  double_wide=double_wide)
    return {"format": RUN_FORMAT, "version": RUN_FORMAT_VERSION,
            "run": {"scene": scene, "mode": mode, "resolution_scale": None, "repeat": repeat},
            "session": {"compositor_fps": 1000}, "machine": {"cpu": "a CPU"},
            "payload": {k: v for k, v in payload.items() if k not in ("id", "state")},
            "threads": {"groups": {"dxvk_cs": {"busy": 0.4 + repeat / 10}}},
            "summary": summarize_run(payload)}


class ReportTests(unittest.TestCase):
    def test_repeats_group_by_scene_mode_and_size_and_render(self):
        runs = [run_file("town", "multipass", r, [frame(14000 + r * 1000)] * 3) for r in range(3)]
        runs += [run_file("town", "single_pass", r, [frame(10000)] * 3, (7168, 3561), True)
                 for r in range(3)]
        summary = summarize(runs)
        self.assertEqual(len(summary["groups"]), 2)
        multipass = next(g for g in summary["groups"] if g["mode"] == "multipass")
        self.assertEqual(multipass["frame_ms"]["median"], 15.0)
        self.assertEqual(multipass["frame_ms"]["spread"], [14.0, 16.0])
        self.assertEqual(multipass["threads_busy"]["dxvk_cs"], 0.5)
        single = next(g for g in summary["groups"] if g["mode"] == "single_pass")
        self.assertEqual(single["eye_size"], [3584, 3561])
        self.assertIn("| town | single_pass | 3584x3561 | 1000 | 10.00 / 10.00", markdown(summary))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, run in enumerate(runs):
                path = root / "runs" / "town" / f"{index}.json"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(run))
            write_summary(root)
            self.assertTrue((root / "summary.md").is_file())
            charts = render(root)
            self.assertTrue(charts)
            self.assertTrue(all(path.read_text().startswith("<svg") for path in charts))

    def test_modes_keep_their_colours(self):
        colours = mode_colours(["fsr-per-eye-off", "single_pass", "multipass"])
        self.assertEqual(colours["multipass"], "#2a78d6")
        self.assertEqual(colours["single_pass"], "#eb6834")

    def test_the_cpu_model_is_read_from_cpuinfo(self):
        self.assertEqual(cpu_model("processor : 0\nmodel name\t: A Test CPU\n"), "A Test CPU")
        self.assertIsNone(cpu_model("processor : 0\n"))


if __name__ == "__main__":
    unittest.main()
