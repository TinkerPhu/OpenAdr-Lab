# Research: Decouple PROFILE from Domain (Phase 4)

**Branch**: `021-decouple-profile-domain` | **Date**: 2026-05-11

No external unknowns or third-party dependencies. All decisions below are architectural choices
resolved through direct codebase inspection on the `refactoring_phase_3` branch.

---

## Decision 1 — Param struct module locations

**Decision**: Asset-specific params co-located in `assets/<asset>.rs`; cross-cutting params in a new `entities/planner_params.rs`.

**Rationale**: Asset params (BatteryParams, EvParams, HeaterParams, PvParams, BaseLoadParams) have exactly one consumer each — the corresponding asset physics + MILP context code. Co-location is the leanest option (no indirection, no import gymnastics). Cross-cutting types (PlannerObjective, PlannerParams, AbsorberParams, SimulatorParams) are consumed by multiple modules with no single obvious home, so `entities/` is correct — it is already the domain types module.

**Alternatives considered**:
- New `params/` module — adds unnecessary indirection; the Lean Architecture principle rejects abstractions without a concrete need
- All in `entities/` — mixes asset-specific concerns into a shared module; makes each asset file import from entities for its own params

---

## Decision 2 — PlannerParams shape

**Decision**: Single flat `PlannerParams` struct carrying all 28 fields from `PlannerConfig`.

**Rationale**: `PlannerConfig` is already a flat struct in the profile. Adding sub-struct nesting (timing params vs weight params) in the domain would introduce indirection not justified by the current codebase. The internal `Phase1Weights`/`Phase2Weights` derivation in `milp_planner/types.rs` remains unchanged — it derives from `PlannerParams` fields exactly as it currently derives from `PlannerConfig` fields.

**Alternatives considered**:
- Split into `PlannerTimingParams` + `PlannerWeightParams` — adds unnecessary nesting; callers would need to construct two structs and thread them separately

---

## Decision 3 — SimulatorParams and persist.rs

**Decision**: Introduce `SimulatorParams` (tick_s, persist_every_s, report_interval_s) in `entities/planner_params.rs` and thread it into `simulator/persist.rs`.

**Rationale**: `persist.rs` currently imports `Profile` only to get `simulator.persist_every_s`. A targeted `SimulatorParams` struct removes this import cleanly. This is the same pattern applied to every other domain module in this phase.

**Alternatives considered**:
- Pass `persist_every_s: u64` directly (rather than a struct) — acceptable for a single field, but using the same pattern as other modules keeps the approach consistent; the struct also documents intent

---

## Decision 4 — HeaterParams: pre-resolve optional fields

**Decision**: `HeaterParams` stores pre-computed effective values (thermal_mass_kwh_per_c, k_loss_kw_per_c, draw_kw, switching_penalty_eur as plain `f64`, not `Option<f64>`). `mid_kw: Option<f64>` is kept optional because its `None` meaning is semantically significant (one-level vs two-level heater model).

**Rationale**: `HeaterConfig` uses `effective_*()` helper methods to resolve defaults for these fields. The resolution logic belongs in the assembly/infrastructure layer where the profile is read — the domain should receive plain values it can use directly. This is cleaner and avoids duplicating resolution logic in domain tests.

**Alternatives considered**:
- Keep Option<f64> fields on HeaterParams with the same effective_* helpers — adds resolution logic to the domain unnecessarily

---

## Decision 5 — PlannerObjective transition strategy

**Decision**: Move `PlannerObjective` to `entities/planner_params.rs` and add a temporary re-export `pub use crate::entities::planner_params::PlannerObjective;` in `profile.rs`. Remove the re-export only after all domain callers are updated to import from `entities/`.

**Rationale**: `PlannerObjective` is referenced in 10+ domain files and also in `main.rs` and `AppCtx`. A re-export in profile.rs allows the files to be updated incrementally (one per task) without intermediate compile failures. This is the same pattern used in large Rust codebase refactors.

**Alternatives considered**:
- Simultaneous update of all callers in one pass — riskier (one large diff, harder to review); the incremental approach keeps each task's diff small and reviewable

---

## Decision 6 — GridParams: not a domain type in Phase 4

**Decision**: `GridConfig` (max_import_kw, max_export_kw) is NOT given a domain param struct in this phase.

**Rationale**: `GridConfig` is currently only consumed by `milp_planner` through the `Profile` reference. However, looking at the actual milp_planner imports, the grid limits are passed via `SimSnapshot` (which already carries `grid_import_limit_kw` and `grid_export_limit_kw` as derived from profile at SimState construction time). The milp_planner does not import GridConfig from profile — it reads grid limits from SimSnapshot. No action required for Phase 4.

**Evidence**: `grep "GridConfig\|grid:" VEN/src/controller/milp_planner/` — no direct GridConfig imports found in milp_planner files.

---

## Decision 7 — Application-layer assembly placement

**Decision**: The Profile → domain params assembly is implemented as a private helper function `build_domain_params(profile: &Profile) -> DomainParams` (or equivalent inline construction) in `main.rs`.

**Rationale**: `services/` is the Phase 5 application layer that doesn't yet exist as a meaningful abstraction. For Phase 4, keeping assembly in `main.rs` is the simplest correct placement — it's where the profile is loaded and where all other startup wiring happens. The application layer constitution principle is satisfied: `main.rs` is the only non-domain ring file that has always imported `profile`.

**Alternatives considered**:
- New `startup.rs` or `wiring.rs` module — adds a file without adding clarity for a single-use function; deferred to Phase 5 if needed
- Move to `services/` now — premature; Phase 5 hasn't defined service boundaries yet

---

## Discovery: `main.rs` `active_objective` wiring

The `active_objective: Arc<RwLock<PlannerObjective>>` field in `AppCtx` and the watch-channel-based runtime objective override (used by `routes/hems.rs` and `tasks/planning.rs`) passes `PlannerObjective` at runtime without touching the profile. After `PlannerObjective` moves to `entities/`, this mechanism continues to work unchanged — the type simply has a different home module. `main.rs` initialises `active_objective` with `profile.planner.objective` mapped to the domain type at startup.

---

## Discovery: `simulator/mod.rs` `from_profile()` scope

`SimState::from_profile(profile)` in `simulator/mod.rs` constructs `SimState` from a `Profile` reference. After Phase 4 it becomes `SimState::from_params(asset_params: &[AssetParams])` where `AssetParams` is an enum or a struct wrapping the typed asset param structs. The `persist.rs` `load_with_profile()` function also passes `profile` to `from_profile()` — after Phase 4 it becomes `load_with_params()` accepting the assembled domain params. Both sites are updated together in one task.

---

## Phase 1 Constitution Re-check

Post-design review against Principle VI invariants:

```
grep -r "use crate::profile" VEN/src/entities VEN/src/assets VEN/src/controller VEN/src/simulator
```

**Expected result after Phase 4**: zero matches.

Line-count check on new files:
- `entities/planner_params.rs`: ~150 lines (enum 20 + PlannerParams 60 + AbsorberParams 30 + SimulatorParams 10 + defaults 30) → well under 500
- Per-asset params additions: ~20–30 lines each → no file crosses 500

**Constitution Re-check: PASSED.**
