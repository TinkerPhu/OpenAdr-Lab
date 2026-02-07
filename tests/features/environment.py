"""behave environment hooks — run before/after the entire test suite."""

from features.helpers.api_client import VTN_BASE_URL, VEN_BASE_URL, BFF_BASE_URL
from features.helpers.wait import wait_for_url


def before_all(context):
    """Wait for VTN, VEN, and BFF to be reachable before running any tests."""
    print(f"Waiting for VTN at {VTN_BASE_URL} ...")
    wait_for_url(f"{VTN_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN at {VEN_BASE_URL} ...")
    wait_for_url(f"{VEN_BASE_URL}/health", timeout=120)

    print(f"Waiting for BFF at {BFF_BASE_URL} ...")
    wait_for_url(f"{BFF_BASE_URL}/api/health", timeout=120)

    print("All services healthy — starting tests.")
