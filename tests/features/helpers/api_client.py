"""Thin HTTP helpers for talking to the VTN and VEN APIs."""

import os
import requests

VTN_BASE_URL = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")
VEN_BASE_URL = os.environ.get("VEN_BASE_URL", "http://test-ven-1:8080")


# ── VTN helpers ──────────────────────────────────────────────────────────────

def get_token(client_id, client_secret):
    """Obtain a bearer token from the VTN auth endpoint."""
    r = requests.post(
        f"{VTN_BASE_URL}/auth/token",
        data={
            "grant_type": "client_credentials",
            "client_id": client_id,
            "client_secret": client_secret,
        },
        timeout=10,
    )
    return r


def get_token_value(client_id, client_secret):
    """Convenience: return just the access_token string (raises on failure)."""
    r = get_token(client_id, client_secret)
    r.raise_for_status()
    return r.json()["access_token"]


def vtn_get(path, token, params=None):
    """Authenticated GET against the VTN."""
    return requests.get(
        f"{VTN_BASE_URL}{path}",
        headers={"Authorization": f"Bearer {token}"},
        params=params,
        timeout=10,
    )


def vtn_post(path, token, json=None):
    """Authenticated POST against the VTN."""
    return requests.post(
        f"{VTN_BASE_URL}{path}",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        json=json,
        timeout=10,
    )


# ── VEN helpers ──────────────────────────────────────────────────────────────

def ven_get(path, params=None):
    """GET against the VEN (no auth required)."""
    return requests.get(
        f"{VEN_BASE_URL}{path}",
        params=params,
        timeout=10,
    )


def ven_post(path, json=None):
    """POST against the VEN (no auth required)."""
    return requests.post(
        f"{VEN_BASE_URL}{path}",
        json=json,
        timeout=10,
    )
