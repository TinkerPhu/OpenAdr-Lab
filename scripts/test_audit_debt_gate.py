#!/usr/bin/env python3
"""Tests for audit_debt_gate.py — run: python -m unittest scripts/test_audit_debt_gate.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import audit_debt_gate as gate  # noqa: E402

REGISTER = """\
| ID | Description | Affected files | Effort | Risk | Priority | Gain |
|----|-------------|----------------|--------|------|----------|------|
| R-89 | A BDD step asserts a sign. | `tests/features/ven_reporting_out.feature:32`, `VEN/src/controller/report_intervals.rs` | Trivial | Low | 🟢 | Low |
| R-77 | Naming audit. | `VEN/src` (see grep for `envelope`) | Medium | Low | 🟡 | Low |
| R-71 | Allow sites. | `VEN/src` (run `grep -rn x`) | Small | Low | Low |
| R-39 | state split. | `VEN/src/state/mod.rs` | Trivial | Mechanical | Low |
| R-33 | JsonDialog twin. | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | Low |
| R-73 | dead methods. | `VEN/src/assets/battery.rs`, `VEN/src/controller/milp_planner/asset_port.rs` | Trivial | Low | Low |
| R-21 | heap corruption. | `VEN/src/controller/milp_planner/` | Medium | Low | Medium |
| R-47 | AppState. | `VEN/src/controller/` | Small | Low | Low |
| 🔴 High | not a debt row | | | | |
"""


class ParseRows(unittest.TestCase):
    def test_only_small_and_trivial_rows_are_gated(self):
        ids = [r.id for r in gate.parse_rows(REGISTER)]
        self.assertEqual(ids, ["R-89", "R-71", "R-39", "R-33", "R-73", "R-47"])

    def test_path_tokens_drop_line_suffix_and_prose(self):
        row = gate.parse_rows(REGISTER)[0]
        self.assertEqual(
            row.paths,
            ["tests/features/ven_reporting_out.feature", "VEN/src/controller/report_intervals.rs"],
        )

    def test_whole_tree_tokens_are_too_broad_to_match(self):
        r71 = [r for r in gate.parse_rows(REGISTER) if r.id == "R-71"][0]
        self.assertEqual(r71.paths, [])


class Matching(unittest.TestCase):
    def rows(self):
        return {r.id: r for r in gate.parse_rows(REGISTER)}

    def test_exact_file(self):
        hit = gate.matched_files(self.rows()["R-39"], ["VEN/src/state/mod.rs"])
        self.assertEqual(hit, ["VEN/src/state/mod.rs"])

    def test_directory_token_matches_children(self):
        hit = gate.matched_files(self.rows()["R-47"], ["VEN/src/controller/arbiter.rs"])
        self.assertEqual(hit, ["VEN/src/controller/arbiter.rs"])

    def test_glob_token(self):
        hit = gate.matched_files(self.rows()["R-33"], ["VEN/ui/src/components/JsonDialog.tsx"])
        self.assertEqual(hit, ["VEN/ui/src/components/JsonDialog.tsx"])

    def test_unrelated_file_does_not_match(self):
        self.assertEqual(gate.matched_files(self.rows()["R-39"], ["VEN/src/state/other.rs"]), [])

    def test_docs_and_register_are_never_a_trigger(self):
        r = self.rows()["R-89"]
        self.assertEqual(gate.matched_files(r, ["docs/reference/TECHNICAL_DEBTS.md"]), [])


class Evaluate(unittest.TestCase):
    def setUp(self):
        self.rows = gate.parse_rows(REGISTER)

    def test_touched_open_debt_is_unresolved(self):
        out = gate.evaluate(self.rows, ["VEN/src/state/mod.rs"], head_ids={"R-39"}, base_ids={"R-39"}, deferrals={})
        self.assertEqual([u.id for u in out.unresolved], ["R-39"])

    def test_removed_from_register_counts_as_resolved(self):
        out = gate.evaluate(self.rows, ["VEN/src/state/mod.rs"], head_ids=set(), base_ids={"R-39"}, deferrals={})
        self.assertEqual(out.unresolved, [])
        self.assertEqual(out.resolved, ["R-39"])

    def test_deferral_with_reason_counts(self):
        out = gate.evaluate(
            self.rows, ["VEN/src/state/mod.rs"], head_ids={"R-39"}, base_ids={"R-39"},
            deferrals={"R-39": "needs its own review, touches AppState wiring"},
        )
        self.assertEqual(out.unresolved, [])
        self.assertEqual(list(out.deferred), ["R-39"])

    def test_deferral_without_reason_is_rejected(self):
        self.assertEqual(gate.parse_deferrals("Debt-deferred: R-39\n"), {})
        self.assertEqual(gate.parse_deferrals("Debt-deferred: R-39: later\n"), {})

    def test_deferral_parsing(self):
        msg = "fix(x): y\n\nDebt-deferred: R-39: needs its own review of AppState\n"
        self.assertEqual(gate.parse_deferrals(msg), {"R-39": "needs its own review of AppState"})

    def test_debt_filed_in_this_branch_is_not_gated(self):
        out = gate.evaluate(self.rows, ["VEN/src/state/mod.rs"], head_ids={"R-39"}, base_ids=set(), deferrals={})
        self.assertEqual(out.unresolved, [])


KINDS_DOC = """intro text
1. `bug` — wrong
2. `wire-contract` — wire
3. `style` — naming
not a kind line
"""

TYPED = """| ID | Description | Affected files | Severity | Kind | Cost | Why open |
|----|-------------|----------------|----------|------|------|----------|
| R-1 | a | `VEN/src/a/b/x.rs` | S3 | style | Small | too-big |
| R-2 | b | `VEN/src/a/b/y.rs` | S1 | wire-contract | Small | needs-decision |
| R-3 | c | `VEN/src/a/b/z.rs` | S2 | wire-contract | Small | needs-decision |
| R-4 | d | `VEN/src/a/b/w.rs` | S1 | bug | Trivial | too-big |
"""


class Classification(unittest.TestCase):
    def test_kind_rank_follows_document_order(self):
        self.assertEqual(gate.parse_kind_rank(KINDS_DOC), {"bug": 0, "wire-contract": 1, "style": 2})

    def test_typed_table_is_read_by_header(self):
        rows = {r.id: r for r in gate.parse_rows(TYPED)}
        self.assertEqual((rows["R-2"].kind, rows["R-2"].severity), ("wire-contract", "S1"))
        self.assertEqual(rows["R-2"].paths, ["VEN/src/a/b/y.rs"])

    def test_untyped_rows_have_no_kind(self):
        row = gate.parse_rows(REGISTER)[0]
        self.assertEqual((row.kind, row.severity), (None, None))

    def test_sort_is_kind_rank_then_severity_then_unknown_last(self):
        rank = gate.parse_kind_rank(KINDS_DOC)
        rows = gate.parse_rows(TYPED) + gate.parse_rows(REGISTER)[:1]
        order = [r.id for r in gate.sort_by_priority(rows, rank)]
        self.assertEqual(order, ["R-4", "R-2", "R-3", "R-1", "R-89"])


if __name__ == "__main__":
    unittest.main()
