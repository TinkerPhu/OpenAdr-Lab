# Plan: Plan Slot Alignment + 3-Tier Variable-Step MILP Solver

**Status:** Part A — ready to implement. Part B — planned, not yet detailed.  
**Branch pattern:** `refactor/slot-alignment` (Part A), `NNN-3-tier-solver` (Part B)  
**Architecture reference:** `docs/architecture/ven_milp_planner.md`

---

## Preamble: Two independent parameters

`replan_interval_s` (how often the planning loop fires) and `plan_step_s` (the planning grid step) are **independently configured**. They happen to be coincidentally close in the current production setup (`replan_interval_s = 300 s`, `plan_step_s = 600 s`), but they have no inherent coupling. The alignment and gate-stability guarantees in this plan hold for any combination of these two values. Documentation and code must never assume they are equal or related.

---

## Preamble: Phase 1 warm-start is deferred

Part A implements **slot alignment only**. Warm-starting Phase 1 from the previous plan (using the aligned grids to map `Plan` slots back to MILP variable initial values) is a meaningful optimization but is a separate concern. It is deferred because:

1. Alignment (A1) delivers the primary benefit — gate stability and clean slot boundaries — without warm-start.
2. Warm-starting Phase 1 requires reconstructing binary variable states (`z_heat_mid`, `z_heat_full`) from the stored plan, which needs `MilpInputs` thresholds, making `plan_to_solve_output` depend on `MilpInputs`. This creates a sequencing constraint (build inputs first, then warm-start) that should be designed cleanly in isolation.
3. Phase 2 warm-start from Phase 1 is already implemented and provides the bulk of the integer-incumbent benefit.

When Phase 1 warm-start is implemented (as a follow-up branch), the aligned grid is its prerequisite — slot `t` of the new plan will map exactly to slot `t` of the previous plan whenever `now_aligned` does not advance (replans within the same zone window). This plan leaves the hooks in place but does not implement the reconstruction.

---

## Part A — Slot alignment

### Why alignment matters

The VEN replans every `replan_interval_s` seconds. Without rounding, each replan's `now` is an arbitrary wall-clock second. Consecutive replans produce slot grids with different boundaries (14:23:47, 14:28:47, ...) so the acceptance gate compares plans that cover different time ranges per slot — making the cost delta meaningless and the adoption decision noisy.

With alignment, `now` is always `floor(wall_clock / step_s) * step_s`. All replans within the same `step_s` window share an identical slot grid. The gate compares plans slot-for-slot. When `now_aligned` does advance (at a zone boundary), it advances by exactly one `step_s` — a clean one-slot shift, which is also the ideal condition for Phase 1 warm-starting (same physical time per slot index).

Secondary benefits: clean tooltip timestamps in the UI, consistent block-commitment anchor positions across replans.

---

### A0 — Add `HorizonZone` to `entities/plan.rs` (Part B scaffolding)

**Architecture note:** `entities/` is the domain ring and cannot import `profile.rs` (infra/outer ring). Although `profile::PlanZone` and the new `HorizonZone` are structurally identical, they must be defined separately to preserve the dependency rule. `PlanZone` stays in `profile.rs` (deserialization input); `HorizonZone` lives in `entities/plan.rs` (domain model). Mapping happens in `controller/milp_planner/results.rs` (application/adapter layer).

**File: `VEN/src/entities/plan.rs`**

Add before `PlanningHorizon`:
```rust
/// One zone of a variable-step planning horizon.
/// Mirrors `profile::PlanZone` but lives in the domain layer (no profile import).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HorizonZone {
    /// Step width for this zone in seconds.
    pub step_s: u64,
    /// Number of slots in this zone.
    pub slots: usize,
}
```

Add to `PlanningHorizon`:
```rust
/// Zone definitions for variable-step plans.
/// Empty in uniform-step plans (current default). Part B populates this with 3 entries.
/// When non-empty, the first zone's step_s is the finest resolution (Zone A).
#[serde(default)]
pub zones: Vec<HorizonZone>,
```

**File: `VEN/src/controller/milp_planner/results.rs`**

In `translate_to_plan`, add helper import (private to the function or at top of file):
```rust
use crate::entities::plan::HorizonZone;
```

In `translate_to_plan`, populate `zones` when building `PlanningHorizon`:
```rust
let horizon = PlanningHorizon {
    start_time: now,
    end_time: horizon_end,
    step_size_s: step_s,
    num_steps: n,
    far_horizon: horizon_end,
    zones: vec![HorizonZone { step_s, slots: n }],  // single-zone: matches current uniform step
};
```

Same in `fallback_plan`:
```rust
let horizon = PlanningHorizon {
    start_time: now,
    end_time: horizon_end,
    step_size_s: step_s,
    num_steps: total_steps,
    far_horizon: horizon_end,
    zones: vec![HorizonZone { step_s, slots: total_steps }],
};
```

**Test:** Add to `entities/plan.rs` tests:
```rust
#[test]
fn test_horizon_zone_serde_roundtrip() {
    let z = HorizonZone { step_s: 300, slots: 96 };
    let json = serde_json::to_string(&z).unwrap();
    let back: HorizonZone = serde_json::from_str(&json).unwrap();
    assert_eq!(back, z);
}
```

---

### A1 — Round `now` to step boundary in `tasks/planning.rs`

#### The rounding function

Extract as a named, testable free function at the top of the `tasks/planning.rs` module:

```rust
/// Align a UTC timestamp down to the nearest `step_s`-second boundary.
/// All plan replans within the same `step_s` window produce identical slot grids,
/// which is the prerequisite for slot-for-slot gate comparisons and Phase 1 warm-starting.
///
/// Uses rem_euclid to handle pre-epoch timestamps correctly (safe for any era).
fn align_to_step(raw: DateTime<Utc>, step_s: u64) -> DateTime<Utc> {
    let ts = raw.timestamp();
    let step = step_s as i64;
    DateTime::<Utc>::from_timestamp(ts - ts.rem_euclid(step), 0)
        .expect("step-aligned timestamp is always valid")
}
```

#### Changes to the planning loop

At line 41, replace:
```rust
let now = Utc::now();
```
With:
```rust
let wall_now = Utc::now();
let now = align_to_step(wall_now, planner.plan_step_s);
```

`wall_now` is used only to set `Plan.created_at` (see below). `now` (aligned) is used for all planning computations: slot timestamps, tariff sampling, asset deadline steps, anchor filtering, envelope computation, and status reports.

#### Patch `Plan.created_at` after solve

The plan is built inside `spawn_blocking` using `now` (aligned). After the blocking solve returns, patch `created_at` to wall-clock time so the gate decay formula measures real elapsed time:

```rust
let mut plan = tokio::task::spawn_blocking(move || {
    controller::milp_planner::run_planner(...)
})
.await
.expect("planner task panicked");

// created_at = real wall-clock time so gate decay (elapsed_s) measures actual age.
// horizon.start_time = now_aligned (grid origin for slot-for-slot comparisons).
plan.created_at = wall_now;
```

This requires `plan` to be `mut`. The `Plan` struct is `Clone + Debug` and mutable field assignment is straightforward.

#### Why `created_at = wall_now`

`evaluate_acceptance_gate` computes:
```rust
elapsed_s = (now - current.created_at).num_seconds()
decay_factor = (1.0 - elapsed_s / decay_s).max(0.0)
```

If `created_at = now_aligned`, two replans within the same window both have `elapsed_s ≈ 0`, making decay inactive until the window advances. With `created_at = wall_now`, decay accumulates in real time — the existing 1500 s default makes every plan decay gracefully regardless of window alignment. The behaviour is unchanged from before alignment was added.

#### Note on PV irradiance patch

The irradiance patch at lines 87–94 uses `now` to compute the natural irradiance at the plan's time origin. After alignment, `now` is up to `step_s − 1` seconds in the past, producing a sin-model irradiance that may differ by at most `~0.1 kW` for a typical 10 kW installation. This error does **not** cumulate across replans (each replan recomputes from fresh sensor state via `inject_snap`). Acceptable per design decision.

#### Tests for `align_to_step`

Add to the `#[cfg(test)]` block in `tasks/planning.rs`:
```rust
#[test]
fn test_align_to_step_rounds_down() {
    use chrono::TimeZone;
    let make = |h: u32, m: u32, s: u32| Utc.with_ymd_and_hms(2026, 4, 11, h, m, s).unwrap();

    // step_s = 600 (10 min)
    assert_eq!(align_to_step(make(14, 23, 47), 600), make(14, 20, 0));
    assert_eq!(align_to_step(make(14, 20,  0), 600), make(14, 20, 0));  // already aligned
    assert_eq!(align_to_step(make(14, 29, 59), 600), make(14, 20, 0));

    // step_s = 300 (5 min)
    assert_eq!(align_to_step(make(14, 23, 47), 300), make(14, 20, 0));
    assert_eq!(align_to_step(make(14, 25,  0), 300), make(14, 25, 0));  // already aligned
    assert_eq!(align_to_step(make(14, 24, 59), 300), make(14, 20, 0));

    // result is always a multiple of step_s past midnight
    for step_s in [300u64, 600, 900, 1800] {
        let raw = make(14, 23, 47);
        let aligned = align_to_step(raw, step_s);
        assert_eq!(aligned.timestamp() % step_s as i64, 0,
            "step_s={step_s}: aligned timestamp not a multiple of step_s");
    }
}
```

---

### A2 — Fix `zones_from_plan` zone origin in `routes/timeline.rs`

**Context:** `zones_from_plan` returns the zone list for `GET /timeline/all`. The `from` field currently uses the HTTP request's `now` (wall clock), not the plan's grid origin. After alignment, `plan.horizon.start_time = now_aligned` — this is the semantically correct `from` for the plan zone: it's where the first slot starts, and what the UI should use as the left boundary for zone background shading.

**File: `VEN/src/routes/timeline.rs`**

Change:
```rust
vec![serde_json::json!({ "from": now, "to": end, "step_s": step_s })]
```
To:
```rust
vec![serde_json::json!({ "from": plan.horizon.start_time, "to": end, "step_s": step_s })]
```

The `now` parameter in `zones_from_plan` is still used to filter out expired plans (`if end <= now { return vec![] }`). Keep that usage.

**Timeline now-point is unaffected (D2):** The "now point" is built by `build_now_point(asset_id, now, snap)` where `now` is the HTTP request time and `snap` is the live simulator state. This path does not use the plan's aligned grid at all. The now-point always carries the exact request timestamp and the exact simulation value at that moment. No change needed.

**Test update:** In `routes/timeline.rs` tests, the existing assertion:
```rust
assert_eq!(zones[0]["from"], ...);  // update to use plan.horizon.start_time
```
Ensure the test builds a plan with an explicit `start_time` and asserts `zones[0]["from"]` equals that `start_time`, not the HTTP request time.

---

### A3 — Heater anchor behaviour during first-cycle transition

When alignment is first deployed, the currently-active plan was built with an unaligned `now_old`. The next replan aligns to `now_aligned ≤ now_old` (up to `step_s − 1` s earlier). `build_heater_anchor` maps old-plan slot indices to new-plan slot indices positionally. The physical time windows overlap by `(step_s − offset)` out of `step_s` seconds — a small mismatch for exactly one replan cycle.

The anchor is a soft hint (the MILP penalises deviation via `heat_initial_z_mid/full` and the lock constraint, but violations are feasible). The one-cycle mismatch does not accumulate: the second replan reads the first aligned plan as its `current_plan`, achieving exact slot-for-slot correspondence. No code change needed; document in KEY_LEARNINGS.

---

### A4 — Verification

#### Local (no Pi4 needed)
1. `wsl cargo check` — must compile clean
2. `wsl cargo test -p ven` — all green; new tests for `align_to_step` and `HorizonZone` pass

#### Manual on Pi4
3. Rebuild and deploy: `ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose build ven && docker compose up -d --force-recreate ven-1 ven-2 ven-3"`
4. Wait for first solve cycle (~80 s after startup)
5. `curl .../api/ven-1/plan | jq '.horizon.start_time, .slots[0].start'` — both should show a clean multiple of `plan_step_s` (e.g. `14:20:00Z`)
6. `curl .../api/ven-1/plan | jq '.created_at'` — should show wall-clock time, a few seconds after the aligned time
7. `curl .../timeline/all | jq '.zones[0].from'` — must equal `plan.horizon.start_time`, not the request time
8. Wait for a second solve cycle; confirm `horizon.start_time` is still the same value (within-window stability)
9. Wait for a window boundary (`plan_step_s = 600 s`); confirm `horizon.start_time` advances by exactly 600 s

#### E2E
10. `bash run_all_tests.sh --e2e` on Pi4 — all ~49 scenarios green
11. No timeout regressions (alignment does not affect MILP solve time; gate decisions change only at window boundaries)

---

### A5 — Determinism note (TECHNICAL_DEBTS.md)

`tasks/planning.rs` uses `Utc::now()` which violates the determinism guideline (inject clock as `Fn() → DateTime<Utc>` parameter). This was already a pre-existing debt. Alignment does not worsen it — `align_to_step` itself is a pure function and fully testable. Add a record to `docs/reference/TECHNICAL_DEBTS.md`:

> **planning clock injection** (Trivial effort): `tasks/planning.rs` calls `Utc::now()` directly. Should accept an injectable clock to make the planning loop fully testable without wall-clock coupling. Blocked on threading the clock through `spawn_planning`'s argument list.

---

## File Change Summary — Part A

| File | Change | Step |
|---|---|---|
| `VEN/src/entities/plan.rs` | Add `HorizonZone` struct; add `zones` field to `PlanningHorizon` | A0 |
| `VEN/src/controller/milp_planner/results.rs` | Import `HorizonZone`; populate `zones` in `translate_to_plan` and `fallback_plan` | A0 |
| `VEN/src/tasks/planning.rs` | Add `align_to_step` fn + tests; round `now`; patch `plan.created_at = wall_now` | A1 |
| `VEN/src/routes/timeline.rs` | `zones_from_plan`: change `from: now` → `from: plan.horizon.start_time`; update test | A2 |
| `docs/reference/TECHNICAL_DEBTS.md` | Add planning clock injection debt entry | A5 |

**Unchanged by Part A:** `milp_planner/inputs.rs`, `solver_phase1.rs`, `solver_phase2.rs`, `mod.rs`, `services/planning.rs`, `entities/planner_params.rs`, `profile.rs`, profiles YAML, all UI files.

---

## Part B — 3-Tier Variable-Step MILP Solver

*(Detailed substep planning deferred — will be a separate planning session once Part A is merged and verified.)*

Part A is the prerequisite for Part B. With aligned grids:
- `now_aligned` uses `plan_zones[0].step_s` (Zone A) as the rounding base — update A1's `planner.plan_step_s` reference to `planner.plan_zones[0].step_s` once zones are wired into `PlannerParams`
- `HorizonZone` slots in `PlanningHorizon.zones` will be populated with 3 entries instead of 1
- `dt_h` in `MilpInputs` is already `Vec<f64>` — ready for variable step sizes
- `zones_from_plan` in `timeline.rs` will emit 3 zone entries — the `from: plan.horizon.start_time` fix (A2) is already the right pattern

High-level steps (not yet substep-detailed):
- B1: Wire `plan_zones` from profile into `PlannerParams`
- B2: Build variable `dt_h` from `plan_zones` in `inputs.rs`; replace uniform slot-time arithmetic with cumulative-seconds array; fix 3 reverse-mapping calls
- B3: Populate `PlanningHorizon.zones` with all 3 zones in `results.rs`
- B4: Update production profile YAMLs with `plan_zones:` block
- B5: Forward-fill Zone B/C plan slots to Zone A resolution in `timeline.rs`; emit 3-entry `zones_from_plan` output
- B6: Normalise `count_heater_switches` to Zone A units (currently returns `usize`, becomes `f64`)
- B7: Tests + Pi4 validation
