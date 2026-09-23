#!/usr/bin/env python3
"""The VEN's hexagonal dependency rules, checked rather than grepped.

`.claude/CLAUDE.md` states four invariants as raw `grep` commands. Run as
written they report violations that are not violations: the milp_planner check
matches the two comment lines in `asset_port.rs` that *document* the invariant,
and the `vtn.rs` check matches nineteen lines of which every one is either a
private helper or inside `#[cfg(test)]`. A check that cries wolf is a check
nobody reads, so this applies the same rules to code only — comments, string
literals and test blocks removed first.

The rules, and why each exists:

  1. No `use crate::profile` in entities/, controller/ or routes/. Profile
     values are injected as typed parameter structs built in the application or
     infra layer; an inner ring that reaches for configuration has made itself
     unusable without it.
  2. No `use crate::assets::` in milp_planner/. The planner accepts
     `Vec<Box<dyn AssetMilpContext>>` — importing A_BAT/A_EV/A_HTR would make
     the solver know which assets exist, which is exactly what the port
     prevents.
  3. No `use crate::assets::` in entities/. The domain does not depend on infra.
  4. `serde_json::Value` in vtn.rs stays internal. The VtnPort's own surface is
     typed; an untyped value crossing it pushes wire-shape decisions into
     callers that cannot see the wire.

Reuses `strip_test_blocks` from audit_file_sizes.py rather than carrying a
second copy of the same rule.
"""

import os
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from audit_file_sizes import strip_test_blocks, is_test_only_path  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
VEN_SRC = REPO_ROOT / "VEN" / "src"

# Doc comments (///, //!) and ordinary ones. Enough for Rust source: this is a
# dependency check, not a parser, and an import never hides inside a string.
COMMENT = re.compile(r"^\s*(///|//!|//|\*|/\*)")


def code_lines(path: Path) -> "list[tuple[int, str]]":
    """Numbered lines that are actual production code."""
    raw = path.read_text(encoding="utf-8", errors="replace").splitlines(keepends=True)
    kept = strip_test_blocks(raw)
    keptset = set(id(x) for x in kept)
    out = []
    for n, line in enumerate(raw, 1):
        if id(line) not in keptset:
            continue
        if COMMENT.match(line):
            continue
        out.append((n, line.rstrip("\n")))
    return out


def rust_files(base: Path, skip_tests: bool = True) -> "list[Path]":
    files = []
    for p in base.rglob("*.rs"):
        rel = str(p.relative_to(REPO_ROOT))
        if skip_tests and is_test_only_path(rel):
            continue
        files.append(p)
    return sorted(files)


def forbid(name: str, bases, pattern: str, skip_tests: bool = True) -> "list[str]":
    rx = re.compile(pattern)
    hits = []
    for base in bases:
        if not base.exists():
            continue
        for f in rust_files(base, skip_tests):
            for n, line in code_lines(f):
                if rx.search(line):
                    hits.append(f"  {f.relative_to(REPO_ROOT)}:{n}: {line.strip()}")
    return hits


def vtn_value_leaks() -> "list[str]":
    """`serde_json::Value` on vtn.rs's *public* surface.

    Private and `pub(crate)` helpers are the documented "internal only"
    allowance -- the rule is about what crosses the port, not about the
    transport's own plumbing.
    """
    f = VEN_SRC / "vtn.rs"
    if not f.exists():
        return []
    hits = []
    for n, line in code_lines(f):
        if "serde_json::Value" not in line:
            continue
        s = line.strip()
        if s.startswith("pub ") and not s.startswith("pub(crate)"):
            hits.append(f"  VEN/src/vtn.rs:{n}: {s}")
    return hits


CHECKS = [
    ("profile in inner rings",
     lambda: forbid("profile", [VEN_SRC / "entities", VEN_SRC / "controller",
                                VEN_SRC / "routes"], r"use crate::profile")),
    ("concrete assets in milp_planner",
     lambda: forbid("assets", [VEN_SRC / "controller" / "milp_planner"],
                    r"use crate::assets::")),
    ("concrete assets in entities",
     lambda: forbid("assets", [VEN_SRC / "entities"], r"use crate::assets::")),
    ("untyped Value on the VtnPort surface", vtn_value_leaks),
]


def main() -> int:
    failed = False
    for name, run in CHECKS:
        hits = run()
        if hits:
            failed = True
            print(f"FAIL  {name}:")
            for h in hits:
                print(h)
        else:
            print(f"OK    {name}")
    if failed:
        print("\nSee the `ven-architecture` section of .claude/CLAUDE.md for what each rule protects.")
        return 1
    print("\nVEN ARCHITECTURE AUDIT PASSED: inner rings depend on nothing outward.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
