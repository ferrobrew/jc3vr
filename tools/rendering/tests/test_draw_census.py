import unittest

from tools.rendering.comparison import draw_accounting as acc
from tools.rendering.comparison.draw_census import CensusError, accounting_report, pass_ids, rows, select, totals
from tools.rendering.experiment import Diagnostics, ExperimentError


def entry(pass_id, name, treatment, calls, submissions, ps=None, block="Block", vs="vs", dispatch=0,
          reissue_eye=None, split_eye=None):
    return {"dispatch": dispatch, "pass": pass_id, "pass_name": name, "block": block,
            "vertex_shader": {"status": "named", "name": vs},
            "pixel_shader": ps or {"status": "unbound"}, "entry": "indexed",
            "reissue_eye": reissue_eye, "split_eye": split_eye, "treatment": treatment, "calls": calls,
            "submissions": submissions}


class DrawCensusTest(unittest.TestCase):
    ENTRIES = [entry(1, "RP_A", "once", 3, 3), entry(2, "RP_B", "per_eye", 2, 4),
               entry(3, "RP_C", "instance_doubled", 5, 5)]

    def test_pass_bounds_are_inclusive_names(self):
        kept = select(self.ENTRIES, "RP_A:RP_B")
        self.assertEqual([e["pass_name"] for e in kept], ["RP_A", "RP_B"])

    def test_an_unrecorded_bound_is_an_error(self):
        with self.assertRaises(CensusError):
            select(self.ENTRIES, "RP_A:RP_Z")
        with self.assertRaises(CensusError):
            select(self.ENTRIES, "RP_A")

    def test_bounds_may_be_unrecorded_passes_named_by_the_definitions(self):
        ids = pass_ids("pub enum RenderPassId {\n    RP_NONE = 0x0,\n    /// doc\n    RP_Z = 0x4,\n}")
        self.assertEqual(ids, {"RP_NONE": 0, "RP_Z": 4})
        kept = select(self.ENTRIES, "RP_NONE:RP_B", ids)
        self.assertEqual([e["pass_name"] for e in kept], ["RP_A", "RP_B"])

    def test_totals_sum_per_treatment(self):
        self.assertEqual(totals(self.ENTRIES),
                         ["instance_doubled: calls=5 submissions=5", "once: calls=3 submissions=3",
                          "per_eye: calls=2 submissions=4"])

    def test_pixel_shaders_are_named_from_the_bundle(self):
        named = rows([entry(1, "RP_A", "once", 1, 1, {"status": "pristine", "sha256": "ab" * 32})],
                     {"ab" * 32: "fragment flare"})
        self.assertIn("ps=flare", named[0])

    def test_the_census_needs_shader_identity(self):
        with self.assertRaises(ExperimentError):
            Diagnostics(draw_census=True)
        self.assertTrue(Diagnostics(shader_identity=True, draw_census=True).draw_census)


class DrawAccountingTest(unittest.TestCase):
    SINGLE_PASS = [
        entry(1, "RP_A", "instance_doubled", 10, 10, block="Model"),
        entry(1, "RP_A", "per_eye", 4, 8, block="Model", vs="instanced"),
        entry(2, "RP_B", "once", 3, 3, block="Tree", reissue_eye=0),
        entry(2, "RP_B", "once", 3, 3, block="Tree", reissue_eye=1),
        entry(3, "RP_C", "once", 2, 2, block="Post", split_eye=0),
        entry(3, "RP_C", "once", 2, 2, block="Post", split_eye=1),
    ]
    MULTIPASS = [entry(p, n, "once", c, c, block=b, vs="stale", dispatch=d)
                 for d in (0, 1) for p, n, b, c in ((1, "RP_A", "Model", 14), (2, "RP_B", "Tree", 3),
                                                     (3, "RP_C", "Post", 2))]

    def label(self, shader):
        return shader["name"]

    def test_the_collapses_second_eye_repeats_are_not_game_calls(self):
        whole = acc.total(acc.account(self.SINGLE_PASS, None, acc.by_pass, self.label))
        self.assertEqual((whole.calls, whole.single_pass, whole.per_eye_extra), (19, 28, 9))

    def test_each_pass_carries_its_multipass_submissions(self):
        passes = acc.account(self.SINGLE_PASS, self.MULTIPASS, acc.by_pass, self.label)
        self.assertEqual(passes[(1, "RP_A")], acc.Account(calls=14, single_pass=18, multipass=28,
                                                          per_eye_extra=4))
        self.assertEqual(passes[(2, "RP_B")], acc.Account(calls=3, single_pass=6, multipass=6,
                                                          per_eye_extra=3))

    def test_shader_families_are_not_compared_against_multipass(self):
        families = acc.account(self.SINGLE_PASS, self.MULTIPASS, acc.by_family, self.label)
        self.assertNotIn(("Model", "stale"), families)
        self.assertEqual(families[("Model", "instanced")].per_eye_extra, 4)

    def test_handling_separates_reissues_splits_and_draw_level_repeats(self):
        handling = acc.account(self.SINGLE_PASS, None, acc.by_handling, self.label)
        self.assertEqual({k[0]: v.per_eye_extra for k, v in handling.items()},
                         {"instance_doubled": 0, "per_eye": 4, "block_reissue": 3, "fullscreen_split": 2})

    def test_blocks_rank_by_the_submissions_a_single_draw_would_save(self):
        report = accounting_report(self.SINGLE_PASS, self.MULTIPASS, {})
        blocks = report[report.index("block: calls single_pass multipass per_eye_extra") + 1:]
        self.assertEqual(blocks[:3], ["Model 14 18 28 4", "Tree 3 6 6 3", "Post 2 4 4 2"])


if __name__ == "__main__":
    unittest.main()
