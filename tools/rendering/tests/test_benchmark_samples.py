"""The draw-task samples' attribution: modules, the code a sample is charged to, engine seams, and
the request that asks for them."""

import json
import tempfile
import unittest
from pathlib import Path

from tools.rendering.benchmark.samples import (
    Module,
    Pool,
    first_caller,
    group_of,
    load,
    pass_names,
    report,
    stage_of,
)
from tools.rendering.protocol.benchmark import BenchmarkMode, BenchmarkRequest

GAME = Module("JustCause3.exe", 0x140000000, 0x3000000)
D3D = Module("d3d11.dll", 0x6FFFF0000000, 0x400000)
PAYLOAD = Module("jc3vr_payload_loaded.dll", 0x6FFFE0000000, 0x2000000)
NTDLL = Module("ntdll.dll", 0x6FFFD0000000, 0x200000)
MODULES = [GAME, D3D, PAYLOAD, NTDLL]


def row(pass_id, rip, *returns, reissue=0, block_type=0):
    return [pass_id, reissue, block_type, rip, *returns] + [0] * (16 - len(returns))


class AttributionTests(unittest.TestCase):
    def test_modules_are_grouped_by_name(self):
        self.assertEqual(group_of(GAME), "game")
        self.assertEqual(group_of(D3D), "d3d (DXVK)")
        self.assertEqual(group_of(PAYLOAD), "payload")
        self.assertEqual(group_of(None), "unmapped (wine unix side)")

    def test_runtime_time_is_charged_to_the_code_that_called_it(self):
        pool = Pool(MODULES, {}, [], 1.0, 1, 0, 0, 0.0)
        through_payload = row(50, D3D.base + 16, D3D.base + 32, PAYLOAD.base + 8, GAME.base + 8)
        self.assertEqual(first_caller(pool, through_payload), PAYLOAD.base + 8)
        from_game = row(50, NTDLL.base + 4, GAME.base + 64)
        self.assertEqual(first_caller(pool, from_game), GAME.base + 64)
        in_game = row(50, GAME.base + 12)
        self.assertEqual(first_caller(pool, in_game), GAME.base + 12)
        self.assertIsNone(first_caller(pool, row(50, D3D.base + 1)))

    def test_passes_fall_into_the_engine_seams(self):
        names = pass_names()
        index = {name: value for value, name in names.items()}
        self.assertEqual(stage_of(255, names), "outside a pass")
        self.assertEqual(stage_of(index["PRE_RP_SHADOW_0"], names), "PreDraw (prepasses)")
        self.assertEqual(stage_of(index["RP_Z_OCCLUDERS"], names), "DrawGBuffer")
        self.assertEqual(stage_of(index["RP_REFLECTIVE_WATER_PLANES"], names), "Draw (scene)")
        self.assertEqual(stage_of(index["RP_POSTEFFECTS"], names), "DrawPosteffects")

    def test_a_run_file_is_reported(self):
        rows = [row(50, GAME.base + 12), row(50, D3D.base + 4, GAME.base + 64),
                row(50, PAYLOAD.base + 4, GAME.base + 64, reissue=2)]
        run = {"payload": {
            "columns": ["frame_us", "draw_task_us"], "frames": [[13900, 10000], [13900, 12000]],
            "draw_task_samples": {
                "period_us": 1000, "mean_suspend_us": 30.0, "idle": 5, "torn": 0,
                "modules": [{"name": m.name, "base": m.base, "size": m.size} for m in MODULES],
                "block_types": [], "columns": [], "rows": rows}}}
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "run.json"
            path.write_text(json.dumps(run))
            pool = load([path])
        self.assertEqual(pool.draw_ms, 11.0)
        text = report(pool, pass_names())
        self.assertIn("| game -> game | 1 |", text)
        self.assertIn("| payload -> payload | 1 |", text)
        self.assertIn("| re-issue, eye 1 | 1 |", text)

    def test_the_request_names_the_sampler_only_when_asked(self):
        plain = BenchmarkRequest(id="b1", label="a", mode=BenchmarkMode.SINGLE_PASS,
                                 record_seconds=1.0)
        self.assertNotIn("sample_draw_task", plain.to_toml())
        sampled = BenchmarkRequest(id="b1", label="a", mode=BenchmarkMode.SINGLE_PASS,
                                   record_seconds=1.0, sample_draw_task=True)
        self.assertIn("sample_draw_task = true", sampled.to_toml())


if __name__ == "__main__":
    unittest.main()
