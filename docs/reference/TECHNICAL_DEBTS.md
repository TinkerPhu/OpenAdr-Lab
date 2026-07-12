# Technical Debts Register

> Source review: `VEN/src/` code quality review conducted 2026-04-28.
> Detailed diagnostics: `docs/plans/refactoring_backlog.md`
>
> **Rule:** Before adding a feature in an affected area, check this file first.
> Refactor the relevant debt before adding new behaviour if effort is Small or Trivial.

Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)

---

| ID | Description | Affected files | Effort | Risk | Priority |
|----|-------------|----------------|--------|------|----------|
| R-03 | Replace hardcoded string asset IDs in `dispatcher.rs` with shared constants. `asset_id()` methods in each asset file are the authoritative source — keep them; only the duplicate literals in dispatcher need replacing. | `VEN/src/controller/dispatcher.rs` | Small | Mechanical | 🟠 |
| R-08 | Replace `AssetConfig` manual dispatch enum (~9 methods × 5 variants) with `dyn Asset` or macro forwarder | `VEN/src/assets/mod.rs` | Large | Serialisation risk | 🔵 |
| R-09 | Inject clock into `tasks/planning.rs` instead of calling `Utc::now()` directly. Accept a `Fn() -> DateTime<Utc>` parameter in `spawn_planning` so the planning loop is testable without wall-clock coupling. `align_to_step` is already a pure function. Blocked on threading the clock through `spawn_planning`'s argument list. | `VEN/src/tasks/planning.rs` | Small | Low | 🟡 |
| R-10 | Replace `serde_json::Value` in public `VtnPort` methods with typed OpenADR 3 structs. `vtn.rs` currently parses all VTN responses as raw `Value` and extracts fields with string indexing — no compile-time type safety. Requires adding `entities/vtn_types.rs` with `OadrProgram`, `OadrEvent`, `OadrReport` structs and updating all `parse_*` functions in `openadr_interface.rs`. Layer-3 adapter contract tests become possible only after this. Should precede `ObligationService` if that is revisited. | `VEN/src/vtn.rs`, `VEN/src/controller/openadr_interface.rs` | Medium | Low | 🟡 |
| R-11 | `routes/timeline.rs` exceeds the 500-line file-size cap (currently ~772 lines). Grew past the cap during the 2026-07 "nice resolution" fix and again during the "real plan slots in future segment" fix, both adding inline `#[cfg(test)]` coverage to the same file. Split the resolution-snapping helpers (`NICE_RESOLUTIONS_S`, `snap_up_to_nice`, `resolve_resolution_s`) and/or the test module into a separate file/submodule. | `VEN/src/routes/timeline.rs` | Small | Low | 🟠 |
| R-12 | `controller/timeline.rs` exceeds the 500-line file-size cap (currently ~1179 lines, pre-existing before 2026-07). Mixes uniform-grid resampling helpers, `build_asset_timeline`/`build_now_point`, and a large inline test module. Split test module out and/or extract resampling helpers (`resample_to_grid`, `locf_weighted_mean`, `compute_uniform_grid`) into their own file. | `VEN/src/controller/timeline.rs` | Medium | Low | 🟠 |
| R-13 | `DISPATCH_SETPOINT` OpenADR payload type has no handling path anywhere in the VEN. `docs/architecture/VEN_ARCHITECTURE.md` §2.1 previously stated it triggers a "Direct Dispatcher override (bypasses Planner)" — never implemented. Only survives as a dead field on the unreferenced `OadrEventCache` struct. Relevant to OpenADR cert Load Control (§8.5) / Custom Dispatch Instructions (§8.12) use cases. | `VEN/src/controller/openadr_interface.rs`, `VEN/src/entities/capacity.rs`, `VEN/src/controller/dispatcher.rs` | Medium | Low (additive) | 🟡 |
| R-14 | `EXPORT_CAPACITY_SUBSCRIPTION`/`EXPORT_CAPACITY_RESERVATION` are not parsed — `OadrCapacityState` has import-side subscription/reservation fields only, no export-side equivalents. | `VEN/src/entities/capacity.rs`, `VEN/src/controller/openadr_interface.rs` | Small | Low (additive) | 🟡 |
| R-15 | `USAGE_FORECAST` outbound reporting was never built, despite the MILP already computing the exact per-slot forecast internally (`planned_state_by_asset`, used today only by `/timeline`). `reportDescriptor.historical` is never parsed, so the VEN cannot distinguish a forecast request from a historical one. | `VEN/src/controller/reporter.rs`, `VEN/src/entities/capacity.rs` | Medium | Low (additive) | 🟡 |
| R-16 | MILP planner samples each slot's tariff at its **start** timestamp only (`interpolate_at(slot_t)`), not the time-weighted mean across the slot. A slot straddling a tariff boundary is priced entirely at the pre-boundary rate. `TimeSeries::time_weighted_mean` (`common/mod.rs`) already exists and would fix this in one call. | `VEN/src/controller/milp_planner/inputs.rs`, `VEN/src/common/mod.rs` | Small | Low | 🟡 |
| R-17 | `assets/ev.rs` production code (excluding `#[cfg(test)]` blocks) exceeds the 500-line file-size cap — 628 lines before Phase 0 WP0.3 (BL-12), ~659 after. The physical `EvCharger`/`EvState` step logic and the `EvMilpContext` MILP-plugin methods (`declare_vars`/`energy_expr`/`constraints`/`objective`/`read_solution`) are two largely independent concerns in one file; moving the MILP-plugin impl block to `assets/ev_milp.rs` would bring it under budget with no behavior change. Discovered 2026-07-08 while adding BL-12 (min charge rate + response delay). | `VEN/src/assets/ev.rs` | Small | Mechanical (impl-block move only) | 🟠 |
| R-18 | The EV `e_ev_extra` reward is structurally inert for MustRun/MayRun sessions: the only coupling is `ev_energy ≤ e_core + e_ev_extra` (an upper bound), so the solver banks the reward `−v_extra·e_ev_extra` by maxing the slack variable without charging a single extra kWh — `v_ev_extra_eur_kwh` never influences allocations, it only shifts the reported objective by a constant. The variable's *cap* role (limit total energy to core + headroom) still works. WP4.1-b sidestepped this for OPPORTUNISTIC/`*_FREE` with a per-slot `p_ev` reward (`free_only` branch in `ev_milp.rs::objective`); the legacy modes still carry the inert term. Fix = couple it (`ev_energy ≥ e_core + e_ev_extra` when rewarded) or move the legacy reward to per-slot form too. Discovered 2026-07-12 while implementing BL-28 PR-b. | `VEN/src/assets/ev_milp.rs`, `VEN/src/controller/milp_planner/solver_phase2.rs` (cost-cap expression) | Small | Behavioural (changes objective accounting) | 🟡 |

---

## Notes

- `AssetProfile` (YAML, `profile.rs`) and `AssetConfig` (runtime physics, `assets/mod.rs`)
  share variant names but hold different inner types. Consider renaming `AssetProfile` →
  `AssetSpec` to avoid newcomer confusion.
- `SimInjectState` mixes three injection behaviours in one flat struct. A tagged `InjectBehaviour`
  enum per field would clarify intent. Track here if promoted to a formal debt item.
