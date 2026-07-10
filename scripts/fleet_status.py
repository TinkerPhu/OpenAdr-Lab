#!/usr/bin/env python3
"""Phase 2 (WP2.5) — report per-VEN health for the generated fleet, cross-
checked against the VTN's own /vens list (ven-manager credential).

Usage:
    python3 scripts/fleet_status.py [--manifest VEN/fleet/manifest.json] [--vtn-url http://localhost:8200]
"""

import argparse
import json
import os
import sys

import requests

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from seed_vtn import get_token  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_MANIFEST = os.path.join(REPO_ROOT, "VEN", "fleet", "manifest.json")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--manifest", default=DEFAULT_MANIFEST)
    p.add_argument("--vtn-url", default="http://localhost:8200")
    args = p.parse_args()

    if not os.path.exists(args.manifest):
        print(f"No fleet manifest at {args.manifest} — has `fleet.sh up` been run?")
        sys.exit(1)

    with open(args.manifest) as f:
        vens = json.load(f)["vens"]

    try:
        token = get_token(args.vtn_url, "ven-manager", "ven-manager")
        r = requests.get(f"{args.vtn_url}/vens", headers={"Authorization": f"Bearer {token}"}, timeout=10)
        r.raise_for_status()
        vtn_ven_names = {v["venName"] for v in r.json()}
    except requests.RequestException as e:
        print(f"Warning: could not reach VTN /vens ({e}) — registration column will show '?'")
        vtn_ven_names = None

    healthy_count = 0
    print(f"{'VEN':<20} {'port':<6} {'health':<8} {'registered':<10}")
    for ven in vens:
        name = ven["ven_name"]
        port = ven["port"]
        try:
            resp = requests.get(f"http://127.0.0.1:{port}/health", timeout=3)
            health = "up" if resp.ok else f"HTTP {resp.status_code}"
            if resp.ok:
                healthy_count += 1
        except requests.RequestException:
            health = "unreachable"

        if vtn_ven_names is None:
            registered = "?"
        else:
            registered = "yes" if name in vtn_ven_names else "NO"

        print(f"{name:<20} {port:<6} {health:<8} {registered:<10}")

    print(f"\n{healthy_count}/{len(vens)} VENs healthy")


if __name__ == "__main__":
    main()
