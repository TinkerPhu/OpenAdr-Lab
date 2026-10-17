#!/usr/bin/env bash
# Runs the CI board web API test (stdlib python, no server).
PY="$(command -v python3 || command -v python)" || { echo "no python"; exit 1; }
exec "$PY" "$(dirname "${BASH_SOURCE[0]}")/test_web.py" "$@"
