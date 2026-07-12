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

Usage:
    python3 experiments/kpi.py --run experiments/results/<dir> [--baseline <s1 dir>]
Writes kpis.json into the run dir and prints a table.
"""

import argparse
import csv
import json
import sqlite3
from datetime import datetime, timezone
from pathlib import Path


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


def main():
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
