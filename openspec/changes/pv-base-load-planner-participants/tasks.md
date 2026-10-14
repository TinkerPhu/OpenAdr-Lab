# Tasks

Branch: `refactor/pv-base-load-participants`. Each group lands green on its own. "Goldens hold"
means `plans.txt` byte-identical and the fingerprint/inputs goldens unchanged unless the task says
otherwise.

## 1. One forecast surplus

- [x] 1.1 Test first: `forecast_surplus_kw` is PV minus the base load after the override, signed,
      per slot (`tests/inputs_golden.rs`).
- [x] 1.2 `MilpInputs::forecast_surplus_kw()` (a method, so hand-built test inputs cannot hold a
      stale copy; signed, because the coexistence test compares the unfloored value with the EV's
      minimum power); the four `pv − base` computations call it; `inject_grid_slots` takes it
      instead of the two series. All goldens hold.

## 2. The dispatched role and forecast readback

- [x] 2.1 `AssetMilpContext::dispatched()` (default `true`); `AssetSolution.forecast`
      (`ForecastLegs`); `site_net_power_expr` for the balance, a dispatched-only sum for the malus;
      `results.rs` skips non-dispatched assets in allocations, `planned_asset_ids` and session
      flexibility. Tests first.

## 3. PV and base load as participants

- [x] 3.1 `PvMilpContext` (`assets/pv_declared.rs`): forecast series, the test pin, its
      used-generation variable, net power, tie-break in cost and friction, readback, warm start,
      `from_forecast` for tests. `BaseLoadMilpContext` (`assets/base_load_declared.rs`): fixed net
      power, readback. `MilpParticipant` for `PvInverter` and `BaseLoad`; `MilpBuildInputs` gains
      `pv_forecast_override`. Unit tests first.
- [x] 3.2 The model: delete `p_pv_used` from the grid, the balance's PV term and `p_base` RHS
      (`p_base_adjust_kw` instead), `pv_use_tiebreak_expr`, `SolveOutput.p_pv_used_kw`;
      `results.rs` writes the site PV and base fields from the summed `ForecastLegs`.
- [x] 3.3 Inputs: delete `forecast_series`, `SiteInputs` PV/base fields, `SolveRequest`
      `pv_forecast_override`/`pv_live_forecast_kw`/`base_load_live_forecast_kw`/`weather_pv_kw`,
      `resolve_pv_forecast_kw`, `resolve_base_load_forecast_kw`,
      `resolve_weather_pv_kw_for_cycle`'s planner use; SIMPLE level 2 reads the forecast
      participants' load; the PV pin travels on `MilpBuildInputs`.
- [x] 3.4 Test fixtures build PV and base-load contexts from the series they used before (D5).
      Goldens: `plans.txt` byte-identical; fingerprint changes classified (as-sent only);
      `milp_inputs.txt` only the moved series.
- [x] 3.5 `tests/multi_instance.rs`: two PV contexts under an export cap.

## 4. Profile and audit

- [x] 4.1 `profile::validate` refuses `weather_pv` without a PV asset (test first).
- [x] 4.2 Architecture audit: the planner core holds no PV/base series (rule 16 already matches
      `ids::ASSET_*`; add `p_pv_kw`/`p_base_kw` names if any survive); run all audits.

## 5. Checks, docs, merge

- [ ] 5.1 fmt, clippy, `cargo audit`, file sizes, duplication, architecture, module graph, debt
      gate; all four suites on Node2.
- [x] 5.2 Docs: `ven_milp_planner.md` (participants, the dispatched role, the surplus),
      `VEN_ARCHITECTURE.md` §3.0d, journal (including D4's remaining part), key learnings, wiki.
- [ ] 5.3 Delete this change; rebase, fast-forward merge, clean up Node2 and the worktree.
