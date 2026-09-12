#!/usr/bin/env python3
"""GB-46 — measured per-VEN limit compliance for fleet experiments.

Pass bar (user decision 2026-09-12): each VEN reaches the limit, or its physical
floor if the limit is unreachable, within 5 minutes of the event start, and holds
it for the rest of the window. Judged from the VEN's own per-minute meter data
(`grid_samples`) and asset traces (`tick_samples`), never from planner-predicted
warnings. Used by kpi.py; run `python3 experiments/compliance.py --self-check`.
"""

import sys
from datetime import datetime, timezone

GRACE_S = 300
TOL_KW = 0.05
ENGAGED_FRACTION = 0.8
# Minutes after a price interval starts during which the recorded tariff may still
# lag (VEN event poll + 1-minute history sampling), skipped by signal_integrity.
PRICE_PROPAGATION_S = 120


def _epoch(iso):
    return int(datetime.strptime(iso, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc).timestamp())


# Action type → (direction, parameter holding the limit; None = fixed 0 kW).
# Reservations and dispatch setpoints are not hard limits and are not scored here.
_LIMIT_ACTIONS = {
    "capacity_limit": ("import", "import_kw"),
    "export_capacity_limit": ("export", "export_kw"),
    "alert": ("import", None),
}


def limit_windows(actions):
    """Hard-limit windows from run.json["actions"] (GB-46 action log with parameters).
    Returns (windows, notes); actions logged before the log carried parameters are
    skipped with a note instead of guessed."""
    windows, notes = [], []
    for a in actions:
        spec = _LIMIT_ACTIONS.get(a.get("type"))
        if spec is None:
            continue
        direction, param = spec
        if "duration_minutes" not in a or (param is not None and param not in a):
            notes.append(f"{a.get('type')} at minute {a.get('at_minute')}: no parameters in run.json, not scored")
            continue
        start = _epoch(a["started_at"])
        windows.append({
            "type": a["type"],
            "direction": direction,
            "limit_kw": float(a[param]) if param else 0.0,
            "start_ts": start,
            "end_ts": start + 60 * int(a["duration_minutes"]),
            "at_minute": a.get("at_minute"),
        })
    return windows, notes


def import_floor_kw(assets, profile):
    """Least import this VEN could achieve this minute if every controllable asset did
    the right thing. `assets`: asset_id → {power_kw, soc_pct, temperature_c} from
    tick_samples; `profile`: the VEN's profile YAML. EV and heater are controllable to
    0 — the heater only counts while at/below its own temp_min_c (a genuine emergency),
    so a thermostat override above it (GB-44) can never be excused. PV is curtailable;
    its generation (negative power) lowers the floor. A battery above min_soc can
    discharge up to max_discharge_kw. Any other asset (base load, a running shiftable
    load) is uncontrollable."""
    by_id = {a.get("id", a["type"]): a for a in profile.get("assets", [])}
    floor = 0.0
    battery_kw = 0.0
    for asset_id, s in assets.items():
        cfg = by_id.get(asset_id, {})
        kind = cfg.get("type", asset_id)
        p = s.get("power_kw") or 0.0
        if kind == "ev":
            continue
        if kind == "heater":
            t = s.get("temperature_c")
            if t is not None and "temp_min_c" in cfg and t <= cfg["temp_min_c"]:
                floor += max(0.0, p)
            continue
        if kind == "battery":
            soc = s.get("soc_pct")
            if soc is not None and soc / 100.0 > cfg.get("min_soc", 0.0) + 0.01:
                battery_kw = cfg.get("max_discharge_kw", 0.0)
            continue
        floor += p  # base load, shiftable loads (≥ 0) and PV generation (< 0)
    return max(0.0, floor - battery_kw)


def window_compliance(rows, window, floor_by_ts=None):
    """Score one VEN against one limit window. `rows`: (ts, import_kw, export_kw, ...)
    per minute; `floor_by_ts`: ts → import floor (import windows only). Target per
    minute = max(limit, floor). time_to_comply_s is sustained compliance: the first
    minute from which the VEN stays at/below target (+TOL_KW) for the rest of the
    window; None if it never does."""
    floor_by_ts = floor_by_ts or {}
    idx = 1 if window["direction"] == "import" else 2
    limit = window["limit_kw"]
    scored = []
    for r in rows:
        ts = r[0]
        if window["start_ts"] <= ts < window["end_ts"]:
            floor = floor_by_ts.get(ts, 0.0) if window["direction"] == "import" else 0.0
            scored.append((ts, r[idx], max(limit, floor), floor))
    if not scored:
        return None
    ok = [actual <= target + TOL_KW for _, actual, target, _ in scored]
    first = None
    for i in range(len(ok) - 1, -1, -1):
        if not ok[i]:
            break
        first = i
    time_to_comply_s = None if first is None else max(0, scored[first][0] - window["start_ts"])
    max_actual = max(a for _, a, _, _ in scored)
    engaged = (
        (max_actual >= ENGAGED_FRACTION * limit and max_actual > TOL_KW)
        or any(f > limit for _, _, _, f in scored)
        or scored[0][1] > limit + TOL_KW
    )
    overshoot = sum(
        max(0.0, actual - target - TOL_KW)
        for ts, actual, target, _ in scored
        if ts >= window["start_ts"] + GRACE_S
    ) / 60.0
    return {
        "type": window["type"],
        "direction": window["direction"],
        "at_minute": window.get("at_minute"),
        "limit_kw": limit,
        "samples": len(scored),
        "max_actual_kw": round(max_actual, 3),
        "max_floor_kw": round(max(f for _, _, _, f in scored), 3),
        "engaged": bool(engaged),
        "time_to_comply_s": time_to_comply_s,
        "pass": time_to_comply_s is not None and time_to_comply_s <= GRACE_S,
        "overshoot_after_grace_kwh": round(overshoot, 4),
    }


def signal_integrity(rows, price_action):
    """Did the recorded tariff equal the scenario's price? `rows`: (ts, import_kw,
    export_kw, import_tariff_eur_kwh, ...). Minutes within PRICE_PROPAGATION_S of an
    interval boundary are skipped (event poll + sampling lag)."""
    start = _epoch(price_action["started_at"])
    step = 60 * int(price_action["interval_minutes"])
    values = price_action["values_eur_kwh"]
    checked = mismatched = 0
    for r in rows:
        offset = r[0] - start
        if offset < 0 or offset >= step * len(values) or offset % step < PRICE_PROPAGATION_S:
            continue
        checked += 1
        recorded = r[3]
        if recorded is None or abs(recorded - values[offset // step]) > 1e-6:
            mismatched += 1
    return {"checked_minutes": checked, "mismatched_minutes": mismatched}


def fleet_coincident_peak(per_ven_rows):
    """Max over minutes of the fleet's summed import (not the sum of per-VEN peaks)."""
    totals = {}
    for rows in per_ven_rows.values():
        for r in rows:
            totals[r[0]] = totals.get(r[0], 0.0) + r[1]
    return round(max(totals.values()), 3) if totals else None


# ── self-checks ──────────────────────────────────────────────────────────────

T0 = "2026-08-31T10:48:00Z"
T0S = _epoch(T0)


def _minute_rows(values, start=T0S, export=None):
    """(ts, import_kw, export_kw) per minute from a list of import values."""
    export = export or [0.0] * len(values)
    return [(start + 60 * i, v, e) for i, (v, e) in enumerate(zip(values, export))]


def _self_check_limit_windows():
    actions = [
        {"at_minute": 0, "type": "price_series", "started_at": T0, "values_eur_kwh": [0.1, 0.45, 0.1], "interval_minutes": 10},
        {"at_minute": 5, "type": "capacity_limit", "started_at": "2026-08-31T10:53:00Z", "import_kw": 1.5, "duration_minutes": 20},
        {"at_minute": 10, "type": "alert", "started_at": "2026-08-31T10:58:00Z", "alert_type": "ALERT_GRID_EMERGENCY", "duration_minutes": 10},
        {"at_minute": 12, "type": "export_capacity_limit", "started_at": "2026-08-31T11:00:00Z", "export_kw": 0.5, "duration_minutes": 30},
        {"at_minute": 14, "type": "capacity_limit", "started_at": "2026-08-31T11:02:00Z"},  # pre-GB-46 run.json: no params
    ]
    windows, notes = limit_windows(actions)
    by_type = {w["type"]: w for w in windows}
    cap = by_type["capacity_limit"]
    assert (cap["direction"], cap["limit_kw"], cap["end_ts"] - cap["start_ts"]) == ("import", 1.5, 1200), cap
    alert = by_type["alert"]
    assert (alert["direction"], alert["limit_kw"]) == ("import", 0.0), alert
    exp = by_type["export_capacity_limit"]
    assert (exp["direction"], exp["limit_kw"]) == ("export", 0.5), exp
    assert len(windows) == 3, "price actions and parameterless actions are not limit windows"
    assert len(notes) == 1 and "capacity_limit" in notes[0], notes
    print("compliance self-check OK: limit_windows")


PROFILE = {
    "assets": [
        {"type": "base_load", "id": "base_load"},
        {"type": "battery", "id": "battery", "max_discharge_kw": 3.5, "min_soc": 0.10},
        {"type": "heater", "id": "heater", "temp_min_c": 18.0},
        {"type": "pv", "id": "pv"},
        {"type": "ev", "id": "ev"},
    ]
}


def _self_check_import_floor():
    only_base = {"assets": [{"type": "base_load", "id": "base_load"}]}
    assert import_floor_kw({"base_load": {"power_kw": 0.5}}, only_base) == 0.5
    # Heater at full power in the comfort band is controllable: not in the floor.
    comfort = {"base_load": {"power_kw": 0.8}, "heater": {"power_kw": 3.5, "temperature_c": 20.0},
               "battery": {"power_kw": 0.0, "soc_pct": 5.0}, "ev": {"power_kw": 7.4}}
    assert abs(import_floor_kw(comfort, PROFILE) - 0.8) < 1e-9
    # Genuine emergency (at/below its own temp_min_c) is part of the floor.
    cold = dict(comfort, heater={"power_kw": 3.5, "temperature_c": 17.9})
    assert abs(import_floor_kw(cold, PROFILE) - 4.3) < 1e-9
    # Battery above min_soc can cover load; PV (negative) reduces the floor; never below 0.
    covered = {"base_load": {"power_kw": 0.8}, "battery": {"power_kw": 0.0, "soc_pct": 60.0},
               "pv": {"power_kw": -0.3}}
    assert import_floor_kw(covered, PROFILE) == 0.0
    partly = {"base_load": {"power_kw": 5.0}, "battery": {"power_kw": 0.0, "soc_pct": 60.0},
              "pv": {"power_kw": -0.5}}
    assert abs(import_floor_kw(partly, PROFILE) - 1.0) < 1e-9
    # Shiftable loads (any asset type the profile doesn't list as controllable) count.
    shiftable = {"base_load": {"power_kw": 0.4}, "wm": {"power_kw": 2.0}}
    assert abs(import_floor_kw(shiftable, only_base) - 2.4) < 1e-9
    print("compliance self-check OK: import_floor_kw")


def _window(limit=1.5, minutes=20, direction="import", start=T0S):
    return {"type": "capacity_limit", "direction": direction, "limit_kw": limit,
            "start_ts": start, "end_ts": start + 60 * minutes, "at_minute": 0}


def _self_check_window_compliance():
    # Late but sustained: over for 6 minutes, then under → 360 s, fail.
    late = window_compliance(_minute_rows([3.6] * 6 + [1.0] * 14), _window())
    assert (late["time_to_comply_s"], late["pass"], late["engaged"]) == (360, False, True), late
    # Immediate compliance → pass.
    ok = window_compliance(_minute_rows([1.4] * 20), _window())
    assert (ok["time_to_comply_s"], ok["pass"]) == (0, True), ok
    # Complies at minute 1, relapses from minute 12 → never sustained.
    relapse = window_compliance(_minute_rows([2.0] + [1.0] * 11 + [2.0] * 8), _window())
    assert (relapse["time_to_comply_s"], relapse["pass"]) == (None, False), relapse
    # Limit far above the VEN's level → not engaged (still passes, excluded from fleet rate).
    idle = window_compliance(_minute_rows([1.0] * 20), _window(limit=4.0))
    assert idle["engaged"] is False and idle["pass"] is True, idle
    # Floor above the limit: target is the floor, so importing the floor complies.
    floor = {T0S + 60 * i: 0.5 for i in range(20)}
    alert = window_compliance(_minute_rows([0.5] * 20), _window(limit=0.0), floor)
    assert (alert["pass"], alert["engaged"]) == (True, True), alert
    # Overshoot counted only after the grace period.
    over = window_compliance(_minute_rows([2.5] * 20), _window())
    assert abs(over["overshoot_after_grace_kwh"] - 15 * (1.0 - TOL_KW) / 60) < 1e-6, over
    # Export direction uses export_kw and a zero floor.
    exp_rows = _minute_rows([0.0] * 10, export=[2.0, 0.4] + [0.3] * 8)
    exp = window_compliance(exp_rows, _window(limit=0.5, minutes=10, direction="export"))
    assert (exp["time_to_comply_s"], exp["pass"], exp["engaged"]) == (60, True, True), exp
    print("compliance self-check OK: window_compliance")


def _self_check_signal_integrity():
    price = {"type": "price_series", "started_at": T0, "values_eur_kwh": [0.10, 0.45, 0.10], "interval_minutes": 10}
    good = [(T0S + 60 * m, 0.0, 0.0, [0.10, 0.45, 0.10][m // 10]) for m in range(30)]
    res = signal_integrity(good, price)
    assert res["mismatched_minutes"] == 0 and res["checked_minutes"] > 20, res
    overridden = [(T0S + 60 * m, 0.0, 0.0, 0.09) for m in range(30)]
    res = signal_integrity(overridden, price)
    assert res["mismatched_minutes"] == res["checked_minutes"] > 20, res
    print("compliance self-check OK: signal_integrity")


def _self_check_fleet_coincident_peak():
    a = [(T0S + 60 * m, 3.0 if m == 2 else 1.0, 0.0) for m in range(8)]
    b = [(T0S + 60 * m, 3.0 if m == 5 else 1.0, 0.0) for m in range(8)]
    assert fleet_coincident_peak({"ven-a": a, "ven-b": b}) == 4.0
    print("compliance self-check OK: fleet_coincident_peak")


def _self_check():
    _self_check_limit_windows()
    _self_check_import_floor()
    _self_check_window_compliance()
    _self_check_signal_integrity()
    _self_check_fleet_coincident_peak()


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--self-check":
        _self_check()
