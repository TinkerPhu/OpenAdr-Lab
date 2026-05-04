#!/bin/bash
echo "=== VITEST TESTS: VEN/ui and VTN/ui ==="
cd /srv/docker/openadr_lab

for UI_DIR in VEN/ui VTN/ui; do
  NAME=$(echo "$UI_DIR" | tr '/.-' '_' | tr 'A-Z' 'a-z')
  echo ""
  echo "--- $UI_DIR ---"

  echo "[$(date +%H:%M:%S)] Phase 1: Docker build (--target build = node+deps+src)"
  BUILD_START=$(date +%s)
  docker build --target build -t "vitest-$NAME" "$UI_DIR/" 2>&1 \
    | grep -E 'CACHED|^#[0-9]+ \[|DONE|ERROR' | head -20
  BUILD_RC=${PIPESTATUS[0]}
  BUILD_END=$(date +%s)
  echo "VITEST_BUILD_${NAME}_S=$((BUILD_END - BUILD_START))"

  if [ $BUILD_RC -ne 0 ]; then
    echo "VITEST_BUILD_${NAME}_FAILED=1"
    continue
  fi

  echo "[$(date +%H:%M:%S)] Phase 2: vitest run (inside container)"
  TEST_START=$(date +%s)
  docker run --rm "vitest-$NAME" sh -c 'cd /app && npm run test 2>&1; echo "VITEST_EXIT=$?"' || true
  TEST_END=$(date +%s)
  echo "VITEST_CONTAINER_${NAME}_S=$((TEST_END - TEST_START))"
done
