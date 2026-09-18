#!/usr/bin/env python3
"""Provision one VEN's VTN entity, user, role and credentials via the API.

Usage: provision_ven.py <venName>   (e.g. provision_ven.py ven-1)

GB-02/GB-03: the fixture (test_user_credentials.sql) seeds the
ven-manager/user-manager/business users, but not the VENs themselves —
entrypoint.sh deletes the fixture's legacy ven-1 rows before this script
runs, so every VEN is provisioned the same way: a real VTN-issued UUID id
and a uniform venName (no special-cased "-name" suffix).

Order matters. The credentials are added **last**, after the VEN entity
exists and the VEN role is attached to the user. The VENs are already
running and retry their token every couple of seconds, so any window in
which the client_id authenticates but carries no VEN role lets a VEN mint a
role-less token and cache it for the token's whole 30-day TTL. The VTN then
filters every list to empty for that VEN — programs and events return `[]`
with HTTP 200, the VEN logs "poll success count=0", and every scenario that
waits for a VEN to see a program times out. Adding credentials last means
the client_id simply does not authenticate until the role is in place, so
the VEN's existing 401-retry carries it to a correct token.

Must run after fixtures are loaded (ven-manager, user-manager exist).
Idempotent — safe to re-run.
"""

import os
import sys

import requests

VTN = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")
TIMEOUT = 10


def get_token(client_id, client_secret):
    r = requests.post(
        f"{VTN}/auth/token",
        data={
            "grant_type": "client_credentials",
            "client_id": client_id,
            "client_secret": client_secret,
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    return r.json()["access_token"]


def auth(token):
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


def provision(ven_name):
    # Credentials are created last, so working credentials prove the whole
    # sequence ran — this check is safe as an idempotency guard.
    r = requests.post(
        f"{VTN}/auth/token",
        data={
            "grant_type": "client_credentials",
            "client_id": ven_name,
            "client_secret": ven_name,
        },
        timeout=TIMEOUT,
    )
    if r.ok:
        print(f"{ven_name} already provisioned — skipping.")
        return

    print(f"Provisioning {ven_name} ...")
    um_token = get_token("user-manager", "user-manager")
    vm_token = get_token("ven-manager", "ven-manager")

    # 1. Create the VEN entity (its id is needed for the role).
    r = requests.post(
        f"{VTN}/vens",
        headers=auth(vm_token),
        json={"venName": ven_name},
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    ven_id = r.json()["id"]
    print(f"  Created VEN entity {ven_id}")

    # 2. Create the user.
    r = requests.post(
        f"{VTN}/users",
        headers=auth(um_token),
        json={
            "reference": f"{ven_name}-user",
            "description": f"test {ven_name}",
            "roles": [],
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    user_id = r.json()["id"]
    print(f"  Created user {user_id}")

    # 3. Attach the VEN role — before any credential can authenticate.
    r = requests.put(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={
            "reference": f"{ven_name}-user",
            "description": f"test {ven_name}",
            "roles": [{"role": "VEN", "id": ven_id}],
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    print("  Assigned VEN role")

    # 4. Credentials last (see the module docstring).
    r = requests.post(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={"client_id": ven_name, "client_secret": ven_name},
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    print("  Added credentials")
    print(f"{ven_name} provisioned successfully.")


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <venName>", file=sys.stderr)
        return 2
    provision(sys.argv[1])
    return 0


if __name__ == "__main__":
    sys.exit(main())
