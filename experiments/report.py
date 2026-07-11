#!/usr/bin/env python3
"""WP3.8 (A-3) — render a markdown comparison report from one or more
kpi.py outputs (run dirs containing kpis.json).

Charts are optional: import-power PNGs are rendered per run when matplotlib
is importable, silently skipped otherwise (it isn't installed on the Pi4
host by default and the tables carry the KPIs either way).

Usage:
    python3 experiments/report.py --runs <dir1> <dir2> ... --out experiments/results/report.md
"""

import argparse
import json
import sqlite3
from pathlib import Path


def load_run(run_dir):
    run_dir = Path(run_dir)
    kpis = json.loads((run_dir / "kpis.json").read_text(encoding="utf-8"))
    meta = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
    return run_dir, meta, kpis


def try_chart(run_dir, meta, out_dir):
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError:
        return None
    from datetime import datetime, timezone

    t0 = datetime.strptime(meta["started_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    t_from = int(t0.timestamp())
    t_to = t_from + meta["duration_minutes"] * 60
    fig, ax = plt.subplots(figsize=(8, 3))
    for ven in meta["vens"]:
        db = run_dir / f"{ven}-history.sqlite"
        if not db.exists():
            continue
        con = sqlite3.connect(db)
        rows = con.execute(
            "SELECT ts, import_kw FROM grid_samples WHERE ts >= ? AND ts < ? ORDER BY ts",
            (t_from, t_to),
        ).fetchall()
        con.close()
        if rows:
            ax.plot([(r[0] - t_from) / 60.0 for r in rows], [r[1] for r in rows], label=ven)
    ax.set_xlabel("minutes into scenario")
    ax.set_ylabel("import kW")
    ax.set_title(meta["scenario"])
    ax.legend(fontsize=8)
    png = out_dir / f"{meta['scenario']}-import.png"
    fig.tight_layout()
    fig.savefig(png, dpi=110)
    return png.name


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--runs", nargs="+", required=True)
    p.add_argument("--out", required=True)
    args = p.parse_args()

    out_path = Path(args.out)
    out_dir = out_path.parent
    out_dir.mkdir(parents=True, exist_ok=True)

    lines = ["# Control-method experiment report", ""]
    kpi_keys = [
        "energy_import_kwh", "energy_export_kwh", "cost_eur",
        "peak_import_kw", "load_factor", "energy_shifted_kwh",
    ]

    for run in args.runs:
        run_dir, meta, kpis = load_run(run)
        lines.append(f"## {kpis['scenario']}  ({meta['started_at']}, {meta['duration_minutes']} min)")
        lines.append("")
        lines.append("| VEN | " + " | ".join(kpi_keys) + " |")
        lines.append("|---|" + "---|" * len(kpi_keys))
        for ven, k in kpis.get("vens", {}).items():
            lines.append(
                f"| {ven} | " + " | ".join(str(k.get(key, "—")) for key in kpi_keys) + " |"
            )
        rt = kpis.get("report_timeliness")
        if rt:
            lines.append("")
            lines.append(
                f"Report timeliness (recorder `report_lag_s`): n={rt['count']}, "
                f"median {rt['median_s']}s, min {rt['min_s']}s, max {rt['max_s']}s."
            )
        chart = try_chart(run_dir, meta, out_dir)
        if chart:
            lines.append("")
            lines.append(f"![import profile]({chart})")
        lines.append("")

    out_path.write_text("\n".join(lines), encoding="utf-8")
    print(f"report written to {out_path}")


if __name__ == "__main__":
    main()
