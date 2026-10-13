#!/usr/bin/env python3
"""Tests for audit_ven_architecture.py — run: python -m unittest scripts/test_audit_ven_architecture.py"""
import os
import sys
import unittest

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

    def test_the_plans_wire_fields_and_kind_free_names_are_not(self):
        for line in [
            "bat_charge_kw: storage.charge_kw[t],",
            "bat_discharge_kw: 0.0,",
            "c_bat_ev_coexist_eur_kwh: p.c_bat_ev_coexist_eur_kwh,",
            "let net_power_kw = asset.net_power_expr(t);",
        ]:
            self.assertFalse(audit.names_asset_kind(line), line)

    def test_the_planner_core_names_no_kind_today(self):
        self.assertEqual(audit.asset_kind_in_planner(), [])
        self.assertEqual(audit.solution_detail_outside_its_kind(), [])


if __name__ == "__main__":
    unittest.main()
