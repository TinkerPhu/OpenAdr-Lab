#!/usr/bin/env python3
"""Phase 2 (WP2.5) — generate N fleet VEN profiles + a docker-compose
override + a manifest, and (unless --no-register) provision each VEN's
OAuth user/credentials/VEN-entity on the VTN via the existing, idempotent
`provision_vens()` from seed_vtn.py.

Absorbs WP2.4's original "VEN self-registration" scope: investigation found
`POST /vens` in openleadr-rs is gated by a hardcoded VenManager role check
(not a scope), so a VEN's own credential can never self-register — only a
VenManager credential can. Registration is therefore done here, once, in
bulk, before the fleet containers start; no fleet VEN ever holds a
VenManager credential.

Usage:
    python3 scripts/gen_fleet_profiles.py --count 10 --seed 42
    python3 scripts/gen_fleet_profiles.py --count 5 --no-register  # profiles/compose only
"""

import argparse
import json
import os
import random
import sys

import yaml

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from seed_vtn import provision_vens  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VEN_DIR = os.path.join(REPO_ROOT, "VEN")
PROFILES_DIR = os.path.join(VEN_DIR, "profiles")
FLEET_DIR = os.path.join(VEN_DIR, "fleet")
COMPOSE_PATH = os.path.join(VEN_DIR, "docker-compose.fleet.yml")
MANIFEST_PATH = os.path.join(FLEET_DIR, "manifest.json")

BASE_PORT = 8300  # 8211-8213 are ven-1..3; fleet instances start well clear of those.
POLL_STARTUP_STRIDE_S = 4  # GB-09: spread instance i's first poll by i*stride seconds.


def gen_profile(rng):
    """One randomized-but-seeded asset mix, structurally identical to the
    hand-written ven-1/2/3 profiles (VEN/profiles/ven-N.yaml)."""
    has_ev = rng.random() < 0.7
    has_battery = rng.random() < 0.6
    battery_kwh = round(rng.uniform(5.0, 15.0), 1)
    ev_battery_kwh = round(rng.uniform(40.0, 75.0), 1)
    pv_kw = round(rng.uniform(3.0, 10.0), 1)
    base_load_kw = round(rng.uniform(0.2, 0.8), 2)

    assets = [
        {"type": "pv", "id": "pv", "rated_kw": pv_kw},
        {"type": "base_load", "id": "base_load", "baseline_kw": base_load_kw},
    ]
    if has_ev:
        assets.append(
            {
                "type": "ev",
                "id": "ev",
                "max_charge_kw": 7.4,
                "initial_soc": round(rng.uniform(0.2, 0.6), 2),
                "battery_kwh": ev_battery_kwh,
                "soc_target": 0.8,
            }
        )
    if has_battery:
        assets.append(
            {
                "type": "battery",
                "id": "battery",
                "capacity_kwh": battery_kwh,
                "max_charge_kw": round(battery_kwh / 2, 1),
                "max_discharge_kw": round(battery_kwh / 2, 1),
                "initial_soc": 0.5,
                "round_trip_efficiency": 0.92,
                "min_soc": 0.1,
            }
        )

    return {
        "assets": assets,
        "simulator": {"tick_s": 1, "persist_every_s": 15, "report_interval_s": 60},
        "planner": {
            "plan_zones": [
                {"step_s": 300, "slots": 96},
                {"step_s": 600, "slots": 96},
                {"step_s": 900, "slots": 96},
            ]
        },
    }


def compose_service(ven_name, port, startup_jitter_s):
    return {
        "build": {"context": ".", "dockerfile": "Dockerfile"},
        "restart": "unless-stopped",
        "ports": [f"{port}:8080"],
        "environment": {
            "LISTEN_ADDR": "0.0.0.0:8080",
            "VTN_BASE_URL": "http://vtn:3000",
            "CLIENT_ID": ven_name,
            "CLIENT_SECRET": ven_name,
            "VEN_NAME": ven_name,
            "POLL_EVENTS_SECS": "30",
            "POLL_PROGRAMS_SECS": "300",
            "POLL_STARTUP_JITTER_S": str(startup_jitter_s),
            "PERSIST_PATH": "/data/state.json",
            "PROFILE_PATH": "/config/profile.yaml",
            "RUST_LOG": "info",
        },
        "volumes": [
            f"./data/{ven_name}:/data",
            f"./profiles/{ven_name}.yaml:/config/profile.yaml:ro",
        ],
        "healthcheck": {
            "test": "curl --fail http://127.0.0.1:8080/health || exit 1",
            "interval": "10s",
            "timeout": "5s",
            "retries": 3,
            "start_period": "5s",
        },
        "networks": ["vtn_openadr-net"],
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--count", type=int, required=True, help="fleet size N")
    p.add_argument("--seed", type=int, default=42, help="RNG seed (reproducible fleet)")
    p.add_argument("--prefix", default="fleet-ven", help="venName prefix")
    p.add_argument("--vtn-url", default="http://localhost:8200", help="VTN base URL for registration")
    p.add_argument("--no-register", action="store_true", help="skip VTN registration (profiles/compose only)")
    args = p.parse_args()

    rng = random.Random(args.seed)
    os.makedirs(PROFILES_DIR, exist_ok=True)
    os.makedirs(FLEET_DIR, exist_ok=True)

    vens = []
    services = {}
    for i in range(args.count):
        ven_name = f"{args.prefix}-{i:03d}"
        port = BASE_PORT + i
        jitter_s = i * POLL_STARTUP_STRIDE_S

        profile_path = os.path.join(PROFILES_DIR, f"{ven_name}.yaml")
        with open(profile_path, "w") as f:
            yaml.safe_dump(gen_profile(rng), f, sort_keys=False)

        services[ven_name] = compose_service(ven_name, port, jitter_s)
        vens.append(
            {
                "ven_name": ven_name,
                "client_id": ven_name,
                "client_secret": ven_name,
                "user_ref": f"{ven_name}-user",
                "port": port,
            }
        )

    compose = {
        "services": services,
        "networks": {"vtn_openadr-net": {"external": True}},
    }
    with open(COMPOSE_PATH, "w") as f:
        yaml.safe_dump(compose, f, sort_keys=False)

    with open(MANIFEST_PATH, "w") as f:
        json.dump({"seed": args.seed, "vens": vens}, f, indent=2)

    print(f"Generated {args.count} profiles under {PROFILES_DIR}")
    print(f"Wrote {COMPOSE_PATH}")
    print(f"Wrote {MANIFEST_PATH}")

    if not args.no_register:
        print(f"Provisioning {args.count} VENs on {args.vtn_url} (idempotent) ...")
        provision_vens(args.vtn_url, vens)


if __name__ == "__main__":
    main()
