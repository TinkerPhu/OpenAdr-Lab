#!/usr/bin/env python3
"""Tests for audit_ven_architecture.py — run: python -m unittest scripts/test_audit_ven_architecture.py"""
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import audit_ven_architecture as audit  # noqa: E402


class RustFiles(unittest.TestCase):
    def test_a_file_is_a_base_of_its_own(self):
        f = audit.VEN_SRC / "controller" / "milp_interactions.rs"
        self.assertEqual(audit.rust_files(f), [f])

    def test_a_directory_is_walked(self):
        files = audit.rust_files(audit.VEN_SRC / "controller" / "milp_planner")
        self.assertIn(audit.VEN_SRC / "controller" / "milp_planner" / "results.rs", files)
        self.assertFalse(any("tests" in p.parts for p in files), "test-only files are skipped")


class AssetKindInPlanner(unittest.TestCase):
    def test_a_kind_named_in_the_planner_is_found(self):
        for line in [
            "let heater_kw = 0.0;",
            "if let Some(v) = &pool.bat {",
            "AssetKind::Battery => 1.0,",
            "crate::ids::ASSET_EV",
            "AssetSolutionDetail::Ev(sol) => sol,",
            "let ev_power_kw = sol.net_power_kw[t];",
        ]:
            self.assertTrue(audit.names_asset_kind(line), line)

    def test_a_kind_named_plan_field_is_found(self):
        for line in ["pub bat_charge_kw: f64,", "bat_discharge_kw: 0.0,"]:
            self.assertTrue(audit.names_asset_kind(line), line)

    def test_kind_free_names_are_not(self):
        for line in [
            "c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,",
            "pub planned_asset_ids: Vec<String>,",
            "let net_power_kw = asset.net_power_expr(t);",
        ]:
            self.assertFalse(audit.names_asset_kind(line), line)

    def test_the_planner_core_names_no_kind_today(self):
        self.assertEqual(audit.asset_kind_in_planner(), [])
        self.assertEqual(audit.solution_detail_outside_its_kind(), [])

    def test_the_plans_wire_types_are_checked(self):
        self.assertIn(audit.VEN_SRC / "entities" / "plan.rs", audit.KIND_FREE_PATHS)

    def test_a_battery_field_planted_in_the_plan_is_found(self):
        plan_rs = (audit.VEN_SRC / "entities" / "plan.rs").read_text(encoding="utf-8")
        anchor = "pub struct PlanTimeSlot {\n"
        self.assertIn(anchor, plan_rs)
        with tempfile.TemporaryDirectory(dir=audit.REPO_ROOT) as tmp:
            copy = Path(tmp) / "plan.rs"
            copy.write_text(plan_rs, encoding="utf-8")
            self.assertEqual(audit.asset_kind_in_planner([copy]), [], "the plan is kind-free")
            copy.write_text(
                plan_rs.replace(anchor, anchor + "    pub bat_charge_kw: f64,\n", 1),
                encoding="utf-8",
            )
            hits = audit.asset_kind_in_planner([copy])
        self.assertEqual(len(hits), 1, hits)
        self.assertIn("pub bat_charge_kw: f64,", hits[0])


class CollapsedContinuation(unittest.TestCase):
    def test_a_lost_continuation_is_found(self):
        with tempfile.TemporaryDirectory(dir=audit.REPO_ROOT) as tmp:
            rs = Path(tmp) / "m.rs"
            rs.write_text(
                'fn f() -> &\'static str {\n    "does not              open the grid"\n}\n',
                encoding="utf-8",
            )
            self.assertEqual(len(audit.collapsed_continuations(rs)), 1)
            rs.write_text(
                'fn f() -> &\'static str {\n    "does not \\\n     open the grid"\n}\n',
                encoding="utf-8",
            )
            self.assertEqual(audit.collapsed_continuations(rs), [])

    def test_the_ven_has_none_today(self):
        self.assertEqual(audit.collapsed_continuations(), [])


if __name__ == "__main__":
    unittest.main()
