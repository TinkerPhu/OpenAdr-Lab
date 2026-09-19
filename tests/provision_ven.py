#!/usr/bin/env python3
"""Provision one VEN's VTN user, credentials and VEN object via the API.

Usage: provision_ven.py <venName>   (e.g. provision_ven.py ven-1)

GB-02/GB-03: the fixture seeds only the `bl-client` business credential, not
the VENs themselves, so every VEN is provisioned the same way — a real
VTN-issued UUID id and a uniform venName (no special-cased "-name" suffix).

OpenADR 3.1 carries scopes on the user object and `POST /users` sets them in
the creation call, so the 3.0 four-step sequence (VEN entity → user → attach
the VEN role → credentials) collapses to three, and the ordering hazard it was
written around is gone. Under 3.0 the role was attached *after* the credential
could exist, so a VEN polling every couple of seconds could mint a role-less
token and cache it for its whole 30-day TTL — authorized to nothing, logging
"poll success count=0", failing every scenario that waits for it to see a
program (GB-49). A 3.1 user has its scopes from the moment it exists, so that
window cannot occur and credentials no longer have to come last.

The VEN object carries its own venName as a target. That is what makes it
targetable at all: 3.1 decides visibility by intersecting an object's targets
with the union of the VEN's targets and its resources', so a VEN with none sees
only untargeted objects however precisely a program names it.

Must run after the fixture is loaded (bl-client exists). Idempotent.
"""

import os
import sys

import requests

VTN = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")
TIMEOUT = 10

# The single business credential seeded in SQL; everything else is created here.
BL_CLIENT_ID = "bl-client"
BL_CLIENT_SECRET = "bl-client"
VEN_SCOPES = ["read_targets", "read_ven_objects", "write_reports_ven"]


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


def _ven_object_exists(ven_name):
    """True when a VEN object with this venName is already registered."""
    token = get_token(BL_CLIENT_ID, BL_CLIENT_SECRET)
    r = requests.get(
        f"{VTN}/vens", headers=auth(token), params={"venName": ven_name}, timeout=TIMEOUT
    )
    r.raise_for_status()
    return any(v.get("venName") == ven_name for v in r.json())


def _create_ven_object(ven_name, token):
    """Create the VEN object, carrying its own name as a target.

    `objectType` is the discriminator for 3.1's VenRequest enum and is
    mandatory. We hold write_vens_bl, so the VTN takes clientID from this body;
    with the write_vens_ven variant it would stamp in our own token subject
    instead and the second VEN would collide on ven_client_id_unique.
    """
    r = requests.post(
        f"{VTN}/vens",
        headers=auth(token),
        json={
            "objectType": "BL_VEN_REQUEST",
            "venName": ven_name,
            "clientID": ven_name,
            "targets": [ven_name],
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    print(f"  Created VEN object {r.json()['id']} targeting {ven_name!r}")


def provision(ven_name):
    # Working credentials mean the user exists; the VEN object is checked
    # separately below, since a run that failed between the two would otherwise
    # leave this VEN skipped forever with no VEN object behind it.
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
        if _ven_object_exists(ven_name):
            print(f"{ven_name} already provisioned — skipping.")
            return
        print(f"{ven_name} has credentials but no VEN object — creating it")
        _create_ven_object(ven_name, get_token(BL_CLIENT_ID, BL_CLIENT_SECRET))
        return

    print(f"Provisioning {ven_name} ...")
    token = get_token(BL_CLIENT_ID, BL_CLIENT_SECRET)

    # 1. User, with its scopes set in the creation call.
    r = requests.post(
        f"{VTN}/users",
        headers=auth(token),
        json={
            "reference": f"{ven_name}-user",
            "description": f"test {ven_name}",
            "scope": VEN_SCOPES,
        },
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    user_id = r.json()["id"]
    print(f"  Created user {user_id} with scopes {VEN_SCOPES}")

    # 2. Credentials for that already-scoped user.
    r = requests.post(
        f"{VTN}/users/{user_id}",
        headers=auth(token),
        json={"client_id": ven_name, "client_secret": ven_name},
        timeout=TIMEOUT,
    )
    r.raise_for_status()
    print("  Added credentials")

    # 3. The VEN object.
    _create_ven_object(ven_name, token)
    print(f"{ven_name} provisioned successfully.")


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <venName>", file=sys.stderr)
        return 2
    provision(sys.argv[1])
    return 0


if __name__ == "__main__":
    sys.exit(main())
