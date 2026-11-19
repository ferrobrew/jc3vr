import unittest

from tools.rendering.comparison.draw_census import CensusError, pass_ids, rows, select, totals
from tools.rendering.experiment import Diagnostics, ExperimentError


def entry(pass_id, name, treatment, calls, submissions, ps=None):
    return {"dispatch": 0, "pass": pass_id, "pass_name": name, "block": "Block",
            "vertex_shader": {"status": "named", "name": "vs"},
            "pixel_shader": ps or {"status": "unbound"}, "entry": "indexed",
            "reissue_eye": None, "treatment": treatment, "calls": calls, "submissions": submissions}


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


if __name__ == "__main__":
    unittest.main()
