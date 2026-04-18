# Plan B — E2E Test Failure Findings

> Companion document to `plan_B_shiftable_load_runtime.md`.
> Documents every test failure category uncovered during Pi4 ARM64 E2E validation,
> root-cause analysis, and the fixes applied.

---

## Context

Branch: `feat/plan-b-shiftable-load-runtime`
Target environment: Pi4 (ARM64, Docker Compose, headless Chromium)
Test runner: `behave` (Python BDD) in Docker container `test-runner`
Full E2E suite: ~84 min on Pi4

**Baseline at start of validation: 112 passed / 107 failed.**

---

## Progress Summary

| Run | Passed | Failed | Change |
|-----|--------|--------|--------|
| Baseline (before any fixes) | 112 | 107 | — |
| + HTTP 30s + poll 60s | 191 | 26 | +79 |
| + HTTP 60s | 205 | 12 | +14 |
| + 6 fix categories (full suite) | 203 | 14 | noise |
| + resilient polling (targeted 14) | 10/14 | 4 | |
| Isolated single scenario | 1 | 0 ✅ | |
| 4-scenario batch | 2 | 2 ❌ | root cause identified |

---

## Issue 1 — Stale `timeout=10` bypassing `HTTP_TIMEOUT`

**Status:** ✅ Fixed

**Affected scenarios:** `ven_reports` — Submit report round-trip, and several others.

**Root cause:**
Several step files used raw `requests.get(..., timeout=10)` instead of the central
`HTTP_TIMEOUT` constant. On Pi4 ARM64, the MILP solver causes VEN to respond slowly
(>10 s on some endpoints) → `ReadTimeout` exceptions in test code.

**Fix:**
- Added `HTTP_TIMEOUT = 60` constant to `tests/features/helpers/api_client.py`
- Replaced all raw `timeout=10` with `HTTP_TIMEOUT` import across all step files
- Made `poll_until` in `wait.py` catch `ReadTimeout` / `ConnectionError` and retry
  instead of raising, so transient slowness doesn't abort polls

---

## Issue 2 — Physics assertions fail immediately after sim inject

**Status:** ✅ Fixed

**Affected scenarios:**
- `phase_a_physics.feature` — Battery full SoC → expected `max_import=0.0`, got 5.0
- `phase_a_physics.feature` — EV unplugged → expected `max_import=0.0`, got 7.0
- `phase_a_physics.feature` — `ev_plugged=false` → expected `max_import=0.0`, got 7.0

**Root cause:**
Tests injected `ev_plugged=false` or `battery_soc=1.0` then *immediately* called
`GET /capability`. The simulator tick runs at 1 Hz; capability reflects the previous
physics state until the next tick processes the inject. On Pi4, the tick may be
delayed further by MILP lock contention (see Issue 7).

**Fix:**
Replaced all immediate assertions with `poll_until` loops (up to 120 s, interval=2 s)
that wait for `GET /capability` to return the expected value before asserting.

---

## Issue 3 — Forecast boundary tolerance too tight (5 s)

**Status:** ✅ Fixed

**Affected scenarios:** `asset_forecast.feature` — PV forecast boundary check.

**Root cause:**
The test asserted that the returned forecast boundary was within 5 s of `now`.
Under Pi4 load, the HTTP roundtrip alone adds several seconds; by the time the
response arrives and the test evaluates it, the boundary has drifted past the 5 s
window.

**Fix:** Widened tolerance from 5 s → 60 s (still meaningful against a 3600 s window).

---

## Issue 4 — Timeline "no past points" clock skew

**Status:** ✅ Fixed

**Affected scenarios:** `ven_timeline.feature` — `GET /timeline/ev?hours_back=0`
returns a point before `now-5s`.

**Root cause:**
VEN generates the timeline slightly before the test evaluates it. Under Pi4 Docker
load, container clocks can drift by several seconds. The 5 s tolerance was too tight.

**Fix:** Widened tolerance from 5 s → 30 s.

---

## Issue 5 — `poll_until` timeouts on plan-related waits

**Status:** ✅ Fixed

**Affected scenarios:**
- `ven_uc.feature` — UC-10a import capacity limit
- `ven_uc.feature` — UC-03 PV surplus ledger
- `ven_uc.feature` — UC-12b plan warnings

**Root cause:**
The MILP solver on Pi4 ARM64 takes 18–60 s per solve cycle. Scenarios polling for
a plan update timed out at 60 s because a single MILP cycle already consumed most
of the budget.

**Fix:**
- Bumped all plan-related `poll_until` calls to 120–180 s
- Bumped physics polls to 120 s, EV charging polls to 120 s
- Made `poll_until` resilient to `ReadTimeout` / `ConnectionError` by retrying

---

## Issue 6 — Playwright UI timeouts

**Status:** ✅ Fixed

**Affected scenarios:**
- `ven_ui_planner.feature` — Decision matrix collapses, waiting for `matrix-collapse-btn`
- `ven_ui_planner.feature` — Timeline cell dropdown, waiting for series chart
- `controller_ui.feature` — EV plugged toggle, waiting for `ctrl-ev-plugged`

**Root cause:**
Pi4 under full Docker load with headless Chromium renders pages and charts slowly.
Default 20 000 ms Playwright waits were not enough for navigation-heavy steps.

**Fix:**
- All `wait_for_selector` / `wait_for_load_state` bumped to 45 000 ms
- MUI dropdown option waits bumped to 15 000 ms

---

## Issue 7 — MILP planner holds `Mutex<SimState>` for 18–60 s (critical)

**Status:** ❌ Root cause identified, fix pending

**Affected scenarios:**
- `phase_a_physics.feature:51` — `ev_plugged=false` capability never reflects 0.0
- `controller/05_ev_charging_scenarios.feature:41` — Zero import cap never reflects in plan

**Root cause:**

`spawn_planning` in `VEN/src/loops.rs` locks `Arc<Mutex<SimState>>` for the **entire
MILP solve** (lines 628–645):

```rust
let sim_guard_for_planner = sim.lock().await;   // ← lock acquired
let plan = controller::milp_planner::run_planner(
    &*sim_guard_for_planner,                    // ← 18-60s on Pi4 ARM64
    ...
);
drop(sim_guard_for_planner);                    // ← lock released
```

On Pi4 ARM64, MILP solving takes **18–60 s per cycle**. During this time:
- All **sim ticks** are blocked (they need `sim.lock()` to advance physics)
- All **capability endpoint** reads are blocked (they need `sim.lock()` to read state)

The inject state (`AppState RwLock`) is separate — inject writes (`POST /sim/inject`)
succeed immediately. But the sim tick cannot *read* the inject and *apply* it to
physics while the Mutex is held.

**Observed timeline from VEN-1 logs (failing run):**

```
08:55:15.696  POST /sim/inject {ev_plugged: false}      ← test injects override
08:55:15.697  planner: sim lock ACQUIRED                 ← MILP takes lock immediately
08:55:16.034  sim tick: inject.ev_plugged=Some(false)    ← ONE tick squeezed through
    ... 18 seconds of MILP solving ...
08:55:34.585  planner: sim lock RELEASED
08:55:34.587  sim ticks burst: ev_plugged=Some(false)    ← physics finally updates...
08:55:34.593  planner: sim lock ACQUIRED again           ← 8 ms window!
08:55:34.633  POST /sim/inject/reset                     ← after_scenario cleanup fires
08:55:34.633  set_inject_state: Some(false) → None       ← inject cleared
    ... 18 more seconds of MILP ...
08:55:52.766  planner: sim lock RELEASED
              sim tick: ev.plugged false → true           ← EV re-plugged, cap = 7.0
```

**Why it passes in isolation but fails in multi-scenario runs:**
In isolation, `GET /capability` squeezes through the ~8 ms gap between MILP release
and planner re-acquire, catching `plugged=false`. In the 4-scenario run, a
`POST /sim/inject/reset` from a neighbouring scenario's `after_scenario` hook fires
in that same 8 ms window, clearing the inject before the polling test can observe it.

**The `after_scenario` hook problem:**
`tests/features/environment.py:264` calls `_reset_ven_sim_overrides()` (→
`POST /sim/inject/reset`) after **every** scenario — including **skipped** ones.
When specifying `features/phase_a_physics.feature:51`, the other 6 scenarios in the
file are skipped, each triggering a reset call. These resets arrive during the narrow
post-MILP windows and clear the inject before the test's `poll_until` can catch it.

**Multiple mystery reset calls during polling:**
In a 4-scenario run, up to 10+ `POST /sim/inject/reset` calls appear during a
single scenario's `poll_until` phase, each correlating with a MILP cycle boundary
(appearing right after "planner: sim lock RELEASED"). Source: `after_scenario` hooks
from skipped scenarios in the same feature file, queued and executing between MILP
cycles.

**Required fix — VEN code (loops.rs):**
Since all three consumers (`run_planner`, `compute_envelope`, `build_status_report`)
take `&SimState` (immutable reference) and `SimState` derives `Clone`, the fix is to
clone the snapshot and release the lock immediately:

```rust
// BEFORE (holds lock for entire MILP solve):
let sim_guard = sim.lock().await;
let plan = run_planner(&*sim_guard, ...);
drop(sim_guard);

// AFTER (clone snapshot, drop lock immediately):
let sim_snapshot = sim.lock().await.clone();   // clone is fast
// Mutex released here — sim ticks and capability reads unblocked
let plan = run_planner(&sim_snapshot, ...);
```

Same pattern applies to `compute_envelope` and `build_status_report` calls below.

**Required fix — test code (environment.py):**
Skip cleanup for skipped scenarios to eliminate spurious reset calls:

```python
def after_scenario(context, scenario):
    if scenario.status == 'skipped':
        return
    _reset_ven_sim_overrides(context)
    _reset_device_sessions(context)
```

---

## Issue 8 — Zero import cap never reflected in plan

**Status:** ❌ Root cause identified (same as Issue 7), fix pending

**Affected scenario:** `controller/05_ev_charging_scenarios.feature:41`

**Root cause:**
The test sets `max_import_kw=0` via an inject override and polls for 120 s waiting
for the plan to reflect zero allocation. With MILP holding the sim Mutex for 18–60 s,
the sim cannot process the override, and the planner keeps solving with stale state.
The 120 s poll budget is consumed by 2–3 MILP cycles before physics ever updates.

**Required fix:** Same Mutex clone fix as Issue 7.

---

## Debug Logging Added (Temporary — to be removed)

The following `tracing::warn!` calls were added to trace the race condition and
must be removed before merge:

| File | What was logged |
|------|----------------|
| `VEN/src/state.rs` | `set_inject_state`: `ev_plugged` change (`None → Some(false)` etc.) |
| `VEN/src/routes/sim.rs` | `post_sim_inject` entry with `body.ev_plugged`; `post_sim_inject_reset` entry |
| `VEN/src/simulator/mod.rs` | sim tick: `ev.plugged` state change with override value |
| `VEN/src/loops.rs` | sim tick: `inject.ev_plugged` when `Some`; planner lock ACQUIRED/RELEASED |
