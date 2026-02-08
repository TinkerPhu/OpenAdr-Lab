#!/usr/bin/env python3
"""Seed the VTN with demo programs and events for all 8 use cases.

Usage:
    python3 seed_vtn.py --vtn-url http://localhost:8200
    python3 seed_vtn.py --vtn-url http://localhost:8200 --demo-cancel
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
            {"type": "VEN_NAME", "values": ["ven-1-name"]},
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
                "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}],
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
                    {"type": "VEN_NAME", "values": ["ven-1-name"]},
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
            {
                "eventName": "ev-charge-pause",
                "priority": 2,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT2H",
                },
                "targets": [
                    {"type": "VEN_NAME", "values": ["ven-2"]},
                    {"type": "VEN_NAME", "values": ["ven-3"]},
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
                    "duration": "PT24H",
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
                "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}],
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
            # UC8: Cancel Demo Event — will be created then deleted with --demo-cancel
            {
                "eventName": "cancel-demo-event",
                "priority": None,
                "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}],
                "intervals": [
                    {
                        "id": 0,
                        "payloads": [{"type": "SIMPLE", "values": [1]}],
                    },
                ],
            },
        ],
    }


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


def delete_event(base_url, token, event_id):
    r = requests.delete(
        f"{base_url}/events/{event_id}",
        headers=auth_headers(token),
        timeout=10,
    )
    r.raise_for_status()


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Seed the VTN with demo programs and events")
    parser.add_argument("--vtn-url", default="http://localhost:8200", help="VTN base URL")
    parser.add_argument("--client-id", default="any-business", help="OAuth client ID")
    parser.add_argument("--client-secret", default="any-business", help="OAuth client secret")
    parser.add_argument("--demo-cancel", action="store_true", help="Demo UC8: create then delete cancel-demo-event")
    args = parser.parse_args()

    base = args.vtn_url.rstrip("/")
    events_data = build_events()

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

    # Check existing events to allow idempotent re-runs
    existing_events = list_events(base, token)
    existing_keys = {(e["programID"], e["eventName"]) for e in existing_events}

    # Create events
    created_events = 0
    skipped_events = 0
    cancel_event_id = None
    for prog_name, events in events_data.items():
        prog_id = program_ids[prog_name]
        for evt in events:
            if (prog_id, evt["eventName"]) in existing_keys:
                print(f"Event '{evt['eventName']}' for '{prog_name}' already exists — skipping")
                skipped_events += 1
                # Track cancel-demo-event ID for --demo-cancel
                if evt["eventName"] == "cancel-demo-event":
                    for ex in existing_events:
                        if ex["programID"] == prog_id and ex["eventName"] == "cancel-demo-event":
                            cancel_event_id = ex["id"]
                            break
            else:
                body = create_event(base, token, prog_id, evt)
                print(f"Created event '{evt['eventName']}' for '{prog_name}'  id={body['id']}")
                created_events += 1
                if evt["eventName"] == "cancel-demo-event":
                    cancel_event_id = body["id"]

    # Summary
    print(f"\nDone: {len(program_ids)} programs, {created_events} events created, {skipped_events} skipped")

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
