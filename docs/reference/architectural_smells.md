# Architectural smells — sweep of 2026-10-09

A sweep of `VEN/src` (production code only: comments and `#[cfg(test)]` blocks stripped with the same
filter `scripts/audit_ven_architecture.py` uses) and the VEN UI, made on `main` at `b1dcf2eb` after
R-112..R-115 and R-110's first step merged. Items already in `docs/reference/TECHNICAL_DEBTS.md` are
listed at the end, not repeated. Severity uses the S1..S4 scale of `docs/reference/ISSUE_KINDS.md`.

Every item is now either fixed, a register row (R-129..R-132) or closed as out of scope (#1); the register is the authority on
what is open, this file only records the sweep and maps its numbers to rows.

## First: the register itself is wrong

The rebase of R-111 (`01ba552c`) re-added the rows **R-107, R-108 and R-109**, which their own commits
had fixed and deleted (`728f8481`/`330696be`, `768e6e1c`, `8e712be0`..`169d59a1`). The code is fixed
— audit rules 6, 8 and 9 pass — so the register reports fixed work as open, and priority is derived
from it. Second time a stale copy came back through a rebase conflict (the first was `.claude/CLAUDE.md`,
restored in `4d8b0447`). **Fixed**: the rows are deleted (by the `docs/register-cleanup` work, which also found R-117 fixed and re-checked every row against the source).

## New smells, by severity

| # | Sev | Smell | Where | Cost | Now |
|---|---|---|---|---|---|
| 1 | S3 | **The plan contract hard-codes one PV and one battery.** `PlanSlot` carries `baseline_kw`, `pv_forecast_kw`, `pv_used_kw`, `bat_charge_kw`, `bat_discharge_kw` beside the generic `allocations`, and readers branch on asset id to pick the field (`controller/timeline.rs:316-330`). A second PV or battery cannot be represented; the UI reads the same fields. | `VEN/src/entities/plan.rs` (PlanSlot), `VEN/src/controller/timeline.rs` | Medium (wire contract to the UI) | Closed, out of scope (2026-10-10): see below |
| 2 | S3 | **The controller decides by asset identity instead of asking the asset.** "battery or EV = has a SoC to protect" (`controller/arbiter.rs:105`), "EV planned?" (`controller/dispatcher.rs:69`), "PV's own embodied CO2" (`controller/monitor.rs:65`), and request routing `is_ev` / `is_heater` compare ids with `"ev"`, `"heater"`, `"boiler"` (`services/user_request.rs:270-279`): an asset named `ev2` is "unrecognised". An `asset-competence` violation; the asset should declare it (has storage, request kind). | `VEN/src/controller/arbiter.rs`, `VEN/src/controller/dispatcher.rs`, `VEN/src/controller/monitor.rs`, `VEN/src/services/user_request.rs` | Small–Medium | **fixed** (R-128) |
| 3 | S3 | **The report routes bypass `VtnPort` and call the concrete `VtnClient`.** `post_reports` / `put_report` cannot be unit-tested (a comment in `routes/reports.rs` says so); `/health` and `/vtn/status` reach the concrete client too. | `VEN/src/routes/reports.rs`, `VEN/src/routes/system.rs` | Small | R-129 |
| 4 | S4 | **Request transitions are string-typed and partly invented.** `ControllerEvent::RequestTransition { from_status, to_status }` are `String`s made with `format!("{:?}")`; cancelling always records `from_status: "Active"`, although `cancel` also accepts a `Failed` request, which is then logged as `Active -> Cancelled`. | `VEN/src/controller/trace.rs`, `VEN/src/services/request_submission.rs` | Small | **fixed** (R-130) |
| 5 | S4 | **Infrastructure types used inside.** `SensorSnapshot` / `SensorInput` live in `simulator/` but are used by `state/` and `routes/`; `services/obligation.rs` imports `vtn::VtnHttpError` instead of receiving a `DomainError` translated at the boundary (`docs/guidelines/ERROR_HANDLING.md`). | `VEN/src/simulator/snapshot.rs`, `VEN/src/services/obligation.rs` | Small | R-131 |
| 6 | S4 | **Inline energy arithmetic left after R-111.** `kw * dt_h` three times in `controller/monitor.rs` and once in `assets/max_power.rs` instead of `units::energy_kwh`; audit rule 10 did not match this spelling. Fixing it found five more sites (`assets/battery.rs` x2, `assets/ev.rs`, `entities/capacity_curve.rs`, `simulator/grid_meter.rs` x2); all now call `energy_kwh`, and rule 10 refuses `<…>kw * dt_h`. | `VEN/src/controller/monitor.rs`, `VEN/src/assets/max_power.rs` | Trivial | **fixed** |
| 7 | S4 | **Dead code hidden from the compiler.** 44 `#[allow(dead_code)]`. The capability traits still say "no implementor yet" though 6 implementations exist; `entities/design_vocabulary.rs` keeps 14 unreferenced "design sketches" in the domain ring (one with an untyped `serde_json::Value` field); `controller/settings_port.rs` silences the whole module. | `VEN/src/assets/capability_traits.rs`, `VEN/src/entities/design_vocabulary.rs`, `VEN/src/controller/settings_port.rs` | Small | R-132 |
| 8 | S4 | **Two ways to reach the simulator.** `SimulatorPort` (one method, `snapshot()`) is implemented on `SimState` itself, so the caller holds the lock; the newer `SimRosterPort` / `SimReadPort` / `HeadroomPort` go through `SimHandle`, which locks inside. Only `tasks/` use the old one. | `VEN/src/controller/simulator_port.rs`, `VEN/src/simulator/mod.rs` | Small — decided: delete it | being deleted (`docs/register-cleanup`) |
| 9 | S4 | **The UI's API types are a hand-written copy of the Rust ones.** `VEN/ui/src/api/types.ts` (1023 lines), no generation; drift is caught only by E2E. | `VEN/ui/src/api/types.ts` | Medium — decided: ts-rs, incrementally | R-133 |

## Already in the register (not repeated above)

R-110 (remainder: the `AppState` god object, `simulator/mod.rs`, the 1445-line `capacity_headroom.rs`),
R-118 (functions over 100 lines), R-32 (VTN BFF / VEN client
duplication), R-125 (SoC names without a unit), R-121 (`ui-charts/` holds more than charts).

## How it was searched

- Ring crossings not covered by the audit: `crate::(simulator|assets|vtn|history_store)` in `services/`,
  `state/`, `routes/`, production lines only.
- Branching on identity: `== ids::ASSET_*` and `ids::ASSET_* =>` outside tests.
- Pattern counts per ring: `.unwrap()` / `.expect()` (22), `serde_json::Value` (50, most of it OpenADR
  wire `values` and route bodies), `format!("{:?}")` used as data (5), `Mutex` / `RwLock` (41, all in
  `tasks/`, `state/`, `boot/` and infra).
- `#[allow(dead_code)]` inventory, the port traits and their implementors, UI file sizes.

## Handling

- Fixed: the register drift (`docs/register-cleanup`) and #6 (`refactor/smell-sweep-v2`).
- Fixed since: #2 (R-128).
- Filed: #3 R-129, #4 R-130, #5 R-131, #7 R-132.
- Closed without a fix: #1 (R-127). **Several assets of one kind in a VEN do not fit into this
  project** (user decision, 2026-10-10). The plan fields are only the visible part: the MILP has one
  variable slot each for battery, EV and heater, takes one PV and one base-load series, and about
  70 places look an asset up by its fixed id. Lifting that is a different product, beyond what an
  OpenADR lab needs. A VEN has at most one asset of each kind (shiftable loads excepted), and the
  one-PV / one-battery plan fields are that boundary written down, not a debt. Do not re-file.
- Decided: #8 delete `SimulatorPort` (tasks call `to_sim_snapshot()` directly; done by the `docs/register-cleanup` work); #9 ts-rs, incrementally (R-133).
