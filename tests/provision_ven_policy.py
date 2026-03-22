#!/usr/bin/env python3
"""Provision ven-policy user/credentials/VEN entity via the VTN API.

Must run after fixtures are loaded (ven-manager, user-manager exist).
Idempotent — safe to re-run.

ven-policy uses its own identity so it does not share reports/events with
ven-1, preventing FK constraint violations when UC8 deletes ven-1 events.
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
    # Check if ven-policy credentials already work
    r = requests.post(
        f"{VTN}/auth/token",
        data={"grant_type": "client_credentials", "client_id": "ven-policy", "client_secret": "ven-policy"},
        timeout=10,
    )
    if r.ok:
        print("ven-policy already provisioned — skipping.")
        return

    print("Provisioning ven-policy ...")
    um_token = get_token("user-manager", "user-manager")
    vm_token = get_token("ven-manager", "ven-manager")

    # 1. Create user
    r = requests.post(
        f"{VTN}/users",
        headers=auth(um_token),
        json={"reference": "ven-policy-user", "description": "test ven-policy", "roles": []},
        timeout=10,
    )
    r.raise_for_status()
    user_id = r.json()["id"]
    print(f"  Created user {user_id}")

    # 2. Add credential
    r = requests.post(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={"client_id": "ven-policy", "client_secret": "ven-policy"},
        timeout=10,
    )
    r.raise_for_status()
    print("  Added credentials")

    # 3. Create VEN entity
    r = requests.post(
        f"{VTN}/vens",
        headers=auth(vm_token),
        json={"venName": "ven-policy"},
        timeout=10,
    )
    r.raise_for_status()
    ven_id = r.json()["id"]
    print(f"  Created VEN entity {ven_id}")

    # 4. Assign VEN role
    r = requests.put(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={
            "reference": "ven-policy-user",
            "description": "test ven-policy",
            "roles": [{"role": "VEN", "id": ven_id}],
        },
        timeout=10,
    )
    r.raise_for_status()
    print("  Assigned VEN role")
    print("ven-policy provisioned successfully.")


if __name__ == "__main__":
    main()
