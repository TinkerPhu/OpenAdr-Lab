# VEN Backend Architecture Refactoring — v2 (Closing Phase Gaps)

**Status:** Proposed  
**Date:** 2026-05-15  
**Scope:** `VEN/src/` — 7 violations across 6 files  
**Predecessor:** `docs/plans/ven_backend_architecture_refactoring.md`

---

## 1. Context

Phases 1–4 and Phase 7 of the original refactoring plan were implemented. A post-implementation audit reveals 7 violations remain — each is a gap in an already-landed phase: files that were missed during a phase, or cases where a trait was introduced but callers were not fully updated.

The architectural rules are unchanged from the original plan:

Reshape the VEN backend toward **Hexagonal Architecture** (Ports & Adapters) with **Clean Architecture's use-case layer**. These two styles are complementary: Hexagonal defines the structural shape; Clean Architecture adds the dependency rule and the use-case ring that Hexagonal leaves open.

- **Dependency rule:** inner rings never import outer rings.
- **Ring map:** Adapters (`routes/`, `tasks/`) → Application (`services/`) → Domain (`entities/`, `controller/`) → Infra (`simulator/`, `vtn.rs`, `assets/`).
- **Profile rule:** no `use crate::profile` in domain or adapter ring files (Phase 4 of original plan).
- **Port rule:** all infra dependencies crossed via traits — `SimulatorPort`, `VtnPort`, `AssetMilpContext`.

---

## 2. Violation Inventory

| ID | File | Violating Import | Ring Breach | Original Phase Gap |
|----|------|-----------------|-------------|--------------------|
| VG-01 | `controller/reporter.rs:9` | `use crate::assets::HistoryPoint` | Domain → Infra | Phase 2 |
| VG-02 | `controller/reporter.rs:18` | `use crate::simulator::SimState` | Domain → Infra | Phase 2 |
| VG-03 | `controller/timeline.rs:13` | `use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState}` | Domain → Infra | Phase 2 |
| VG-04 | `tasks/sim_tick/tick.rs:10` + `mod.rs:14` | `use crate::profile::Profile` | Adapter holds raw config | Phase 4 |
| VG-05 | `tasks/planning.rs:13` | `use crate::vtn::VtnClient` | Adapter bypasses VtnPort | Phase 7 |
| VG-06 | `tasks/sim_tick/tick.rs:13`, `mod.rs:16`, `publish.rs:16` | `use crate::vtn::VtnClient` | Adapter bypasses VtnPort | Phase 7 |
| VG-07 | `services/user_request.rs:5,9` | `use crate::assets::AssetConfig` + `use crate::simulator::AssetEntry` | App service → Infra | Phase 5 |

VG-01/02/03 are the highest severity: `controller/` is the innermost domain ring; importing from `assets/` and `simulator/` (both infra) is a hard dependency-rule violation making those functions untestable without a live simulator.

---

## 3. New Invariants (additions to original plan §8)

After all phases complete, these greps must return empty:

```bash
# VG-01/02/03: no infra imports anywhere in controller domain
grep -r "use crate::simulator\|use crate::assets" VEN/src/controller

# VG-04: no raw profile in tasks
grep -r "use crate::profile" VEN/src/tasks

# VG-05/06: no concrete VtnClient in tasks
grep -r "use crate::vtn::VtnClient" VEN/src/tasks

# VG-07: no infra imports in services
grep -r "use crate::assets\|use crate::simulator" VEN/src/services
```

---

## 4. Phases

Each phase is independently shippable. All existing tests must remain green after each phase. The BDD suite on Pi4-Server docker is the safety net.

---

### Phase 1 — Controller reporter: domain-side snapshot types (VG-01, VG-02)

**Principle (original plan Phase 2):** domain modules accept `SimSnapshot` or plain scalars, never `&SimState`. `SimSnapshot` (already in `controller/simulator_port.rs`) carries current per-asset state. The reporter also needs _recent history_ — not in `SimSnapshot`. The fix: define a minimal domain-side record type inside `controller/reporter.rs` and have the calling adapter (`publish.rs`) extract and pass pre-sliced history.

#### Files to change

**`VEN/src/controller/reporter.rs`**
- Add at top of file:
  ```rust
  pub struct AssetReportSample {
      pub ts: DateTime<Utc>,
      pub power_kw: f64,
      pub soc: Option<f64>,
  }
  ```
- `latest_net_import_kw(sim: &SimState)` and `latest_net_export_kw(sim: &SimState)` → replace with versions accepting `grid_net_import_kw: f64` / `grid_net_export_kw: f64` as plain scalars. Callers derive these from `SimSnapshot.assets.values()` (sum of positive/negative `power_kw` fields).
- `build_measurement_report(event, sim: &SimState, ven_name)` → `build_measurement_report(event, asset_samples: &HashMap<String, Vec<AssetReportSample>>, grid_net_import_kw: f64, ven_name)`.
- `build_status_report(event, sim: &SimState, ven_name, ...)` → `build_status_report(event, snap: &SimSnapshot, ven_name, ...)` — status reporting only needs current power (available in `AssetSnapshot.power_kw`).
- Remove `use crate::assets::HistoryPoint` and `use crate::simulator::SimState`.

**`VEN/src/tasks/sim_tick/publish.rs`** (the only caller of these reporter functions)
- Before calling `reporter::build_measurement_reports_for_active_events(...)`: acquire sim lock, extract `HashMap<String, Vec<AssetReportSample>>` from `sim.assets[*].history` (map each `HistoryPoint` → `AssetReportSample { ts: p.ts, power_kw: p.power_kw, soc: p.state.soc() }`), then release lock.
- Compute `grid_net_import_kw` / `grid_net_export_kw` from the current `SimSnapshot` before calling.
- Pass the extracted map and scalars to the updated reporter functions.

#### New invariant
```bash
grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs  # → empty
```

#### Test deliverable
`#[cfg(test)]` in `reporter.rs`: `build_measurement_report` with constructed `Vec<AssetReportSample>` fixtures; `build_status_report` with a manually constructed `SimSnapshot` — no `SimState`, no asset module.

**Estimated effort: 1 day**

---

### Phase 2 — Controller timeline: remove concrete asset type embedding (VG-03)

**Principle (original plan Phase 2):** the `TimelineSnapshot` type must live in the domain ring and use only domain types. Currently `TimelineAssetData` embeds three infra types: `AssetConfig` (physics enum from `assets/`), `AssetHistoryBuffer` (VecDeque from `assets/`), `AssetState` (enum from `assets/`). The rendering logic needs only asset type label, ID, and `(ts, power_kw)` pairs — no physics config.

#### Files to change

**`VEN/src/controller/timeline.rs`**
- Remove `use crate::assets::{AssetConfig, AssetHistoryBuffer, AssetState}`.
- Add `use crate::entities::asset::AssetType`.
- Define inside this file:
  ```rust
  pub struct TimelinePoint {
      pub ts: DateTime<Utc>,
      pub power_kw: f64,
  }
  ```
- Redefine `TimelineAssetData`:
  ```rust
  pub struct TimelineAssetData {
      pub asset_id: String,
      pub asset_type: AssetType,        // entities ring — clean
      pub history: Vec<TimelinePoint>,  // domain-side type
      pub current_power_kw: f64,
  }
  ```
- `TimelineSnapshot.grid_history: AssetHistoryBuffer` → `grid_history: Vec<TimelinePoint>`.
- Adjust all downstream functions to use `&[TimelinePoint]` where they previously iterated `&AssetHistoryBuffer`.

**`VEN/src/simulator/mod.rs`** — `to_timeline_snapshot()` (infra side — allowed to import both rings)
- Convert each `AssetEntry.history` (`AssetHistoryBuffer` of `HistoryPoint`) → `Vec<TimelinePoint>` by mapping `{ ts: p.ts, power_kw: p.power_kw }`.
- Convert `AssetConfig` variant to `AssetType` enum value via a `match` on the config variant.
- No logic changes — pure type mapping at the infra boundary.

#### New invariant
```bash
grep "use crate::assets" VEN/src/controller/timeline.rs  # → empty
```

#### Test deliverable
`#[cfg(test)]` in `timeline.rs`: `build_asset_timeline()` called with a manually constructed `TimelineAssetData { history: vec![TimelinePoint {...}], ... }` — no simulator dependency.

**Estimated effort: 1 day**

---

### Phase 3 — Complete Profile decoupling in sim_tick (VG-04)

**Principle (original plan Phase 4):** "The `Profile` struct remains in the infrastructure ring. The application service reads it at startup and constructs domain-level parameter structs." `main.rs::build_domain_params()` already applies this pattern for `PlannerParams`, `SimulatorParams`, and `AssetParams`. The gap: `spawn_sim_tick()` still receives `Arc<Profile>` and passes it down to `tick_once()` and `helpers.rs`.

`AbsorberParams` already exists in `entities/planner_params.rs` — the domain-level struct is ready.

#### Files to change

**`VEN/src/main.rs`** — `build_domain_params()`
- Extract `AbsorberParams` from profile here (alongside the existing `PlannerParams` extraction).
- Add `absorber_params: AbsorberParams` to the return value.
- Pass `absorber_params` to `tasks::spawn_sim_tick(...)`.

**`VEN/src/tasks/sim_tick/mod.rs`** — `spawn_sim_tick()`
- Replace `profile: Arc<Profile>` parameter with `absorber_params: AbsorberParams`.
- Remove `use crate::profile::Profile`.
- Forward `absorber_params` to `tick_once()`.

**`VEN/src/tasks/sim_tick/tick.rs`** — `tick_once()` (⚠ 193/200 lines — ensure net zero additions)
- Replace `profile: Arc<Profile>` with `absorber_params: AbsorberParams` in the function signature.
- Remove `use crate::profile::Profile`.
- Line 96: replace `super::helpers::build_absorber_params(&profile)` with direct use of `absorber_params`.
- Line 170: pass `&absorber_params` to `accumulate_deviation` directly.
- Net effect: removes the import line and rewires two call sites — no line count increase.

**`VEN/src/tasks/sim_tick/helpers.rs`**
- Remove `use crate::profile::Profile`.
- Delete `build_absorber_params(profile: &Profile)` — its caller now has params pre-built.

#### New invariant
```bash
grep -r "use crate::profile" VEN/src/tasks  # → empty
```

#### Test deliverable
`tick_once()` callable in a unit test by constructing `AbsorberParams::default()` — no YAML, no `Profile`.

**Estimated effort: 0.5–1 day**

---

### Phase 4 — Wire VtnPort in all tasks (VG-05, VG-06)

**Principle (original plan Phase 7):** "Define `VtnPort` trait so the domain can call the VTN without knowing `reqwest` exists." `VtnPort` lives in `controller/vtn_port.rs`; `VtnClient` implements it. The gap: all three task files still hold the concrete `VtnClient`. The fix is mechanical — change the parameter type at each spawn call site.

#### Files to change

**`VEN/src/tasks/planning.rs`**
- Replace `vtn: VtnClient` with `vtn: Arc<dyn VtnPort>`.
- Remove `use crate::vtn::VtnClient`; add `use crate::controller::vtn_port::VtnPort` (or the existing re-export in `controller/mod.rs`).

**`VEN/src/tasks/sim_tick/mod.rs`**
- Replace `vtn: VtnClient` with `vtn: Arc<dyn VtnPort>`.
- Remove `use crate::vtn::VtnClient`; add `VtnPort` import.
- Forward `vtn.clone()` to `tick_once()`.

**`VEN/src/tasks/sim_tick/tick.rs`** (⚠ 193/200 lines — one-line type change only)
- Replace `vtn: VtnClient` → `vtn: Arc<dyn VtnPort>` in `tick_once()` signature.
- Remove `use crate::vtn::VtnClient`; add `VtnPort` import.

**`VEN/src/tasks/sim_tick/publish.rs`**
- Replace `vtn: &VtnClient` → `vtn: &dyn VtnPort`.
- Remove `use crate::vtn::VtnClient`; add `VtnPort` import.

**`VEN/src/main.rs`**
- Wrap `VtnClient` before passing to spawn functions:
  ```rust
  let vtn_port: Arc<dyn VtnPort> = Arc::new(vtn.clone());
  tasks::spawn_planning(..., vtn_port.clone(), ...);
  tasks::spawn_sim_tick(..., vtn_port.clone(), ...);
  ```
- Other tasks that poll events/reports continue to receive `VtnClient` through their own existing call path.

#### New invariant
```bash
grep -r "use crate::vtn::VtnClient" VEN/src/tasks  # → empty
```

#### Test deliverable
Demonstrate that `spawn_planning` compiles and runs with `MockVtn` (already in `services/test_support/mock_vtn.rs`) — no live HTTP required.

**Estimated effort: 0.5 day**

---

### Phase 5 — Clean UserRequestService from infra imports (VG-07)

**Principle (original plan Phase 5):** "Route handlers call services. `AppState` becomes a pure state store — no business rules." Application services must not import infrastructure types.

`UserRequestService::create_ev()` and `create_heater()` pass `&[AssetEntry]` and `&[AssetConfig]` (infra types) through to `controller/user_request::create_from_body()`. `AssetParams` already exists in `entities/asset_params.rs` as the domain representation of asset configuration.

#### Files to change

**`VEN/src/controller/user_request.rs`** — `create_from_body()`
- Change `assets: &[AssetEntry], asset_configs: &[AssetConfig]` → `asset_params: &[AssetParams]`.
- Replace all asset lookups inside to use `AssetParams` fields (asset ID, type, capacity values).
- Remove `use crate::assets::AssetConfig` and `use crate::simulator::AssetEntry`.

**`VEN/src/services/user_request.rs`** — `create_ev()` and `create_heater()`
- Change signatures: replace `assets: &[AssetEntry], asset_configs: &[AssetConfig]` with `asset_params: &[AssetParams]`.
- Remove `use crate::assets::AssetConfig` and `use crate::simulator::AssetEntry`.
- Forward `asset_params` directly to `create_from_body()`.

**Callers** (`VEN/src/routes/hems.rs`)
- Build `Vec<AssetParams>` from `AppCtx` at the adapter layer (single lock acquisition on `AppCtx.sim`) before calling the service.

#### New invariant
```bash
grep -r "use crate::assets\|use crate::simulator" VEN/src/services  # → empty
```

#### Test deliverable
`UserRequestService::create_ev()` and `create_heater()` unit tests using `AssetParams` and `CreateUserRequestBody` directly — no `SimState`, no `AssetConfig` construction.

**Estimated effort: 1 day**

---

## 5. Phase Order and Dependencies

```
Phase 1 (reporter)   ─── independent — highest domain purity impact
Phase 2 (timeline)   ─── independent — highest domain purity impact
Phase 3 (Profile)    ─── independent — mechanical Profile removal
Phase 4 (VtnPort)    ─── independent — mechanical type change
Phase 5 (services)   ─── independent
```

Recommended order: **1 → 2 → 3 → 4 → 5** (domain core purity first, adapter wire-up second, service layer last).

---

## 6. Testing Strategy

Follows the test pyramid from the original plan. Each phase ships with tests — no structural-only commits.

| Phase | Minimum test deliverable |
|-------|--------------------------|
| 1 — reporter | `build_measurement_report` + `build_status_report` with `AssetReportSample` / `SimSnapshot` fixtures; no `SimState` |
| 2 — timeline | `build_asset_timeline()` with constructed `Vec<TimelinePoint>` — no simulator |
| 3 — Profile | `tick_once()` with `AbsorberParams::default()` — no YAML or `Profile` |
| 4 — VtnPort | `spawn_planning` smoke test using `MockVtn` from `services/test_support/mock_vtn.rs` |
| 5 — services | `create_ev` + `create_heater` unit tests using `AssetParams` + `CreateUserRequestBody` only |

---

## 7. Success Criteria

| Criterion | How to verify |
|-----------|--------------|
| No infra imports in `controller/reporter.rs` | `grep "use crate::simulator\|use crate::assets" VEN/src/controller/reporter.rs` → empty |
| No infra imports in `controller/timeline.rs` | `grep "use crate::assets" VEN/src/controller/timeline.rs` → empty |
| No raw Profile in `tasks/` | `grep -r "use crate::profile" VEN/src/tasks` → empty |
| All tasks use VtnPort trait | `grep -r "use crate::vtn::VtnClient" VEN/src/tasks` → empty |
| Services have no infra imports | `grep -r "use crate::assets\|use crate::simulator" VEN/src/services` → empty |
| All original plan invariants still hold | see `docs/plans/ven_backend_architecture_refactoring.md` §8 |
| All BDD scenarios green after each phase | BDD test run passes on Pi4-Server docker stack |
| `tick.rs` stays ≤ 200 lines | `wsl wc -l VEN/src/tasks/sim_tick/tick.rs` ≤ 200 |
| No new file exceeds 500 lines | enforced in PR review |



## 8. Clean up

  Over 40 build warnings. Running cargo fix on them. The warnings fall into categories like:
  - #[allow(dead_code)] on fields constructed via serde deserialization (some VTN/port types)
  - Unused variables (e.g. _n hint in heater tests)
  - async fn in traits (pre-async_trait migration artefacts)
  - Clippy-style suggestions like using if let chains

  None of them indicate a bug or architecture issue. They are intentionally deferred because:
  1. They don't affect correctness
  2. Some (like dead_code on serde-deserialized fields) are false positives — the fields ARE used at runtime via
  deserialization
  3. Fixing them all in every feature PR adds noise to the diff

  They are addressed in this dedicated "clippy/warnings cleanup" task, not mixed into structural refactoring PRs where the signal matters more than style. The cargo fix --bin "ven-app" suggestion from the compiler shall apply the mechanical ones automatically when that cleanup PR is done.

Success Criteria:
  ALL tests must be passing/green.