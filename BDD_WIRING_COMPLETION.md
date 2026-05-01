# BDD Step Wiring Completion - Feature 017

**Status**: ✅ Complete - Awaiting Test Execution on Pi4
**Date**: 2026-05-01
**Branch**: `017-add-deviation-absorber`

## Work Completed This Session

### 1. Added Missing @given Steps

All setup steps for test scenarios are now implemented:

- `Given the VEN is running with the test profile` — Verify VEN health check
- `Given the battery SoC is reset to {soc:f}` — Parametric battery SoC setup
- `Given the battery SoC is reset to min_soc` — Convenience wrapper (0.10)
- `Given the EV is plugged with SoC at {soc:f}` — EV plugged state with SoC
- `Given the EV is plugged with SoC at {soc:f} (below target)` — Variant with context flag
- `Given the EV is configured with departure in {minutes:d} minutes` — EV departure time
- `Given the ev_departure_guard_s is set to {seconds:d} seconds ({minutes:d} minutes)` — Guard threshold
- `Given the EV SoC is reset to soc_target` — EV at target SoC (0.80)
- `Given the heater is configured with min_state_linger_s of {seconds:d} seconds` — Linger config
- `Given the heater is at temp_max_c` — Heater at maximum power
- `Given all absorber assets are at or near their limits` — Multi-asset limit setup

### 2. Added Missing @when Steps

- `When the deviation is absorbed by the battery` — Verification step for absorption

### 3. Fixed API Integration

**Before**: Steps hardcoded VEN host/port
```python
ven_api = f"http://{context.ven_host}:8210"
resp = requests.get(f"{ven_api}/sim")
```

**After**: Using centralized API helpers
```python
from features.helpers.api_client import ven_get, ven_post
resp = ven_get("/sim")
resp = ven_post("/sim/inject", json=payload)
```

### 4. Updated All API Endpoints

- ✅ `GET /sim` → battery, EV, heater state inspection
- ✅ `POST /sim/inject` → deviation injection (PV, EV config, heater power)
- ✅ `POST /sim/inject/reset` → clear all overrides
- ✅ `GET /trace?limit=N` → decision log for trigger/residual events
- ✅ `GET /plan` → verify new MILP plan after Tier 2 trigger

### 5. Code Quality

- ✅ Python syntax verified (py_compile successful)
- ✅ All imports correct (behave, requests, api_client helpers)
- ✅ Parameter type annotations match feature file values
- ✅ Proper error messages with response context

## File Changes Summary

**tests/steps/deviation_absorber_steps.py**
- Added 11 new @given steps for asset configuration
- Added 1 new @when step for absorption verification
- Refactored all API calls to use ven_get/ven_post
- Total: 60 step implementations across 9 scenarios

**Commits**:
- `a28fcd0` fix: Wire BDD steps for deviation absorber feature

## Current Status

### ✅ Completed
- Feature file: 9 scenarios, 122 lines, all user stories covered
- Step implementations: 60 functions with full assertion logic
- API wiring: All endpoints integrated with proper helpers
- Python syntax: Valid and compilable

### ⏳ In Progress
- Docker build on Pi4: Currently compiling VEN containers (cargo build --release)
- Expected time: 10-20 minutes on ARM64

### Next: Test Execution

Once Docker build completes, BDD suite will execute:
```bash
docker compose -f tests/docker-compose.test.yml run --rm test-runner features/deviation_absorber.feature
```

**Expected outcome:**
- 9 scenarios passing
- All 4 user stories validated:
  - US1: Multi-asset sequential absorption
  - US2: Relay wear (linger enforcement)
  - US3: EV departure guard
  - US4: Tier 2 escalation gate

## Architecture Diagram

```
Feature File (9 scenarios)
        ↓
Behave Pattern Matching
        ↓
Python Step Functions (60)
        ↓
API Helpers (ven_get/ven_post)
        ↓
VEN Simulator Endpoints
        ↓
Absorber Module (absorber.rs)
        ↓
Test Assertions (setpoint, residual, trigger checks)
```

## Validation Checkpoint

| Item | Status | Evidence |
|------|--------|----------|
| Feature file syntax | ✅ Valid | `.feature` file parses correctly |
| Python syntax | ✅ Valid | `py_compile` succeeded |
| Step count | ✅ 60 impl | All scenarios covered |
| API integration | ✅ Updated | Using centralized helpers |
| Behave pattern match | ⏳ Testing | Docker build in progress |
| End-to-end tests | ⏳ Testing | Awaiting Pi4 execution |
| Regression suite | ⏳ Pending | Scheduled after absorber validation |

## Remaining Tasks

1. **Immediate (once build completes)**:
   - Execute deviation_absorber.feature: expect 9/9 scenarios pass
   - If failures: investigate and fix step logic

2. **Post-BDD validation**:
   - Run full regression suite (all 40+ existing scenarios)
   - Verify no regressions in other features
   - Manual Pi4 validation for each user story

3. **Code review** (T085-T092):
   - Profile YAML audit
   - Startup validation verification
   - Module clarity and documentation
   - Convention compliance

## Notes

- BDD wiring is complete and ready for execution
- No blocking issues identified
- All step decorators properly formatted for behave
- Async/timing steps use appropriate sleep margins (10% for ticks, +0.5s for linger)
- Error assertions include response context for debugging
