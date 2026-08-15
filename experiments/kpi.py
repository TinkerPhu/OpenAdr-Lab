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
import re
import sqlite3
import statistics
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
    """Columns 0-4 (ts, import_kw, export_kw, import_tariff_eur_kwh,
    export_tariff_eur_kwh) are the original WP3.8 shape most callers use;
    columns 5-6 (import_limit_kw, export_limit_kw — the DOE capacity limit
    active in each 1-min window, `NULL` when none was) are appended at the
    end rather than inserted, so `ven_kpis()`'s positional indexing doesn't
    shift."""
    con = sqlite3.connect(db_path)
    try:
        return con.execute(
            "SELECT ts, import_kw, export_kw, import_tariff_eur_kwh, export_tariff_eur_kwh,"
            " import_limit_kw, export_limit_kw"
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


def grid_envelope_compliance(db_path, t_from, t_to, direction):
    """Stakeholder KPI reframe: 'did this VEN stay under the grid operator's
    declared capacity envelope' — for `direction="import"`, `import_kw` vs.
    `import_limit_kw`; for `direction="export"`, `export_kw` vs.
    `export_limit_kw`. Export is not an afterthought: an uncurtailed PV
    export spike is a real overvoltage/appliance-damage risk, not just a
    grid-stability one, so this is called once per direction, not import-only.
    Restricted to samples where a limit was actually active (`*_limit_kw` not
    `NULL`); `None` when no sample in the window ever had one set (nothing to
    score, not zero compliance)."""
    actual_idx, limit_idx = (1, 5) if direction == "import" else (2, 6)
    rows = grid_rows(db_path, t_from, t_to)
    scored = [(r[actual_idx], r[limit_idx]) for r in rows if r[limit_idx] is not None]
    if not scored:
        return None
    dt_h = 1.0 / 60.0
    under = sum(1 for actual, limit in scored if actual <= limit)
    overshoot_kwh = sum(max(0.0, actual - limit) for actual, limit in scored) * dt_h
    return {
        "samples_under_limit": under,
        "samples_with_limit": len(scored),
        "compliance_pct": round(100.0 * under / len(scored), 1),
        "overshoot_kwh": round(overshoot_kwh, 4),
    }


_LATENCY_ACTION_TYPES = {
    "import": {"alert", "capacity_limit", "capacity_reservation"},
    "export": {"export_capacity_limit"},
}


def compliance_latency_s(db_path, t_from, t_to, actions, direction):
    """Stakeholder KPI reframe: 'how fast did this VEN react once the grid
    operator signaled a constraint' — first grid sample after a qualifying
    action (see `_LATENCY_ACTION_TYPES`) whose import/export magnitude has
    dropped >= 20% below the sample immediately preceding the action's start.
    `actions` is `run.json["actions"]` (`run_experiment.py`'s new
    action-start log). Finally implements the KPI this module's docstring
    has described, import-only and unbuilt, since WP3.8 -- and is the first
    time an export-side signal-response latency exists at all.

    A pre-action baseline at or below zero has nothing left to shed, so it
    counts as immediately compliant (`latency_s: 0.0`) rather than a
    mathematically meaningless '20% below zero'. `latency_s: None` (not a
    dropped row) means a qualifying action fired but no compliant sample was
    ever found in the window -- a real non-response, kept visible."""
    qualifying = [a for a in actions if a.get("type") in _LATENCY_ACTION_TYPES[direction]]
    if not qualifying:
        return []
    rows = grid_rows(db_path, t_from, t_to)
    actual_idx = 1 if direction == "import" else 2
    out = []
    for action in qualifying:
        start_ts = datetime.strptime(action["started_at"], "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=timezone.utc
        ).timestamp()
        pre_val = None
        for r in rows:
            if r[0] <= start_ts:
                pre_val = r[actual_idx]
            else:
                break
        latency_s = None
        if pre_val is not None and pre_val <= 0:
            latency_s = 0.0
        elif pre_val is not None:
            threshold = pre_val * 0.8
            for r in rows:
                if r[0] <= start_ts:
                    continue
                if r[actual_idx] <= threshold:
                    latency_s = round(r[0] - start_ts, 1)
                    break
        out.append({
            "action_type": action["type"],
            "at_minute": action["at_minute"],
            "latency_s": latency_s,
        })
    return out


def tariff_response_correlation(db_path, t_from, t_to):
    """Stakeholder KPI reframe (energy-business side): does this VEN's
    import actually track the prevailing tariff -- the thing demand response
    is for. `pearson_r` (expected negative for good price-following
    behaviour) plus a plain-language companion: mean import in the cheapest
    priced tercile of the window vs. the most expensive tercile, as a %
    difference. `None` when fewer than 5 distinct price points are in the
    window -- correlation is meaningless on a near-flat series (most
    `realistic`-tier 30 min scenarios only vary price 2-3 times; the
    24h `s9_diurnal` scenario exists specifically to give this KPI enough
    price variation to say something)."""
    rows = grid_rows(db_path, t_from, t_to)
    pairs = [(r[3], r[1]) for r in rows if r[3] is not None]
    if len({p[0] for p in pairs}) < 5:
        return None
    n = len(pairs)
    mean_x = sum(p[0] for p in pairs) / n
    mean_y = sum(p[1] for p in pairs) / n
    var_x = sum((p[0] - mean_x) ** 2 for p in pairs)
    var_y = sum((p[1] - mean_y) ** 2 for p in pairs)
    pearson_r = None
    if var_x > 0 and var_y > 0:
        cov = sum((p[0] - mean_x) * (p[1] - mean_y) for p in pairs)
        pearson_r = round(cov / (var_x**0.5 * var_y**0.5), 3)
    ordered = sorted(pairs, key=lambda p: p[0])
    tercile = n // 3
    cheap_mean = expensive_mean = pct_diff = None
    if tercile > 0:
        cheap = ordered[:tercile]
        expensive = ordered[-tercile:]
        cheap_mean = round(sum(p[1] for p in cheap) / len(cheap), 3)
        expensive_mean = round(sum(p[1] for p in expensive) / len(expensive), 3)
        if expensive_mean:
            pct_diff = round(100.0 * (cheap_mean - expensive_mean) / abs(expensive_mean), 1)
    return {
        "n": n,
        "pearson_r": pearson_r,
        "cheap_tercile_mean_import_kw": cheap_mean,
        "expensive_tercile_mean_import_kw": expensive_mean,
        "cheap_vs_expensive_pct": pct_diff,
    }


_ISO8601_DURATION_RE = re.compile(
    r"^P(?:(?P<years>\d+)Y)?(?:(?P<months>\d+)M)?(?:(?P<days>\d+)D)?"
    r"(?:T(?:(?P<hours>\d+)H)?(?:(?P<minutes>\d+)M)?(?:(?P<seconds>\d+(?:\.\d+)?)S)?)?$"
)


def _parse_iso8601_duration_hours(s):
    """Full ISO 8601 duration parser (years down to fractional seconds).
    Handles both the VEN's own compact reporter output (`PT5M`, no all-zero
    date fields) AND the fully-qualified form (`P0Y0M0DT0H5M0S`) that
    archived reports_received rows actually carry once a duration has been
    round-tripped through the VTN's own wire types — found live during the
    fleet-run-2 results review: the previous PT-prefix-only parser silently
    returned 0.0 for every P0Y... row, so `event_impact_kwh` had been
    computing 0.0 for every VEN in every scenario, always -- not a real
    "no impact" result, a parsing bug (see KEY_LEARNINGS.md). 30-day months,
    365-day years -- approximate; these experiment scenarios only ever
    produce minute/hour-scale report intervals, so the Y/M/D fields are
    expected to stay at 0 in practice and this approximation's error never
    actually matters here. Unknown/unmatched shapes parse as 0."""
    if not s or not s.startswith("P"):
        return 0.0
    m = _ISO8601_DURATION_RE.match(s)
    if not m:
        return 0.0
    parts = {k: float(v) if v else 0.0 for k, v in m.groupdict().items()}
    return (
        parts["years"] * 365 * 24
        + parts["months"] * 30 * 24
        + parts["days"] * 24
        + parts["hours"]
        + parts["minutes"] / 60.0
        + parts["seconds"] / 3600.0
    )


def _report_energy_kwh(csv_path, t_from, t_to, ven_name, report_type, event_ids=None):
    """Sum a VEN's `report_type` report intervals (values in W) into kWh,
    restricted to rows whose `received_at` falls in [t_from, t_to). `event_ids`
    (when given), further restricts to reports whose payload's `eventID`
    belongs to this run's own events -- Node1's VTN is shared with other
    pre-existing/concurrent test programs, whose own periodic report traffic
    can otherwise leak into the time window purely by coincidence of when it
    happened to arrive (found live during the fleet-run-2 S-1..S-8 run: an
    unrelated leftover 'test-rd-check' program's reports, with a stale
    interval reference producing report_lag_s in the millions of seconds,
    were being time-windowed into this run's own report_timeliness stats)."""
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
            if event_ids is not None and payload.get("eventID") not in event_ids:
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


def event_impact_kwh(csv_path, t_from, t_to, ven_name, event_ids=None):
    """WP5.4: Σ(baseline − actual) over the run window, from archived BASELINE
    and USAGE reports. `None` when no BASELINE reports were archived for this
    VEN in the window (nothing to compare against). See `_report_energy_kwh`
    for why `event_ids` matters on a shared VTN."""
    baseline_kwh = _report_energy_kwh(csv_path, t_from, t_to, ven_name, "BASELINE", event_ids)
    if baseline_kwh is None:
        return None
    usage_kwh = _report_energy_kwh(csv_path, t_from, t_to, ven_name, "USAGE", event_ids) or 0.0
    return round(baseline_kwh - usage_kwh, 4)


def report_lag_stats(csv_path, t_from, t_to, event_ids=None):
    """Only reports the recorder received during the run window count —
    the archive holds every report ever seen, including ancient ones.
    `event_ids` (when given) further restricts to this run's own events, for
    the same reason `_report_energy_kwh` does — see its docstring. Without
    this, a shared VTN's unrelated concurrent report traffic can produce
    wildly wrong lag values (observed live: report_lag_s in the millions of
    seconds from a leftover test program's stale interval reference)."""
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
            if event_ids is not None:
                try:
                    payload = json.loads(row["payload_json"])
                except (KeyError, ValueError):
                    continue
                if payload.get("eventID") not in event_ids:
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
        "median_s": round(statistics.median(lags), 1),
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
            "median": statistics.median(s),
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

    def row(report_type, received_offset_s, payload_type, value_w, duration, event_id=None):
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
        if event_id is not None:
            payload["eventID"] = event_id
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

        # Scenario 3: a shared VTN can carry another program's concurrent
        # report traffic that happens to fall in the same time window --
        # event_ids restricts to this run's own event, so the unrelated
        # "other-evt" BASELINE row (which alone would otherwise flip the
        # result) is excluded and only "our-evt"'s rows count.
        rows3 = [
            row("baseline-report", 60, "BASELINE", 2000.0, "PT15M", event_id="our-evt"),
            row("usage-report", 90, "USAGE", 1000.0, "PT15M", event_id="our-evt"),
            row("baseline-report", 100, "BASELINE", 999000.0, "PT15M", event_id="other-evt"),
        ]
        with open(csv_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(rows3[0].keys()))
            writer.writeheader()
            writer.writerows(rows3)
        impact3 = event_impact_kwh(csv_path, t_from, t_to, "ven-1", event_ids={"our-evt"})
        assert impact3 == 0.25, f"expected 0.25 (other-evt excluded), got {impact3}"
        impact3_unfiltered = event_impact_kwh(csv_path, t_from, t_to, "ven-1")
        assert impact3_unfiltered != 0.25, "unfiltered call should differ once other-evt is mixed in"

        # Scenario 4: archived reports_received rows carry the fully-qualified
        # ISO 8601 duration form (e.g. "P0Y0M0DT0H5M0S"), not the VEN's own
        # compact "PT5M" -- found live during the fleet-run-2 results review,
        # where this made event_impact_kwh silently compute 0.0 for every VEN
        # in every scenario (the old parser only recognized a leading "PT").
        rows4 = [
            row("baseline-report", 60, "BASELINE", 2000.0, "P0Y0M0DT0H15M0S", event_id="e4"),
            row("usage-report", 90, "USAGE", 1000.0, "P0Y0M0DT0H15M0S", event_id="e4"),
        ]
        with open(csv_path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(rows4[0].keys()))
            writer.writeheader()
            writer.writerows(rows4)
        impact4 = event_impact_kwh(csv_path, t_from, t_to, "ven-1", event_ids={"e4"})
        assert impact4 == 0.25, f"expected 0.25 from the full-form duration, got {impact4}"

    assert _parse_iso8601_duration_hours("P0Y0M0DT0H5M0S") == 1 / 12, "full-form 5-minute duration"
    assert _parse_iso8601_duration_hours("PT5M") == 1 / 12, "compact-form 5-minute duration"
    assert _parse_iso8601_duration_hours("") == 0.0
    assert _parse_iso8601_duration_hours("garbage") == 0.0

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
        assert summary["solver_ms"] == {"median": 100.0, "max": 120, "mean": 100.0}, summary["solver_ms"]
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

    with tempfile.TemporaryDirectory() as tmp:
        db_path = Path(tmp) / "ven-1-history.sqlite"
        con = sqlite3.connect(db_path)
        con.execute(
            "CREATE TABLE grid_samples (ts INTEGER NOT NULL, import_kw REAL NOT NULL,"
            " export_kw REAL NOT NULL, import_tariff_eur_kwh REAL, export_tariff_eur_kwh REAL,"
            " co2_g_kwh REAL, import_limit_kw REAL, export_limit_kw REAL)"
        )
        # 10 one-minute samples starting at t0. Minutes 0-4: import ramps
        # 5.0 -> 3.0 kW against a 4.0 kW import_limit_kw (breaches minutes
        # 0-1, complies from minute 2). Minutes 0-4: export flat at 6.0 kW
        # against an 8.0 kW export_limit_kw (always compliant, exercises the
        # other direction independently). Minutes 5-9: no limit active on
        # either leg (limit columns NULL) -- must not count toward compliance.
        rows = [
            (t0 + i * 60, imp, 6.0, 0.10 + 0.01 * i, 0.05, None, limit_imp, 8.0 if i < 5 else None)
            for i, (imp, limit_imp) in enumerate(
                [(5.0, 4.0), (4.5, 4.0), (4.0, 4.0), (3.5, 4.0), (3.0, 4.0)]
                + [(2.0, None)] * 5
            )
        ]
        con.executemany(
            "INSERT INTO grid_samples VALUES (?,?,?,?,?,?,?,?)",
            [(int(ts), imp, exp, itar, etar, co2, ilim, elim) for ts, imp, exp, itar, etar, co2, ilim, elim in rows],
        )
        con.commit()
        con.close()

        compliance = grid_envelope_compliance(db_path, int(t0), int(t0) + 600, "import")
        assert compliance == {
            "samples_under_limit": 3, "samples_with_limit": 5,
            "compliance_pct": 60.0, "overshoot_kwh": round((1.0 + 0.5) / 60.0, 4),
        }, compliance

        export_compliance = grid_envelope_compliance(db_path, int(t0), int(t0) + 600, "export")
        assert export_compliance == {
            "samples_under_limit": 5, "samples_with_limit": 5,
            "compliance_pct": 100.0, "overshoot_kwh": 0.0,
        }, export_compliance

        # An action starting at minute 0 (import 5.0 kW pre-baseline, 20%
        # drop threshold = 4.0 kW) first complies at minute 2 (4.0 <= 4.0),
        # i.e. 120s latency.
        started_at = datetime.fromtimestamp(t0, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        actions = [{"at_minute": 0, "type": "capacity_limit", "started_at": started_at}]
        latency = compliance_latency_s(db_path, int(t0), int(t0) + 600, actions, "import")
        assert latency == [{"action_type": "capacity_limit", "at_minute": 0, "latency_s": 120.0}], latency

        # export direction: no export_capacity_limit action present -> [].
        assert compliance_latency_s(db_path, int(t0), int(t0) + 600, actions, "export") == []

        # Only 5 distinct import tariffs across 10 samples (0.10..0.14, then
        # repeats aren't present here -- each of the 10 rows has a distinct
        # value 0.10+0.01*i) -> 10 distinct values, correlation computable.
        # import_kw trends down as tariff rises -> negative correlation.
        corr = tariff_response_correlation(db_path, int(t0), int(t0) + 600)
        assert corr["n"] == 10, corr
        assert corr["pearson_r"] < 0, corr
        assert corr["cheap_tercile_mean_import_kw"] > corr["expensive_tercile_mean_import_kw"], corr

    print("kpi.py self-check OK: grid_envelope_compliance, compliance_latency_s, tariff_response_correlation")


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
    # Restrict report-derived KPIs to this run's own VTN events -- Node1's
    # VTN is shared with other concurrent/leftover test programs, whose
    # report traffic can otherwise leak into the time window. `or None` so an
    # older run.json without an "events" key (or a run with none) falls back
    # to the unfiltered behavior rather than matching nothing.
    event_ids = set(meta.get("events", [])) or None
    actions = meta.get("actions", [])  # [] for run.json written before this KPI reframe
    tier = meta.get("tier", "realistic")  # old runs predate scenario tiering -- assume realistic

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
                    baseline[ven] = {"energy_import_kwh": k["energy_import_kwh"], "cost_eur": k["cost_eur"]}

    out = {"scenario": meta["scenario"], "meta": {"tier": tier}, "vens": {}}
    for ven in meta["vens"]:
        db = run_dir / f"{ven}-history.sqlite"
        if not db.exists():
            continue
        k = ven_kpis(db, t_from, t_to)
        if k is None:
            continue

        # Grid stakeholder: envelope compliance + signal-response latency,
        # both directions -- export gets equal billing with import (an
        # uncurtailed PV export spike is an overvoltage risk, not just a
        # grid-stability one).
        grid_regulation = {
            "import": grid_envelope_compliance(db, t_from, t_to, "import"),
            "export": grid_envelope_compliance(db, t_from, t_to, "export"),
            "latency_s": {
                "import": compliance_latency_s(db, t_from, t_to, actions, "import"),
                "export": compliance_latency_s(db, t_from, t_to, actions, "export"),
            },
        }

        # Energy-business stakeholder: does import track the tariff curve.
        energy_business = {"tariff_response": tariff_response_correlation(db, t_from, t_to)}
        if ven in baseline:
            energy_business["energy_shifted_kwh"] = round(
                baseline[ven]["energy_import_kwh"] - k["energy_import_kwh"], 4
            )

        # VEN stakeholder: what participating cost this household, in money
        # and (proxy) comfort.
        ven_impact = {"cost_eur": k["cost_eur"]}
        if ven in baseline:
            ven_impact["cost_eur_delta"] = round(baseline[ven]["cost_eur"] - k["cost_eur"], 4)
        impact = event_impact_kwh(
            run_dir / "recorder-reports_received.csv", t_from, t_to, ven, event_ids
        )
        if impact is not None:
            ven_impact["event_impact_kwh"] = impact

        # Mechanism health: is the planner itself working correctly --
        # explicitly separate from "is the fleet's behaviour good" above, so
        # warnings from a deliberately-forced stress fixture (tier=stress)
        # don't read as a behavioural failure.
        mechanism_health = {}
        diag = plan_history_summary(run_dir, ven)
        if diag is None:
            diag = plan_diagnostics_summary(run_dir, ven)
        if diag is not None:
            mechanism_health["plan_diagnostics"] = diag
            # BUDGET_SHORTFALL count as the available comfort-shortfall
            # proxy (a true "did the EV reach its target by its deadline"
            # metric needs new VEN instrumentation -- see plan's out-of-scope).
            warning_kinds = diag.get("warning_kind_counts")
            if warning_kinds is not None:
                ven_impact["budget_shortfall_warnings"] = warning_kinds.get("BUDGET_SHORTFALL", 0)
            # Cost of compliance: already-collected per-plan-cycle cost
            # fields (GB-25), just regrouped under the VEN's own viewpoint.
            compliance_cost = {
                f: diag["cost_stats"][f]
                for f in ("c_violations_eur", "c_peak_penalty_eur", "c_wear_eur")
                if f in diag.get("cost_stats", {})
            }
            if compliance_cost:
                ven_impact["compliance_cost_eur"] = compliance_cost
        fa = forecast_accuracy_summary(run_dir, ven)
        if fa is not None:
            mechanism_health["forecast_accuracy"] = fa

        out["vens"][ven] = {
            "raw": k,
            "grid_regulation": grid_regulation,
            "energy_business": energy_business,
            "ven_impact": ven_impact,
            "mechanism_health": mechanism_health,
        }

    out["report_timeliness"] = report_lag_stats(
        run_dir / "recorder-reports_received.csv", t_from, t_to, event_ids
    )

    # WP4.5: persona segmentation — mean KPIs per persona group so the
    # experiment report shows the behavioural spread across the fleet.
    if args.manifest:
        manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
        persona_of = {v["ven_name"]: v.get("persona") for v in manifest["vens"]}
        groups = {}
        for ven, v in out["vens"].items():
            persona = persona_of.get(ven)
            if persona:
                flat = {
                    **v["raw"],
                    "energy_shifted_kwh": v["energy_business"].get("energy_shifted_kwh"),
                }
                groups.setdefault(persona, []).append(flat)
        metrics = ("energy_import_kwh", "cost_eur", "peak_import_kw", "energy_shifted_kwh")
        out["personas"] = {
            persona: {
                "vens": len(ks),
                **{
                    f"mean_{m}": round(sum(k[m] for k in ks) / len(ks), 4)
                    for m in metrics
                    if all(k.get(m) is not None for k in ks)
                },
            }
            for persona, ks in sorted(groups.items())
        }

    (run_dir / "kpis.json").write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
