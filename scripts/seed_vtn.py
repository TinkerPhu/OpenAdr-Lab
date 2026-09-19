#!/usr/bin/env python3
"""Seed the VTN with demo programs and events for all 8 use cases.

Also provisions the fleet's VEN users, credentials and VEN objects via the API
if they don't exist yet (GB-02/GB-03: uniform "ven-N" venName + VTN-issued UUID
id, no special case for ven-1).

OpenADR 3.1: everything here authenticates as the single `bl-client` business
credential, which is the only user seeded in SQL
(VTN/fixtures/01_bl_client.sql) -- nothing can call /users without a token, so
exactly one bootstrap credential has to pre-exist. Every VEN user is then
created through the API, which means the VTN hashes each secret itself and no
password hash is maintained in this repository.

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
        "targets": ["ven-1", "ven-2"],
    },
    {
        "programName": "EV Managed Charging",
        "targets": ["ven-2", "ven-3"],
    },
    {
        "programName": "HVAC Optimization",
        "targets": [],  # open — an empty target list is visible to every VEN
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
                "targets": ["ven-1"],
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
                "targets": ["ven-1", "ven-2"],
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
                "targets": ["ven-2"],
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
                "targets": ["ven-1"],
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
                "targets": ["ven-1"],
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

# The single business-layer credential. 3.1's scope model lets one client hold
# every scope this script needs (write_users to create the VEN users,
# write_vens_bl to create their VEN objects, write_programs/write_events for the
# demo data), replacing the 3.0 any-business/ven-manager/user-manager trio.
# Seeded by VTN/fixtures/01_bl_client.sql.
BL_CLIENT_ID = "bl-client"
BL_CLIENT_SECRET = "bl-client"

VENS_TO_PROVISION = [
    {"ven_name": "ven-1", "client_id": "ven-1", "client_secret": "ven-1", "user_ref": "ven-1-user"},
    {"ven_name": "ven-2", "client_id": "ven-2", "client_secret": "ven-2", "user_ref": "ven-2-user"},
    {"ven_name": "ven-3", "client_id": "ven-3", "client_secret": "ven-3", "user_ref": "ven-3-user"},
    # BL-41: ven-4 runs on a second physical host (Node2), so it advertises its
    # own reachable origin via the DASHBOARD_URL attribute instead of relying
    # on same-host Docker DNS (see _ensure_dashboard_url_attribute below).
    {"ven_name": "ven-4", "client_id": "ven-4", "client_secret": "ven-4", "user_ref": "ven-4-user",
     "dashboard_url": "http://192.168.1.104:8211"},
    # ven-5..ven-13: 9 more VENs on Node2 (same LAN-reachability reasoning as
    # ven-4 above), rounding out a 10-VEN, mixed-asset Node2 fleet — see
    # VEN/profiles/ven-{5..13}.yaml for each instance's asset mix.
    {"ven_name": "ven-5", "client_id": "ven-5", "client_secret": "ven-5", "user_ref": "ven-5-user",
     "dashboard_url": "http://192.168.1.104:8215"},
    {"ven_name": "ven-6", "client_id": "ven-6", "client_secret": "ven-6", "user_ref": "ven-6-user",
     "dashboard_url": "http://192.168.1.104:8216"},
    {"ven_name": "ven-7", "client_id": "ven-7", "client_secret": "ven-7", "user_ref": "ven-7-user",
     "dashboard_url": "http://192.168.1.104:8217"},
    {"ven_name": "ven-8", "client_id": "ven-8", "client_secret": "ven-8", "user_ref": "ven-8-user",
     "dashboard_url": "http://192.168.1.104:8218"},
    {"ven_name": "ven-9", "client_id": "ven-9", "client_secret": "ven-9", "user_ref": "ven-9-user",
     "dashboard_url": "http://192.168.1.104:8219"},
    {"ven_name": "ven-10", "client_id": "ven-10", "client_secret": "ven-10", "user_ref": "ven-10-user",
     "dashboard_url": "http://192.168.1.104:8220"},
    {"ven_name": "ven-11", "client_id": "ven-11", "client_secret": "ven-11", "user_ref": "ven-11-user",
     "dashboard_url": "http://192.168.1.104:8221"},
    {"ven_name": "ven-12", "client_id": "ven-12", "client_secret": "ven-12", "user_ref": "ven-12-user",
     "dashboard_url": "http://192.168.1.104:8222"},
    {"ven_name": "ven-13", "client_id": "ven-13", "client_secret": "ven-13", "user_ref": "ven-13-user",
     "dashboard_url": "http://192.168.1.104:8223"},
    # ven-14..ven-20: 7 more VENs on Node2, added to fill asset-mix diversity
    # gaps in the fleet (battery+heater, PV+heater, battery+EV, and richer
    # 3-asset combos) — see VEN/profiles/ven-{14..20}.yaml.
    {"ven_name": "ven-14", "client_id": "ven-14", "client_secret": "ven-14", "user_ref": "ven-14-user",
     "dashboard_url": "http://192.168.1.104:8224"},
    {"ven_name": "ven-15", "client_id": "ven-15", "client_secret": "ven-15", "user_ref": "ven-15-user",
     "dashboard_url": "http://192.168.1.104:8225"},
    {"ven_name": "ven-16", "client_id": "ven-16", "client_secret": "ven-16", "user_ref": "ven-16-user",
     "dashboard_url": "http://192.168.1.104:8226"},
    {"ven_name": "ven-17", "client_id": "ven-17", "client_secret": "ven-17", "user_ref": "ven-17-user",
     "dashboard_url": "http://192.168.1.104:8227"},
    {"ven_name": "ven-18", "client_id": "ven-18", "client_secret": "ven-18", "user_ref": "ven-18-user",
     "dashboard_url": "http://192.168.1.104:8228"},
    {"ven_name": "ven-19", "client_id": "ven-19", "client_secret": "ven-19", "user_ref": "ven-19-user",
     "dashboard_url": "http://192.168.1.104:8229"},
    {"ven_name": "ven-20", "client_id": "ven-20", "client_secret": "ven-20", "user_ref": "ven-20-user",
     "dashboard_url": "http://192.168.1.104:8230"},
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
    # 3.1 dropped programType, programLongName, country, principalSubdivision,
    # bindingEvents, localPrice, businessId and the retailer fields. `targets`
    # is a flat list of clientIds and is non-optional: [] means "every VEN".
    body = {
        "programName": prog["programName"],
        "intervalPeriod": None,
        "programDescriptions": None,
        "targets": prog.get("targets", []),
    }
    if prog.get("payloadDescriptors"):
        body["payloadDescriptors"] = prog["payloadDescriptors"]
    if prog.get("attributes"):
        body["attributes"] = prog["attributes"]
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
    body = {"programName": prog["programName"], "targets": prog.get("targets", [])}
    if prog.get("payloadDescriptors"):
        body["payloadDescriptors"] = prog["payloadDescriptors"]
    if prog.get("attributes"):
        body["attributes"] = prog["attributes"]
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


def _ensure_dashboard_url_attribute(base, token, ven_name, dashboard_url):
    """BL-41: set/replace the DASHBOARD_URL attribute on an already-provisioned
    VEN. PUT /vens/{id} is a full-content replace, so this reads the current
    attributes first and merges the DASHBOARD_URL entry in, preserving any
    existing attribute (e.g. PERSONA)."""
    r = requests.get(f"{base}/vens", headers=auth_headers(token),
                      params={"venName": ven_name}, timeout=10)
    r.raise_for_status()
    matches = [v for v in r.json() if v["venName"] == ven_name]
    if not matches:
        print(f"  WARNING: VEN '{ven_name}' not found — cannot set DASHBOARD_URL")
        return
    ven = matches[0]
    ven_id = ven["id"]
    attributes = [a for a in (ven.get("attributes") or []) if a.get("type") != "DASHBOARD_URL"]
    attributes.append({"type": "DASHBOARD_URL", "values": [dashboard_url]})

    # A Ven is BlVenRequest flattened with id/createdDateTime/
    # modificationDateTime; PUT /vens/{id} takes the request body only, so drop
    # the VTN-provisioned fields. clientID stays: it is part of the body in 3.1
    # and dropping it would fail validation.
    body = {k: v for k, v in ven.items() if k not in ("id", "createdDateTime", "modificationDateTime")}
    body["attributes"] = attributes
    r = requests.put(f"{base}/vens/{ven_id}", headers=auth_headers(token), json=body, timeout=10)
    r.raise_for_status()
    print(f"  '{ven_name}' DASHBOARD_URL set to {dashboard_url}")


VEN_SCOPES = ["read_targets", "read_ven_objects", "write_reports_ven"]


def provision_vens(base, vens):
    """Provision VEN users, credentials and VEN entities via API. Idempotent.

    OpenADR 3.1 replaced roles with scopes carried on the user object itself, so
    the 3.0 four-step dance (create user -> credential -> VEN entity -> PUT the
    VEN role back onto the user) collapses to three, and the scopes are set in
    the very first call.

    That ordering matters beyond tidiness. Under 3.0 the role was attached only
    after the credential existed, which left a window in which a VEN could mint
    a token that carried no role at all and then cache it for the token's whole
    lifetime -- authorized to nothing, polling happily, seeing an empty world
    (GB-49). In 3.1 a user has its scopes from the moment it exists, so that
    window cannot occur.
    """
    token = get_token(base, BL_CLIENT_ID, BL_CLIENT_SECRET)

    for ven in vens:
        # Already provisioned? The credential answering is the test.
        r = requests.post(
            f"{base}/auth/token",
            data={"grant_type": "client_credentials", "client_id": ven["client_id"], "client_secret": ven["client_secret"]},
            timeout=10,
        )
        if r.ok:
            print(f"VEN '{ven['ven_name']}' already provisioned — skipping.")
            if ven.get("dashboard_url"):
                _ensure_dashboard_url_attribute(base, token, ven["ven_name"], ven["dashboard_url"])
            continue

        print(f"Provisioning VEN '{ven['ven_name']}' ...")

        # 1. user, with its scopes set on creation
        r = requests.post(f"{base}/users", headers=auth_headers(token),
                          json={"reference": ven["user_ref"],
                                "description": f"VEN {ven['ven_name']}",
                                "scope": VEN_SCOPES}, timeout=10)
        r.raise_for_status()
        user_id = r.json()["id"]

        # 2. credential for that already-scoped user
        r = requests.post(f"{base}/users/{user_id}", headers=auth_headers(token),
                          json={"client_id": ven["client_id"], "client_secret": ven["client_secret"]}, timeout=10)
        r.raise_for_status()

        # 3. the VEN object, carrying the clientID that identifies it. We hold
        #    write_vens_bl, so the VTN takes clientID from this body; with the
        #    write_vens_ven variant it would instead stamp in our own token
        #    subject and every VEN would collide on ven_client_id_unique.
        ven_body = {"venName": ven["ven_name"], "clientID": ven["client_id"], "targets": []}
        # WP4.5: persona tag as an OpenADR VEN attribute so the UI dropdown
        # can label fleet entries (only present on persona fleets).
        # BL-41: DASHBOARD_URL for VENs on a different host than the VTN/UI.
        attributes = []
        if ven.get("persona"):
            attributes.append({"type": "PERSONA", "values": [ven["persona"]]})
        if ven.get("dashboard_url"):
            attributes.append({"type": "DASHBOARD_URL", "values": [ven["dashboard_url"]]})
        if attributes:
            ven_body["attributes"] = attributes
        r = requests.post(f"{base}/vens", headers=auth_headers(token), json=ven_body, timeout=10)
        r.raise_for_status()
        ven_id = r.json()["id"]
        print(f"  '{ven['ven_name']}' provisioned (user={user_id}, ven={ven_id})")


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Seed the VTN with demo programs and events")
    parser.add_argument("--vtn-url", default="http://localhost:8200", help="VTN base URL")
    parser.add_argument("--client-id", default=BL_CLIENT_ID, help="OAuth client ID")
    parser.add_argument("--client-secret", default=BL_CLIENT_SECRET, help="OAuth client secret")
    parser.add_argument("--demo-cancel", action="store_true", help="Demo UC8: create then delete cancel-demo-event")
    parser.add_argument("--skip-provision", action="store_true", help="Skip VEN provisioning (e.g. test stack handles it separately)")
    args = parser.parse_args()

    base = args.vtn_url.rstrip("/")
    events_data = build_events()

    # Provision VENs before creating programs (programs targeting a clientId
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
