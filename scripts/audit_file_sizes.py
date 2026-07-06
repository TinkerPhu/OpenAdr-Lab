#!/usr/bin/env python3
"""File-size cap audit for VEN/src/.

Caps (per .claude/CLAUDE.md):
  - VEN/src/tasks/  : 200 production lines
  - VEN/src/ (rest) : 500 production lines

"Production lines" = non-blank lines, excluding inline `#[cfg(test)] mod X { ... }`
blocks. Whole files/directories gated by an out-of-file `#[cfg(test)] mod tests;`
declaration (e.g. controller/milp_planner/tests/) are test-only and excluded by
path (any directory component literally named `tests`).

ALLOWLIST holds files accepted as an exception to the 500-line cap: cohesive
dispatch/glue code where a line-count-driven split would hurt navigability more
than it helps (see docs/plans/review_items_resolution_strategy.md R4). Keep this
list short and re-justify each entry there before adding to it.
"""
import os
import sys

ALLOWLIST = {
    # AssetConfig enum-dispatch boilerplate — real fix is the enum->trait refactor
    # already tracked in docs/plans/refactoring_backlog.md, not a line-count split.
    os.path.join('VEN', 'src', 'assets', 'mod.rs'),
}


def strip_test_blocks(lines):
    out = []
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        if line.strip().startswith('#[cfg(test)]'):
            # skip attribute line and subsequent block until balanced braces
            j = i + 1
            # find opening brace
            while j < n and '{' not in lines[j]:
                j += 1
            if j >= n:
                i = j
                continue
            # found opening brace
            brace_count = lines[j].count('{') - lines[j].count('}')
            j += 1
            while j < n and brace_count > 0:
                brace_count += lines[j].count('{') - lines[j].count('}')
                j += 1
            i = j
            continue
        else:
            out.append(line)
            i += 1
    return out


def count_production_lines(path):
    try:
        with open(path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
    except Exception as e:
        print(f"SKIP {path}: {e}")
        return 0
    stripped = strip_test_blocks(lines)
    prod_lines = sum(1 for l in stripped if l.strip() != '')
    return prod_lines


def is_test_only_path(path):
    """True if any path component is literally `tests` — whole-file test modules
    gated by an out-of-file `#[cfg(test)] mod tests;` declaration in their parent."""
    parts = path.split(os.sep)
    return 'tests' in parts


def audit_dir(base, cap, skip_test_paths=True):
    """Return list of (path, line_count) exceeding cap, honoring ALLOWLIST."""
    failures = []
    if not os.path.isdir(base):
        return failures
    for root, dirs, files in os.walk(base):
        for fn in files:
            if not fn.endswith('.rs'):
                continue
            path = os.path.join(root, fn)
            if skip_test_paths and is_test_only_path(path):
                continue
            if path in ALLOWLIST:
                continue
            n = count_production_lines(path)
            if n > cap:
                failures.append((path, n))
    return failures


def main():
    tasks_base = os.path.join('VEN', 'src', 'tasks')
    src_base = os.path.join('VEN', 'src')
    if not os.path.isdir(src_base):
        print(f"No VEN/src directory found at {src_base}; skipping audit (0 files)")
        sys.exit(0)

    tasks_failures = audit_dir(tasks_base, 200)

    # VEN/src/ cap applies to everything except tasks/ (which has its own, tighter
    # cap and is checked separately above).
    src_failures = []
    for root, dirs, files in os.walk(src_base):
        if os.path.commonpath([root, tasks_base]) == tasks_base or root.startswith(
            tasks_base + os.sep
        ):
            continue
        for fn in files:
            if not fn.endswith('.rs'):
                continue
            path = os.path.join(root, fn)
            if is_test_only_path(path) or path in ALLOWLIST:
                continue
            n = count_production_lines(path)
            if n > 500:
                src_failures.append((path, n))

    failures = tasks_failures + src_failures
    if failures:
        print("FILE SIZE AUDIT FAILED:")
        for p, n in tasks_failures:
            print(f"  {p}: {n} lines (cap: 200, VEN/src/tasks/)")
        for p, n in src_failures:
            print(f"  {p}: {n} lines (cap: 500, VEN/src/)")
        sys.exit(2)
    else:
        print(
            "FILE SIZE AUDIT PASSED: VEN/src/tasks/ <=200 and VEN/src/ (rest) <=500 "
            "production lines (excluding #[cfg(test)] blocks, test-only paths, and "
            "the documented allowlist)."
        )
        sys.exit(0)


if __name__ == '__main__':
    main()
