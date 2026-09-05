# Tasks: `assetMaxPower` Primitive + `limitTier`

## 1. Survey (confirm design.md's claims against current code)

- [x] 1.1 Re-confirmed unchanged.
- [x] 1.2 Re-confirmed unchanged.
- [x] 1.3 Confirmed unchanged.

## 2. `LimitTier`

- [x] 2.1 Added `LimitTier { Physical, Contractual, UserSet }` to
      `entities/capacity_curve.rs`, alongside `CommitmentDirection`, with the
      honest-scope doc comment.

## 3. `max_effort_setpoint` (D1)

- [x] 3.1 Test-first, including the PV Import → 0.0 and Heater Export → 0.0
      regression cases.
- [x] 3.2 Added `max_effort_setpoint` to `Asset` (`assets/asset_trait.rs`)
      with the default body from design.md D1. No name collision found.
- [x] 3.3 `PvInverter` overrides `max_effort_setpoint`; the PV↔`LimitTier`
      mapping (Manual→UserSet, Plan/Capacity/Arbiter/CommsLoss→Contractual)
      and the "Physical = true uncurtailed ceiling" semantics were confirmed
      with the user via AskUserQuestion rather than guessed.
- [x] 3.4 Tier-invariance tests added for Battery/EvCharger/Heater/BaseLoad.

## 4. `max_effort_schedule` (D3) and `ShiftableLoadAsset`'s override

- [x] 4.1 Added `max_effort_schedule` to `Asset`; default body calls through
      `max_effort_setpoint` and steps at 60s resolution (upgraded from
      design.md's flat two-point sketch — see project_journal.md entry for
      why: `step()` only checks exhaustion at the start of a `dt` window, so
      a coarse two-point schedule silently over-reports power/energy past
      exhaustion).
- [x] 4.2 Test-first: default-body schedule matches `simulate_forward`
      driven by `max_effort_setpoint` directly, for Battery.
- [x] 4.3 `ShiftableLoadAsset::max_effort_schedule` places the run at
      `earliest_start` (Import) / `latest_end - duration` (Export), reusing
      `step_inner`'s own physics via `simulate_forward`.
- [x] 4.4 Degenerate window-doesn't-fit case covered
      (`a_window_the_run_cannot_even_begin_in_contributes_nothing`).

## 5. `asset_max_power` (D4)

- [x] 5.1 Test-first, for Battery (boundary-exact and ample-headroom cases)
      and covered transitively for ShiftableLoadAsset via its schedule tests.
- [x] 5.2 Implemented `asset_max_power` — split into new sibling file
      `assets/max_power.rs` (see 6.1).
- [x] 5.3 Energy integration is `power_kw.abs() × dt_h` summed over
      `windows(2)` of the trajectory, matching `CapacityCurve`'s own
      integration convention.

## 6. Cross-cutting verification

- [x] 6.1 `asset_trait.rs` exceeded budget (538 lines) after adding the two
      new trait methods; split `Trajectory`/`TrajectoryPoint`/
      `asset_max_power` into new sibling `assets/max_power.rs`. Audit passes.
- [x] 6.2 Architecture invariants confirmed clean (grep checks pass).
- [x] 6.3 `cargo fmt --check` / `clippy -D warnings` clean.
- [x] 6.4 Full Rust unit suite green (1271/1271).
- [x] 6.5 UI unit suites green (VEN UI + VTN UI 71/71) — no UI changes needed,
      confirmed: this primitive has no consumer yet (Spec E wires it in),
      so there is no new backend capability or derived state to surface.
- [x] 6.6 E2E + resilience suites run on Node2 — green, unchanged as expected.

## 7. Documentation

- [x] 7.1 `docs/history/project_journal.md` — entry added.
- [x] 7.2 `docs/reference/KEY_LEARNINGS.md` — durable lesson added.
- [x] 7.3 `docs/architecture/VEN_ARCHITECTURE.md` — updated.
- [x] 7.4 `docs/plans/asset-max-power-forecast-master-plan.md` — Spec C marked
      complete.
- [x] 7.5 Confirmed: no `docs/use-cases/*.md` update needed — no
      user-observable behavior change (primitive has no production call site
      yet).
- [x] 7.6 Change directory deleted after merge.
