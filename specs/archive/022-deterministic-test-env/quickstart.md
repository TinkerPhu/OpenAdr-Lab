# Quickstart: Verify Deterministic Test Environment

**Branch**: `022-deterministic-test-env`
**Date**: 2026-05-12

---

## Prerequisites

VEN + VTN stacks running (local or Pi4):

```bash
cd VEN && docker compose up -d --build
```

---

## 1. Verify the new field appears in GET /sim/inject

```bash
curl -s http://localhost:8211/sim/inject | jq '.pv_plan_kw'
# Expected: null
```

---

## 2. Set and verify the forecast override

```bash
curl -s -X POST http://localhost:8211/sim/inject \
  -H "Content-Type: application/json" \
  -d '{"pv_plan_kw": 0.0}'
# Expected: HTTP 204

curl -s http://localhost:8211/sim/inject | jq '.pv_plan_kw'
# Expected: 0.0
```

---

## 3. Clear the forecast override

```bash
curl -s -X POST http://localhost:8211/sim/inject \
  -H "Content-Type: application/json" \
  -d '{"pv_plan_kw": null}'
# Expected: HTTP 204

curl -s http://localhost:8211/sim/inject | jq '.pv_plan_kw'
# Expected: null
```

---

## 4. Verify no replan is triggered on inject

```bash
# Watch the planner log — should see NO "planner loop: starting plan cycle" within 2s after inject
curl -s -X POST http://localhost:8211/sim/inject \
  -H "Content-Type: application/json" \
  -d '{"pv_plan_kw": 0.0}'
docker compose -f VEN/docker-compose.yml logs --tail=20 ven-1
```

---

## 5. Unit tests

```bash
cd VEN
SQLX_OFFLINE=true cargo test --workspace
# Expected: 0 failures
```

---

## 6. Run the unblocked BDD scenario (3× at different times of day)

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab

# Run the primary unblocked scenario
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/deviation_absorber.feature:149

# Repeat at different times (morning, afternoon, evening)
# All three must pass
```

---

## 7. Full deviation_absorber suite (non-regression)

```bash
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/deviation_absorber.feature

docker compose -f tests/docker-compose.test.yml down -v
```

---

## Acceptance Checklist

- [ ] `grep "pv_plan_kw" VEN/src/state.rs VEN/src/routes/sim.rs VEN/src/tasks/planning.rs` — all 3 infrastructure files match; `pv_plan_kw` absent from `VEN/src/controller/milp_planner/` (which uses the `pv_forecast_override` parameter name instead)
- [ ] `pv_plan_kw` absent from `VEN/src/entities/` and `VEN/src/controller/` (domain ring stays clean)
- [ ] Scenario `deviation_absorber.feature:149` passes in 3 consecutive time-of-day runs without `@wip`
- [ ] Battery pre-discharge ≤ 0.1 kW when `pv_plan_kw=0.0`
- [ ] Full BDD suite: zero regressions
- [ ] `cargo test --workspace`: zero failures
