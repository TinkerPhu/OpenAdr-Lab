"""Helper to manage Docker Compose services from within the test-runner container."""

import subprocess
import time

PROJECT_NAME = "openadr-test"
COMPOSE_FILE = "/compose/docker-compose.test.yml"


def _compose(*args):
    """Run a docker compose command and return the CompletedProcess."""
    cmd = [
        "docker", "compose",
        "-p", PROJECT_NAME,
        *args,
    ]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=60)


def stop_service(service):
    """Stop a running compose service (container stays, process stops)."""
    result = _compose("stop", "-t", "5", service)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to stop {service}: {result.stderr}")


def start_service(service):
    """Start a previously stopped compose service."""
    result = _compose("start", service)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to start {service}: {result.stderr}")


def restart_service(service):
    """Restart a compose service."""
    result = _compose("restart", "-t", "5", service)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to restart {service}: {result.stderr}")


def wait_for_healthy(url, timeout=60, interval=2):
    """Poll a URL until it returns HTTP 200."""
    import requests
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
