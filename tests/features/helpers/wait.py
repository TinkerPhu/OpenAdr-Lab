"""Polling helpers for startup waits and eventual-consistency checks."""

import time
import requests


def wait_for_url(url, timeout=60, interval=2):
    """Block until *url* returns HTTP 200 (or any 2xx)."""
    deadline = time.time() + timeout
    last_err = None
    while time.time() < deadline:
        try:
            r = requests.get(url, timeout=5)
            if r.ok:
                return
        except requests.RequestException as exc:
            last_err = exc
        time.sleep(interval)
    raise TimeoutError(
        f"{url} not reachable after {timeout}s (last error: {last_err})"
    )


def poll_until(fn, predicate, timeout=60, interval=1, description="condition"):
    """Call *fn()* repeatedly until *predicate(result)* is truthy.

    Returns the first result that satisfies the predicate.
    Raises TimeoutError if the deadline passes.
    """
    deadline = time.time() + timeout
    last_result = None
    while time.time() < deadline:
        last_result = fn()
        if predicate(last_result):
            return last_result
        time.sleep(interval)
    raise TimeoutError(
        f"poll_until({description}) timed out after {timeout}s. "
        f"Last result: {last_result!r}"
    )
