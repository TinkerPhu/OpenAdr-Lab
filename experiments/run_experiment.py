#!/usr/bin/env python3
"""WP3.8 (A-3) — run one control-method scenario against the live stack and
snapshot both data stores for KPI extraction.

Scenarios run in REAL TIME: the sim clock is wall time (tick_once stamps
Utc::now and event windows are absolute), so time acceleration is not
externally drivable without an injectable clock through the whole tick/poll
path — the spike result from the phase plan. S-1..S-6 are therefore short
same-day windows (default 30 min each) rather than simulated days.

GB-28: by default (--paired-baseline), each run first spends one more
duration_minutes-long window with no events posted, immediately before the
scenario itself, so kpi.py's energy_shifted_kwh has a same-VEN baseline
captured minutes -- not hours -- away in wall-clock time. This roughly
doubles the script's total runtime; pass --no-paired-baseline to skip it.

GB-29: for a single-host run (VENs on Node1 only), this can run ON that
docker host, same convention as fleet.sh. For a multi-host fleet (Node1 +
Node2), it instead runs OFF-host, e.g. from a workstation that can reach
both docker hosts over LAN/ssh but isn't one of them itself -- Node1 has no
configured ssh trust to Node2, so `ssh Node1 "ssh Node2 ..."` isn't an
option. `--fleet-map` (experiments/fleet_map.json) routes each VEN's
snapshot to a local copy or a remote scp/ssh pull per its "host" entry, and
`--pg-host` routes the recorder-DB dump through ssh when not run on the
VTN's own host:
    python3 experiments/run_experiment.py --scenario experiments/scenarios/s2_price_spike.yaml
    ... --vens ven-1,ven-2,ven-3            # which VEN data dirs to snapshot
    ... --out experiments/results           # output root
    ... --fleet-map experiments/fleet_map.json --pg-host Node1  # multi-host run

Steps: create a program, replay the scenario's actions at their offsets
(one action type, `budget_shortfall`, bypasses the VTN entirely -- see
below), wait out the window, delete the created events/program/requests,
snapshot each VEN's history.sqlite plus the lab_recorder tables (CSV via
psql in the vtn-db container), then (GB-25, --fleet-map only) pull each
VEN's `GET /history/plans` and `GET /history/forecast-accuracy` for the
same window into per-VEN JSON files.

`budget_shortfall` (S-8): unlike every other action type, this does NOT go
through the VTN -- it's a direct `POST /user-requests` against one target
VEN's own HTTP API, with a deliberately too-tight budget, so that VEN's own
MILP planner raises a real BudgetShortfall plan warning (see
VEN/src/controller/milp_planner/inputs.rs's `budget_warning`). Requires
--fleet-map (the target VEN's base URL is resolved from it).
"""

import argparse
import json
import shlex
import shutil
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import requests
import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent


def get_token(base, client_id, client_secret):
    r = requests.post(
        f"{base}/auth/token",
        data={"grant_type": "client_credentials", "client_id": client_id, "client_secret": client_secret},
        timeout=10,
    )
    r.raise_for_status()
    return r.json()["access_token"]


def auth(token):
    return {"Authorization": f"Bearer {token}"}


def iso(dt):
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def post_event(base, token, body):
    r = requests.post(f"{base}/events", headers=auth(token), json=body, timeout=10)
    r.raise_for_status()
    return r.json()["id"]


# GB-27: attached to the first event of a window (--request-reports, on by
# default) so the VEN actually has an obligation to report against --
# without a reportDescriptor on some event, extract_report_obligations()
# never creates one and report_lag_stats()/event_impact_kwh() stay null
# forever. frequency is *seconds*, not an ISO 8601 duration (a documented
# past gotcha) -- 300s keeps reports well inside every scenario's 30 min
# window. historical=false on BASELINE requests a forecast (the M&V
# counterfactual), not a replay of past data.
REPORT_DESCRIPTORS = [
    {"payloadType": "BASELINE", "readingType": "DIRECT_READ", "frequency": 300, "historical": False},
    {"payloadType": "USAGE", "readingType": "DIRECT_READ", "frequency": 300},
]


def build_event(program_id, action, start, report_descriptors=None):
    """Translate one scenario action into an OpenADR event body."""
    t = action["type"]
    if t == "price_series":
        minutes = action["interval_minutes"]
        intervals = [
            {
                "id": i,
                "intervalPeriod": {
                    "start": iso(start + timedelta(minutes=i * minutes)),
                    "duration": f"PT{minutes}M",
                },
                "payloads": [{"type": "PRICE", "values": [v]}],
            }
            for i, v in enumerate(action["values_eur_kwh"])
        ]
        body = {"programID": program_id, "eventName": "exp-price", "intervals": intervals}
        if report_descriptors:
            body["reportDescriptors"] = report_descriptors
        return body

    window = {
        "start": iso(start),
        "duration": f"PT{action['duration_minutes']}M",
    }
    payload = {
        "capacity_limit": ("IMPORT_CAPACITY_LIMIT", action.get("import_kw")),
        "capacity_reservation": ("IMPORT_CAPACITY_RESERVATION", action.get("import_kw")),
        # Mirrors capacity_limit but for the export leg -- the VEN already
        # implements EXPORT_CAPACITY_LIMIT end-to-end (openadr_interface.rs,
        # GridSample.export_limit_kw); this scenario tooling just never
        # exercised it before (see the stakeholder KPI-reframe plan).
        "export_capacity_limit": ("EXPORT_CAPACITY_LIMIT", action.get("export_kw")),
        "alert": (action.get("alert_type", "ALERT_GRID_EMERGENCY"), "experiment alert"),
        "simple": ("SIMPLE", action.get("level")),
        "dispatch": ("DISPATCH_SETPOINT", action.get("setpoint_kw")),
    }[t]
    body = {
        "programID": program_id,
        "eventName": f"exp-{t.replace('_', '-')}",
        "intervalPeriod": window,
        "intervals": [{"id": 0, "payloads": [{"type": payload[0], "values": [payload[1]]}]}],
    }
    if report_descriptors:
        body["reportDescriptors"] = report_descriptors
    return body


def post_user_request(base_url, body):
    """POST {base_url}/user-requests directly against a VEN's own HTTP API --
    unlike every other action type (which goes through the VTN via
    build_event/post_event), this talks straight to the target VEN. Used by
    the `budget_shortfall` scenario action to force a real BudgetShortfall
    plan warning (VEN/src/controller/milp_planner/inputs.rs's
    `budget_warning`). No auth header: /user-requests is a same-host
    VEN-local endpoint, not behind the VTN's bearer-token auth. Returns the
    created request's id."""
    r = requests.post(f"{base_url}/user-requests", json=body, timeout=10)
    r.raise_for_status()
    return r.json()["id"]


def delete_user_request(base_url, request_id):
    """Best-effort cleanup for post_user_request, mirroring the event/program
    deletion in run_window's `finally` block (and setup_persona_sessions'
    teardown()) -- never raises, a cleanup failure must not abort the run."""
    try:
        requests.delete(f"{base_url}/user-requests/{request_id}", timeout=10)
    except requests.RequestException as e:
        print(f"WARN: user-request cleanup {base_url}/{request_id}: {e}")


def _snapshot_local(out_dir, ven, ven_data_root):
    src = Path(ven_data_root) / ven / "history.sqlite"
    if not src.exists():
        print(f"WARN: no history store at {src}")
        return
    # WAL mode: recent rows live in the -wal sidecar until the daily
    # prune checkpoint — copy all three files so the snapshot opens
    # with the un-checkpointed data visible.
    for suffix in ("", "-wal", "-shm"):
        side = Path(str(src) + suffix)
        if side.exists():
            shutil.copy2(side, out_dir / f"{ven}-history.sqlite{suffix}")


def _snapshot_remote(out_dir, ven, ssh_host, remote_data_root):
    """Same-shape copy as `_snapshot_local` but the VEN's data dir lives on a
    different docker host (Node2) — pull the sqlite files over `ssh`/`scp`
    instead of a local filesystem copy. Node2 has no Postgres of its own, so
    only history.sqlite is remote; recorder CSVs always come from Node1's DB."""
    remote_src = f"{remote_data_root}/{ven}/history.sqlite"
    for suffix in ("", "-wal", "-shm"):
        remote_path = f"{remote_src}{suffix}"
        dest = out_dir / f"{ven}-history.sqlite{suffix}"
        res = subprocess.run(
            ["scp", "-q", f"{ssh_host}:{remote_path}", str(dest)],
            capture_output=True, text=True, timeout=30,
        )
        if res.returncode != 0:
            if suffix == "":
                print(f"WARN: no history store at {ssh_host}:{remote_path} ({res.stderr.strip()})")
            # -wal/-shm sidecars are optional (may not exist if nothing pending checkpoint)


# GB-26: reports_received/events_published grow unboundedly with deployment
# age (unfiltered dumps hit ~122 MB on this deployment's accumulated
# history), so every run's snapshot is filtered to its own [t_from, t_to)
# window via each table's own timestamp column. ven_snapshots is a
# PK-per-VEN "latest state" table, not an append log, so it's exempt --
# there's exactly one row per VEN regardless of window width.
_TABLE_TIME_COL = {"reports_received": "received_at", "events_published": "seen_at"}


def snapshot(out_dir, vens, pg_container, ven_data_root, fleet_map=None, pg_host="local",
             t_from=None, t_to=None):
    """Copy VEN sqlite stores + dump lab_recorder tables to CSV.

    `fleet_map` (parsed experiments/fleet_map.json's "vens" dict), when given,
    routes each VEN to a local filesystem copy or a remote scp pull depending
    on its "host" entry. Without it, every VEN is assumed local (unchanged
    Node1-only behavior). `pg_host`: "local" runs `docker exec` directly
    (script running on the VTN's own host); any other value is an ssh alias
    to run it on instead (script running off-host, e.g. from a workstation
    that can reach both docker hosts but isn't one of them).

    `t_from`/`t_to` (GB-26), when both given, restrict the reports_received
    and events_published dumps to that window instead of the whole table's
    history -- see _TABLE_TIME_COL above."""
    out_dir.mkdir(parents=True, exist_ok=True)
    for ven in vens:
        entry = (fleet_map or {}).get(ven, {"host": "local"})
        if entry["host"] == "local":
            _snapshot_local(out_dir, ven, ven_data_root)
        else:
            _snapshot_remote(out_dir, ven, entry["host"], entry["remote_data_root"])
    for table in ("reports_received", "events_published", "ven_snapshots"):
        time_col = _TABLE_TIME_COL.get(table)
        where = ""
        if time_col and t_from is not None and t_to is not None:
            where = f" WHERE {time_col} >= '{iso(t_from)}' AND {time_col} < '{iso(t_to)}'"
        copy_sql = f"COPY (SELECT * FROM lab_recorder.{table}{where}) TO STDOUT WITH CSV HEADER"
        if pg_host == "local":
            cmd = ["docker", "exec", pg_container, "psql", "-U", "openadr", "openadr", "-c", copy_sql]
        else:
            # ssh joins trailing argv with spaces before the remote shell parses it,
            # so the -c argument must be pre-quoted as a single shell token.
            remote_cmd = f"docker exec {pg_container} psql -U openadr openadr -c {shlex.quote(copy_sql)}"
            cmd = ["ssh", pg_host, remote_cmd]
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if res.returncode == 0:
            (out_dir / f"recorder-{table}.csv").write_text(res.stdout, encoding="utf-8")
        else:
            print(f"WARN: recorder dump {table} failed: {res.stderr.strip()}")


def _ven_base_url(entry):
    if entry["host"] == "local":
        return f"http://127.0.0.1:{entry['port']}"
    return f"http://{entry['lan_ip']}:{entry['port']}"


def fetch_plan_history(out_dir, vens, fleet_map, t_from, t_to):
    """GB-25: pull each VEN's `GET /history/plans?from=&to=` for the run
    window into `{ven}-plan-history.json` -- one row per plan cycle, with
    solver_ms/mip_gap_target/typed warning_kinds (see
    entities::history::PlanHistorySample and VEN_ARCHITECTURE.md §4.9a).
    This is kpi.py's primary source for plan_history_summary(); the
    poll_plan_diagnostics() poller below is retained only for live progress
    visibility during an unattended run, not as the analysis source of
    record. Gated on `fleet_map` (same as poll_plan_diagnostics) since only
    it provides per-VEN base URLs. Never aborts the run: any failure
    (request exception, non-200 including the route's own possible 503
    "history store disabled") prints a WARN and continues."""
    for ven in vens:
        entry = fleet_map.get(ven, {"host": "local", "port": None})
        if entry.get("port") is None:
            continue
        base = _ven_base_url(entry)
        try:
            r = requests.get(
                f"{base}/history/plans",
                params={"from": iso(t_from), "to": iso(t_to)},
                timeout=15,
            )
            r.raise_for_status()
            data = r.json()
        except requests.RequestException as e:
            print(f"WARN: plan-history fetch {ven}: {e}")
            continue
        (out_dir / f"{ven}-plan-history.json").write_text(
            json.dumps(data, indent=2), encoding="utf-8"
        )


def fetch_forecast_accuracy(out_dir, vens, fleet_map, t_from, t_to):
    """Same shape as fetch_plan_history, for `GET /history/forecast-accuracy`
    (entities::history::ForecastAccuracySample rows) -- no asset_id/lead_kind
    filter applied here; kpi.py's forecast_accuracy_summary() groups by
    (asset_id, lead_kind) itself. Never aborts the run on failure."""
    for ven in vens:
        entry = fleet_map.get(ven, {"host": "local", "port": None})
        if entry.get("port") is None:
            continue
        base = _ven_base_url(entry)
        try:
            r = requests.get(
                f"{base}/history/forecast-accuracy",
                params={"from": iso(t_from), "to": iso(t_to)},
                timeout=15,
            )
            r.raise_for_status()
            data = r.json()
        except requests.RequestException as e:
            print(f"WARN: forecast-accuracy fetch {ven}: {e}")
            continue
        (out_dir / f"{ven}-forecast-accuracy.json").write_text(
            json.dumps(data, indent=2), encoding="utf-8"
        )


def poll_plan_diagnostics(out_dir, vens, fleet_map, interval_s, stop_event):
    """Background poll loop (runs for the scenario's duration): every
    `interval_s`, GET /plan on each VEN and append the fields useful for
    judging plan *quality* (not just grid-power KPIs) as JSONL —
    solve_status, warnings, cost_breakdown (incl. c_violations_eur),
    objective/friction, penalty_rules_active. One file per VEN so kpi.py can
    summarize per VEN like it does for grid_samples.

    GB-25 superseded this as the analysis source of record: solver_ms,
    mip_gap_target, and typed warning_kinds are now available in bulk via
    `GET /history/plans` (see fetch_plan_history() above), fetched once per
    window instead of polled. This live poller is retained specifically for
    progress visibility during an unattended run (watching solve_status /
    warning trends as they happen), not as where kpi.py's plan-quality KPIs
    come from.
    """
    handles = {ven: open(out_dir / f"{ven}-plan-diagnostics.jsonl", "a", encoding="utf-8") for ven in vens}
    try:
        while not stop_event.is_set():
            for ven in vens:
                entry = fleet_map.get(ven, {"host": "local", "port": None})
                if entry.get("port") is None:
                    continue
                base = _ven_base_url(entry)
                try:
                    r = requests.get(f"{base}/plan", timeout=10)
                    r.raise_for_status()
                    plan = r.json()
                except requests.RequestException as e:
                    record = {"ts": iso(datetime.now(timezone.utc)), "ven": ven, "error": str(e)}
                else:
                    if plan is None:
                        # No plan cycle has completed yet for this VEN (e.g. just
                        # redeployed) -- GET /plan returns a bare `null`, not an
                        # HTTP error, so this isn't caught by the except above.
                        record = {"ts": iso(datetime.now(timezone.utc)), "ven": ven, "error": "no plan yet"}
                        handles[ven].write(json.dumps(record) + "\n")
                        handles[ven].flush()
                        continue
                    record = {
                        "ts": iso(datetime.now(timezone.utc)),
                        "ven": ven,
                        "solve_status": plan.get("solve_status"),
                        "warnings": plan.get("warnings"),
                        "cost_breakdown": plan.get("cost_breakdown"),
                        "objective_eur": plan.get("objective_eur"),
                        "friction_eur": plan.get("friction_eur"),
                        "penalty_rules_active": plan.get("penalty_rules_active"),
                    }
                handles[ven].write(json.dumps(record) + "\n")
                handles[ven].flush()
            stop_event.wait(interval_s)
    finally:
        for f in handles.values():
            f.close()


def _persona_departure(hour_utc, now):
    """Next occurrence of hour_utc (UTC), at least 2 h out so the deadline is plannable."""
    dep = now.replace(hour=hour_utc, minute=0, second=0, microsecond=0)
    while dep < now + timedelta(hours=2):
        dep += timedelta(days=1)
    return dep


def setup_persona_sessions(manifest_path, host):
    """WP4.5: give each fleet VEN its persona's EV session + comfort curve.
    Returns (ven_names, teardown) — call teardown() in the finally block.

    The per-device `/ev-session` CRUD API (BL-41) was retired in favour of the
    unified `POST /user-requests` (asset_id="ev") — only `GET /ev-session` (a
    read-only projection) survives. This creates via `/user-requests` and tears
    down via `DELETE /user-requests/:id` using the id `/user-requests` returns.
    """
    import sys
    sys.path.insert(0, str(REPO_ROOT / "scripts"))
    from personas import PERSONAS

    manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
    now = datetime.now(timezone.utc)
    created = []  # (base_url, request_id, had_curve)
    for ven in manifest["vens"]:
        persona = ven.get("persona")
        if not persona:
            continue
        preset = PERSONAS[persona]
        base = f"http://{host}:{ven['port']}"
        r = requests.post(
            f"{base}/assets/ev/comfort_curve", json=preset["comfort_curve"], timeout=10
        )
        curve_ok = r.status_code in (200, 201)
        if not curve_ok and r.status_code != 404:
            print(f"WARN: comfort curve for {ven['ven_name']}: {r.status_code}")
        dep_hour = preset["ev_departure_hour_utc"]
        dep = _persona_departure(dep_hour, now) if dep_hour is not None else now + timedelta(hours=8)
        body = {
            "asset_id": "ev",
            "target_soc": preset["ev_target_soc"],
            "deadlines": [{"latest_end": iso(dep)}],
            "mode": preset["ev_mode"],
        }
        if preset["ev_budget_eur"] is not None:
            body["budget_eur"] = preset["ev_budget_eur"]
        r = requests.post(f"{base}/user-requests", json=body, timeout=10)
        request_id = None
        if r.status_code not in (200, 201):
            print(f"WARN: user-request for {ven['ven_name']} ({persona}): {r.status_code} {r.text[:120]}")
        else:
            request_id = r.json().get("id")
            print(f"  persona {persona:<9} {ven['ven_name']}: mode={preset['ev_mode']}")
        created.append((base, request_id, curve_ok))

    def teardown():
        for base, request_id, had_curve in created:
            try:
                if request_id:
                    requests.delete(f"{base}/user-requests/{request_id}", timeout=10)
                if had_curve:
                    requests.delete(f"{base}/assets/ev/comfort_curve", timeout=10)
            except requests.RequestException as e:
                print(f"WARN: persona cleanup {base}: {e}")

    ven_names = [v["ven_name"] for v in manifest["vens"]]
    return ven_names, teardown


def run_window(args, ven_names, fleet_map, duration_min, run_dir, scenario_label, program_name, actions,
               report_descriptors=None, tier="realistic"):
    """Run one real-time window against the VTN: create a throwaway program,
    post `actions` (each translated via build_event) at their offsets, wait
    out the window, clean up, then snapshot. `actions=[]` runs a pure
    no-intervention window -- used by the GB-28 paired baseline below, and
    reusable for any future baseline-only scenario.

    `report_descriptors` (GB-27), when given, is attached to the first event
    posted so the VEN has something to report BASELINE/USAGE against. For a
    pure no-intervention window (`actions=[]`) there is no first action to
    attach it to, so one is synthesized: a SIMPLE level=0 event spanning the
    whole window -- level 0 is the spec's "normal operations" SIMPLE level
    (`docs/openadr_3_0_specs/2_OpenADR 3.0 Definition v3.0.1.md`), and the
    planner's SIMPLE-level handling (`milp_planner/inputs.rs`) treats any
    level other than 1/2/3 as the unrestricted contractual cap -- a genuine
    no-op for planning, unlike posting a real price/capacity/alert event.

    One action type, `budget_shortfall`, does not become a VTN event at all:
    it's posted straight to its `target_ven`'s own `/user-requests` (via
    post_user_request) and tracked/cleaned up separately from
    `created_events` -- see the action loop below. Requires `fleet_map`.

    Runs the plan-diagnostics poller (if `fleet_map` given) and writes
    `run.json` + the fleet snapshot into `run_dir`, exactly like the
    single-window flow this replaces.

    `run.json["actions"]` records each posted action's wall-clock start
    (`{"at_minute", "type", "started_at"}`), consumed by kpi.py's
    `compliance_latency_s` to measure signal-to-response time. `tier`
    (`"realistic"` or `"stress"`, from the scenario YAML) is passed straight
    through into `run.json["tier"]` so kpi.py/a report can group runs by it."""
    t0 = datetime.now(timezone.utc)

    diag_stop = None
    diag_thread = None
    if fleet_map:
        import threading
        run_dir.mkdir(parents=True, exist_ok=True)
        diag_stop = threading.Event()
        diag_thread = threading.Thread(
            target=poll_plan_diagnostics,
            args=(run_dir, ven_names, fleet_map, args.plan_poll_interval_s, diag_stop),
            daemon=True,
        )
        diag_thread.start()

    token = get_token(args.vtn_url, "any-business", "any-business")
    r = requests.post(
        f"{args.vtn_url}/programs",
        headers=auth(token),
        json={"programName": program_name},
        timeout=10,
    )
    r.raise_for_status()
    program_id = r.json()["id"]
    created_events = []
    created_requests = []  # (base_url, request_id) — budget_shortfall's /user-requests, not VTN events
    actions_log = []  # {"at_minute", "type", "started_at"} — see run.json["actions"] in the docstring

    try:
        pending = sorted(actions, key=lambda a: a["at_minute"])
        if not pending and report_descriptors:
            noop = {"type": "simple", "level": 0, "duration_minutes": duration_min, "at_minute": 0}
            body = build_event(program_id, noop, t0, report_descriptors)
            eid = post_event(args.vtn_url, token, body)
            created_events.append(eid)
            print(f"  +  0 min  report-only (SIMPLE level=0)  event={eid}")
        for i, action in enumerate(pending):
            target = t0 + timedelta(minutes=action["at_minute"])
            wait_s = (target - datetime.now(timezone.utc)).total_seconds()
            if wait_s > 0:
                time.sleep(wait_s)

            if action["type"] == "budget_shortfall":
                # Bypasses the VTN entirely — direct POST to the target VEN's
                # own /user-requests, deliberately under-budgeted so its MILP
                # planner raises a real BudgetShortfall warning.
                target_ven = action["target_ven"]
                entry = (fleet_map or {}).get(target_ven)
                if entry is None or entry.get("port") is None:
                    print(f"WARN: budget_shortfall target_ven={target_ven} not in --fleet-map — skipped")
                    continue
                base = _ven_base_url(entry)
                now_a = datetime.now(timezone.utc)
                window_end = t0 + timedelta(minutes=duration_min)
                action_end = now_a + timedelta(minutes=action.get("duration_minutes", duration_min))
                # A few minutes before the scenario window closes, whichever is sooner.
                latest_end = min(action_end, window_end - timedelta(minutes=2))
                body = {
                    "asset_id": "ev",
                    # MAX_COST mode is what actually routes budget_eur into the EV
                    # MILP context (services/user_request.rs create_ev ->
                    # assets/ev_milp.rs) and triggers budget_warning in
                    # milp_planner/inputs.rs — target_soc (not target_energy_kwh)
                    # drives the resulting core-energy shortfall computation there.
                    "target_soc": action.get("target_soc", 0.8),
                    "mode": "MAX_COST",
                    "budget_eur": action["budget_eur"],
                    "deadlines": [{"latest_end": iso(latest_end)}],
                }
                rid = post_user_request(base, body)
                created_requests.append((base, rid))
                actions_log.append({
                    "at_minute": action["at_minute"], "type": action["type"],
                    "started_at": iso(now_a),
                })
                print(f"  +{action['at_minute']:>3} min  budget_shortfall  ven={target_ven}  request={rid}")
                continue

            descriptors = report_descriptors if i == 0 else None
            started_at = datetime.now(timezone.utc)
            body = build_event(program_id, action, started_at, descriptors)
            eid = post_event(args.vtn_url, token, body)
            created_events.append(eid)
            actions_log.append({
                "at_minute": action["at_minute"], "type": action["type"],
                "started_at": iso(started_at),
            })
            print(f"  +{action['at_minute']:>3} min  {action['type']}  event={eid}")

        end = t0 + timedelta(minutes=duration_min)
        wait_s = (end - datetime.now(timezone.utc)).total_seconds()
        if wait_s > 0:
            print(f"  waiting out the window ({int(wait_s)}s remaining) ...")
            time.sleep(wait_s)
    finally:
        # Deletion == cancellation in OpenADR 3; always clean up.
        token = get_token(args.vtn_url, "any-business", "any-business")
        for eid in created_events:
            requests.delete(f"{args.vtn_url}/events/{eid}", headers=auth(token), timeout=10)
        requests.delete(f"{args.vtn_url}/programs/{program_id}", headers=auth(token), timeout=10)
        for base, rid in created_requests:
            delete_user_request(base, rid)
        if diag_stop:
            diag_stop.set()
            diag_thread.join(timeout=args.plan_poll_interval_s + 10)

    t_end = datetime.now(timezone.utc)
    snapshot(run_dir, ven_names, args.pg_container, args.ven_data_root, fleet_map, args.pg_host,
             t_from=t0, t_to=t_end)
    if fleet_map:
        fetch_plan_history(run_dir, ven_names, fleet_map, t0, t_end)
        fetch_forecast_accuracy(run_dir, ven_names, fleet_map, t0, t_end)
    meta = {
        "scenario": scenario_label,
        "started_at": iso(t0),
        "duration_minutes": duration_min,
        "vens": ven_names,
        "events": created_events,
        "actions": actions_log,
        "tier": tier,
    }
    (run_dir / "run.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenario", required=True)
    p.add_argument("--vtn-url", default="http://localhost:8200")
    p.add_argument("--vens", default="ven-1,ven-2,ven-3", help="comma-separated VEN data dirs to snapshot")
    p.add_argument("--ven-data-root", default=str(REPO_ROOT / "VEN" / "data"))
    p.add_argument("--pg-container", default="vtn-db-1")
    p.add_argument(
        "--pg-host", default="local",
        help="ssh alias to run the recorder-DB dump on, when this script itself runs "
             "off the VTN's host (e.g. \"Node1\"); \"local\" runs docker exec directly",
    )
    p.add_argument("--out", default=str(REPO_ROOT / "experiments" / "results"))
    p.add_argument(
        "--personas",
        action="store_true",
        help="WP4.5: create each fleet VEN's persona EV session + comfort curve "
             "from VEN/fleet/manifest.json before the scenario, remove them after",
    )
    p.add_argument("--fleet-manifest", default=str(REPO_ROOT / "VEN" / "fleet" / "manifest.json"))
    p.add_argument("--fleet-host", default="localhost")
    p.add_argument(
        "--fleet-map",
        help="experiments/fleet_map.json — host/port for each VEN. When given, --vens "
             "defaults to every VEN in the map (all 13) instead of ven-1,ven-2,ven-3, "
             "remote (Node2) VENs are snapshotted over scp instead of local file copy, "
             "and GET /plan is polled on every VEN for the run's duration.",
    )
    p.add_argument("--plan-poll-interval-s", type=int, default=60)
    p.add_argument(
        "--paired-baseline", action=argparse.BooleanOptionalAction, default=True,
        help="GB-28: run a same-duration, no-event window immediately before the "
             "scenario (same VENs), snapshotted to {run_dir}-baseline/, so kpi.py's "
             "energy_shifted_kwh compares against a baseline captured minutes -- not "
             "hours -- away in wall-clock time instead of a separately-run S-1. "
             "Roughly doubles this script's total runtime. Use --no-paired-baseline "
             "for quick dry runs or scenarios that are themselves baseline-only.",
    )
    p.add_argument(
        "--request-reports", action=argparse.BooleanOptionalAction, default=True,
        help="GB-27: attach a reportDescriptors array (BASELINE + USAGE, 300s frequency) "
             "to the first event of every window (including the paired baseline's "
             "synthetic SIMPLE level=0 event), so report_lag_stats/event_impact_kwh in "
             "kpi.py actually get data instead of coming back null. Use "
             "--no-request-reports to skip.",
    )
    p.add_argument(
        "--start-at",
        help="ISO 8601 UTC instant (e.g. 2026-08-20T22:00:00Z) to sleep until before "
             "the *scenario* window's actions start (the paired baseline, if any, still "
             "runs immediately). For scenarios whose behaviour depends on real solar "
             "time (e.g. a diurnal price curve or an export-capacity test that needs to "
             "overlap actual peak PV output), this is how the run is anchored to local "
             "midnight / pre-solar-noon rather than whenever the script happens to be "
             "launched. Errors immediately if the timestamp is already in the past.",
    )
    args = p.parse_args()

    start_at = None
    if args.start_at:
        start_at = datetime.strptime(args.start_at, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
        if start_at <= datetime.now(timezone.utc):
            p.error(f"--start-at {args.start_at} is already in the past")

    report_descriptors = REPORT_DESCRIPTORS if args.request_reports else None

    fleet_map = None
    if args.fleet_map:
        fleet_map = json.loads(Path(args.fleet_map).read_text(encoding="utf-8"))["vens"]

    scenario = yaml.safe_load(Path(args.scenario).read_text(encoding="utf-8"))
    name = scenario["name"]
    duration_min = scenario["duration_minutes"]
    t0 = datetime.now(timezone.utc)
    run_dir = Path(args.out) / f"{t0.strftime('%Y%m%d-%H%M')}-{name}"

    vens_explicit = "--vens" in sys.argv
    if fleet_map and not vens_explicit:
        ven_names = sorted(fleet_map.keys(), key=lambda v: int(v.split("-")[1]))
    else:
        ven_names = args.vens.split(",")

    persona_teardown = None
    if args.personas:
        fleet_names, persona_teardown = setup_persona_sessions(args.fleet_manifest, args.fleet_host)
        ven_names = sorted(set(ven_names) | set(fleet_names))

    try:
        baseline_dir = None
        if args.paired_baseline:
            baseline_dir = Path(str(run_dir) + "-baseline")
            print(f"=== paired baseline for {name}: {duration_min} min, no events ===")
            run_window(
                args, ven_names, fleet_map, duration_min, baseline_dir,
                scenario_label=f"{name}-baseline",
                program_name=f"exp-{name}-baseline-{datetime.now(timezone.utc).strftime('%H%M%S')}",
                actions=[],
                report_descriptors=report_descriptors,
                tier=scenario.get("tier", "realistic"),
            )

        if start_at:
            wait_s = (start_at - datetime.now(timezone.utc)).total_seconds()
            if wait_s > 0:
                print(f"  waiting for --start-at {args.start_at} ({int(wait_s)}s) ...")
                time.sleep(wait_s)

        print(f"=== scenario {name}: {scenario.get('description', '')} ({duration_min} min) ===")
        run_window(
            args, ven_names, fleet_map, duration_min, run_dir,
            scenario_label=name,
            program_name=f"exp-{name}-{datetime.now(timezone.utc).strftime('%H%M%S')}",
            actions=scenario["actions"],
            report_descriptors=report_descriptors,
            tier=scenario.get("tier", "realistic"),
        )
    finally:
        if persona_teardown:
            persona_teardown()

    print(f"=== snapshot written to {run_dir} ===")
    baseline_hint = str(baseline_dir) if baseline_dir else "<s1 run dir>"
    print(f"Next: python3 experiments/kpi.py --run {run_dir} [--baseline {baseline_hint}]")


if __name__ == "__main__":
    main()
