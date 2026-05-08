#!/usr/bin/env python3
import os
import sys


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


def main():
    failures = []
    base = os.path.join('VEN', 'src', 'tasks')
    if not os.path.isdir(base):
        print(f"No tasks directory found at {base}; skipping audit (0 files)")
        sys.exit(0)
    for root, dirs, files in os.walk(base):
        for fn in files:
            if fn.endswith('.rs'):
                path = os.path.join(root, fn)
                prod_lines = count_production_lines(path)
                if prod_lines > 200:
                    failures.append((path, prod_lines))
    if failures:
        print("FILE SIZE AUDIT FAILED: The following files exceed 200 production lines:")
        for p, n in failures:
            print(f"{p}: {n} lines")
        sys.exit(2)
    else:
        print("FILE SIZE AUDIT PASSED: All files under VEN/src/tasks/ are <=200 production lines (excluding #[cfg(test)] blocks).")
        sys.exit(0)


if __name__ == '__main__':
    main()
