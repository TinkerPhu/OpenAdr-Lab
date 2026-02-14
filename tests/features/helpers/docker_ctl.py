"""Helper to manage Docker containers from within the test-runner container.

Uses raw `docker stop/start` instead of `docker compose stop/start` to avoid
Docker Compose aborting the entire stack when a container exits.
"""

import subprocess
import time

PROJECT_NAME = "openadr-test"


def _container_name(service):
    """Map a compose service name to its container name."""
    return f"{PROJECT_NAME}-{service}-1"


def _docker(*args):
    """Run a docker command and return the CompletedProcess."""
    cmd = ["docker", *args]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=60)


def stop_service(service):
    """Stop a running container (container stays, process stops)."""
    name = _container_name(service)
    result = _docker("stop", "-t", "5", name)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to stop {name}: {result.stderr}")


def start_service(service):
    """Start a previously stopped container."""
    name = _container_name(service)
    result = _docker("start", name)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to start {name}: {result.stderr}")


def restart_service(service):
    """Restart a container."""
    name = _container_name(service)
    result = _docker("restart", "-t", "5", name)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to restart {name}: {result.stderr}")


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
