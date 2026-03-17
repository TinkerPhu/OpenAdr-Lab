# Quickstart: VEN Timeline UI

**Branch**: `005-ven-timeline-ui`
**Date**: 2026-03-16

Integration scenarios for manual testing and smoke-testing after deployment.

---

## Prerequisites

- VEN running locally or on Pi4-Server (`ven-ven-1-1:8080`)
- At least 60 seconds of uptime (history buffer populated)
- At least one plan generated (EV or battery present in profile)
- Docker stack up: `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose up -d"`

---

## Smoke Test: Backend Timeline Endpoints

```bash
BASE=http://pi4-server:8211   # ven-1

# 1. Per-asset timeline — EV past + future merged
curl "$BASE/timeline/ev?hours_back=1&hours_forward=1" | jq 'length, .[0]'
# Expect: N (integer > 0), then first point with ts + values.power_kw

# 2. Grid timeline with tariff data
curl "$BASE/timeline/grid?hours_back=1&hours_forward=1" | jq '.[0].values'
# Expect: object with power_kw; future points should also have import_price_eur_kwh

# 3. All-asset timeline
curl "$BASE/timeline/all?hours_back=1&hours_forward=1" | jq 'keys'
# Expect: ["base_load","battery","ev","grid","heater","pv"]

# 4. Extended window
curl "$BASE/timeline/ev?hours_back=0&hours_forward=24" | jq 'length'
# Expect: more points than the 1h window (plan slots projected 24h)

# 5. Unknown asset → 404
curl -o /dev/null -w "%{http_code}" "$BASE/timeline/unknown_xyz"
# Expect: 404

# 6. Sim schema
curl "$BASE/sim/schema" | jq 'keys'
# Expect: ["base_load","battery","ev","heater","pv"]
```

---

## Smoke Test: UI Verification

1. Open `http://pi4-server:8214/controller-v2`
2. Verify all asset cells (EV, Battery, Heater, PV, BaseLoad) show charts with no empty past section
3. Verify PV cell shows 0 kW at night / positive during daytime (not a flat limit line)
4. Verify NOW reference line is visible at chart centre on every cell
5. Click the extended window toggle on the EV cell → chart should expand to 24h forward
6. Click again → chart returns to ±1h
7. Verify EV cell right section shows: "Plugged in" switch + "SoC" slider + "Target SoC" slider (schema-driven)
8. Toggle "Plugged in" → verify sim override takes effect within ~10s
9. Verify GridAccumulatedCell stacked area shows per-asset power contributions

---

## BDD Integration Test

```bash
# From repo root, on Pi4-Server or locally with Docker:
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/ven_timeline.feature"

# Run all features to confirm no regressions:
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

Expected: `0 failed` across all features including the new `ven_timeline.feature`.

---

## Unit Test: `build_asset_timeline`

```bash
cd VEN && cargo test controller::timeline
# Tests should cover:
#   - past-only window (hours_forward=0)
#   - future-only window (hours_back=0)
#   - merged window with both past and future points
#   - grid asset with tariff intervals
#   - unknown asset_id returns empty vec
#   - sort order: result ascending by ts
```

---

## UI Unit Tests (vitest)

```bash
cd VEN/ui && npm test -- --run
# Key test files:
#   - src/api/hooks.test.ts — useTimeline, useAllTimelines, useSimSchema, useTariffs
#   - src/components/controller-v2/AssetTimelineChart.test.tsx — new data shape
#   - src/components/controller-v2/DynamicControl.test.tsx — Slider/Switch/NumberInput rendering
```
