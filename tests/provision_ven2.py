#!/usr/bin/env python3
"""Provision ven-2 user/credentials/VEN entity via the VTN API.

Must run after fixtures are loaded (ven-manager, user-manager exist).
Idempotent — safe to re-run.
"""

import os
import sys
import requests

VTN = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")


def get_token(client_id, client_secret):
    r = requests.post(
        f"{VTN}/auth/token",
        data={"grant_type": "client_credentials", "client_id": client_id, "client_secret": client_secret},
        timeout=10,
    )
    r.raise_for_status()
    return r.json()["access_token"]


def auth(token):
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


def main():
    # Check if ven-2 credentials already work
    r = requests.post(
        f"{VTN}/auth/token",
        data={"grant_type": "client_credentials", "client_id": "ven-2", "client_secret": "ven-2"},
        timeout=10,
    )
    if r.ok:
        print("ven-2 already provisioned — skipping.")
        return

    print("Provisioning ven-2 ...")
    um_token = get_token("user-manager", "user-manager")
    vm_token = get_token("ven-manager", "ven-manager")

    # 1. Create user
    r = requests.post(
        f"{VTN}/users",
        headers=auth(um_token),
        json={"reference": "ven-2-user", "description": "test ven-2", "roles": []},
        timeout=10,
    )
    r.raise_for_status()
    user_id = r.json()["id"]
    print(f"  Created user {user_id}")

    # 2. Add credential
    r = requests.post(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={"client_id": "ven-2", "client_secret": "ven-2"},
        timeout=10,
    )
    r.raise_for_status()
    print("  Added credentials")

    # 3. Create VEN entity
    r = requests.post(
        f"{VTN}/vens",
        headers=auth(vm_token),
        json={"venName": "ven-2"},
        timeout=10,
    )
    r.raise_for_status()
    ven_id = r.json()["id"]
    print(f"  Created VEN entity {ven_id}")

    # 4. Assign VEN role
    role_payload = {"roles": [{"role": "VEN", "id": ven_id}]}
    print(f"  Assigning role: PUT /users/{user_id} with {role_payload}")
    r = requests.put(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json=role_payload,
        timeout=10,
    )
    if not r.ok:
        print(f"  Role assignment failed: {r.status_code} {r.text[:500]}")
    r.raise_for_status()
    print("  Assigned VEN role")
    print("ven-2 provisioned successfully.")


if __name__ == "__main__":
    main()
