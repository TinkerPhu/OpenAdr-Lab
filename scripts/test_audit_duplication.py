#!/usr/bin/env python3
"""Tests for audit_duplication.py — run: python -m unittest scripts/test_audit_duplication.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import audit_duplication as dup  # noqa: E402

BLOCK = [f"let value_{i} = compute({i});" for i in range(10)]


def src(*blocks):
    return "\n".join(line for b in blocks for line in b) + "\n"


class Windows(unittest.TestCase):
    def test_comments_blanks_and_bare_brackets_are_not_significant(self):
        text = "// note\n\nlet a = 1;\n}\n  let   b = 2;\n/// doc\n"
        self.assertEqual([s for _, s in dup.significant_lines(text, ".rs")], ["let a = 1;", "let b = 2;"])

    def test_inline_test_module_is_cut_for_rust(self):
        text = "let a = 1;\n#[cfg(test)]\nmod tests {\n let b = 2;\n}\n"
        self.assertEqual([s for _, s in dup.significant_lines(text, ".rs")], ["let a = 1;"])

    def test_same_block_in_two_files_is_one_cluster(self):
        files = {"a.rs": src(BLOCK), "b.rs": src(["let other = 0;"], BLOCK)}
        clusters = dup.duplicate_clusters(files)
        self.assertEqual(list(clusters), [("a.rs", "b.rs")])
        self.assertEqual(clusters[("a.rs", "b.rs")], len(BLOCK) - dup.WINDOW + 1)

    def test_distinct_files_have_no_cluster(self):
        files = {"a.rs": src(BLOCK), "b.rs": src([f"let other_{i} = 0;" for i in range(10)])}
        self.assertEqual(dup.duplicate_clusters(files), {})

    def test_repetition_inside_one_file_counts_as_a_cluster_of_that_file(self):
        files = {"a.rs": src(BLOCK, ["let gap = 1;"], BLOCK)}
        self.assertIn(("a.rs",), dup.duplicate_clusters(files))


class LongFunctions(unittest.TestCase):
    def test_a_function_over_the_limit_is_reported_with_its_length(self):
        body = "\n".join("    let x = 1;" for _ in range(dup.MAX_FN_LINES + 5))
        text = f"pub fn big() {{\n{body}\n}}\n\nfn small() {{\n    let y = 2;\n}}\n"
        self.assertEqual(dup.long_functions({"a.rs": text}), {"a.rs::big": dup.MAX_FN_LINES + 7})


class Ratchet(unittest.TestCase):
    BASE = {"clusters": {"a.rs|b.rs": 5}, "long_functions": {"a.rs::big": 120}}

    def test_unchanged_passes(self):
        self.assertEqual(dup.regressions({("a.rs", "b.rs"): 5}, {"a.rs::big": 120}, self.BASE), [])

    def test_improvement_passes(self):
        self.assertEqual(dup.regressions({("a.rs", "b.rs"): 2}, {"a.rs::big": 90}, self.BASE), [])

    def test_a_new_cluster_fails(self):
        out = dup.regressions({("a.rs", "b.rs"): 5, ("c.rs", "d.rs"): 1}, {"a.rs::big": 120}, self.BASE)
        self.assertEqual(len(out), 1)
        self.assertIn("c.rs", out[0])

    def test_a_grown_cluster_fails(self):
        self.assertEqual(len(dup.regressions({("a.rs", "b.rs"): 6}, {"a.rs::big": 120}, self.BASE)), 1)

    def test_a_longer_or_new_long_function_fails(self):
        self.assertEqual(len(dup.regressions({("a.rs", "b.rs"): 5}, {"a.rs::big": 121}, self.BASE)), 1)
        self.assertEqual(len(dup.regressions({("a.rs", "b.rs"): 5}, {"a.rs::big": 120, "x.rs::new": 101}, self.BASE)), 1)


if __name__ == "__main__":
    unittest.main()
