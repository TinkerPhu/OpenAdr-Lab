#!/usr/bin/env python3
"""WP3.8 (A-3) — run one control-method scenario against the live stack and
snapshot both data stores for KPI extraction.

Scenarios run in REAL TIME: the sim clock is wall time (tick_once stamps
Utc::now and event windows are absolute), so time acceleration is not
externally drivable without an injectable clock through the whole tick/poll
path — the spike result from the phase plan. S-1..S-6 are therefore short
same-day windows (default 30 min each) rather than simulated days.

Runs ON the docker host (Pi4), same convention as fleet.sh:
    python3 experiments/run_experiment.py --scenario experiments/scenarios/s2_price_spike.yaml
    ... --vens ven-1,ven-2,ven-3            # which VEN data dirs to snapshot
    ... --out experiments/results           # output root

Steps: create a program, replay the scenario's actions at their offsets,
wait out the window, delete the created events/program, then snapshot each
VEN's history.sqlite plus the lab_recorder tables (CSV via psql in the
vtn-db container).
"""

import argparse
import json
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


def build_event(program_id, action, start):
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
        return {"programID": program_id, "eventName": "exp-price", "intervals": intervals}

    window = {
        "start": iso(start),
        "duration": f"PT{action['duration_minutes']}M",
    }
    payload = {
        "capacity_limit": ("IMPORT_CAPACITY_LIMIT", action.get("import_kw")),
        "capacity_reservation": ("IMPORT_CAPACITY_RESERVATION", action.get("import_kw")),
        "alert": (action.get("alert_type", "ALERT_GRID_EMERGENCY"), "experiment alert"),
        "simple": ("SIMPLE", action.get("level")),
        "dispatch": ("DISPATCH_SETPOINT", action.get("setpoint_kw")),
    }[t]
    return {
        "programID": program_id,
        "eventName": f"exp-{t.replace('_', '-')}",
        "intervalPeriod": window,
        "intervals": [{"id": 0, "payloads": [{"type": payload[0], "values": [payload[1]]}]}],
    }


def snapshot(out_dir, vens, pg_container, ven_data_root):
    """Copy VEN sqlite stores + dump lab_recorder tables to CSV."""
    out_dir.mkdir(parents=True, exist_ok=True)
    for ven in vens:
        src = Path(ven_data_root) / ven / "history.sqlite"
        if src.exists():
            # WAL mode: recent rows live in the -wal sidecar until the daily
            # prune checkpoint — copy all three files so the snapshot opens
            # with the un-checkpointed data visible.
            for suffix in ("", "-wal", "-shm"):
                side = Path(str(src) + suffix)
                if side.exists():
                    shutil.copy2(side, out_dir / f"{ven}-history.sqlite{suffix}")
        else:
            print(f"WARN: no history store at {src}")
    for table in ("reports_received", "events_published", "ven_snapshots"):
        cmd = [
            "docker", "exec", pg_container, "psql", "-U", "openadr", "openadr",
            "-c", f"COPY (SELECT * FROM lab_recorder.{table}) TO STDOUT WITH CSV HEADER",
        ]
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        if res.returncode == 0:
            (out_dir / f"recorder-{table}.csv").write_text(res.stdout, encoding="utf-8")
        else:
            print(f"WARN: recorder dump {table} failed: {res.stderr.strip()}")


def _persona_departure(hour_utc, now):
    """Next occurrence of hour_utc (UTC), at least 2 h out so the deadline is plannable."""
    dep = now.replace(hour=hour_utc, minute=0, second=0, microsecond=0)
    while dep < now + timedelta(hours=2):
        dep += timedelta(days=1)
    return dep


def setup_persona_sessions(manifest_path, host):
    """WP4.5: give each fleet VEN its persona's EV session + comfort curve.
    Returns (ven_names, teardown) — call teardown() in the finally block."""
    import sys
    sys.path.insert(0, str(REPO_ROOT / "scripts"))
    from personas import PERSONAS

    manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
    now = datetime.now(timezone.utc)
    created = []  # (base_url, had_curve)
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
            "target_soc": preset["ev_target_soc"],
            "departure_time": iso(dep),
            "mode": preset["ev_mode"],
        }
        if preset["ev_budget_eur"] is not None:
            body["budget_eur"] = preset["ev_budget_eur"]
        r = requests.post(f"{base}/ev-session", json=body, timeout=10)
        if r.status_code not in (200, 201):
            print(f"WARN: ev-session for {ven['ven_name']} ({persona}): {r.status_code} {r.text[:120]}")
        else:
            print(f"  persona {persona:<9} {ven['ven_name']}: mode={preset['ev_mode']}")
        created.append((base, curve_ok))

    def teardown():
        for base, had_curve in created:
            try:
                requests.delete(f"{base}/ev-session", timeout=10)
                if had_curve:
                    requests.delete(f"{base}/assets/ev/comfort_curve", timeout=10)
            except requests.RequestException as e:
                print(f"WARN: persona cleanup {base}: {e}")

    ven_names = [v["ven_name"] for v in manifest["vens"]]
    return ven_names, teardown


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenario", required=True)
    p.add_argument("--vtn-url", default="http://localhost:8200")
    p.add_argument("--vens", default="ven-1,ven-2,ven-3", help="comma-separated VEN data dirs to snapshot")
    p.add_argument("--ven-data-root", default=str(REPO_ROOT / "VEN" / "data"))
    p.add_argument("--pg-container", default="vtn-db-1")
    p.add_argument("--out", default=str(REPO_ROOT / "experiments" / "results"))
    p.add_argument(
        "--personas",
        action="store_true",
        help="WP4.5: create each fleet VEN's persona EV session + comfort curve "
             "from VEN/fleet/manifest.json before the scenario, remove them after",
    )
    p.add_argument("--fleet-manifest", default=str(REPO_ROOT / "VEN" / "fleet" / "manifest.json"))
    p.add_argument("--fleet-host", default="localhost")
    args = p.parse_args()

    scenario = yaml.safe_load(Path(args.scenario).read_text(encoding="utf-8"))
    name = scenario["name"]
    duration_min = scenario["duration_minutes"]
    t0 = datetime.now(timezone.utc)
    run_dir = Path(args.out) / f"{t0.strftime('%Y%m%d-%H%M')}-{name}"
    print(f"=== scenario {name}: {scenario.get('description', '')} ({duration_min} min) ===")

    persona_teardown = None
    ven_names = args.vens.split(",")
    if args.personas:
        fleet_names, persona_teardown = setup_persona_sessions(args.fleet_manifest, args.fleet_host)
        ven_names = sorted(set(ven_names) | set(fleet_names))

    token = get_token(args.vtn_url, "any-business", "any-business")
    r = requests.post(
        f"{args.vtn_url}/programs",
        headers=auth(token),
        json={"programName": f"exp-{name}-{t0.strftime('%H%M%S')}"},
        timeout=10,
    )
    r.raise_for_status()
    program_id = r.json()["id"]
    created_events = []

    try:
        pending = sorted(scenario["actions"], key=lambda a: a["at_minute"])
        for action in pending:
            target = t0 + timedelta(minutes=action["at_minute"])
            wait_s = (target - datetime.now(timezone.utc)).total_seconds()
            if wait_s > 0:
                time.sleep(wait_s)
            body = build_event(program_id, action, datetime.now(timezone.utc))
            eid = post_event(args.vtn_url, token, body)
            created_events.append(eid)
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
        if persona_teardown:
            persona_teardown()

    snapshot(run_dir, ven_names, args.pg_container, args.ven_data_root)
    meta = {
        "scenario": name,
        "started_at": iso(t0),
        "duration_minutes": duration_min,
        "vens": ven_names,
        "events": created_events,
        "personas": args.personas,
    }
    (run_dir / "run.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"=== snapshot written to {run_dir} ===")
    print(f"Next: python3 experiments/kpi.py --run {run_dir} [--baseline <s1 run dir>]")


if __name__ == "__main__":
    main()
