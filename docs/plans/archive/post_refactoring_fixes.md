# Plan: Fix Remaining Architecture Invariant Gaps + Missing Test Deliverables

## Context

Post-implementation verification of `docs/plans/ven_backend_architecture_refactoring_v2.md`
chapters 6 & 7 revealed 4 items still unresolved after phases 1–4 were committed (027–029):

1. **Invariant 4** — `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` is not empty: 4 files were
   missed during Phase 4 (VG-05/06 scope only covered `planning.rs` + `sim_tick/`).
2. **Invariant 5** — `grep -r "use crate::assets|use crate::simulator" VEN/src/services` is not empty:
   `services/obligation.rs` uses `SimState` (not covered by Phase 5 / VG-07 scope).
3. **Ch6 Phase 3 test missing** — `tick_once()` has no unit test; tick.rs is at 193 lines (≤200 limit).
4. **Stale invariant path** — original plan §8 grep uses `controller/milp` but the directory is
   `controller/milp_planner`.

User confirmed: all 4 items are **in scope** and should be fixed now.

---

## Item 1 — VtnClient in 4 remaining task files (Invariant 4)

**Root cause:** `obligation.rs`, `poll_events.rs`, `poll_programs.rs`, `poll_reports.rs` were not
listed in the VG-05/06 violation inventory. Each already internally casts to `&dyn VtnPort` for
method calls, so the change is mechanical.

### Files to change

**`VEN/src/tasks/poll_programs.rs`**
- `spawn_program_poll(state, vtn: VtnClient, secs)` → `vtn: Arc<dyn VtnPort>`
- Remove `use crate::vtn::VtnClient`; add `use std::sync::Arc`
- `VtnPort` already imported (`use crate::controller::VtnPort`)
- Inside closure: replace `let vtn_port: &dyn VtnPort = &vtn;` with direct `vtn.fetch_programs().await`

**`VEN/src/tasks/poll_reports.rs`**
- Same pattern as poll_programs.rs
- `spawn_report_poll(state, vtn: VtnClient, secs)` → `vtn: Arc<dyn VtnPort>`
- Remove `use crate::vtn::VtnClient`; add `use std::sync::Arc`
- Direct `vtn.fetch_reports_raw().await`

**`VEN/src/tasks/poll_events.rs`**
- `spawn_event_poll(...)` → change `vtn: VtnClient` to `vtn: Arc<dyn VtnPort>`
- Remove `use crate::vtn::VtnClient`
- Same cast-removal pattern

**`VEN/src/tasks/obligation.rs`**
- `spawn_obligation_check(state, sim, vtn: VtnClient, ven_name)` → `vtn: Arc<dyn VtnPort>`
- Remove `use crate::vtn::VtnClient`
- Pass `vtn.as_ref()` (or `&*vtn`) to `ObligationService::check_and_report()` which already
  accepts `&dyn VtnPort`

**`VEN/src/main.rs`**
- The `vtn_port: Arc<dyn VtnPort>` wrapping already exists (for planning + sim_tick).
- Change the 4 spawn call sites to pass `vtn_port.clone()` instead of `vtn.clone()`.

### Invariant after fix
```bash
grep -r "use crate::vtn::VtnClient" VEN/src/tasks  # → empty
```

---

## Item 2 — SimState in services/obligation.rs (Invariant 5)

**Root cause:** `ObligationService::check_and_report` locks `Arc<Mutex<SimState>>` and iterates
`sim_guard.assets` to extract `(id, history_slice)`. This is the same extraction pattern as
Phase 1 (reporter), which already defined `AssetReportSample` in `controller/reporter.rs`.

### Approach
Move the lock acquisition + sample extraction up to the caller (the task layer), then pass
domain types into the service. Mirrors the reporter pattern exactly.

### Files to change

**`VEN/src/controller/reporter.rs`** — no change needed; `AssetReportSample` already exported.

**`VEN/src/services/obligation.rs`**
- `check_and_report(state, sim: &Arc<Mutex<SimState>>, vtn, ven_name, now)`
  → `check_and_report(state, asset_samples: HashMap<String, Vec<AssetReportSample>>, vtn, ven_name, now)`
- Remove `use crate::simulator::SimState` (both at line 8 and in test module at line 89)
- Add `use crate::controller::reporter::AssetReportSample` and `use std::collections::HashMap`
- Replace internal lock/extract logic with direct iteration over `asset_samples`
- The per-point mapping (`p.ts`, `p.power_kw`, `p.state.soc()`) maps directly to
  `AssetReportSample { ts, power_kw, soc }`

**`VEN/src/tasks/obligation.rs`**
- Before calling `ObligationService::check_and_report(...)`:
  acquire sim lock, build `HashMap<String, Vec<AssetReportSample>>` from `sim_guard.assets`,
  release lock, then call service with the map.
- Add imports: `use crate::controller::reporter::AssetReportSample`,
  `use std::collections::HashMap`, `use chrono::Duration`

### Invariant after fix
```bash
grep -r "use crate::assets|use crate::simulator" VEN/src/services  # → empty
```

---

## Item 3 — Phase 3 test: tick_once with AbsorberParams (Ch6)

`tick_once()` signature has 15 parameters including `Arc<Mutex<SimState>>` (not a port). A
minimal smoke test that calls it without YAML/Profile is the goal.

### Approach
Create a new file `VEN/src/tasks/sim_tick/tick_tests.rs` with `#[cfg(test)]`.
Add `#[cfg(test)] mod tick_tests;` in `tick.rs` (1 line → 194 lines total, within limit).

### Files to change

**`VEN/src/tasks/sim_tick/tick_tests.rs`** (new file)
```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::{Mutex, watch};
    use std::sync::atomic::AtomicBool;

    use crate::entities::planner_params::AbsorberParams;
    use crate::simulator::{SimState, AbsorberState};
    use crate::state::AppState;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::tasks::sim_tick::tick::tick_once;
    // PlanTrigger, PlannerEventTx — import from wherever they're defined

    #[tokio::test]
    async fn tick_once_runs_without_profile() {
        let sim = Arc::new(Mutex::new(SimState::default_for_test()));
        let (trigger_tx, _rx) = watch::channel(PlanTrigger::Periodic);
        let (event_tx, _event_rx) = /* channel */;
        let deviation = Arc::new(AtomicBool::new(false));
        let vtn = Arc::new(MockVtn::new());

        let (_abs_state, _pc, _rc) = tick_once(
            AbsorberState::default(),
            AppState::new(),
            sim,
            AbsorberParams::default(),
            "test-ven".to_string(),
            vtn,
            Arc::new(trigger_tx),
            "/tmp".to_string(),
            event_tx,
            deviation,
            0, 100,   // persist_counter, persist_every_ticks
            0, 100,   // report_counter, report_every_ticks
            1,        // tick_s
        ).await;
        // passes if no panic
    }
}
```

**`VEN/src/tasks/sim_tick/tick.rs`**
- Add `#[cfg(test)] mod tick_tests;` at the end (line 194)

**Dependencies to verify during implementation:**
- Whether `AbsorberParams` derives `Default` — if not, add `#[derive(Default)]` in
  `entities/planner_params.rs`
- Whether `SimState` has a `default_for_test()` or `new()` constructor — if not, add one
- Whether `AbsorberState` derives `Default`

---

## Item 4 — Phase 4 test: spawn_planning smoke test with MockVtn (Ch6)

`spawn_planning` is at `VEN/src/tasks/planning.rs` (259 lines, well within 500 limit).

### Files to change

**`VEN/src/tasks/planning.rs`**
- Add `#[cfg(test)] mod tests { ... }` at the bottom
- Test constructs: `MockVtn::new()`, minimal `PlannerParams`, empty `Vec<AssetParams>`,
  `AppState::new()`, `SimState`, watch channels, `Arc<RwLock<PlannerObjective>>`, event_tx,
  `AtomicBool`
- Calls `spawn_planning(...)`, then immediately calls `.abort()` on the handle
- Asserts handle is aborted (or simply verifies no panic on construction)

**Key imports in test module:**
- `use crate::services::test_support::mock_vtn::MockVtn`
- `use crate::entities::planner_params::PlannerParams`
- `use crate::entities::asset_params::AssetParams`

---

## Item 5 — Fix stale invariant path in original plan §8

**File:** `docs/plans/ven_backend_architecture_refactoring.md`

Find the invariant:
```bash
grep -r "use crate::assets::" VEN/src/controller/milp
```
Change `milp` → `milp_planner`:
```bash
grep -r "use crate::assets::" VEN/src/controller/milp_planner
```

Also update the descriptive text around it to note that `*Params` imports are permitted
(physics data structs); the invariant guards against concrete asset types (A_BAT, A_EV, A_HTR).

---

## Execution Order

1. Item 5 (doc fix — 1 line, no risk)
2. Item 1 (mechanical VtnClient → Arc<dyn VtnPort> in 4 task files + main.rs)
3. Item 2 (SimState extraction lift to task layer)
4. Item 3 (tick_tests.rs — verify AbsorberParams::default() first)
5. Item 4 (planning.rs test)

After each item: run `wsl cargo check --manifest-path VEN/Cargo.toml` to catch compile errors.
After all items: run full invariant greps from ch7, then BDD suite on Pi4-Server.

---

## Verification

```bash
# All 5 chapter 7 greps must be empty:
wsl bash -c "grep 'use crate::simulator\|use crate::assets' VEN/src/controller/reporter.rs"
wsl bash -c "grep 'use crate::assets' VEN/src/controller/timeline.rs"
wsl bash -c "grep -r 'use crate::profile' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::assets\|use crate::simulator' VEN/src/services"
wsl bash -c "wc -l VEN/src/tasks/sim_tick/tick.rs"   # ≤ 200

# Unit tests pass locally:
wsl bash -c "cd VEN && cargo test 2>&1 | tail -30"

# BDD suite on Pi4-Server (final gate):
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose run --rm ven-test 2>&1 | tail -30"
```

## Critical files

- `VEN/src/tasks/obligation.rs` — item 1+2 (caller side)
- `VEN/src/tasks/poll_events.rs`, `poll_programs.rs`, `poll_reports.rs` — item 1
- `VEN/src/services/obligation.rs` — item 2
- `VEN/src/main.rs` — item 1 (4 spawn call sites)
- `VEN/src/tasks/sim_tick/tick.rs` — item 3 (add mod declaration)
- `VEN/src/tasks/sim_tick/tick_tests.rs` — item 3 (new file)
- `VEN/src/tasks/planning.rs` — item 4
- `VEN/src/entities/planner_params.rs` — verify AbsorberParams::default()
- `docs/plans/ven_backend_architecture_refactoring.md` §8 — item 5
