"""Does the module graph still describe the code?

docs/architecture/module_dependency_graph.md has two parts. The hand-drawn component diagram is
checked here; the generated import graph below it is checked by `gen_module_graph.py --check`.

1. Every edge in the hand-drawn diagram names a node it defines. The 2026-10-03 pass removed a
   node (`C_ENVELOPE`, when site_headroom moved to simulator/) and left an edge pointing at it.
   Mermaid does not complain -- it silently invents an empty node.
2. Every `tasks/` loop (`fn spawn*` in production code) and every top-level `VEN/src` directory is
   named in it. The diagram's first content check (2026-09-23) found six task loops missing; a
   top-level file is covered by the generated graph, a directory and a loop are not.
3. The generated import graph is current (R-35).
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_module_graph as gen  # noqa: E402
from audit_ven_architecture import VEN_SRC, code_lines, rust_files  # noqa: E402

SPAWN_FN = re.compile(r"\bfn (spawn\w*)")


def hand_drawn(doc: str) -> str:
    """The doc without its generated section."""
    start = doc.find(gen.BEGIN)
    return doc if start < 0 else doc[:start] + doc[doc.find(gen.END) + len(gen.END):]


def unresolved_edges(diagram: str) -> "list[str]":
    defined = set(re.findall(r"^\s*([A-Za-z_0-9]+)\[", diagram, re.M))
    defined |= set(re.findall(r"subgraph ([A-Za-z_0-9]+)\[", diagram))
    used = set()
    for line in diagram.splitlines():
        if "-->" not in line and "-.->" not in line:
            continue
        # strip edge labels so words inside |...| / "..." are not read as nodes
        bare = re.sub(r'\|[^|]*\|', " ", line)
        bare = re.sub(r'"[^"]*"', " ", bare)
        used.update(re.findall(r"\b([A-Za-z_][A-Za-z_0-9]*)\b", bare))
    return sorted(t for t in used - defined if t.isupper() or "_" in t)


def coverage_gaps(diagram: str, spawn_fns, directories) -> "list[str]":
    """Loops and directories the diagram does not name, directories first."""
    gaps = [d + "/" for d in sorted(directories) if d + "/" not in diagram]
    return gaps + sorted(f for f in set(spawn_fns) if f not in diagram)


def task_spawn_fns() -> "list[str]":
    return [m for f in rust_files(VEN_SRC / "tasks") for _, line in code_lines(f)
            for m in SPAWN_FN.findall(line)]


def top_level_directories() -> "list[str]":
    return [p.name for p in VEN_SRC.iterdir() if p.is_dir()]


def main() -> int:
    diagram = hand_drawn(gen.DOC.read_text(encoding="utf-8"))
    failed = False
    missing = unresolved_edges(diagram)
    if missing:
        print("FAIL  edges reference undefined nodes:", ", ".join(missing))
        failed = True
    gaps = coverage_gaps(diagram, task_spawn_fns(), top_level_directories())
    if gaps:
        print("FAIL  not named in the component diagram:", ", ".join(gaps))
        failed = True
    if not failed:
        print("OK    module graph: every edge resolves, every task loop and directory is named")
    return (1 if failed else 0) | gen.main(["--check"])


if __name__ == "__main__":
    sys.exit(main())
