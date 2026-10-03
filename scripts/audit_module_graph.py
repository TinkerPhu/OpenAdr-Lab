"""Does every edge in the module graph name a node the graph defines?

The 2026-10-03 pass removed a node (`C_ENVELOPE`, when site_headroom moved to
simulator/) and left an edge pointing at it. Mermaid does not complain — it
silently invents an empty node — so the diagram would have rendered a blank
box and nobody would have known.
"""
import re
import sys
import pathlib

s = pathlib.Path("docs/architecture/module_dependency_graph.md").read_text(encoding="utf-8")

defined = set(re.findall(r"^\s*([A-Za-z_0-9]+)\[", s, re.M))
defined |= set(re.findall(r"subgraph ([A-Za-z_0-9]+)\[", s))

used = set()
for line in s.splitlines():
    if "-->" not in line and "-.->" not in line:
        continue
    # strip edge labels so words inside |...| / "..." are not read as nodes
    bare = re.sub(r'\|[^|]*\|', " ", line)
    bare = re.sub(r'"[^"]*"', " ", bare)
    for tok in re.findall(r"\b([A-Za-z_][A-Za-z_0-9]*)\b", bare):
        used.add(tok)

missing = sorted(t for t in used - defined if t.isupper() or "_" in t)
if missing:
    print("FAIL  edges reference undefined nodes:", ", ".join(missing))
    sys.exit(1)
print(f"OK    module graph: {len(defined)} nodes, every edge resolves")
