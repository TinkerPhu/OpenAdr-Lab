#!/bin/bash
set -e
echo "=== RUST TESTS: VEN/src ==="
cd /srv/docker/openadr_lab

echo "[$(date +%H:%M:%S)] Phase 1: Docker build (builder stage only)"
BUILD_START=$(date +%s)
docker build --target builder -f VEN/Dockerfile -t ven-test-builder VEN/ 2>&1 \
  | grep -E 'CACHED|^#[0-9]+ \[|DONE|ERROR' | head -40
BUILD_END=$(date +%s)
echo "RUST_BUILD_TIME_S=$((BUILD_END - BUILD_START))"

echo "[$(date +%H:%M:%S)] Phase 2: cargo test --workspace (compile + run)"
TEST_START=$(date +%s)
docker run --rm ven-test-builder bash -c '
  cd /app
  COMPILE_START=$(date +%s%3N)
  cargo test --workspace 2>&1
  RC=$?
  RUN_END=$(date +%s%3N)
  echo ""
  echo "RUST_TEST_TOTAL_MS=$((RUN_END - COMPILE_START))"
  exit $RC
'
TEST_END=$(date +%s)
echo "RUST_TEST_CONTAINER_TIME_S=$((TEST_END - TEST_START))"
