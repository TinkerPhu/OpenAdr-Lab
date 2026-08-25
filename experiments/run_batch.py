#!/usr/bin/env python3
"""Run a sequence of scenarios through run_experiment.py, resumably.

A full fleet matrix (S-1..S-8 + S-10) is ~10h of wall clock. Twice now that
sequence has been driven by a hand-written throwaway shell script, and twice
something interrupted it partway: once a corrupted-stdio process that had to
be killed, once a transient network drop that killed the run mid-S-5. Both
times the recovery was to hand-edit a second script listing only the
scenarios that had not finished yet -- error-prone bookkeeping exactly when
the run is already in trouble.

This runner makes that recovery the default: `--resume` (on by default) skips
any scenario that already has a completed result directory, so re-invoking the
same command after any interruption picks up precisely where it left off.

Each scenario runs as its own `run_experiment.py` subprocess. That isolation
is deliberate -- it is what made resuming possible at all, since a scenario
that dies leaves every previously-completed scenario's snapshot untouched.

    python3 experiments/run_batch.py \\
        --scenarios s1_flat,s2_price_spike,s3_capacity_limit \\
        -- --fleet-map experiments/fleet_map.json --pg-host Node1 \\
           --vtn-url http://192.168.1.103:8200

Everything after `--` is passed through unchanged to every run_experiment.py
invocation. Scenarios declaring `is_baseline: true` additionally get
`--no-paired-baseline` (a baseline scenario pairing against its own baseline
is just wasted wall clock).
"""

import argparse
import subprocess
import sys
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
SCENARIO_DIR = REPO_ROOT / "experiments" / "scenarios"


def result_dir_scenario(dir_name):
    """The scenario a result directory belongs to, or None if it isn't one.

    run_experiment.py names these `<YYYYmmdd>-<HHMM>-<scenario name>`, so the
    name is everything after the second dash -- but only once the first two
    fields actually look like that timestamp, or any dashed directory sitting
    in the results root would be read as a scenario result. Matching the name
    exactly (rather than by suffix) keeps a paired baseline's
    `...-<name>-baseline` directory from counting as the scenario itself -- it
    holds the no-events window, not the run being resumed."""
    parts = dir_name.split("-", 2)
    if len(parts) != 3:
        return None
    date, hhmm, name = parts
    if not (len(date) == 8 and date.isdigit() and len(hhmm) == 4 and hhmm.isdigit()):
        return None
    return name


def scenario_completed(results_root, name):
    """True when `name` already has a finished result directory.

    `run.json` is the completion marker because run_experiment.py writes it
    last, after the snapshot -- the interrupted S-5 left a directory full of
    poller output but no run.json, which is exactly the state that must not
    count as done."""
    root = Path(results_root)
    if not root.is_dir():
        return False
    for child in root.iterdir():
        if not child.is_dir():
            continue
        if result_dir_scenario(child.name) == name and (child / "run.json").exists():
            return True
    return False


def scenario_path(name):
    return SCENARIO_DIR / f"{name}.yaml"


def is_baseline_scenario(path):
    """Whether the scenario declares itself the fleet's baseline.

    Declared in the scenario YAML (`is_baseline: true`) rather than matched on
    a hard-coded name here, following `incompatible_ev_session_modes`: the
    scenario is where its own properties belong."""
    scenario = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
    return bool(scenario.get("is_baseline", False))


def build_command(name, passthrough, baseline=False, python=sys.executable):
    cmd = [python, "experiments/run_experiment.py", "--scenario", str(scenario_path(name))]
    if baseline:
        cmd.append("--no-paired-baseline")
    return cmd + list(passthrough)


def main():
    sys.stdout.reconfigure(line_buffering=True)

    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument(
        "--scenarios", required=True,
        help="comma-separated scenario names (file stems under experiments/scenarios/), "
             "run in the order given",
    )
    p.add_argument("--out", default=str(REPO_ROOT / "experiments" / "results"),
                   help="results root, matching run_experiment.py's --out")
    p.add_argument(
        "--resume", action=argparse.BooleanOptionalAction, default=True,
        help="skip scenarios that already have a completed result directory (one "
             "containing run.json). On by default so re-running the same command "
             "after an interruption continues rather than redoing finished work; "
             "--no-resume forces every scenario to run again.",
    )
    p.add_argument(
        "--keep-going", action="store_true",
        help="carry on with the remaining scenarios after one fails, instead of "
             "stopping. Off by default: a failure is usually environmental (a dead "
             "host, an expired lock) and the rest would fail the same way.",
    )
    p.add_argument("passthrough", nargs="*",
                   help="args after `--`, forwarded verbatim to every run_experiment.py call")
    args = p.parse_args()

    names = [s.strip() for s in args.scenarios.split(",") if s.strip()]
    missing = [n for n in names if not scenario_path(n).exists()]
    if missing:
        p.error(f"no scenario file for: {', '.join(missing)} (looked in {SCENARIO_DIR})")

    failed = []
    for i, name in enumerate(names, 1):
        if args.resume and scenario_completed(args.out, name):
            print(f"=== [{i}/{len(names)}] {name}: already complete, skipping ===")
            continue
        cmd = build_command(name, args.passthrough, baseline=is_baseline_scenario(scenario_path(name)))
        print(f"=== [{i}/{len(names)}] {name} ===")
        rc = subprocess.run(cmd, cwd=str(REPO_ROOT)).returncode
        if rc != 0:
            failed.append(name)
            print(f"=== {name} FAILED (rc={rc}) ===")
            if not args.keep_going:
                print(f"=== stopping; re-run the same command to resume from {name} ===")
                break

    if failed:
        print(f"=== BATCH INCOMPLETE: {len(failed)} failed ({', '.join(failed)}) ===")
        return 1
    print("=== ALL SCENARIOS DONE ===")
    return 0


def _self_check_result_dir_scenario():
    assert result_dir_scenario("20260824-0518-s10_overexport") == "s10_overexport"
    assert result_dir_scenario("20260823-2013-s1_flat") == "s1_flat"
    # A paired baseline is a different directory than the scenario itself.
    assert result_dir_scenario("20260824-0518-s10_overexport-baseline") == "s10_overexport-baseline"
    assert result_dir_scenario("not-a-run") is None
    print("_self_check_result_dir_scenario OK")


def _self_check_scenario_completed(tmp):
    root = Path(tmp) / "results"
    (root / "20260823-2013-s1_flat").mkdir(parents=True)
    (root / "20260823-2013-s1_flat" / "run.json").write_text("{}", encoding="utf-8")
    assert scenario_completed(root, "s1_flat")

    # The interrupted-S-5 shape: poller output written, but no run.json
    # because the run never reached its snapshot. Must not count as done.
    partial = root / "20260824-0106-s5_dispatch"
    partial.mkdir(parents=True)
    (partial / "ven-1-plan-diagnostics.jsonl").write_text("{}\n", encoding="utf-8")
    assert not scenario_completed(root, "s5_dispatch")

    # A completed baseline window alone must not mark its scenario done --
    # that was exactly the S-5 state, baseline fine and scenario lost.
    bdir = root / "20260824-0106-s5_dispatch-baseline"
    bdir.mkdir(parents=True)
    (bdir / "run.json").write_text("{}", encoding="utf-8")
    assert not scenario_completed(root, "s5_dispatch")

    assert not scenario_completed(root, "s7_stress")
    assert not scenario_completed(root / "does-not-exist", "s1_flat")
    print("_self_check_scenario_completed OK")


def _self_check_build_command():
    passthrough = ["--fleet-map", "experiments/fleet_map.json", "--pg-host", "Node1"]

    cmd = build_command("s2_price_spike", passthrough, baseline=False, python="py")
    assert cmd[:2] == ["py", "experiments/run_experiment.py"], cmd
    assert "--no-paired-baseline" not in cmd, cmd
    # Passthrough must survive verbatim, and stay after the scenario args.
    assert cmd[-len(passthrough):] == passthrough, cmd

    baseline_cmd = build_command("s1_flat", passthrough, baseline=True, python="py")
    assert "--no-paired-baseline" in baseline_cmd, baseline_cmd
    print("_self_check_build_command OK")


def _self_check_is_baseline_scenario():
    assert is_baseline_scenario(scenario_path("s1_flat")), \
        "s1_flat must declare is_baseline: true -- it IS the fleet baseline"
    assert not is_baseline_scenario(scenario_path("s2_price_spike"))
    print("_self_check_is_baseline_scenario OK")


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--self-check":
        import tempfile
        _self_check_result_dir_scenario()
        with tempfile.TemporaryDirectory() as tmp:
            _self_check_scenario_completed(tmp)
        _self_check_build_command()
        _self_check_is_baseline_scenario()
    else:
        sys.exit(main())
