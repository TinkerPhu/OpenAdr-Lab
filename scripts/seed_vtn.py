#!/usr/bin/env python3
"""Seed the VTN with demo programs and events for all 8 use cases.

Also provisions ven-1, ven-2, and ven-3 via API if they don't exist yet
(GB-02/GB-03: uniform "ven-N" venName + VTN-issued UUID id for all three,
no special case for ven-1). The SQL fixture test_user_credentials.sql still
seeds the ven-manager/user-manager/business users this script authenticates
as, but no longer seeds ven-1 itself.

Usage:
    python3 seed_vtn.py --vtn-url http://localhost:8200
    python3 seed_vtn.py --vtn-url http://localhost:8200 --demo-cancel
    python3 seed_vtn.py --vtn-url http://localhost:8200 --skip-provision
"""

import argparse
import sys
import time
from datetime import datetime, timedelta, timezone

import requests

# ── Demo data ────────────────────────────────────────────────────────────────

PROGRAMS = [
    {
        "programName": "Summer Peak DR",
        "programLongName": "Summer Peak Demand Response Program",
        "programType": "DEMAND_RESPONSE",
        "targets": [
            {"type": "VEN_NAME", "values": ["ven-1"]},
            {"type": "VEN_NAME", "values": ["ven-2"]},
        ],
    },
    {
        "programName": "EV Managed Charging",
        "programLongName": "Electric Vehicle Managed Charging Program",
        "programType": "LOAD_SHIFTING",
        "targets": [
            {"type": "VEN_NAME", "values": ["ven-2"]},
            {"type": "VEN_NAME", "values": ["ven-3"]},
        ],
    },
    {
        "programName": "HVAC Optimization",
        "programLongName": "Building HVAC Pre-Cool/Pre-Heat Optimization",
        "programType": "OPTIMIZATION",
        "targets": None,  # open — visible to all VENs
    },
]


def build_events():
    """Build the EVENTS dict with realistic timing based on current time."""
    now = datetime.now(timezone.utc)
    tomorrow_14 = (now + timedelta(days=1)).replace(hour=14, minute=0, second=0, microsecond=0)
    midnight = (now + timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)

    return {
        "Summer Peak DR": [
            # UC1: Emergency Load Shed — max priority, starts soon, 30min
            {
                "eventName": "emergency-load-shed",
                "priority": 0,
                "intervalPeriod": {
                    "start": (now + timedelta(minutes=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT30M",
                },
                "targets": [{"type": "VEN_NAME", "values": ["ven-1"]}],
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [{"type": "SIMPLE", "values": [0]}],
                    },
                ],
            },
            # UC4: Peak Shaving — moderate priority, tomorrow afternoon, 4 hours
            {
                "eventName": "peak-shave-afternoon",
                "priority": 3,
                "intervalPeriod": {
                    "start": tomorrow_14.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT4H",
                },
                "targets": [
                    {"type": "VEN_NAME", "values": ["ven-1"]},
                    {"type": "VEN_NAME", "values": ["ven-2"]},
                ],
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [50.0]}],
                    },
                ],
            },
        ],
        "EV Managed Charging": [
            # UC2: Export Limitation — 3 intervals (ramp-down/hold/ramp-up)
            {
                "eventName": "export-limit-rampdown",
                "priority": 5,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT1H",
                },
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {"start": (now + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT20M"},
                        "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}],
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {"start": (now + timedelta(hours=1, minutes=20)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT20M"},
                        "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [50.0]}],
                    },
                    {
                        "id": 2,
                        "intervalPeriod": {"start": (now + timedelta(hours=1, minutes=40)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT20M"},
                        "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}],
                    },
                ],
            },
            # UC5: EV Charge Pause — 2 intervals (pause/resume)
            # Event targets only ven-2 (not ven-3) to demonstrate two-layer filtering:
            # program enrollment (ven-2 + ven-3) vs event targeting (ven-2 only)
            {
                "eventName": "ev-charge-pause",
                "priority": 2,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT2H",
                },
                "targets": [
                    {"type": "VEN_NAME", "values": ["ven-2"]},
                ],
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {"start": (now + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT1H"},
                        "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [0.0]}],
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {"start": (now + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT1H"},
                        "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [7.4]}],
                    },
                ],
            },
        ],
        "HVAC Optimization": [
            # UC3: Dynamic Pricing — 24 hourly intervals, day-ahead
            {
                "eventName": "tou-pricing-day-ahead",
                "priority": None,
                "intervalPeriod": {
                    "start": midnight.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "P9999Y",
                },
                "intervals": [
                    {
                        "id": h,
                        "intervalPeriod": {
                            "start": (midnight + timedelta(hours=h)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                            "duration": "PT1H",
                        },
                        "payloads": [{"type": "PRICE", "values": [p]}],
                    }
                    for h, p in enumerate([
                        0.08, 0.07, 0.06, 0.06, 0.07, 0.09,   # 00-05: off-peak
                        0.12, 0.18, 0.25, 0.22, 0.15, 0.14,   # 06-11: morning ramp
                        0.13, 0.14, 0.20, 0.28, 0.35, 0.40,   # 12-17: afternoon peak
                        0.38, 0.30, 0.20, 0.14, 0.10, 0.08,   # 18-23: evening wind-down
                    ])
                ],
            },
            # UC6: Battery Dispatch — 3 irregular intervals (charge/idle/discharge)
            {
                "eventName": "battery-dispatch-cycle",
                "priority": 4,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT5H",
                },
                "targets": [{"type": "VEN_NAME", "values": ["ven-1"]}],
                "intervals": [
                    {
                        "id": 0,
                        "intervalPeriod": {"start": (now + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT2H"},
                        "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [80.0]}],
                    },
                    {
                        "id": 1,
                        "intervalPeriod": {"start": (now + timedelta(hours=5)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT1H"},
                        "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [0.0]}],
                    },
                    {
                        "id": 2,
                        "intervalPeriod": {"start": (now + timedelta(hours=6)).strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT2H"},
                        "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [-50.0]}],
                    },
                ],
            },
            # UC7: Connectivity Check — no timing, simple no-op
            {
                "eventName": "connectivity-check",
                "priority": None,
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [{"type": "SIMPLE", "values": [0]}],
                    },
                ],
            },
            # Export tariff — 24 hourly EXPORT_PRICE intervals, repeats forever
            # Values are ~75% of the import tariff (retailer/DSO captures the spread)
            {
                "eventName": "tou-export-pricing-day-ahead",
                "priority": None,
                "intervalPeriod": {
                    "start": midnight.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "P9999Y",
                },
                "intervals": [
                    {
                        "id": h,
                        "intervalPeriod": {
                            "start": (midnight + timedelta(hours=h)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                            "duration": "PT1H",
                        },
                        "payloads": [{"type": "EXPORT_PRICE", "values": [p]}],
                    }
                    for h, p in enumerate([
                        0.06, 0.05, 0.04, 0.04, 0.05, 0.07,   # 00-05: off-peak
                        0.09, 0.14, 0.19, 0.17, 0.11, 0.11,   # 06-11: morning ramp
                        0.10, 0.11, 0.15, 0.21, 0.26, 0.30,   # 12-17: afternoon peak
                        0.29, 0.23, 0.15, 0.11, 0.08, 0.06,   # 18-23: evening wind-down
                    ])
                ],
            },
            # GHG intensity — 24 hourly GHG intervals, repeats forever
            # German-style diurnal carbon intensity (gCO2/kWh):
            # night wind dip → morning gas ramp → solar noon low → evening peak high
            {
                "eventName": "tou-ghg-intensity-day-ahead",
                "priority": None,
                "intervalPeriod": {
                    "start": midnight.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "P9999Y",
                },
                "intervals": [
                    {
                        "id": h,
                        "intervalPeriod": {
                            "start": (midnight + timedelta(hours=h)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                            "duration": "PT1H",
                        },
                        "payloads": [{"type": "GHG", "values": [p]}],
                    }
                    for h, p in enumerate([
                        280, 260, 250, 240, 250, 270,   # 00-05: wind overnight, coal baseload
                        320, 380, 400, 370, 320, 270,   # 06-11: gas ramp-up, solar rising
                        220, 210, 230, 290, 360, 430,   # 12-17: solar noon low, evening gas peak
                        450, 440, 420, 390, 350, 310,   # 18-23: evening demand, wind recovering
                    ])
                ],
            },
            # UC8: Cancel Demo Event — will be created then deleted with --demo-cancel
            {
                "eventName": "cancel-demo-event",
                "priority": None,
                "targets": [{"type": "VEN_NAME", "values": ["ven-1"]}],
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [{"type": "SIMPLE", "values": [1]}],
                    },
                ],
            },
        ],
    }


# ── VENs to provision via API (GB-02/GB-03: uniform pattern, no special case
# for ven-1 — see the note in vtn_setup_from_blog_step_by_step.md about
# clearing the fixture's legacy ven-1 rows before running this) ─────────────

VENS_TO_PROVISION = [
    {"ven_name": "ven-1", "client_id": "ven-1", "client_secret": "ven-1", "user_ref": "ven-1-user"},
    {"ven_name": "ven-2", "client_id": "ven-2", "client_secret": "ven-2", "user_ref": "ven-2-user"},
    {"ven_name": "ven-3", "client_id": "ven-3", "client_secret": "ven-3", "user_ref": "ven-3-user"},
]


# ── Helpers ──────────────────────────────────────────────────────────────────

def get_token(base_url, client_id, client_secret):
    r = requests.post(
        f"{base_url}/auth/token",
        data={
            "grant_type": "client_credentials",
            "client_id": client_id,
            "client_secret": client_secret,
        },
        timeout=10,
    )
    r.raise_for_status()
    return r.json()["access_token"]


def auth_headers(token):
    return {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    }


def list_programs(base_url, token):
    r = requests.get(f"{base_url}/programs", headers=auth_headers(token), timeout=10)
    r.raise_for_status()
    return r.json()


def create_program(base_url, token, prog):
    body = {"programName": prog["programName"], "intervalPeriod": None, "programDescriptions": None}
    if prog.get("programLongName"):
        body["programLongName"] = prog["programLongName"]
    if prog.get("programType"):
        body["programType"] = prog["programType"]
    if prog.get("targets"):
        body["targets"] = prog["targets"]
    r = requests.post(
        f"{base_url}/programs",
        headers=auth_headers(token),
        json=body,
        timeout=10,
    )
    r.raise_for_status()
    return r.json()


def update_program(base_url, token, program_id, prog):
    """PUT targets/metadata onto an existing program (idempotent re-runs)."""
    body = {"programName": prog["programName"]}
    if prog.get("programLongName"):
        body["programLongName"] = prog["programLongName"]
    if prog.get("programType"):
        body["programType"] = prog["programType"]
    body["targets"] = prog.get("targets")  # None clears targets (open program)
    r = requests.put(
        f"{base_url}/programs/{program_id}",
        headers=auth_headers(token),
        json=body,
        timeout=10,
    )
    r.raise_for_status()
    return r.json()


def list_events(base_url, token):
    r = requests.get(f"{base_url}/events", headers=auth_headers(token), timeout=10)
    r.raise_for_status()
    return r.json()


def create_event(base_url, token, program_id, evt):
    """Create an event with full OpenADR fields."""
    body = {
        "programID": program_id,
        "eventName": evt["eventName"],
        "intervals": evt["intervals"],
    }
    if evt.get("priority") is not None:
        body["priority"] = evt["priority"]
    if evt.get("intervalPeriod"):
        body["intervalPeriod"] = evt["intervalPeriod"]
    if evt.get("targets"):
        body["targets"] = evt["targets"]
    r = requests.post(
        f"{base_url}/events",
        headers=auth_headers(token),
        json=body,
        timeout=10,
    )
    r.raise_for_status()
    return r.json()


def list_reports(base_url, token):
    r = requests.get(f"{base_url}/reports", headers=auth_headers(token), timeout=10)
    r.raise_for_status()
    return r.json()


def delete_report(base_url, token, report_id):
    r = requests.delete(
        f"{base_url}/reports/{report_id}",
        headers=auth_headers(token),
        timeout=10,
    )
    r.raise_for_status()


def delete_event(base_url, token, event_id):
    r = requests.delete(
        f"{base_url}/events/{event_id}",
        headers=auth_headers(token),
        timeout=10,
    )
    r.raise_for_status()


def provision_vens(base, vens):
    """Provision VEN users, credentials, and VEN entities via API. Idempotent."""
    um_token = get_token(base, "user-manager", "user-manager")
    vm_token = get_token(base, "ven-manager", "ven-manager")

    for ven in vens:
        # Check if already provisioned by testing the credentials
        r = requests.post(
            f"{base}/auth/token",
            data={"grant_type": "client_credentials", "client_id": ven["client_id"], "client_secret": ven["client_secret"]},
            timeout=10,
        )
        if r.ok:
            print(f"VEN '{ven['ven_name']}' already provisioned — skipping.")
            continue

        print(f"Provisioning VEN '{ven['ven_name']}' ...")

        r = requests.post(f"{base}/users", headers=auth_headers(um_token),
                          json={"reference": ven["user_ref"], "description": f"VEN {ven['ven_name']}", "roles": []}, timeout=10)
        r.raise_for_status()
        user_id = r.json()["id"]

        r = requests.post(f"{base}/users/{user_id}", headers=auth_headers(um_token),
                          json={"client_id": ven["client_id"], "client_secret": ven["client_secret"]}, timeout=10)
        r.raise_for_status()

        ven_body = {"venName": ven["ven_name"]}
        # WP4.5: persona tag as an OpenADR VEN attribute so the UI dropdown
        # can label fleet entries (only present on persona fleets).
        if ven.get("persona"):
            ven_body["attributes"] = [{"type": "PERSONA", "values": [ven["persona"]]}]
        r = requests.post(f"{base}/vens", headers=auth_headers(vm_token),
                          json=ven_body, timeout=10)
        r.raise_for_status()
        ven_id = r.json()["id"]

        r = requests.put(f"{base}/users/{user_id}", headers=auth_headers(um_token),
                         json={"reference": ven["user_ref"], "description": f"VEN {ven['ven_name']}",
                               "roles": [{"role": "VEN", "id": ven_id}]}, timeout=10)
        r.raise_for_status()
        print(f"  '{ven['ven_name']}' provisioned (user={user_id}, ven={ven_id})")


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Seed the VTN with demo programs and events")
    parser.add_argument("--vtn-url", default="http://localhost:8200", help="VTN base URL")
    parser.add_argument("--client-id", default="any-business", help="OAuth client ID")
    parser.add_argument("--client-secret", default="any-business", help="OAuth client secret")
    parser.add_argument("--demo-cancel", action="store_true", help="Demo UC8: create then delete cancel-demo-event")
    parser.add_argument("--skip-provision", action="store_true", help="Skip VEN provisioning (e.g. test stack handles it separately)")
    args = parser.parse_args()

    base = args.vtn_url.rstrip("/")
    events_data = build_events()

    # Provision VENs before creating programs (programs with VEN_NAME targets
    # require those VEN entities to already exist in the VTN)
    if not args.skip_provision:
        provision_vens(base, VENS_TO_PROVISION)
        print()

    # Authenticate
    print(f"Authenticating as {args.client_id} at {base} ...")
    token = get_token(base, args.client_id, args.client_secret)
    print("  OK\n")

    # Check existing programs to allow idempotent re-runs
    existing = list_programs(base, token)
    existing_names = {p["programName"]: p["id"] for p in existing}

    # Create programs (or update targets on existing)
    program_ids = {}  # programName -> id
    for prog in PROGRAMS:
        name = prog["programName"]
        if name in existing_names:
            program_ids[name] = existing_names[name]
            update_program(base, token, program_ids[name], prog)
            print(f"Program '{name}' already exists — updated targets  id={program_ids[name]}")
        else:
            body = create_program(base, token, prog)
            program_ids[name] = body["id"]
            print(f"Created program '{name}'  id={program_ids[name]}")

    print()

    # Build set of (programID, eventName) keys that belong to the seed data.
    # Only these will be deleted on re-run — user-created events are preserved.
    seed_keys = set()
    for prog_name, events in events_data.items():
        prog_id = program_ids[prog_name]
        for evt in events:
            seed_keys.add((prog_id, evt["eventName"]))

    # Delete existing seed events (removes stale timings before recreating)
    existing_events = list_events(base, token)
    seed_event_ids = {
        ex["id"] for ex in existing_events
        if (ex["programID"], ex["eventName"]) in seed_keys
    }

    # Delete reports that reference seed events first (FK constraint)
    if seed_event_ids:
        all_reports = list_reports(base, token)
        for rpt in all_reports:
            if rpt.get("eventID") in seed_event_ids:
                delete_report(base, token, rpt["id"])
                print(f"Deleted report '{rpt.get('clientName', '?')}'  id={rpt['id']}")

    deleted_events = 0
    for ex in existing_events:
        if ex["id"] in seed_event_ids:
            delete_event(base, token, ex["id"])
            print(f"Deleted stale event '{ex['eventName']}'  id={ex['id']}")
            deleted_events += 1

    if deleted_events:
        print()

    # Create events with fresh timings
    created_events = 0
    cancel_event_id = None
    for prog_name, events in events_data.items():
        prog_id = program_ids[prog_name]
        for evt in events:
            body = create_event(base, token, prog_id, evt)
            print(f"Created event '{evt['eventName']}' for '{prog_name}'  id={body['id']}")
            created_events += 1
            if evt["eventName"] == "cancel-demo-event":
                cancel_event_id = body["id"]

    # Summary
    print(f"\nDone: {len(program_ids)} programs, {deleted_events} old seed events removed, {created_events} events created")

    # UC8: Demo cancellation
    if args.demo_cancel and cancel_event_id:
        print(f"\n--- UC8: Event Cancellation Demo ---")
        print(f"Event 'cancel-demo-event' exists with id={cancel_event_id}")
        print(f"Waiting 5 seconds for VENs to poll the event...")
        time.sleep(5)
        delete_event(base, token, cancel_event_id)
        print(f"Deleted event 'cancel-demo-event' — VENs will see it vanish on next poll")
    elif args.demo_cancel and not cancel_event_id:
        print(f"\nWarning: --demo-cancel specified but cancel-demo-event was not found/created")


if __name__ == "__main__":
    main()
