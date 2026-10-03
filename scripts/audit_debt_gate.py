#!/usr/bin/env python3
"""Debt gate: a branch may not touch the files of an open Small/Trivial debt and walk away.

docs/reference/TECHNICAL_DEBTS.md says "refactor the relevant debt before adding new behaviour
if effort is Small or Trivial". Nothing enforced that, so the rows stayed open. This script
compares the files a branch changes against the `Affected files` of every open Small/Trivial
row and fails unless each hit is, in this branch, either

  * resolved  -- the row is gone from the register (it existed at the merge base), or
  * deferred  -- a commit message carries `Debt-deferred: R-NN: <reason, >= 8 chars>`.

Debt first filed on this very branch is never gated against itself.

Usage:  python scripts/audit_debt_gate.py [--base REF] [--warn]
  --base REF  compare against REF (default: origin/main, else main)
  --warn      print the findings but exit 0 (for introducing the gate)
"""
import argparse
import fnmatch
import re
import subprocess
import sys
from collections import namedtuple

REGISTER = "docs/reference/TECHNICAL_DEBTS.md"
GATED_EFFORTS = {"small", "trivial"}
MIN_REASON_CHARS = 8
# A directory token must name at least this many path components, or it matches every change
# (`VEN/src` is the whole VEN crate; the debt it sits in is about the crate, not a file).
MIN_DIR_COMPONENTS = 3

Row = namedtuple("Row", "id effort paths severity kind", defaults=(None, None))
KINDS_DOC = "docs/reference/ISSUE_KINDS.md"
Outcome = namedtuple("Outcome", "unresolved resolved deferred")


def path_tokens(cell):
    """Backticked file/dir/glob tokens of an `Affected files` cell, narrowed to real paths."""
    out = []
    for tok in re.findall(r"`([^`]+)`", cell):
        tok = tok.strip().split()[0] if tok.strip() else ""
        tok = re.sub(r":\d+(-\d+)?$", "", tok).removeprefix("./")
        if "/" not in tok and "." not in tok:
            continue
        last = tok.rstrip("/").rsplit("/", 1)[-1]
        is_dir = tok.endswith("/") or "." not in last
        if is_dir and not any(c in tok for c in "*?") and len(tok.rstrip("/").split("/")) < MIN_DIR_COMPONENTS:
            continue
        out.append(tok)
    return out


def parse_kind_rank(text):
    """Kind -> rank (0 = highest) from the numbered, backticked list in ISSUE_KINDS.md."""
    kinds = re.findall(r"^\s*\d+\.\s*`([a-z-]+)`", text, re.M)
    return {k: i for i, k in enumerate(kinds)}


def parse_rows(text):
    """Open rows of gated effort. Column positions come from the table header when it names
    them (`Affected files`, `Effort`/`Cost`, `Severity`, `Kind`); the older tables have no
    such columns for severity/kind and keep files in column 2, effort in column 3."""
    rows = []
    cols = {"files": 2, "effort": 3}
    for line in text.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if cells and cells[0] == "ID":
            low = [c.lower() for c in cells]
            cols = {"files": 2, "effort": 3}
            for key, names in (("files", ("affected files",)), ("effort", ("effort", "cost")),
                               ("severity", ("severity",)), ("kind", ("kind",))):
                for n in names:
                    if n in low:
                        cols[key] = low.index(n)
            continue
        if len(cells) < 5 or not re.fullmatch(r"R-\d+", cells[0]):
            continue
        effort = cells[cols["effort"]].split(" ")[0].lower().rstrip(",.;")
        if effort not in GATED_EFFORTS:
            continue
        pick = lambda k: cells[cols[k]] if k in cols and cols[k] < len(cells) else None  # noqa: E731
        rows.append(Row(cells[0], effort, path_tokens(cells[cols["files"]]), pick("severity"), pick("kind")))
    return rows


def sort_by_priority(rows, kind_rank):
    """Kind rank first (unknown last), then severity S1..S4 (unknown last), then id."""
    return sorted(rows, key=lambda r: (kind_rank.get(r.kind, len(kind_rank)), r.severity or "S9", r.id))


def _is_exempt(path):
    return path.startswith("docs/") or path == REGISTER or path.startswith("openspec/")


def matched_files(row, changed):
    hits = []
    for f in changed:
        if _is_exempt(f):
            continue
        for tok in row.paths:
            bare = tok.rstrip("/")
            is_glob = any(c in tok for c in "*?")
            if (
                f == tok
                or f.endswith("/" + tok)
                or (is_glob and (fnmatch.fnmatch(f, tok) or fnmatch.fnmatch(f, "*/" + tok)))
                or ("/" + bare + "/") in ("/" + f)
            ):
                hits.append(f)
                break
    return hits


def parse_deferrals(messages):
    out = {}
    for m in re.finditer(r"^Debt-deferred:\s*(R-\d+)\s*:\s*(.+?)\s*$", messages, re.M):
        if len(m.group(2)) >= MIN_REASON_CHARS:
            out[m.group(1)] = m.group(2)
    return out


def evaluate(rows, changed, head_ids, base_ids, deferrals):
    unresolved, resolved, deferred = [], [], {}
    for row in rows:
        if row.id not in base_ids or not matched_files(row, changed):
            continue
        if row.id not in head_ids:
            resolved.append(row.id)
        elif row.id in deferrals:
            deferred[row.id] = deferrals[row.id]
        else:
            unresolved.append(row)
    return Outcome(unresolved, resolved, deferred)


def _git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8", check=True).stdout


def _ids(text):
    return set(re.findall(r"^\|\s*(R-\d+)\s*\|", text, re.M))


def _default_base():
    for ref in ("origin/main", "main"):
        if subprocess.run(["git", "rev-parse", "--verify", "-q", ref], capture_output=True).returncode == 0:
            return ref
    sys.exit("audit_debt_gate: no origin/main or main to compare against; pass --base")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base")
    ap.add_argument("--warn", action="store_true")
    args = ap.parse_args()

    merge_base = _git("merge-base", args.base or _default_base(), "HEAD").strip()
    changed = [p for p in _git("diff", "--name-only", merge_base).splitlines() if p]
    with open(REGISTER, encoding="utf-8") as f:
        head_text = f.read()
    base_text = _git("show", f"{merge_base}:{REGISTER}")
    messages = _git("log", "--format=%B", f"{merge_base}..HEAD")

    out = evaluate(parse_rows(head_text) + [r for r in parse_rows(base_text) if r.id not in _ids(head_text)],
                   changed, _ids(head_text), _ids(base_text), parse_deferrals(messages))

    for rid in out.resolved:
        print(f"RESOLVED  {rid}  (removed from the register in this branch)")
    for rid, why in out.deferred.items():
        print(f"DEFERRED  {rid}  {why}  (allowed, not preferred: fix it when you are in the file)")
    try:
        with open(KINDS_DOC, encoding="utf-8") as f:
            kind_rank = parse_kind_rank(f.read())
    except OSError:
        kind_rank = {}
    for row in sort_by_priority(out.unresolved, kind_rank):
        tag = " ".join(t for t in (row.severity, row.kind) if t)
        print(f"OPEN      {row.id} [{row.effort}{', ' + tag if tag else ''}]  touched: {', '.join(matched_files(row, changed))}")
    if out.unresolved:
        print(
            "\nFix each OPEN debt in this branch and delete its row, or add a commit-message line\n"
            "  Debt-deferred: R-NN: <why this is not done now>\n"
            "(docs/reference/TECHNICAL_DEBTS.md: refactor Small/Trivial debt before adding behaviour)."
        )
        return 0 if args.warn else 1
    print("debt gate: ok" if not (out.resolved or out.deferred) else "\ndebt gate: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
