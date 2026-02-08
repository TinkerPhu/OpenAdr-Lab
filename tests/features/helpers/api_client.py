"""Thin HTTP helpers for talking to the VTN, VEN, and BFF APIs."""

import os
import requests

VTN_BASE_URL = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")
VEN_BASE_URL = os.environ.get("VEN_BASE_URL", "http://test-ven-1:8080")
VEN2_BASE_URL = os.environ.get("VEN2_BASE_URL", "http://test-ven-2:8080")
BFF_BASE_URL = os.environ.get("BFF_BASE_URL", "http://test-bff:8090")


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


def vtn_delete(path, token):
    """Authenticated DELETE against the VTN."""
    return requests.delete(
        f"{VTN_BASE_URL}{path}",
        headers={"Authorization": f"Bearer {token}"},
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


# ── VEN-2 helpers ────────────────────────────────────────────────────────────

def ven2_get(path, params=None):
    """GET against VEN-2 (no auth required)."""
    return requests.get(
        f"{VEN2_BASE_URL}{path}",
        params=params,
        timeout=10,
    )


def ven2_post(path, json=None):
    """POST against VEN-2 (no auth required)."""
    return requests.post(
        f"{VEN2_BASE_URL}{path}",
        json=json,
        timeout=10,
    )


# ── BFF helpers ──────────────────────────────────────────────────────────────

def bff_get(path, params=None):
    """GET against the BFF (no auth — BFF handles VTN auth internally)."""
    return requests.get(f"{BFF_BASE_URL}{path}", params=params, timeout=10)


def bff_post(path, json=None):
    """POST against the BFF."""
    return requests.post(f"{BFF_BASE_URL}{path}", json=json, timeout=10)


def bff_put(path, json=None):
    """PUT against the BFF."""
    return requests.put(f"{BFF_BASE_URL}{path}", json=json, timeout=10)


def bff_delete(path):
    """DELETE against the BFF."""
    return requests.delete(f"{BFF_BASE_URL}{path}", timeout=10)
