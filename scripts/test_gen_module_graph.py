#!/usr/bin/env python3
"""Tests for gen_module_graph.py and audit_module_graph.py's coverage check.

Run: python -m unittest scripts/test_gen_module_graph.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import gen_module_graph as gen  # noqa: E402
import audit_module_graph as audit  # noqa: E402


class ModuleOf(unittest.TestCase):
    def test_top_level_directory_and_file(self):
        self.assertEqual(gen.module_of("routes/hems/ev.rs"), "routes")
        self.assertEqual(gen.module_of("vtn.rs"), "vtn")

    def test_milp_planner_is_its_own_module(self):
        # The ring map puts controller/milp_planner/ in infra, the rest of controller/ in domain.
        self.assertEqual(gen.module_of("controller/milp_planner/inputs.rs"), "controller/milp_planner")
        self.assertEqual(gen.module_of("controller/dispatcher.rs"), "controller")


class Edges(unittest.TestCase):
    def test_a_crate_path_is_an_edge_and_a_self_reference_is_not(self):
        lines = {
            "routes/sim.rs": ["use crate::services::user_request::X;", "let a = crate::routes::y();"],
            "services/forecast.rs": ["fn f() -> crate::entities::plan::Plan {"],
        }
        self.assertEqual(gen.edges(lines), {("routes", "services"), ("services", "entities")})

    def test_milp_planner_paths_are_told_apart_from_controller(self):
        lines = {
            "services/planning/mod.rs": ["use crate::controller::milp_planner::run_planner;"],
            "controller/milp_planner/inputs.rs": ["use crate::controller::sim_snapshot::S;"],
        }
        self.assertEqual(
            gen.edges(lines),
            {("services", "controller/milp_planner"), ("controller/milp_planner", "controller")},
        )

    def test_a_crate_root_re_export_is_not_a_module(self):
        # `crate::AppCtx` names a type main.rs re-exports, not a module: it must not become a node.
        lines = {"routes/sim.rs": ["use crate::AppCtx;", "use crate::state::AppState;"]}
        self.assertEqual(gen.edges(lines, modules={"routes", "state"}), {("routes", "state")})

    def test_lab_core_is_an_external_node(self):
        self.assertEqual(gen.edges({"vtn.rs": ["use lab_core::vtn_auth::ClientCredentials;"]}),
                         {("vtn", "lab-core")})


class Render(unittest.TestCase):
    def test_output_is_deterministic_and_grouped_by_ring(self):
        mods = ["routes", "services", "entities", "boot"]
        e = {("services", "entities"), ("routes", "services"), ("boot", "routes")}
        a = gen.render(mods, e)
        self.assertEqual(a, gen.render(list(reversed(mods)), set(e)))
        self.assertTrue(a.startswith("```mermaid\n") and a.endswith("```\n"))
        self.assertLess(a.index("Adapters"), a.index("M_routes["))
        self.assertLess(a.index("Wiring"), a.index("M_boot["))
        self.assertIn("M_routes --> M_services", a)


class Section(unittest.TestCase):
    DOC = "# T\n\nintro\n\n" + gen.BEGIN + "\nold\n" + gen.END + "\n\ntail\n"

    def test_replace_section_keeps_what_surrounds_it(self):
        out = gen.replace_section(self.DOC, "NEW\n")
        self.assertEqual(out, "# T\n\nintro\n\n" + gen.BEGIN + "\nNEW\n" + gen.END + "\n\ntail\n")

    def test_a_doc_without_markers_is_refused(self):
        with self.assertRaises(ValueError):
            gen.replace_section("# no markers\n", "NEW\n")


class Coverage(unittest.TestCase):
    def test_a_task_loop_or_directory_missing_from_the_diagram_is_reported(self):
        diagram = 'T_A["spawn_a()"]\nROUTES["routes/"]'
        missing = audit.coverage_gaps(diagram, spawn_fns=["spawn_a", "spawn_b"],
                                      directories=["routes", "boot"])
        self.assertEqual(missing, ["boot/", "spawn_b"])

    def test_nothing_missing_reports_nothing(self):
        self.assertEqual(audit.coverage_gaps('X["spawn_a() routes/"]', ["spawn_a"], ["routes"]), [])


if __name__ == "__main__":
    unittest.main()
