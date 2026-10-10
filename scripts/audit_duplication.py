#!/usr/bin/env python3
"""Duplication and long-function ratchet (R-119).

Two measures over production code (Rust and TypeScript, test code excluded):

  clusters         runs of WINDOW consecutive significant lines that occur in two or more places,
                   counted per set of files they occur in
  long functions   Rust functions longer than MAX_FN_LINES lines

Test code is not measured file set by file set (a fixture refactor moves windows between sets)
but as one total of duplicated windows per directory in TEST_ROOTS, which may not grow (R-120).

The existing state is the baseline (scripts/audit_duplication_baseline.json). The gate fails only
when a branch ADDS a cluster, GROWS one, adds a long function or lengthens one; improvements pass
and are recorded by `--update`. It makes `one-concept-one-function` checkable instead of
remembered: run it before any VEN/UI merge, like audit_file_sizes.py.

  python scripts/audit_duplication.py            # check against the baseline
  python scripts/audit_duplication.py --update   # re-record the baseline (after reducing debt)
"""
import collections
import hashlib
import json
import os
import re
import sys

ROOTS = ["VEN/src", "lab-core/src", "VTN/bff/src", "VTN/ui/src", "VEN/ui/src", "ui-charts/src"]
EXTENSIONS = (".rs", ".ts", ".tsx")
WINDOW = 8
MAX_FN_LINES = 100
TEST_ROOTS = ["VEN/src/controller/milp_planner/tests"]
BASELINE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "audit_duplication_baseline.json")

_BRACKETS_ONLY = re.compile(r"[\}\)\];,{(]*")
_FN_START = re.compile(r"^\s*(?:pub(?:\([a-z:]+\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)")


def is_test_path(path):
    return (
        "/tests/" in path
        or "__tests__" in path
        or path.endswith("_tests.rs")
        or ".test." in path
        or "/test_support/" in path
        or "testUtils" in path
    )


def production_text(text, ext):
    """Rust: everything before the first inline `#[cfg(test)]`; other languages unchanged."""
    if ext != ".rs":
        return text
    lines = text.split("\n")
    for i, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            return "\n".join(lines[:i])
    return text


def significant_lines(text, ext):
    """(line number, whitespace-normalised text) of every line that carries logic."""
    out = []
    for number, line in enumerate(production_text(text, ext).split("\n"), 1):
        s = line.strip()
        if not s or s.startswith(("//", "*", "/*", "#[", "use ", "import ")):
            continue
        if _BRACKETS_ONLY.fullmatch(s):
            continue
        out.append((number, re.sub(r"\s+", " ", s)))
    return out


def duplicate_clusters(files):
    """{(sorted distinct file tuple): number of duplicated windows}, for windows seen in 2+ places."""
    seen = collections.defaultdict(set)
    for path, text in files.items():
        sig = significant_lines(text, os.path.splitext(path)[1])
        for k in range(len(sig) - WINDOW + 1):
            digest = hashlib.md5("\n".join(s for _, s in sig[k:k + WINDOW]).encode()).hexdigest()
            seen[digest].add((path, sig[k][0]))
    clusters = collections.Counter()
    for spots in seen.values():
        if len(spots) >= 2:
            clusters[tuple(sorted({p for p, _ in spots}))] += 1
    return dict(clusters)


def long_functions(files):
    """{"path::name": length in lines} for Rust functions over MAX_FN_LINES."""
    out = {}
    for path, text in files.items():
        if not path.endswith(".rs"):
            continue
        lines = production_text(text, ".rs").split("\n")
        i = 0
        while i < len(lines):
            m = _FN_START.match(lines[i])
            if not m:
                i += 1
                continue
            depth, opened, j = 0, False, i
            while j < len(lines):
                code = re.sub(r'"(?:\\.|[^"\\])*"', '""', lines[j].split("//")[0])
                depth += code.count("{") - code.count("}")
                opened = opened or "{" in code
                if opened and depth <= 0:
                    break
                if not opened and code.rstrip().endswith(";"):
                    break  # trait method without a body
                j += 1
            length = j - i + 1
            if opened and length > MAX_FN_LINES:
                out[f"{path}::{m.group(1)}"] = length
            i = j + 1
    return out


def duplicated_windows(files):
    """Total duplicated windows in `files`: the one number test directories are held to."""
    return sum(duplicate_clusters(files).values())


def regressions(clusters, long_fns, baseline, test_windows=None):
    out = []
    for files, count in sorted(clusters.items()):
        before = baseline["clusters"].get("|".join(files), 0)
        if count > before:
            what = "new duplicated code" if before == 0 else f"duplication grew {before} -> {count} windows"
            out.append(f"{what}: {' | '.join(files)}")
    for name, length in sorted(long_fns.items()):
        before = baseline["long_functions"].get(name, 0)
        if length > before:
            what = f"new function over {MAX_FN_LINES} lines" if before == 0 else f"function grew {before} -> {length} lines"
            out.append(f"{what}: {name}")
    tracked = baseline.get("test_duplicated_windows", {})
    for directory, count in sorted((test_windows or {}).items()):
        if directory in tracked and count > tracked[directory]:
            out.append(f"test fixtures duplicated more, {tracked[directory]} -> {count} windows: {directory}")
    return out


def collect(root="."):
    files = {}
    for base in ROOTS:
        for directory, dirs, names in os.walk(os.path.join(root, base)):
            dirs[:] = [d for d in dirs if d != "node_modules"]
            for name in names:
                path = os.path.join(directory, name).replace("\\", "/")
                rel = os.path.relpath(path, root).replace("\\", "/")
                if name.endswith(EXTENSIONS) and not is_test_path(rel):
                    with open(path, encoding="utf-8", errors="ignore") as f:
                        files[rel] = f.read()
    return files


def collect_tests(directory, root="."):
    """Every Rust file under one test directory, whole (no production cut)."""
    files = {}
    for base, _, names in os.walk(os.path.join(root, directory)):
        for name in names:
            if name.endswith(".rs"):
                path = os.path.join(base, name).replace("\\", "/")
                with open(path, encoding="utf-8", errors="ignore") as f:
                    files[os.path.relpath(path, root).replace("\\", "/")] = f.read()
    return files


def main(argv):
    files = collect()
    clusters, long_fns = duplicate_clusters(files), long_functions(files)
    test_windows = {d: duplicated_windows(collect_tests(d)) for d in TEST_ROOTS}
    current = {
        "clusters": {"|".join(k): v for k, v in sorted(clusters.items())},
        "long_functions": dict(sorted(long_fns.items())),
        "test_duplicated_windows": test_windows,
    }
    if "--update" in argv:
        with open(BASELINE, "w", encoding="utf-8", newline="\n") as f:
            json.dump(current, f, indent=1)
            f.write("\n")
        print(f"baseline written: {len(current['clusters'])} clusters, {len(current['long_functions'])} long functions, "
              f"test windows {test_windows}")
        return 0
    with open(BASELINE, encoding="utf-8") as f:
        baseline = json.load(f)
    bad = regressions(clusters, long_fns, baseline, test_windows)
    for line in bad:
        print("FAIL", line)
    if bad:
        print("Consolidate the copy (one-concept-one-function) or split the function; if the growth is "
              "justified, run --update and say why in the commit message.")
        return 1
    print(f"OK    duplication: {len(current['clusters'])} clusters and {len(current['long_functions'])} "
          f"long functions, none added or grown against the baseline; test windows {test_windows}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
