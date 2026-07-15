#!/usr/bin/env python3
"""WP5.2 (BL-14) — seed ~4 weeks of synthetic history + learned heuristics
on one or more VENs via `POST /debug/heuristics/preload`. Reuses the same
dual VEN-enumeration convention as `experiments/run_experiment.py`: a
static comma-list for the fixed ven-1/ven-2/ven-3 docker-compose trio, or
a fleet manifest.json for the arbitrary-N `fleet.sh` stack.

Usage:
    python3 scripts/seed_history.py --host <pi4-ip-or-hostname>
    python3 scripts/seed_history.py --fleet-manifest VEN/fleet/manifest.json --host <pi4-ip>
"""

import argparse
import json
import os
import sys

import requests

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def static_vens(vens_arg, port_base=8211):
    names = vens_arg.split(",")
    return [{"ven_name": n, "port": port_base + i} for i, n in enumerate(names)]


def fleet_vens(manifest_path):
    with open(manifest_path) as f:
        return json.load(f)["vens"]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--vens",
        default="ven-1,ven-2,ven-3",
        help="comma-separated VEN names, mapped to ports 8211/8212/8213 (static docker-compose trio)",
    )
    p.add_argument("--host", default="localhost")
    p.add_argument(
        "--fleet-manifest",
        help="use VEN/fleet/manifest.json (or a given path) instead of --vens",
    )
    args = p.parse_args()

    vens = fleet_vens(args.fleet_manifest) if args.fleet_manifest else static_vens(args.vens)

    failures = []
    for ven in vens:
        base = f"http://{args.host}:{ven['port']}"
        try:
            r = requests.post(f"{base}/debug/heuristics/preload", timeout=60)
            r.raise_for_status()
            for asset in r.json()["preloaded"]:
                print(
                    f"{ven['ven_name']}: {asset['asset_id']} — "
                    f"{asset['samples_seeded']} samples, "
                    f"seasonal_factor={asset['seasonal_factor']:.2f}"
                )
        except requests.RequestException as e:
            print(f"{ven['ven_name']}: FAILED — {e}", file=sys.stderr)
            failures.append(ven["ven_name"])

    if failures:
        print(f"\nFailed VENs: {', '.join(failures)}", file=sys.stderr)
        sys.exit(1)

    print(f"\nSeeded {len(vens)} VEN(s) successfully.")


if __name__ == "__main__":
    main()
