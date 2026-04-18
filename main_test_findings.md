# Main Branch Test Findings

**Date:** 2026-04-18  
**Branch:** `main` @ `65bf800`  
**Target:** Pi4-Server (Raspberry Pi 4, ARM64, /srv/docker/openadr_lab)

---

## Summary

### Final state (after all fixes)

| Suite | Passed | Failed | Skipped | Duration |
|---|---|---|---|---|
| E2E (behave) | 217 | 0 | 2 | ~27m |
| **Total** | **217** | **0** | **2** | ~27m |

### Progression

| Run | Branch | Passed | Failed | Root cause |
|---|---|---|---|---|
| Initial | `main` @ `29833f0` (24h horizon) | 170 | 47 | MILP lock contention (10-24s solve) |
| After horizon reduction | `main` @ `d928b11` (2h horizon) | 206 | 11 | EV SoC target infeasible in 2h window |
| **Final** | **`main` @ `65bf800`** | **217** | **0** | — |

---

## Root Cause Analysis

### Root Cause 1: Sim Mutex starvation during MILP plan cycles (39 of 47 failures)

The VEN's HEMS planner runs a HiGHS MILP solver synchronously while holding the `Arc<Mutex<SimState>>` lock (`VEN/src/loops.rs:564-577`). On Pi4 ARM64, each plan cycle takes **10–24 seconds**. During this time, every HTTP handler that calls `ctx.sim.lock().await` is blocked:

- `GET /forecast/:asset_id` — `routes/assets.rs:35`
- `GET /history/:asset_id` — `routes/assets.rs:66`
- `GET /capability/:asset_id` — `routes/assets.rs:99`
- `GET /timeline/:asset_id` — `routes/timeline.rs:111`
- `GET /timeline/all` — `routes/timeline.rs:228`
- `POST /sim/inject` — `routes/sim.rs:134`
- `POST /sim/reset/:id` — `routes/sim.rs:172`
- `GET /sim` — `routes/sim.rs:110`
- `GET /trace/history` — `routes/trace.rs:45`
- `POST /user-requests` — `routes/hems.rs:61`

All VEN HTTP helpers in `tests/features/helpers/api_client.py` use a hardcoded `timeout=10` seconds. When the sim lock is held for 10-24s by the planner, these requests hit `ReadTimeout`.

**Evidence:** VEN-1 logs show plan cycle timestamps ~10-15s apart throughout the entire test run. 78 of 78 `ReadTimeout` errors target `test-ven-1:8080`.

### Root Cause 2: `poll_until` timeouts on plan-dependent state (5 of 47 failures)

Several scenarios poll VEN endpoints waiting for plan state to reflect a newly-created event:

- `poll_until(VEN /plan has a slot with import_cap_kw <= 5.0)` — 60s timeout
- `poll_until(VEN /plan has a slot with import_cap_kw <= 0.1)` — 60s timeout
- `poll_until(VEN /plan slots have import_cap_kw ≤ 10.0)` — 60s timeout
- `poll_until(VEN has >= 1 events)` — 30s timeout
- `poll_until(VEN /capacity import_limit_kw == 2.0)` — 30s timeout
- `poll_until(VEN /sim field 'ev.power_kw' == 0.0)` — 15s timeout

These fail because: (a) each poll attempt may itself time out due to RC1, eating 10s of the budget, and (b) the plan cycle interval means it takes 10-24s for a new event to propagate through VTN polling → event parse → plan recompute.

### Root Cause 3: Playwright UI timeout (1 of 47 failures)

`controller/03_simulation_controls.feature:9` — waiting for `[data-testid="ctrl-ev-plugged"]` times out at 20000ms. This is a downstream effect of RC1: the VEN UI fetches from VEN-1 API, which is blocked by the sim lock, so the React component never receives data and never renders the toggle.

### Root Cause 4: Missing `programID` in status reports (bug — not directly causing test failures)

Every plan cycle emits a status report via `build_status_report()` (`VEN/src/controller/reporter.rs:530-543`). The report JSON lacks the required `programID` field, causing a 400 from the VTN:

```
"Failed to deserialize the JSON body into the target type: missing field `programID` at line 1 column 257"
```

This fires every 10-15s throughout the test run. While it doesn't directly cause test failures (the error is caught and logged), it adds unnecessary VTN load and log noise.

---

## Failure Groups

### Group A — Direct ReadTimeout on VEN-1 HTTP calls (39 scenarios)

All fail with `requests.exceptions.ReadTimeout` after 10s. The failing step function is the first VEN HTTP call in the scenario that happens to coincide with a plan cycle holding the sim lock.

| Feature | Scenarios | Failing step pattern |
|---|---|---|
| `asset_forecast.feature` | 3 | `GET /forecast/{asset}` |
| `asset_history.feature` | 1 | `GET /history/pv` |
| `phase_a_physics.feature` | 6 | `POST /sim/reset/battery` or `GET /capability/{asset}` |
| `timeline_grid.feature` | 8 | `GET /timeline/all` or `GET /timeline/{asset}` |
| `ven_entity_model.feature` | 1 | `GET /trace/history` |
| `ven_timeline.feature` | 4 | `GET /timeline/ev` or `GET /timeline/all` |
| `ven_reports.feature` | 1 | `POST /reports` via VEN |
| `ven_user_request.feature` | 7 | `POST /user-requests` |
| `ven_uc_normal.feature` | 2 | `POST /user-requests` (continue policy) |
| `ven_uc_edge_cases.feature` | 3 | `POST /user-requests` or `GET /sim` |
| `ven_uc_stress.feature` | 2 | Plan polling with capacity constraints |
| `ui_use_cases.feature` | 2 | VEN or VTN event polling |
| `use_cases.feature` | 1 | VEN event polling |
| `ven_integration.feature` | 1 | VEN event polling |

### Group B — `poll_until` timeouts (5 scenarios)

These use `poll_until()` to wait for state propagation but the timeout budget is consumed by RC1 + slow plan cycles.

| Feature | Scenario | poll_until description | Timeout |
|---|---|---|---|
| `controller/05_ev_charging_scenarios.feature:13` | (b) IMPORT_CAPACITY_LIMIT caps net import | plan slot import_cap_kw ≤ 5.0 | 60s |
| `controller/05_ev_charging_scenarios.feature:25` | (e) User request capped by cap limit | plan slot import_cap_kw ≤ 5.0 | 60s |
| `controller/05_ev_charging_scenarios.feature:41` | (c) Zero cap reflected in plan | plan slot import_cap_kw ≤ 0.1 | 60s |
| `controller/05_ev_charging_scenarios.feature:53` | (f) Zero cap user request | plan slot import_cap_kw ≤ 0.1 | 60s |
| `ven_integration.feature:10` | VEN reflects events | VEN has ≥ 1 events | 30s |

### Group C — Playwright timeout (1 scenario)

| Feature | Scenario | Selector |
|---|---|---|
| `controller/03_simulation_controls.feature:9` | EV plugged toggle visible | `[data-testid="ctrl-ev-plugged"]` |

### Group D — Report submission bug (non-failing, persistent error)

Every plan cycle logs: `status report (plan cycle) submission failed: /reports returned 400 Bad Request: missing field programID`.

---

## Fix Strategy

### Strategy 1: Increase HTTP timeout in api_client.py (Quick win — fixes Group A)

**Change:** Raise `timeout=10` to `timeout=30` in all functions in `tests/features/helpers/api_client.py` (`ven_get`, `ven_post`, `ven_delete`, `ven2_get`, `ven2_post`).

**Why 30s:** The longest observed plan cycle on Pi4 is ~24s. A 30s timeout ensures the HTTP request survives one full lock hold. The default 10s timeout is too short for any endpoint that needs the sim Mutex.

**Risk:** Low. Makes tests slower when VEN is truly down, but that's the correct behavior.

**Estimated impact:** Fixes ~39 of 47 failures outright.

```python
# api_client.py — change all VEN timeouts
def ven_get(path, params=None):
    return requests.get(f"{VEN_BASE_URL}{path}", params=params, timeout=30)
```

### Strategy 2: Increase poll_until timeouts (Quick win — fixes Group B)

**Change:** Increase timeouts in affected step files:
- `ev_charging_steps.py`: `timeout=60` → `timeout=120` for plan slot polling
- `ven_integration_steps.py`: `timeout=30` → `timeout=60` for event polling
- `uc_steps.py`: similar increases
- `ven_uc_edge_cases` / `ven_uc_stress`: similar increases

**Why:** Each poll attempt may block up to 30s (with Strategy 1), and 2-3 plan cycles may be needed for state propagation. 120s provides headroom.

**Estimated impact:** Fixes remaining ~5 poll_until failures.

### Strategy 3: Increase Playwright timeout for controller UI tests (Quick win — fixes Group C)

**Change:** Raise the `wait_for_selector` timeout from 20000ms to 40000ms in `controller_steps.py` for steps that depend on VEN API data loading.

**Estimated impact:** Fixes the 1 Playwright timeout.

### Strategy 4: Fix missing `programID` in `build_status_report` (Bug fix — fixes Group D)

**Change:** In `VEN/src/controller/reporter.rs:530`, add a `programID` field to the status report JSON. The function currently only has access to the `ControllerEvent` and `SimState` — it needs to look up the first enrolled program ID from state or accept it as a parameter.

**Options:**
1. Pass the first active `program_id` from the plan cycle loop into `build_status_report()`
2. Add a hardcoded fallback `programID` (e.g., from the VEN's enrolled programs)
3. Skip report submission when no program is enrolled (return `None`)

Option 3 is simplest and most correct — if there's no program context, don't submit a status report.

**Estimated impact:** Eliminates the recurring 400 errors in VEN logs.

### Strategy 5: Decouple MILP solver from sim Mutex (Architectural — long-term)

**Problem:** `run_planner()` in `loops.rs:565` calls the HiGHS solver while holding `sim.lock()`. The solver reads sim state but doesn't write to it, yet it blocks all readers for 10-24s on Pi4.

**Change:** Clone the necessary sim state snapshot *before* running the solver, then drop the lock:

```rust
// Instead of:
let sim_guard = sim.lock().await;
let plan = run_planner(&*sim_guard, ...);
drop(sim_guard);

// Do:
let sim_snapshot = {
    let sim_guard = sim.lock().await;
    sim_guard.snapshot_for_planner()  // clone needed fields
};
let plan = run_planner(&sim_snapshot, ...);
```

**Impact:** Eliminates the fundamental cause. Sim lock would be held for microseconds instead of 10-24s. All HTTP endpoints would respond instantly regardless of planner activity.

**Risk:** Medium — requires refactoring `run_planner` to accept a snapshot type instead of `&SimState`. But `run_planner` only reads from `SimState`, so no write access is lost.

### Strategy 6: Add strategic logging for future diagnosis

Add timing instrumentation to help detect lock contention in future runs:

1. **In `loops.rs`** around the planner call:
   ```rust
   let t0 = std::time::Instant::now();
   let sim_guard = sim.lock().await;
   let lock_wait = t0.elapsed();
   let plan = run_planner(...);
   let solve_time = t0.elapsed();
   drop(sim_guard);
   info!(lock_wait_ms = lock_wait.as_millis(), solve_ms = solve_time.as_millis(), "plan cycle timing");
   ```

2. **In HTTP route handlers** (e.g., `routes/assets.rs`, `routes/timeline.rs`):
   ```rust
   let t0 = std::time::Instant::now();
   let sim = ctx.sim.lock().await;
   let lock_wait = t0.elapsed();
   if lock_wait > std::time::Duration::from_secs(1) {
       warn!(lock_wait_ms = lock_wait.as_millis(), "sim lock contention in HTTP handler");
   }
   ```

---

## Recommended Fix Order

1. **Strategy 1 + 2 + 3** (test-side timeouts) — immediate, low-risk, unblocks CI. Can be done in a single commit.
2. **Strategy 4** (missing programID bug) — small Rust fix, eliminates log noise.
3. **Strategy 6** (logging) — add instrumentation before tackling Strategy 5.
4. **Strategy 5** (decouple solver from Mutex) — architectural fix, eliminates root cause permanently.

Strategies 1-3 should bring the failure count from 47 to ~0 without any Rust changes. Strategy 5 is the correct long-term fix that also improves production VEN responsiveness.

---

## Part 2: MILP Complexity Analysis

### Current Model Size (n=288 slots, 24h horizon, 5min steps)

| Category | Variables | Binary | Constraints |
|---|---|---|---|
| Grid (p_imp, p_exp, u_grid, slack) | 1,440 | 288 | 1,440 |
| Battery (ch/dis/soc/u_bat) | 1,153 | 288 | 1,440 |
| Battery activity + transitions | 575 | 575 | 575 |
| EV (p_ev, z_ev_on, z_ev_core, e_extra) | 578 | 578 | 576 |
| EV startup transitions (delta_ev) | 287 | 287 | 287 |
| Heater (z_heat_mid, z_heat_full, z_heat_ready) | 577 | 577 | 288 |
| EV ramp capture | 287 | 0 | 574 |
| Battery ramp capture | 287 | 0 | 574 |
| McCormick coexist (≈75 PV-surplus slots) | ~75 | 0 | ~225 |
| Shiftable loads | ~150 | ~150 | ~5 |
| Energy + terminal constraints | 0 | 0 | ~7 |
| **Total** | **~5,400** | **~2,743** | **~5,991** |

**Key insight:** Binary variables dominate solve time. HiGHS uses branch-and-bound, so each binary potentially doubles the search tree. The ~2,700 binaries are the main driver of the 10-24s solve time on Pi4 ARM64.

### Option A: Reduce Slot Count (n)

The most impactful single lever. All variable/constraint counts scale linearly with n.

| Configuration | step_s | horizon_h | n | Est. binaries | Est. solve (Pi4) |
|---|---|---|---|---|---|
| **Current** | 300 (5min) | 24 | 288 | ~2,700 | 10-24s |
| 10min steps | 600 | 24 | 144 | ~1,350 | 3-8s |
| 15min steps | 900 | 24 | 96 | ~900 | 1-4s |
| 8h horizon | 300 | 8 | 96 | ~900 | 1-4s |
| **Test profile** | 300 | 2 | 24 | ~220 | <1s |

**Recommendation for tests:** Create a `test_fast.yaml` profile with `plan_horizon_h: 2` and `plan_step_s: 300` → n=24 slots. This is what the unit tests already use (`milp_planner.rs:1610-1611`). The planner still exercises all constraint paths, just with a much smaller model. Solve time drops from 10-24s to <1s.

**Trade-off:** A 2h horizon doesn't test long-horizon planning behavior (e.g., overnight EV charging with dynamic tariffs). For production, keep 24h. For CI tests, 2h is sufficient to verify constraint correctness.

### Option B: Disable Optional Penalty Constraints

Several constraint groups are gated by weight parameters. Setting the weight to 0.0 eliminates the variables and constraints entirely:

| Parameter | Default | Variables saved | Constraints saved | Binaries saved |
|---|---|---|---|---|
| `c_ev_startup_eur: 0.0` | 0.01 | 287 | 287 | 287 |
| `c_bat_startup_eur: 0.0` | 0.01 | 575 | 575 | 575 |
| `c_ev_ramp_eur_kw: 0.0` | 0.005 | 287 | 574 | 0 |
| `c_bat_ramp_eur_kw: 0.0` | 0.005 | 287 | 574 | 0 |
| `c_bat_ev_coexist_eur_kwh: 0.0` | 0.5 | ~75 | ~225 | 0 |
| **All disabled** | | **~1,511** | **~2,235** | **~862** |

With all optional penalties disabled: ~1,880 binaries → ~3,750 constraints. Estimated solve time: 4-10s (40-50% reduction).

**Can the EV and battery startup counters be combined?** No — they track different physical assets with independent on/off states. The solver needs per-asset control. However:

- The battery formulation is asymmetrically expensive: it uses **both** `z_bat_active[t]` (n binaries) **and** `delta_bat_active[i]` (n-1 binaries), while EV uses only `delta_ev[i]` (n-1 binaries). The battery's activity linkage constraint (`p_bat_ch[t] + p_bat_dis[t] ≤ big_M × z_bat_active[t]`, line 888) adds 288 extra constraints.

- **This linkage is redundant when the penalty weight is the only consumer.** The objective penalty `c_bat_startup_eur × delta_bat_active[i]` already discourages transitions. Removing the linkage saves 288 constraints and n binaries (288), reducing battery startup tracking from 575 variables to 287 — matching the EV pattern exactly.

### Option C: Increase MIP Gap / Reduce Time Limit

```rust
// Current (milp_planner.rs:995-996)
model = model.with_time_limit(60.0);
model = model.with_mip_gap(0.02)?;    // 2%

// Proposed for test profile
model = model.with_time_limit(15.0);   // Cap worst case at 15s
model = model.with_mip_gap(0.05)?;     // 5% — still <€2.50 on a €50 plan
```

This could be made configurable via profile.yaml. Currently hardcoded.

### Option D: Warm-Start from Previous Plan

The planner is called every `replan_interval_s` (default 20s in test, 300s in production). Each call rebuilds the model from scratch. Passing the previous solution as an initial feasible point would let HiGHS skip the initial heuristic phase:

- HiGHS supports `set_solution()` for warm-starting
- The `good_lp` crate may not expose this directly; would require raw HiGHS FFI
- Estimated speedup: 15-30% on subsequent cycles

### Option E: Snapshot-and-Release (decouple from Mutex)

As described in Strategy 5 (Part 1), clone the SimState snapshot before solving:

```rust
let snapshot = {
    let guard = sim.lock().await;
    guard.planner_snapshot()   // ~50µs clone
};
drop(guard);  // Mutex released immediately
let plan = run_planner(&snapshot, ...);  // 10-24s but non-blocking
```

This doesn't reduce solve time, but eliminates the HTTP blocking. The HTTP timeout failures disappear regardless of MILP complexity.

### Recommended Combination for Tests

| Layer | Change | Impact | Effort |
|---|---|---|---|
| **Test profile** | `plan_horizon_h: 2` | n: 288→24, solve <1s | 5 min |
| **Test profile** | Disable ramp + coexist penalties | -1,500 constraints | 5 min |
| **Solver config** | Expose `time_limit` + `mip_gap` in profile | Future-proofs | 30 min |
| **Rust architecture** | Snapshot-and-release Mutex | Eliminates HTTP blocking | 2-4h |
| **Rust optimization** | Remove battery activity linkage | -288 constraints always | 15 min |

**Creating `test_fast.yaml` with `plan_horizon_h: 2` is the single highest-impact change.** It reduces the MILP from ~6,000 variables to ~500 and eliminates solve-time-induced test failures entirely.

### Option F: Reduce slot resolution instead of horizon (architectural note)

An alternative to reducing the horizon would have been to reduce slot resolution (increase `step_s`) while keeping the 24h horizon:

| Configuration | step_s | horizon_h | n | MILP complexity |
|---|---|---|---|---|
| Chosen fix | 300 (5 min) | 2h | 24 | ~500 vars, <1s |
| Alternative | 3600 (60 min) | 24h | 24 | ~500 vars, <1s |
| Intermediate | 1800 (30 min) | 24h | 48 | ~1,000 vars, ~1-2s |

Both approaches achieve the same MILP slot count (n=24). The tradeoffs are:

**Advantages of coarser resolution (60 min / 24h):**
- Realistic tariff modeling: captures morning/evening price peaks over a full day
- EV overnight planning: `target_soc 0.90` from 0.5 is feasible (4 × 6.4 kWh = 25.6 kWh)
- Full PV generation arc (sunrise → noon → sunset) visible to the optimizer
- Battery can arbitrage across meaningful daily price spreads
- More natural test semantics: no need to lower `target_soc` to fit a 2h window

**Disadvantages of coarser resolution:**
- `step_s` is coupled to the dispatcher hold period in `loops.rs` — at 60 min, the VEN would hold one power level for 12 real 5-minute dispatch intervals. Either the dispatch loop must be fixed to interpolate, or power changes only once per hour in the sim.
- Larger per-step SoC jumps: at 60-min steps, a single slot can change battery SoC by ~8% (5 kW × 1h / 60 kWh), reducing constraint accuracy
- More invasive change: `step_s` is in `plan_step_s` profile field but its effect propagates into dispatch timing and sim physics integration — requires verifying Rust behavior, not just YAML values

**Why horizon reduction was the right choice for the test profile:**
The `plan_horizon_h` change was a pure YAML edit with no Rust code impact. Dispatch granularity, SoC accuracy, and dispatch-loop integrity were all preserved. The test suite validates constraint correctness, not production planning quality.

**Long-term architectural recommendation:** Decouple `plan_slot_s` (MILP resolution) from `dispatch_interval_s` (how often the dispatcher re-reads the plan). This would allow 60-min plan slots with 5-min dispatch without coupling issues, enabling both realistic optimization and fast dispatch.

---

## Part 3: Test Consolidation Opportunities

### Current Test Suite Size

41 feature files, 228 scenarios. Per-scenario overhead:
- VTN token acquisition: ~200ms
- VEN state resets: ~500ms (4 DELETE calls)
- VTN resource cleanup: ~1s (per created event)
- Browser page lifecycle (@ven-ui only): 2-5s
- **Total overhead: ~1-2s per scenario** (40-60% of typical test duration)

### High-Value Consolidation Groups

#### Group 1: IMPORT_CAPACITY_LIMIT — 12 scenarios across 4 files

These scenarios share identical setup: create program → inject sim → create capacity event → check plan.

| Source file | Scenarios | What's tested |
|---|---|---|
| `05_ev_charging_scenarios.feature` | 4 | EV + user request capped at limit |
| `ven_uc_edge_cases.feature` | 2 | Plan reflects limit (UC-10a, UC-10b) |
| `ven_uc_stress.feature` | 2 | Multi-asset within cap (UC-12a, UC-12b) |
| `ven_uc_vtn_coordination.feature` | 2 | Capacity endpoint updated (UC-06a, UC-06b) |

**Consolidation:** Use `Scenario Outline` with `Examples` table for the different limit values:

```gherkin
Scenario Outline: Import capacity limit <limit> kW constrains plan
  Given a rate-system program with IMPORT_CAPACITY_LIMIT <limit> kW event
  And sim injected with pv=0 and ev_soc=0.5
  When the plan is computed
  Then all capped slots have import_cap_kw ≤ <expected_max>

  Examples:
    | limit | expected_max |
    | 0.0   | 0.1          |
    | 2.0   | 2.1          |
    | 5.0   | 5.1          |
    | 10.0  | 10.1         |
```

**Savings:** 11 fewer setup/teardown cycles → ~22-33 seconds.

#### Group 2: User Request Lifecycle — 15 scenarios across 3 files

`ven_user_request.feature` (10), `ven_uc_edge_cases.feature` (3), `ven_uc_normal.feature` (2)

Many test individual fields (session_id, status, max_total_cost_eur) on the same POST response. Could be combined into fewer scenarios that check multiple fields per assertion step.

**Savings:** ~7 fewer cycles → ~15-25 seconds.

#### Group 3: Timeline & Forecast APIs — 29 scenarios across 3 files

`timeline_grid.feature` (15), `ven_timeline.feature` (7), `asset_forecast.feature` (7)

These all call `GET /timeline/*` or `GET /forecast/*` and check structural properties (resolution, grid alignment, interpolation type). Many could use `Scenario Outline` with `Examples`.

**Savings:** ~23 fewer cycles → ~35-60 seconds.

#### Group 4: Controller UI (@ven-ui) — 16 scenarios across 4 files

Each scenario creates a new browser page (2-5s overhead) and closes it. Reusing a single page across scenarios in the same feature would save 10+ browser lifecycle operations.

**Savings:** ~20-40 seconds (browser page overhead).

### Implementation Approach

| Priority | Action | Scenarios affected | Time saved | Effort |
|---|---|---|---|---|
| 🔴 Critical | Scenario Outlines for capacity limits | 12 → 4 | ~30s | 20 min |
| 🟠 High | Merge user request field checks | 15 → 8 | ~20s | 30 min |
| 🟠 High | Reuse browser page per feature | 16 | ~30s | 20 min |
| 🟡 Medium | Scenario Outlines for timeline tests | 29 → 10 | ~40s | 40 min |
| 🟡 Medium | Move VTN token to `before_feature` hook | All | ~10s | 15 min |

**Total potential savings: 3-5 minutes per full test run.**

### Important Caveat

Combining scenarios has a trade-off: when a combined scenario fails, it's harder to pinpoint which specific assertion triggered the failure. Using `Scenario Outline` with `Examples` is the safest approach because each row still generates an independent test case in the report — you just avoid duplicating the setup logic.

---

## Appendix: Full MILP Variable/Constraint Reference

The complete solver is in `VEN/src/controller/milp_planner.rs`. Key code locations:

| Component | Lines | Description |
|---|---|---|
| `MilpInputs` struct | 74-159 | All input parameters |
| `MilpWeights` struct | 45-69 | Objective function coefficients |
| `build_milp_inputs()` | 260-530 | State → MILP input translation |
| `solve_milp()` | 578-1045 | Variable creation, objective, constraints |
| Variable declarations | 595-765 | All decision variables |
| Objective function | 769-833 | Cost/penalty/reward terms |
| Per-slot constraints | 837-906 | Power balance, bounds, mutual exclusion |
| Energy constraints | 914-948 | EV/heater deadline energy |
| Transition constraints | 952-975 | Startup counting, ramp capture |
| McCormick constraints | 978-984 | Battery-EV co-existence linearization |
| Shiftable load constraints | 987-993 | Start-once binary sum |
| Solver config | 995-996 | `time_limit(60)`, `mip_gap(0.02)` |
| `PlannerConfig` | `profile.rs:322-396` | All configurable parameters |
| Test profile | `VEN/profiles/test.yaml` | `plan_step_s: 300`, `plan_horizon_h: 24` |
