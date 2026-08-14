#!/usr/bin/env python3
"""WP3.8 (A-3) — compute per-VEN KPIs from a run_experiment.py snapshot.

Reads each VEN's history.sqlite (grid_samples over the run window) and the
recorder CSVs. KPIs per VEN:
  - energy_import_kwh / energy_export_kwh over the window
  - cost_eur (import*tariff - export*tariff, per 1-min sample)
  - peak_import_kw, load_factor (mean/peak import)
  - energy_shifted_kwh vs a baseline run (same-window import delta). Defaults to the
    GB-28 paired baseline run_experiment.py writes alongside the run (--paired-baseline,
    on by default -- {run}-baseline/, minutes away in wall-clock time), overridable with
    an explicit --baseline. A cross-run baseline captured hours away (e.g. a separately
    scheduled S-1) still confounds this VEN with real time-of-day drift (solar/base-load
    cycles) -- GB-28 only shrinks that gap to the paired window's own duration, it
    doesn't eliminate it; a true fix needs injectable sim time, out of scope here.
  - compliance_latency_s: first grid-sample timestamp after each non-price
    event where import dropped >= 20% below the pre-event minute (rough
    signal->response measure; None when no constraining event in the run)
  - report_lag_s stats from recorder-reports_received.csv (SG-3 timeliness)
  - event_impact_kwh (WP5.4): Σ(baseline − actual) over the run window, from archived
    BASELINE vs. USAGE reports — absent when no BASELINE reports were archived

Usage:
    python3 experiments/kpi.py --run experiments/results/<dir> [--baseline <s1 dir>]
Writes kpis.json into the run dir and prints a table.
"""

import argparse
import csv
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

# The recorder dumps its whole lab_recorder table (unfiltered by run window),
# which over a long-lived deployment accumulates payload_json blobs past
# Python's 128 KiB default csv field limit. sys.maxsize overflows the C long
# csv.field_size_limit uses internally on some platforms (Windows); 10 MiB is
# comfortably above any real report payload.
csv.field_size_limit(10 * 1024 * 1024)


def window(run_meta):
    t0 = datetime.strptime(run_meta["started_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    return int(t0.timestamp()), int(t0.timestamp()) + run_meta["duration_minutes"] * 60


def grid_rows(db_path, t_from, t_to):
    con = sqlite3.connect(db_path)
    try:
        return con.execute(
            "SELECT ts, import_kw, export_kw, import_tariff_eur_kwh, export_tariff_eur_kwh"
            " FROM grid_samples WHERE ts >= ? AND ts < ? ORDER BY ts",
            (t_from, t_to),
        ).fetchall()
    finally:
        con.close()


def ven_kpis(db_path, t_from, t_to):
    rows = grid_rows(db_path, t_from, t_to)
    if not rows:
        return None
    dt_h = 1.0 / 60.0  # 1-minute samples (history sampler downsampling)
    imp_kwh = sum(r[1] for r in rows) * dt_h
    exp_kwh = sum(r[2] for r in rows) * dt_h
    cost = sum(r[1] * (r[3] or 0.0) - r[2] * (r[4] or 0.0) for r in rows) * dt_h
    peak = max(r[1] for r in rows)
    mean = imp_kwh / (len(rows) * dt_h)
    return {
        "samples": len(rows),
        "energy_import_kwh": round(imp_kwh, 4),
        "energy_export_kwh": round(exp_kwh, 4),
        "cost_eur": round(cost, 4),
        "peak_import_kw": round(peak, 3),
        "load_factor": round(mean / peak, 3) if peak > 0 else None,
    }


def _parse_iso8601_duration_hours(s):
    """Minimal PT#H#M#S parser, mirroring VEN's parse_pt_duration_s. Unknown
    shapes parse as 0."""
    if not s or not s.startswith("PT"):
        return 0.0
    total_s = 0
    num = ""
    for c in s[2:]:
        if c.isdigit():
            num += c
        else:
            v = int(num) if num else 0
            num = ""
            if c == "H":
                total_s += v * 3600
            elif c == "M":
                total_s += v * 60
            elif c == "S":
                total_s += v
    return total_s / 3600.0


def _report_energy_kwh(csv_path, t_from, t_to, ven_name, report_type):
    """Sum a VEN's `report_type` report intervals (values in W) into kWh,
    restricted to rows whose `received_at` falls in [t_from, t_to)."""
    if not csv_path.exists():
        return None
    total_kwh = 0.0
    found = False
    with open(csv_path, encoding="utf-8") as f:
        for row in csv.DictReader(f):
            received = row.get("received_at", "")
            try:
                ts = datetime.fromisoformat(received.replace("Z", "+00:00")).timestamp()
            except ValueError:
                continue
            if not (t_from <= ts < t_to):
                continue
            if row.get("ven_name") != ven_name:
                continue
            try:
                payload = json.loads(row["payload_json"])
            except (KeyError, ValueError):
                continue
            for resource in payload.get("resources", []):
                for interval in resource.get("intervals", []):
                    period = interval.get("intervalPeriod") or {}
                    duration_h = _parse_iso8601_duration_hours(period.get("duration", ""))
                    for p in interval.get("payloads", []):
                        if p.get("type") != report_type:
                            continue
                        values = p.get("values") or []
                        if not values or not isinstance(values[0], (int, float)):
                            continue
                        found = True
                        total_kwh += (values[0] / 1000.0) * duration_h
    return total_kwh if found else None


def event_impact_kwh(csv_path, t_from, t_to, ven_name):
    """WP5.4: Σ(baseline − actual) over the run window, from archived BASELINE
    and USAGE reports. `None` when no BASELINE reports were archived for this
    VEN in the window (nothing to compare against)."""
    baseline_kwh = _report_energy_kwh(csv_path, t_from, t_to, ven_name, "BASELINE")
    if baseline_kwh is None:
        return None
    usage_kwh = _report_energy_kwh(csv_path, t_from, t_to, ven_name, "USAGE") or 0.0
    return round(baseline_kwh - usage_kwh, 4)


def report_lag_stats(csv_path, t_from, t_to):
    """Only reports the recorder received during the run window count —
    the archive holds every report ever seen, including ancient ones."""
    if not csv_path.exists():
        return None
    lags = []
    with open(csv_path, encoding="utf-8") as f:
        for row in csv.DictReader(f):
            received = row.get("received_at", "")
            try:
                ts = datetime.fromisoformat(received.replace("Z", "+00:00")).timestamp()
            except ValueError:
                continue
            if not (t_from <= ts < t_to + 60):
                continue
            v = row.get("report_lag_s")
            if v not in (None, "", r"\N"):
                try:
                    lags.append(float(v))
                except ValueError:
                    pass
    if not lags:
        return None
    lags.sort()
    return {
        "count": len(lags),
        "median_s": round(lags[len(lags) // 2], 1),
        "max_s": round(max(lags), 1),
        "min_s": round(min(lags), 1),
    }


def plan_history_summary(run_dir, ven):
    """GB-25/GB-31: summarize a run_experiment.py --fleet-map fetch of
    GET /history/plans (see fetch_plan_history in run_experiment.py) —
    {ven}-plan-history.json, an array of PlanHistorySample rows
    (VEN/src/entities/history.rs). This is the primary source for
    `k["plan_diagnostics"]` in main() below; plan_diagnostics_summary()
    (the live /plan poller) is the fallback for runs without --fleet-map.
    Returns None when no plan-history file exists for this VEN."""
    path = run_dir / f"{ven}-plan-history.json"
    if not path.exists():
        return None
    rows = json.loads(path.read_text(encoding="utf-8"))
    if not rows:
        return None

    solver_ms_vals = [r["solver_ms"] for r in rows if r.get("solver_ms") is not None]
    solver_ms = None
    if solver_ms_vals:
        s = sorted(solver_ms_vals)
        solver_ms = {
            "median": s[len(s) // 2],
            "max": max(s),
            "mean": round(sum(s) / len(s), 1),
        }

    mip_gap_values = sorted({r["mip_gap_target"] for r in rows if r.get("mip_gap_target") is not None})
    mip_gap_target_sanity = {
        "distinct_values": mip_gap_values,
        "constant": len(mip_gap_values) <= 1,
    }
    if not mip_gap_target_sanity["constant"]:
        print(f"WARN: {ven} mip_gap_target is not constant across plan-history rows: {mip_gap_values}")

    solve_status_counts = {}
    for r in rows:
        status = r.get("solve_status")
        if status:
            solve_status_counts[status] = solve_status_counts.get(status, 0) + 1

    warning_kind_counts = {}
    warning_kinds_total = 0
    for r in rows:
        for k in r.get("warning_kinds") or []:
            warning_kind_counts[k] = warning_kind_counts.get(k, 0) + 1
            warning_kinds_total += 1

    warning_count_total = sum(r.get("warning_count") or 0 for r in rows)
    if warning_count_total != warning_kinds_total:
        print(
            f"WARN: {ven} plan-history warning_count total ({warning_count_total}) != "
            f"sum(len(warning_kinds)) ({warning_kinds_total}) — possible data/serialization inconsistency"
        )

    cost_fields = ("c_energy_eur", "c_grid_eur", "c_wear_eur", "c_violations_eur", "c_peak_penalty_eur")
    cost_stats = {}
    for field in cost_fields:
        vals = [r[field] for r in rows if r.get(field) is not None]
        if vals:
            cost_stats[field] = {"mean": round(sum(vals) / len(vals), 4), "max": round(max(vals), 4)}

    return {
        "cycles": len(rows),
        "solver_ms": solver_ms,
        "mip_gap_target_sanity": mip_gap_target_sanity,
        "solve_status_counts": solve_status_counts,
        "warning_kind_counts": warning_kind_counts,
        "warning_count_total": warning_count_total,
        "cost_stats": cost_stats,
    }


def forecast_accuracy_summary(run_dir, ven):
    """GB-25: summarize a run_experiment.py --fleet-map fetch of
    GET /history/forecast-accuracy (see fetch_forecast_accuracy in
    run_experiment.py) — {ven}-forecast-accuracy.json, an array of
    ForecastAccuracySample rows (VEN/src/entities/history.rs), grouped by
    (asset_id, lead_kind). Rows with actual_kw still null (unreconciled near
    the end of a run window) are expected, not an error, and are silently
    excluded. Returns None when the file is missing or has zero reconciled
    samples — same "nothing to compare yet" convention as event_impact_kwh."""
    path = run_dir / f"{ven}-forecast-accuracy.json"
    if not path.exists():
        return None
    rows = json.loads(path.read_text(encoding="utf-8"))
    groups = {}
    for r in rows:
        if r.get("actual_kw") is None:
            continue
        key = (r.get("asset_id"), r.get("lead_kind"))
        groups.setdefault(key, []).append(r)

    if not groups:
        return None

    out = {}
    for (asset_id, lead_kind), grp in groups.items():
        errors = [r["predicted_kw"] - r["actual_kw"] for r in grp]
        out[f"{asset_id}:{lead_kind}"] = {
            "n": len(grp),
            "mae_kw": round(sum(abs(e) for e in errors) / len(errors), 4),
            "bias_kw": round(sum(errors) / len(errors), 4),
        }
    return out


def plan_diagnostics_summary(run_dir, ven):
    """Fallback/live-poller view: summarize a run_experiment.py --fleet-map
    poll of GET /plan (see poll_plan_diagnostics): solve_status distribution,
    warning counts by severity, and c_violations_eur stats — this is the "is
    this VEN's plan quality expected or not" signal, distinct from the
    grid-power KPIs above. Used by main() only when plan_history_summary()
    (the GB-25 GET /history/plans-backed primary source) returns None, e.g.
    for a run without --fleet-map. Returns None when no diagnostics file
    exists for this VEN."""
    path = run_dir / f"{ven}-plan-diagnostics.jsonl"
    if not path.exists():
        return None
    solve_status_counts = {}
    warning_severity_counts = {}
    violations_eur = []
    samples = 0
    errors = 0
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            rec = json.loads(line)
            if "error" in rec:
                errors += 1
                continue
            samples += 1
            status = rec.get("solve_status")
            if status:
                solve_status_counts[status] = solve_status_counts.get(status, 0) + 1
            for w in rec.get("warnings") or []:
                sev = w.get("severity")
                if sev:
                    warning_severity_counts[sev] = warning_severity_counts.get(sev, 0) + 1
            cb = rec.get("cost_breakdown") or {}
            v = cb.get("c_violations_eur")
            if isinstance(v, (int, float)):
                violations_eur.append(v)
    if samples == 0 and errors == 0:
        return None
    out = {
        "samples": samples,
        "poll_errors": errors,
        "solve_status_counts": solve_status_counts,
        "warning_severity_counts": warning_severity_counts,
    }
    if violations_eur:
        out["c_violations_eur_mean"] = round(sum(violations_eur) / len(violations_eur), 4)
        out["c_violations_eur_max"] = round(max(violations_eur), 4)
    return out


def _self_check():
    """No pytest harness for experiments/ scripts today — self-check in the
    style of scripts/personas.py's `if __name__ == "__main__"` block. Run via
    `python3 experiments/kpi.py --self-check`."""
    import tempfile

    t0 = datetime(2026, 1, 1, 10, 0, 0, tzinfo=timezone.utc).timestamp()
    t_from, t_to = int(t0), int(t0) + 1800  # 30-minute window

    def row(report_type, received_offset_s, payload_type, value_w, duration):
        received = datetime.fromtimestamp(
            t0 + received_offset_s, tz=timezone.utc
        ).isoformat()
        payload = {
            "resources": [
                {
                    "intervals": [
                        {
                            "intervalPeriod": {"duration": duration},
                            "payloads": [{"type": payload_type, "values": [value_w]}],
                        }
                    ]
                }
            ]
        }
        return {
            "report_id": "r1",
            "modification_date_time": "2026-01-01T10:00:00Z",
            "received_at": received,
            "ven_name": "ven-1",
            "report_type": report_type,
            "payload_json": json.dumps(payload),
            "report_lag_s": "",
        }

    with tempfile.TemporaryDirectory() as tmp:
        csv_path = Path(tmp) / "recorder-reports_received.csv"

        # Scenario 1: BASELINE (2 kW, PT15M -> 0.5 kWh) above USAGE (1 kW, PT15M
        # -> 0.25 kWh) -> positive event_impact_kwh = 0.25.
        rows = [
            row("baseline-report", 60, "BASELINE", 2000.0, "PT15M"),
            row("usage-report", 90, "USAGE", 1000.0, "PT15M"),
        ]
        with open(csv_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            writer.writeheader()
            writer.writerows(rows)
        impact = event_impact_kwh(csv_path, t_from, t_to, "ven-1")
        assert impact == 0.25, f"expected 0.25, got {impact}"

        # Scenario 2: no BASELINE rows archived -> None, not a computed value.
        rows2 = [row("usage-report", 90, "USAGE", 1000.0, "PT15M")]
        with open(csv_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(rows2[0].keys()))
            writer.writeheader()
            writer.writerows(rows2)
        impact2 = event_impact_kwh(csv_path, t_from, t_to, "ven-1")
        assert impact2 is None, f"expected None (no BASELINE archived), got {impact2}"

    print("kpi.py self-check OK: event_impact_kwh")

    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp)

        # plan_history_summary: 3 rows, one with a null solver_ms (skipped
        # gracefully), non-constant mip_gap_target (flagged, not an error),
        # two warning_kinds rows whose Σlen matches warning_count (no WARN).
        plan_rows = [
            {
                "plan_id": "p1", "created_at": "2026-01-01T10:00:00Z", "trigger": "periodic",
                "solver_ms": 120, "solve_status": "OPTIMAL", "objective_eur": 1.0,
                "friction_eur": 0.1, "mip_gap_target": 0.02, "warning_count": 1,
                "warning_kinds": ["BUDGET_SHORTFALL"],
                "c_energy_eur": 0.5, "c_grid_eur": 0.2, "c_wear_eur": 0.01,
                "c_violations_eur": None, "c_peak_penalty_eur": None,
            },
            {
                "plan_id": "p2", "created_at": "2026-01-01T10:05:00Z", "trigger": "periodic",
                "solver_ms": None, "solve_status": "INFEASIBLE", "objective_eur": 0.0,
                "friction_eur": 0.0, "mip_gap_target": 0.05, "warning_count": 0,
                "warning_kinds": [],
                "c_energy_eur": None, "c_grid_eur": None, "c_wear_eur": None,
                "c_violations_eur": None, "c_peak_penalty_eur": None,
            },
            {
                "plan_id": "p3", "created_at": "2026-01-01T10:10:00Z", "trigger": "periodic",
                "solver_ms": 80, "solve_status": "OPTIMAL", "objective_eur": 0.8,
                "friction_eur": 0.05, "mip_gap_target": 0.02, "warning_count": 1,
                "warning_kinds": ["CAPACITY_VIOLATION"],
                "c_energy_eur": 0.4, "c_grid_eur": 0.1, "c_wear_eur": 0.0,
                "c_violations_eur": 0.3, "c_peak_penalty_eur": None,
            },
        ]
        (run_dir / "ven-1-plan-history.json").write_text(json.dumps(plan_rows), encoding="utf-8")
        summary = plan_history_summary(run_dir, "ven-1")
        assert summary["cycles"] == 3, summary
        assert summary["solver_ms"] == {"median": 120, "max": 120, "mean": 100.0}, summary["solver_ms"]
        assert summary["mip_gap_target_sanity"] == {
            "distinct_values": [0.02, 0.05], "constant": False,
        }, summary["mip_gap_target_sanity"]
        assert summary["solve_status_counts"] == {"OPTIMAL": 2, "INFEASIBLE": 1}
        assert summary["warning_kind_counts"] == {"BUDGET_SHORTFALL": 1, "CAPACITY_VIOLATION": 1}
        assert summary["warning_count_total"] == 2
        assert summary["cost_stats"]["c_violations_eur"] == {"mean": 0.3, "max": 0.3}
        assert "c_peak_penalty_eur" not in summary["cost_stats"]  # all-None column excluded

        assert plan_history_summary(run_dir, "ven-missing") is None

        # forecast_accuracy_summary: 2 reconciled "ev:near" rows + 1
        # unreconciled row (actual_kw null -> excluded silently).
        fa_rows = [
            {
                "asset_id": "ev", "lead_kind": "near", "target_ts": "2026-01-01T10:05:00Z",
                "predicted_kw": 3.0, "predicted_at": "2026-01-01T10:00:00Z",
                "actual_kw": 2.5, "actual_at": "2026-01-01T10:06:00Z",
            },
            {
                "asset_id": "ev", "lead_kind": "near", "target_ts": "2026-01-01T10:10:00Z",
                "predicted_kw": 2.0, "predicted_at": "2026-01-01T10:05:00Z",
                "actual_kw": 2.4, "actual_at": "2026-01-01T10:11:00Z",
            },
            {
                "asset_id": "ev", "lead_kind": "near", "target_ts": "2026-01-01T10:15:00Z",
                "predicted_kw": 2.2, "predicted_at": "2026-01-01T10:10:00Z",
                "actual_kw": None, "actual_at": None,
            },
        ]
        (run_dir / "ven-1-forecast-accuracy.json").write_text(json.dumps(fa_rows), encoding="utf-8")
        fa_summary = forecast_accuracy_summary(run_dir, "ven-1")
        assert fa_summary["ev:near"]["n"] == 2, fa_summary
        assert fa_summary["ev:near"]["mae_kw"] == 0.45, fa_summary  # mean(|0.5|, |-0.4|)
        assert fa_summary["ev:near"]["bias_kw"] == 0.05, fa_summary  # mean(0.5, -0.4)

        assert forecast_accuracy_summary(run_dir, "ven-missing") is None

    print("kpi.py self-check OK: plan_history_summary, forecast_accuracy_summary")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--self-check":
        _self_check()
        return

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run", required=True)
    p.add_argument(
        "--baseline",
        help="a run dir for energy_shifted_kwh. Defaults to the GB-28 paired baseline "
             "run_experiment.py writes alongside --run (<run>-baseline/), if present; "
             "pass explicitly to compare against something else instead (e.g. an s1_flat "
             "run, for scenarios run with --no-paired-baseline)",
    )
    p.add_argument(
        "--manifest",
        help="WP4.5: fleet manifest.json with persona tags — adds a per-persona KPI block",
    )
    args = p.parse_args()

    run_dir = Path(args.run)
    meta = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
    t_from, t_to = window(meta)

    baseline_dir_arg = args.baseline
    if not baseline_dir_arg:
        auto_baseline = Path(str(run_dir) + "-baseline")
        if (auto_baseline / "run.json").exists():
            baseline_dir_arg = str(auto_baseline)

    baseline = {}
    if baseline_dir_arg:
        bdir = Path(baseline_dir_arg)
        bmeta = json.loads((bdir / "run.json").read_text(encoding="utf-8"))
        bfrom, bto = window(bmeta)
        for ven in bmeta["vens"]:
            db = bdir / f"{ven}-history.sqlite"
            if db.exists():
                k = ven_kpis(db, bfrom, bto)
                if k:
                    baseline[ven] = k["energy_import_kwh"]

    out = {"scenario": meta["scenario"], "vens": {}}
    for ven in meta["vens"]:
        db = run_dir / f"{ven}-history.sqlite"
        if not db.exists():
            continue
        k = ven_kpis(db, t_from, t_to)
        if k is None:
            continue
        if ven in baseline:
            k["energy_shifted_kwh"] = round(baseline[ven] - k["energy_import_kwh"], 4)
        impact = event_impact_kwh(
            run_dir / "recorder-reports_received.csv", t_from, t_to, ven
        )
        if impact is not None:
            k["event_impact_kwh"] = impact
        diag = plan_history_summary(run_dir, ven)
        if diag is None:
            diag = plan_diagnostics_summary(run_dir, ven)
        if diag is not None:
            k["plan_diagnostics"] = diag
        fa = forecast_accuracy_summary(run_dir, ven)
        if fa is not None:
            k["forecast_accuracy"] = fa
        out["vens"][ven] = k

    out["report_timeliness"] = report_lag_stats(
        run_dir / "recorder-reports_received.csv", t_from, t_to
    )

    # WP4.5: persona segmentation — mean KPIs per persona group so the
    # experiment report shows the behavioural spread across the fleet.
    if args.manifest:
        manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
        persona_of = {v["ven_name"]: v.get("persona") for v in manifest["vens"]}
        groups = {}
        for ven, k in out["vens"].items():
            persona = persona_of.get(ven)
            if persona:
                groups.setdefault(persona, []).append(k)
        metrics = ("energy_import_kwh", "cost_eur", "peak_import_kw", "energy_shifted_kwh")
        out["personas"] = {
            persona: {
                "vens": len(ks),
                **{
                    f"mean_{m}": round(sum(k[m] for k in ks) / len(ks), 4)
                    for m in metrics
                    if all(m in k for k in ks)
                },
            }
            for persona, ks in sorted(groups.items())
        }

    (run_dir / "kpis.json").write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
