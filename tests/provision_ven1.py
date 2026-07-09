#!/usr/bin/env python3
"""Provision ven-1 user/credentials/VEN entity via the VTN API.

GB-02/GB-03: the fixture (test_user_credentials.sql) still seeds the
ven-manager/user-manager/business users, but no longer seeds ven-1 itself —
entrypoint.sh deletes the fixture's legacy ven-1 rows before this script
runs, so ven-1 is provisioned the same way as ven-2/ven-3: a real VTN-issued
UUID id and venName "ven-1" (no special-cased "-name" suffix).

Must run after fixtures are loaded (ven-manager, user-manager exist) and
after entrypoint.sh clears the legacy ven-1 rows. Idempotent — safe to re-run.
"""

import os
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
    # Check if ven-1 credentials already work
    r = requests.post(
        f"{VTN}/auth/token",
        data={"grant_type": "client_credentials", "client_id": "ven-1", "client_secret": "ven-1"},
        timeout=10,
    )
    if r.ok:
        print("ven-1 already provisioned — skipping.")
        return

    print("Provisioning ven-1 ...")
    um_token = get_token("user-manager", "user-manager")
    vm_token = get_token("ven-manager", "ven-manager")

    # 1. Create user
    r = requests.post(
        f"{VTN}/users",
        headers=auth(um_token),
        json={"reference": "ven-1-user", "description": "test ven-1", "roles": []},
        timeout=10,
    )
    r.raise_for_status()
    user_id = r.json()["id"]
    print(f"  Created user {user_id}")

    # 2. Add credential
    r = requests.post(
        f"{VTN}/users/{user_id}",
        headers=auth(um_token),
        json={"client_id": "ven-1", "client_secret": "ven-1"},
        timeout=10,
    )
    r.raise_for_status()
    print("  Added credentials")

    # 3. Create VEN entity
    r = requests.post(
        f"{VTN}/vens",
        headers=auth(vm_token),
        json={"venName": "ven-1"},
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
            "reference": "ven-1-user",
            "description": "test ven-1",
            "roles": [{"role": "VEN", "id": ven_id}],
        },
        timeout=10,
    )
    r.raise_for_status()
    print("  Assigned VEN role")
    print("ven-1 provisioned successfully.")


if __name__ == "__main__":
    main()
