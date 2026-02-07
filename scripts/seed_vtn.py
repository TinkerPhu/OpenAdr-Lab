#!/usr/bin/env python3
"""Seed the VTN with demo programs and events.

Usage:
    python3 seed_vtn.py --vtn-url http://localhost:8200
"""

import argparse
import sys

import requests

# ── Demo data ────────────────────────────────────────────────────────────────

PROGRAMS = [
    {"programName": "Summer Peak DR", "desc": "Demand response for summer peak hours"},
    {"programName": "EV Managed Charging", "desc": "Managed EV charging load shifting"},
    {"programName": "HVAC Optimization", "desc": "Building HVAC pre-cool/pre-heat"},
]

EVENTS = {
    "Summer Peak DR": [
        {"eventName": "peak-curtail-1", "values": [5.0]},
        {"eventName": "peak-curtail-2", "values": [10.0]},
    ],
    "EV Managed Charging": [
        {"eventName": "ev-shift-morning", "values": [3.5]},
        {"eventName": "ev-shift-evening", "values": [7.0]},
    ],
    "HVAC Optimization": [
        {"eventName": "precool-event", "values": [2.0]},
        {"eventName": "preheat-event", "values": [4.0]},
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


def create_program(base_url, token, program_name):
    r = requests.post(
        f"{base_url}/programs",
        headers=auth_headers(token),
        json={"programName": program_name, "intervalPeriod": None, "programDescriptions": None},
        timeout=10,
    )
    r.raise_for_status()
    return r.json()


def create_event(base_url, token, program_id, event_name, values):
    r = requests.post(
        f"{base_url}/events",
        headers=auth_headers(token),
        json={
            "programID": program_id,
            "eventName": event_name,
            "intervals": [
                {"id": 0, "payloads": [{"type": "SIMPLE", "values": values}]},
            ],
        },
        timeout=10,
    )
    r.raise_for_status()
    return r.json()


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Seed the VTN with demo programs and events")
    parser.add_argument("--vtn-url", default="http://localhost:8200", help="VTN base URL")
    parser.add_argument("--client-id", default="any-business", help="OAuth client ID")
    parser.add_argument("--client-secret", default="any-business", help="OAuth client secret")
    args = parser.parse_args()

    base = args.vtn_url.rstrip("/")

    # Authenticate
    print(f"Authenticating as {args.client_id} at {base} ...")
    token = get_token(base, args.client_id, args.client_secret)
    print("  OK\n")

    # Check existing programs to allow idempotent re-runs
    existing = list_programs(base, token)
    existing_names = {p["programName"]: p["id"] for p in existing}

    # Create programs
    program_ids = {}  # programName -> id
    for prog in PROGRAMS:
        name = prog["programName"]
        if name in existing_names:
            program_ids[name] = existing_names[name]
            print(f"Program '{name}' already exists  id={program_ids[name]}")
        else:
            body = create_program(base, token, name)
            program_ids[name] = body["id"]
            print(f"Created program '{name}'  id={program_ids[name]}")

    print()

    # Create events
    created_events = 0
    for prog_name, events in EVENTS.items():
        prog_id = program_ids[prog_name]
        for evt in events:
            body = create_event(base, token, prog_id, evt["eventName"], evt["values"])
            print(f"Created event '{evt['eventName']}' for '{prog_name}'  id={body['id']}")
            created_events += 1

    # Summary
    print(f"\nDone: {len(program_ids)} programs, {created_events} events")


if __name__ == "__main__":
    main()
