"""Does every sentence in tests/features/ still resolve to exactly one step?

`tests/entrypoint.sh` runs `behave --dry-run` and **aborts the whole E2E suite**
before starting if any step is undefined or ambiguous. So one sentence/pattern
mismatch costs a full remote round trip — docker build included, ~20 minutes —
to discover something this finds in about a second.

That is not hypothetical. 2026-10-04: a step declared `{pct:f}` while its feature
wrote `at most 2 percent`. In the `parse` library `f` is *fixed-point* and requires
a decimal point, so `2` does not match and the step reported as undefined even
though it was defined four lines from the sentence calling it. It reached main and
blocked every branch's E2E until someone's run died on it.

    parse.parse('… at most {pct:f} percent …', '… at most 2 percent …')  -> None
    parse.parse('… at most {pct:g} percent …', '… at most 2 percent …')  -> 2.0

Hence the house rule this check enforces in practice: prefer `{x:g}` over `{x:f}`
for a numeric step argument. `g` accepts `2` and `2.0` and still yields a float,
so a feature author cannot phrase the number wrongly; `f` makes the decimal point
load-bearing in prose, which is a trap with no upside.

**This is a pre-filter, not the authority.** The container pins `behave==1.2.6`
(tests/requirements.txt) and a developer machine may have something newer, so a
clean result here does not *prove* the container's dry-run is clean — the suite's
own dry-run remains the gate. It catches the mismatches that matter in practice,
locally, for free.

Exit codes follow `check_base_image_digests.py`: 0 clean, 1 findings, 2 could not
run — which deliberately does not read as clean.
"""
import os
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
TESTS = REPO / "tests"

if not (TESTS / "features").is_dir():
    print(f"SKIP  no tests/features under {REPO}")
    sys.exit(2)

try:
    import behave  # noqa: F401
except ImportError:
    print("SKIP  behave is not installed here -- `pip install -r tests/requirements.txt`")
    print("      (the suite's own dry-run still gates; this is the local pre-filter)")
    sys.exit(2)

env = dict(os.environ)
# Without this the run dies on a non-ASCII character in a step name with
# UnicodeEncodeError on Windows consoles (cp1252), which looks like a failure
# of the suite rather than of the terminal.
env["PYTHONIOENCODING"] = "utf-8"
env["PYTHONUTF8"] = "1"

proc = subprocess.run(
    [sys.executable, "-m", "behave", "--dry-run", "--format", "null", "--no-summary"],
    cwd=TESTS,
    env=env,
    capture_output=True,
    text=True,
    encoding="utf-8",
    errors="replace",
)
out = (proc.stdout or "") + (proc.stderr or "")

flagged = any("undefined" in ln.lower() or "ambiguous" in ln.lower()
              for ln in out.splitlines())
if proc.returncode != 0 or flagged:
    print("FAIL  some feature sentences do not resolve to a step:")
    # behave reports each unresolved sentence as the decorator it would need,
    # e.g. `@then(u'the capability ... is at most 2 percent of the baseline')`.
    # That names the sentence verbatim and is the only genuinely useful line —
    # the surrounding prose ("you can implement step definitions ...") is not.
    sentences = [ln.strip() for ln in out.splitlines()
                 if ln.lstrip().startswith(("@given(", "@when(", "@then(", "@step("))]
    for ln in sentences:
        print(f"      {ln}")
    if not sentences:
        # Something else went wrong (an import error in a step module, say).
        for ln in out.splitlines()[-25:]:
            print(f"      {ln.rstrip()}")
    print("\n      A step CAN be defined and still report undefined: check the")
    print("      argument types in its pattern (`{x:f}` needs a decimal point,")
    print("      so a feature writing `2` does not match it -- use `{x:g}`).")
    sys.exit(1)

features = len(list((TESTS / "features").rglob("*.feature")))
steps = len(list((TESTS / "features" / "steps").glob("*.py")))
print(f"OK    step definitions: {features} feature files against {steps} step modules, "
      "every sentence resolves")
