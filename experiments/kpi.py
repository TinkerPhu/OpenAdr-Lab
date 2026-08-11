#!/usr/bin/env python3
"""WP3.8 (A-3) — compute per-VEN KPIs from a run_experiment.py snapshot.

Reads each VEN's history.sqlite (grid_samples over the run window) and the
recorder CSVs. KPIs per VEN:
  - energy_import_kwh / energy_export_kwh over the window
  - cost_eur (import*tariff - export*tariff, per 1-min sample)
  - peak_import_kw, load_factor (mean/peak import)
  - energy_shifted_kwh vs a baseline run (same-window import delta, needs --baseline)
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


def plan_diagnostics_summary(run_dir, ven):
    """Summarize a run_experiment.py --fleet-map poll of GET /plan (see
    poll_plan_diagnostics): solve_status distribution, warning counts by
    severity, and c_violations_eur stats — this is the "is this VEN's plan
    quality expected or not" signal, distinct from the grid-power KPIs above.
    Returns None when no diagnostics file exists for this VEN (e.g. a run
    without --fleet-map)."""
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


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--self-check":
        _self_check()
        return

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run", required=True)
    p.add_argument("--baseline", help="an s1_flat run dir for energy_shifted_kwh")
    p.add_argument(
        "--manifest",
        help="WP4.5: fleet manifest.json with persona tags — adds a per-persona KPI block",
    )
    args = p.parse_args()

    run_dir = Path(args.run)
    meta = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
    t_from, t_to = window(meta)

    baseline = {}
    if args.baseline:
        bdir = Path(args.baseline)
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
        diag = plan_diagnostics_summary(run_dir, ven)
        if diag is not None:
            k["plan_diagnostics"] = diag
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
