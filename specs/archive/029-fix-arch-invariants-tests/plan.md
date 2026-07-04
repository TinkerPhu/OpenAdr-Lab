# Implementation Plan: Fix Architecture Invariant Gaps and Missing Tests

**Branch**: `029-fix-arch-invariants-tests` | **Date**: 2026-05-16 | **Spec**: [spec.md](spec.md)  
**Input**: Feature specification from `/specs/029-fix-arch-invariants-tests/spec.md`

## Summary

Four residual gaps from the VEN backend architecture refactoring remain after phases 1–4 (features 024–028). This plan closes them in execution order: doc fix → SimState boundary fix → tick_once test → spawn_planning test. No new user-facing behavior; no BDD scenarios added. The success gate is all five invariant greps empty + unit tests green + BDD suite unchanged at 44 features / 238 scenarios.

## Technical Context

**Language/Version**: Rust stable 2021 edition  
**Primary Dependencies**: `tokio`, `serde_json`, `chrono`, `std::collections::HashMap`, `std::sync::Arc` (all existing — no new Cargo.toml entries)  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo test` (unit), `docker compose run --rm ven-test` on Pi4-Server (BDD)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4) via Docker Compose  
**Project Type**: Library / service (VEN backend)  
**Performance Goals**: No change — internal refactor  
**Constraints**: `tasks/` files ≤ 200 lines; `VEN/src/` files ≤ 500 lines  
**Scale/Scope**: 4 source file edits, 1 new test file, 1 doc fix

## Constitution Check

**Principle I (OpenADR Spec Fidelity)**: ✅ No field name or payload structure changes.

**Principle II (BDD-First Testing)**: ✅ No new user behavior — existing BDD suite covers all VEN behavior. Unit tests added for internal functions per the architecture refactoring plan §6 deliverables. New tests are written as part of implementation (not post-hoc).

**Principle III (Upstream Compatibility)**: ✅ No changes to `openleadr-rs/` submodule.

**Principle IV (Lean Architecture)**: ✅ Changes are minimal: one signature lift, one `derive(Default)`, two test modules, one doc fix. No new abstractions.

**Principle V (Infrastructure Parity)**: ✅ BDD gate runs on Pi4-Server via Docker Compose.

**Principle VI (VEN Hexagonal Architecture)**: ✅ This plan *enforces* Principle VI — it removes the last `services/` → `simulator` boundary violation (Invariant 5) and delivers the Phase 3/4 test deliverables mandated by the architecture plan §6.

**File size constraints**: `tick.rs` currently 193 lines → 194 after mod declaration (✅ ≤ 200). `planning.rs` currently 258 lines → ~290 with test module (✅ ≤ 500).

## Project Structure

### Documentation (this feature)

```text
specs/029-fix-arch-invariants-tests/
├── plan.md              ← this file
├── research.md          ← Phase 0 output
├── data-model.md        ← Phase 1 output
├── quickstart.md        ← Phase 1 output
└── tasks.md             ← Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code (changes only)

```text
VEN/src/
├── controller/
│   └── absorber.rs            # Item 3: add #[derive(Default)] to AbsorberState
├── services/
│   └── obligation.rs          # Item 2: remove SimState import; change signature to HashMap param
├── tasks/
│   ├── obligation.rs          # Item 2: add lock+extract before calling service
│   ├── planning.rs            # Item 4: add #[cfg(test)] mod tests { smoke test }
│   └── sim_tick/
│       ├── tick.rs            # Item 3: add #[cfg(test)] mod tick_tests; (1 line)
│       └── tick_tests.rs      # Item 3: new file — tick_once smoke test
docs/plans/
└── ven_backend_architecture_refactoring.md  # Item 5: fix milp → milp_planner path
```

**Structure Decision**: Single-project (VEN backend only). No frontend, no new submodules.

## Phase 0: Research Summary

All unknowns resolved — see [research.md](research.md).

Key findings:
- `AbsorberState` needs `#[derive(Default)]` (all fields have natural zero defaults)
- `SimState` for tests: use `serde_json::from_value(json!({...}))` pattern (already in obligation tests)
- `PlannerEventTx = Arc<broadcast::Sender<PlannerEvent>>` → `Arc::new(broadcast::channel::<PlannerEvent>(1).0)`
- ObligationService test module also imports `SimState` (line 89) — must be cleaned up too
- No new BDD scenarios needed (internal architecture fix only)

## Phase 1: Design Detail

### Item 5 (doc fix)

**File**: `docs/plans/ven_backend_architecture_refactoring.md`

Find and replace in §8 invariant grep section:
```
grep -r "use crate::assets::" VEN/src/controller/milp
```
→
```
grep -r "use crate::assets::" VEN/src/controller/milp_planner
```

Also update surrounding text: clarify that `*Params` struct imports (e.g. `BatteryParams`, `EvParams`) are permitted in `milp_planner`; the invariant guards against concrete asset type imports (`A_BAT`, `A_EV`, `A_HTR`).

Secondary: verify if `.claude/CLAUDE.md` `ven-architecture` section also references `controller/milp` (research found it does) — fix that path too.

---

### Item 2 (SimState leak in ObligationService)

**`VEN/src/services/obligation.rs`**

Remove:
```rust
use crate::simulator::SimState;  // line 8
use tokio::sync::Mutex;
```

Change signature:
```rust
pub async fn check_and_report(
    state: &AppState,
    asset_samples: std::collections::HashMap<String, Vec<crate::controller::reporter::AssetReportSample>>,
    vtn: &dyn VtnPort,
    ven_name: &str,
    now: DateTime<Utc>,
) -> Result<()>
```

Remove internal lock block (lines 28–49); replace with direct iteration over `asset_samples`.

Test module (line 85+): remove `use crate::simulator::SimState` (line 89); replace `make_sim()` helper with a direct `HashMap::new()` (or a helper that returns a pre-built sample map).

**`VEN/src/tasks/obligation.rs`**

Add before the `ObligationService::check_and_report` call:
```rust
use chrono::Duration;
use crate::controller::reporter::AssetReportSample;
use std::collections::HashMap;

let asset_samples: HashMap<String, Vec<AssetReportSample>> = {
    let sim_guard = sim.lock().await;
    sim_guard
        .assets
        .iter()
        .map(|entry| {
            let history = entry.history.slice(Duration::seconds(3600), now);
            let samples = history
                .iter()
                .map(|p| AssetReportSample {
                    ts: p.ts,
                    power_kw: p.power_kw,
                    soc: p.state.soc(),
                })
                .collect();
            (entry.id.clone(), samples)
        })
        .collect()
};
```

Then pass `asset_samples` to `ObligationService::check_and_report(...)`.

---

### Item 3 (tick_once test)

**`VEN/src/controller/absorber.rs`**

Add `Default` to derive on `AbsorberState`:
```rust
#[derive(Debug, Clone, Default)]
pub struct AbsorberState { ... }
```

**`VEN/src/tasks/sim_tick/tick.rs`**

Add at end of file (line 194):
```rust
#[cfg(test)]
mod tick_tests;
```

**`VEN/src/tasks/sim_tick/tick_tests.rs`** (new file)

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::{Mutex, watch, broadcast};

    use crate::controller::VtnPort;
    use crate::controller::absorber::AbsorberState;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::planner_params::AbsorberParams;
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::simulator::SimState;
    use crate::state::AppState;
    use crate::tasks::sim_tick::tick::tick_once;

    fn minimal_sim() -> Arc<Mutex<SimState>> {
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState");
        Arc::new(Mutex::new(s))
    }

    #[tokio::test]
    async fn tick_once_runs_without_profile() {
        let sim = minimal_sim();
        let (trigger_tx, _trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let trigger_tx = Arc::new(trigger_tx);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let deviation_pending = Arc::new(AtomicBool::new(false));
        let vtn: Arc<dyn VtnPort> = Arc::new(MockVtn::new());

        let (_abs, _pc, _rc) = tick_once(
            AbsorberState::default(),
            AppState::new(),
            sim,
            AbsorberParams::default(),
            "test-ven".to_string(),
            vtn,
            trigger_tx,
            "/tmp".to_string(),
            event_tx,
            deviation_pending,
            0, 100,   // persist_counter, persist_every_ticks → no persist
            0, 100,   // report_counter, report_every_ticks → no report
            1,        // tick_s
        )
        .await;
        // passes if no panic
    }
}
```

---

### Item 4 (spawn_planning smoke test)

**`VEN/src/tasks/planning.rs`** — append at end of file:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::{Mutex, RwLock, broadcast, watch};

    use crate::controller::absorber::AbsorberState;
    use crate::entities::asset::PlanTrigger;
    use crate::entities::planner_params::{PlannerObjective, PlannerParams};
    use crate::planner_events::PlannerEvent;
    use crate::services::test_support::mock_vtn::MockVtn;
    use crate::simulator::SimState;
    use crate::state::AppState;
    use super::spawn_planning;

    fn minimal_sim() -> Arc<Mutex<SimState>> {
        let s: SimState = serde_json::from_value(serde_json::json!({
            "asset_configs": [],
            "assets": [],
            "grid": {
                "net_power_w": 0.0, "import_w": 0.0, "export_w": 0.0,
                "voltage_v": 0.0, "import_kwh": 0.0, "export_kwh": 0.0
            },
            "last_tick": chrono::Utc::now().to_rfc3339()
        }))
        .expect("minimal SimState");
        Arc::new(Mutex::new(s))
    }

    #[tokio::test]
    async fn spawn_planning_constructs_without_panic() {
        let (trigger_tx, trigger_rx) = watch::channel(PlanTrigger::Periodic);
        let (event_bcast_tx, _) = broadcast::channel::<PlannerEvent>(1);
        let event_tx = Arc::new(event_bcast_tx);
        let vtn = Arc::new(MockVtn::new());
        let sim = minimal_sim();
        let active_objective = Arc::new(RwLock::new(PlannerObjective::default()));
        let deviation_pending = Arc::new(AtomicBool::new(false));

        let handle = spawn_planning(
            AppState::new(),
            PlannerParams::default(),
            10.0,  // grid_max_import_kw
            10.0,  // grid_max_export_kw
            vec![],
            vtn,
            "test-ven".to_string(),
            trigger_rx,
            sim,
            active_objective,
            event_tx,
            deviation_pending,
        );
        handle.abort();
        // passes if no panic during construction and abort
        let _ = trigger_tx; // keep alive until abort
    }
}
```

## Complexity Tracking

No violations. All changes are the simplest possible solution for each item.
