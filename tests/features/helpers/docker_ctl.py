"""Helper to manage Docker containers from within the test-runner container.

Uses raw `docker stop/start/restart` to avoid Docker Compose aborting the entire
stack on container exit.  For restart, the service is resolved by its Docker
Compose label (com.docker.compose.service) so it is found regardless of whether
the container was started via `docker compose up` or `docker compose run`.
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


def _find_container(service):
    """Return the container ID for a running service by compose labels.

    Falls back to the standard name pattern if label lookup returns nothing.
    """
    result = _docker(
        "ps", "-q",
        "--filter", f"label=com.docker.compose.service={service}",
        "--filter", f"label=com.docker.compose.project={PROJECT_NAME}",
    )
    if result.returncode == 0 and result.stdout.strip():
        return result.stdout.strip().split("\n")[0]
    return _container_name(service)


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
    """Restart a container, resolving it by compose service label if needed."""
    container = _find_container(service)
    result = _docker("restart", "-t", "5", container)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to restart {service}: {result.stderr}")


def get_logs(service, since=None):
    """Return the container's stdout/stderr log lines (list of str).

    `since` is an optional Docker-accepted time value (e.g. an RFC3339
    timestamp or a duration like "2m") passed straight to `docker logs
    --since`. Used by the WP2.1 backoff resilience scenario to inspect
    structured `tracing` JSON log lines emitted by the poll loops.
    """
    name = _container_name(service)
    args = ["logs"]
    if since is not None:
        args += ["--since", since]
    args.append(name)
    result = _docker(*args)
    if result.returncode != 0:
        raise RuntimeError(f"Failed to fetch logs for {name}: {result.stderr}")
    # tracing_subscriber's fmt().json() writes to stdout by default; capture
    # stderr too so this stays correct if that ever changes.
    text = result.stdout + result.stderr
    return [line for line in text.splitlines() if line.strip()]


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
