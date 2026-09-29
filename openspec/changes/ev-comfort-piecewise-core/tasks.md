# Tasks

## 1. Baseline

- [x] 1.1 Record the starting point: run the full VEN Rust suite and note the pass count, and
      run `gb41_sweep_sites_against_pricing_and_limits -- --nocapture`, saving its `z_ev_core`
      table. That table is the before-picture the new behaviour is compared against.
- [x] 1.2 Record `solver_ms` for a representative profile (a ven-3-shaped heater+EV solve from
      `tests/heater.rs` or a live `/plan` fetch) so Decision 1's "one binary fewer" claim can be
      measured rather than assumed (R-97).

### Baseline recorded 2026-09-28 (before any change)

Rust suite: **1464 passed, 0 failed, 3 ignored**.

`gb41_sweep_sites_against_pricing_and_limits`: **z_ev_core = 1.00 and 25.00 kWh delivered in
all 12 site x variant combinations** — the offline model always takes the whole core at the
profile-default 1.0 EUR/kWh reward. (The decline only appears at a low comfort bid, which is
`gb41_a_low_comfort_rate_declines_grid_charging_but_not_free_charging`.)

Live `solver_ms` / `solve_status` per VEN (Node1 + Node2, 2026-09-28 ~09:00Z):

| VEN | heater? | status | solver_ms |
|---|---|---|---|
| ven-5 | yes | TIME_LIMIT | 120319 |
| ven-3 | yes | TIME_LIMIT | 66282 |
| ven-18 | yes | TIME_LIMIT | 62319 |
| ven-1 | no | GAP_LIMIT | 11928 |
| ven-2 | yes | GAP_LIMIT | 11919 |
| ven-11 | no | GAP_LIMIT | 247 |
| ven-7 | no | OPTIMAL | 210 |

Three of seven sampled VENs hit the per-phase solver timeout in production right now — R-97
live, and GB-38's signature. Every TIME_LIMIT VEN has a heater; the two fastest have none,
which points at the heater's tier binaries rather than the EV's single one. Removing the EV
binary is therefore unlikely to fix this on its own: task 8.2 compares against these numbers,
and a null result is the expected honest outcome.

## 2. Curve validation (test-first)

- [x] 2.1 Write a test that `services/comfort.rs::validate_curve` rejects a curve whose
      `max_marginal_price` rises with fill, with the error naming the offending point index, and
      accepts equal-or-falling bids. Confirm it fails, then implement the rule.
- [x] 2.2 Write a test that the shipped curves still validate, so the new rule provably breaks
      no existing data. Done as `every_built_in_default_curve_satisfies_the_new_rule` over the
      EV/heater/base_load built-in defaults — **finding:** the profile YAMLs' `packets:` blocks
      carry `comfort_rates` (0.50 → 0.05 in three test profiles) but **nothing in `VEN/src`
      parses `packets` at all**, so those are dead config; the curves that actually reach the
      planner are the built-in defaults and user overrides. All shipped curves fall, none rise.

## 3. Segments from the curve (test-first)

- [x] 3.1 Write unit tests for a segment builder in `assets/ev_comfort.rs` that turns
      (curve, `soc_init`, `battery_kwh`, `w_ghg_eur_kg`) into `Vec<EvEnergySegment{kwh,
      eur_per_kwh}>` spanning `soc_init → 1.0`: total kWh equals `(1 − soc_init) × battery_kwh`;
      bids are non-increasing; a single-point curve yields one segment; an empty curve falls back
      to the profile defaults (`v_ev_core_eur_kwh` / `v_ev_extra_eur_kwh`) exactly as
      `resolve_ev_comfort_reward` does today; the CO2 bid is monetized into the same €/kWh and
      added. Confirm failing, then implement by rewriting `resolve_ev_comfort_reward` — reuse
      `ComfortRate::value_at_fill`/`co2_value_at_fill`, do not re-derive interpolation.
- [x] 3.2 Write a test that an EV at 100 % SoC produces no segments (nothing to value).

## 4. MILP: continuous segments replace the binary (test-first)

- [x] 4.1 Write a solver test that a soft request whose bid covers part of the energy delivers
      that part (assert delivered kWh strictly between zero and the full amount). Confirm it
      fails against today's all-or-nothing model, then implement: `EvMilpContext` carries
      `segments`, `declare_vars` emits one bounded continuous variable per segment and **no**
      `z_ev_core`, `constraints` uses `ev_energy == Σ e_seg[k]` for every mode, and `objective`
      rewards each segment at its own bid. Reuse `energy_expr` for the deadline bound unchanged.
- [x] 4.2 Write a test that a firm request still delivers at least `(soc_target − soc_init) ×
      battery_kwh` by its deadline even when the bid is far below the cost of energy, then
      implement the floor constraint (Decision 4). `clamp_core_to_reachable_energy` and its
      `core_unmet_warning` plumbing (`EvScalars`, `EvMilpContext`, `MilpInputs`) were **deleted**
      rather than retargeted: the cap now lives at the one place that imposes the floor, as
      `EvMilpContext::reachable_energy_kwh` in `constraints`. Shrinking the requirement upstream
      was the reason the shortfall was invisible to the diagnostic — `e_required_kwh` now stays
      what the user asked for, and the gap is what `firm_shortfall` reports.
- [x] 4.3 Write a test that a firm request whose window cannot deliver the floor still solves,
      charges as much as the window allows, and reports delivered vs required.
- [x] 4.4 Remove `z_ev_core` from `EvMilpVars`, `EvSolOutput`, `EvScalars`, `MilpInputs` and the
      phase-2/duals pinning (`solver_phase2.rs`, `solver_duals.rs`); verify `cargo check
      --all-targets` is clean and no production reference remains
      (`grep -rn "z_ev_core" VEN/src --include=*.rs` returns only test history, if any).
- [x] 4.5 Confirm the free/opportunistic modes are untouched: the existing `reward_per_slot`
      tests (`tests/modes.rs`, the BY_DEADLINE_FREE/ASAP_FREE/Opportunistic scenarios) pass
      unchanged. They keep their profile-constant flat rate — design Decision 5 as corrected:
      these modes never read the curve, so connecting them to it was dropped.

## 5. Diagnostics (test-first)

- [x] 5.1 Write a test that a **soft** request which charges partially produces **no**
      `EvCoreEnergyUnmet` warning, and that a **firm** shortfall does, carrying delivered and
      required kWh in its message. Then narrow `milp_planner/ev_diagnostics.rs` accordingly and
      delete the now-duplicate clamp warning path.

## 6. Rewrite the GB-41 probes to state the new behaviour

- [x] 6.1 Rewrite `gb41_free_energy_with_partial_surplus_charges_nothing_rather_than_partially`
      as "charges what the surplus covers" — assert delivered kWh ≈ the available surplus rather
      than zero.
- [x] 6.2 Rewrite `gb41_a_low_comfort_rate_declines_grid_charging_but_not_free_charging` as
      graceful degradation: a 0.10 €/kWh bid buys the kWh whose cost it covers on both sites, and
      strictly more on the site with surplus.
- [x] 6.3 Re-run the whole `gb41_*` module and record the new sweep table; compare against 1.1
      and state the difference in the journal entry (task 8.1).

## 7. Surfaces

- [x] 7.1 `ComfortCurveCard.tsx`: relabel the fill axis as state of charge and add the
      non-increasing rule to the helper text; surface the API's rejection message on a failed
      POST. Extend the existing card test. The axis label is declared per asset in
      `CURVE_ASSETS` rather than branched on at the field (`declare-dont-branch`), and
      `api/client.ts::postComfortCurve` now reads the route's `{"error": ...}` body instead of
      throwing a bare status code — without that the rejection message had nowhere to come from.
- [x] 7.2 `EvCard.tsx` / `PlanHistory.tsx` / `api/types.ts`: reword the warning label to the
      firm-shortfall meaning; keep the `EV_CORE_ENERGY_UNMET` wire string (it is persisted).
- [x] 7.3 Add a BDD scenario in `tests/features/` for the user-observable statement of this
      change: a soft request with a bid below the cost of energy charges **partially**, and its
      plan carries no unmet-obligation warning.

## 8. Verify, document, close

- [ ] 8.1 Full gate: `cargo test -p ven-app`, `cargo fmt --check`, `cargo clippy --all-targets
      --all-features -- -D warnings`, `scripts/audit_file_sizes.py`, the four architecture greps,
      `cd VEN/ui && npm test` + eslint, then E2E on Node2 (`DOCKER_HOST=Node2 bash
      run_all_tests.sh --e2e`) watching the EV and heater scenarios.
- [ ] 8.2 Compare `solver_ms` against the 1.2 baseline and record the result either way (R-97).
- [ ] 8.3 Fold into current-state docs: the comfort-curve semantics into
      `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` (how to set a curve, what a firm vs
      soft request guarantees, the worked examples), the mechanism into
      `docs/architecture/VEN_ARCHITECTURE.md`'s EV section, and a `docs/history/project_journal.md`
      entry. Close GB-41 in `docs/BACKLOG.md` as resolved-by-removal and drop R-95's
      warning-kind-collision note in `docs/reference/TECHNICAL_DEBTS.md`.
      **Correction:** no such note exists in `TECHNICAL_DEBTS.md` — that claim was written from
      memory and is wrong (`grep -n "EvCoreEnergyUnmet\|collision"` finds nothing). R-95 is about
      the missing sustained-commitment series only and stays as it is. Also filed **GB-53** for
      the cumulative-spend curve UX and the max-vs-planned cost display, per the review questions.
- [ ] 8.4 Delete `openspec/changes/ev-comfort-piecewise-core/` per workflow rule 3 (do not
      archive), once 8.1–8.3 are green and merged.

### New sweep table recorded 2026-09-29 (task 6.3)

`sweep_sites_against_pricing_and_limits`, delivered kWh per site x variant:

| variant | bare | + PV | + battery | + PV + battery |
|---|---|---|---|---|
| minimal weights, ample import | 29.60 | 35.00 | 35.00 | 32.94 |
| fleet weights (0.22 malus) | 25.00 | 25.00 | 25.00 | 25.00 |
| fleet weights, import capped | 25.00 | 25.00 | 25.00 | 25.00 |

Against the 1.1 baseline (`z_ev_core = 1.00`, **25.00 kWh in all twelve combinations**): the
fleet-weight rows are unchanged, because the default bid still clears tariff-plus-malus for the
energy up to target and the 0.10 EUR/kWh band beyond it does not. The minimal-weight rows now
deliver **more than the old core** (29.6-35.0 kWh) — with no malus, the band beyond the target
is worth buying, which the all-or-nothing core could not express at all. Nothing delivers less,
so the change adds reachable outcomes rather than trading one cliff for another.

Rust suite after the change: **1473 passed, 0 failed, 3 ignored** (up from 1464 - 9 rewritten
GB-41 probes, 4 curve-validation tests, 7 segment-builder tests, 2 diagnostics tests).
