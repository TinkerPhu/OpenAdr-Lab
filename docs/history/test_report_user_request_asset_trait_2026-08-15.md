# Test Report: `user_request.rs` validation + `Asset` trait trajectory defaults

2026-08-15. `fix/user-request-asset-trait-tests`. Both files were identified as
the next-most-pressing coverage gaps after `simulator/persist.rs` and
`simulator/plan_context.rs` (see `docs/history/coverage_report_2026-08-14.md`):
real domain logic with little or no direct unit coverage, as opposed to the
bulk of this project's remaining coverage gap, which is Axum/task-loop wiring
correctly left to the E2E/BDD suites instead.

22 new tests in `controller/user_request.rs`, 4 new tests in
`assets/asset_trait.rs` (plus its 5 pre-existing `AssetHandle` delegation
tests, unchanged). All 26 new tests pass; full suite (1052+ tests), `cargo fmt
--check`, and `clippy --all-targets --all-features -- -D warnings` all clean
on Node2.

## `controller/user_request.rs::create_from_body`

This function turns every `POST /requests` body — EV session, heater target,
or shiftable load — into a validated `UserRequest`. It had **zero direct unit
tests** before this change; its ~53% line coverage came entirely from
incidental exercise by other tests and routes. Each test below targets one
specific branch a real caller can actually hit, not a synthetic input chosen
to move a coverage number.

**Rejections** — the three ways a request can be invalid, and the two
different code paths that both mean "there's no energy left to deliver":

- `no_deadlines_is_rejected` — an empty `deadlines` list is always invalid.
- `unknown_asset_id_is_rejected` — the body names an asset the caller doesn't
  know about (asserts the *specific* unknown id is echoed back in the error).
- `explicit_target_energy_kwh_zero_is_rejected` /
  `..._negative_is_rejected` — an explicit target energy must be strictly
  positive.
- `asset_already_at_or_above_target_soc_is_rejected` — the *other* way to hit
  `ZeroEnergy`: no explicit `target_energy_kwh`, but the asset's current SoC
  already meets or exceeds the target, so `resolve_request_target` returns
  `None`. This is a distinct code path from the explicit-zero case above, and
  it's the more likely real-world trigger (a user re-requesting a charge that
  already finished).

**Target-energy resolution** — the function has two ways to arrive at a
target energy, and this is where a bug would most directly cost the user
money or leave a device under-charged:

- `resolves_target_energy_from_soc_gap_when_not_explicit` — with no explicit
  energy, the target is computed from `(target_soc − current_soc) ×
  capacity_kwh`, and `desired_power_kw` falls back to the asset's own
  `max_charge_kw`. Verifies the arithmetic, not just "it doesn't error."
- `resolves_target_energy_using_explicit_target_soc_over_asset_default` — an
  explicit `target_soc` in the body overrides the asset's
  `default_soc_target`, changing the resolved energy accordingly (3.0 kWh vs.
  5.0 kWh for the same asset — proves the override actually takes effect, not
  just that a value is returned).
- `explicit_target_energy_kwh_defaults_desired_power_to_one_kw_when_unspecified`
  — when the caller *does* give an explicit energy but no power, the fallback
  is a hardcoded 1.0 kW, not the asset's `max_charge_kw` (a different default
  than the SoC-resolution path above — an easy place for a future refactor to
  silently unify incorrectly).
- `explicit_desired_power_kw_overrides_asset_max_charge_kw` — an explicit
  power always wins over any default.

**Completion policy** — the string stored on `UserRequest` and later read by
the planner:

- `completion_policy_defaults_from_asset_continue` /
  `..._defaults_from_asset_stop` — both enum variants round-trip to their
  correct `SCREAMING_SNAKE_CASE` string when the caller doesn't specify one
  (two tests, not one, because the mapping is a real `match` with two arms —
  testing only one variant would leave the other completely unverified).
- `completion_policy_explicit_override_wins_over_asset_default` — an explicit
  string in the body is used as-is, bypassing the asset's own policy.

**Comfort rates** — the value curve the MILP planner later reads to shape its
reward:

- `comfort_rates_default_from_asset_when_unspecified` — no override → the
  asset's own `comfort_rates` (including whatever CO₂ preference it carries)
  passes through unchanged.
- `comfort_rates_explicit_override_replaces_asset_default_and_zeroes_co2_bid`
  — pins down a genuinely non-obvious behavior: the request body's
  `ComfortRateInput` only carries `fill`/`bid` (price), with **no user-facing
  CO₂ field**, so an explicit override always resets `max_marginal_co2` to
  `0.0` even if the asset's own default carried a real value. Without this
  test, a future "let's also let users override the CO₂ bid" change could
  silently miss that the current code already drops it — or someone reading
  the code could reasonably assume the asset's CO₂ preference survives an
  explicit price override, and be wrong.

**Budget / tier bookkeeping**:

- `budget_eur_backfills_first_deadline_cost_ceiling_when_unset` — a top-level
  `budget_eur` shorthand fills in the first deadline's `max_total_cost_eur`
  only when that deadline doesn't already have one.
- `budget_eur_does_not_override_an_already_set_deadline_cost_ceiling` — the
  mirror case: an explicit per-deadline ceiling is never clobbered by the
  shorthand, even when both are present in the same request.
- `tier_count_reflects_number_of_deadlines` — `tier_count` and the API's
  convenience `max_total_cost_eur` field both derive from the deadline list
  (3 deadlines → `tier_count == 3`, and the cost figure comes from the
  *first* tier specifically, not any tier or a sum).
- `min_completion_defaults_to_0_8_when_unspecified` — an omitted
  `min_completion` on a deadline defaults to 80%, not 100% or 0%.

**Request mode / interruptibility** — smaller, but each is a real
caller-visible default:

- `mode_defaults_to_by_deadline_when_unspecified` /
  `mode_explicit_value_is_passed_through` — BL-28's `UserRequestMode`
  defaults to the legacy `ByDeadline` behavior, and an explicit mode is
  honored.
- `interruptible_defaults_to_false_when_unspecified` /
  `..._explicit_true_is_passed_through` — same pattern for the leeway flag.

## `assets/asset_trait.rs`::`Asset` trait defaults

`AssetHandle`'s delegated methods (`id`, `current_state`, `step`,
`capability`) already had 5 tests. What was untested is the **`Asset`
trait's own default implementations** — `simulate_forward`, `simulate_free`,
and `capability_trajectory` — real accumulation/projection logic used by
lookahead precompute, not boilerplate. All four new tests exercise these
defaults through a battery-backed `AssetHandle` (the only concrete,
already-trusted physics implementation available), since the defaults only
make sense in terms of a real `Asset`.

- `simulate_forward_reports_pre_step_state_paired_with_post_step_actual_power`
  — pins down a contract that is easy to get backwards: each
  `TrajectoryPoint` pairs the state *before* that window's step with the
  *actual* power achieved *during* the step, not the state after. Verified
  by hand-computing SoC through two 1-hour charging windows (0.2 → 0.5 →
  0.8) and checking each point holds the *pre*-step SoC alongside the power
  that produced the *next* one. Getting this backwards would silently
  corrupt every consumer of a precomputed lookahead trajectory.
- `simulate_forward_reports_clamped_actual_power_not_the_requested_setpoint`
  — requesting 20 kW against a battery whose `max_charge_kw` is 5.0 must show
  up in the trajectory as the clamped 5.0. This is the entire reason the
  field is named "actual," not "requested" — a test that only fed feasible
  setpoints would never catch a regression that started reporting raw
  request values instead.
- `simulate_free_holds_soc_steady_at_zero_setpoint` — "free run" means
  "untouched," not "drains or charges." Battery has no self-discharge model,
  so idling at 0 kW for 2 hours must leave SoC exactly where it started —
  verified explicitly rather than assumed.
- `capability_trajectory_projects_n_steps_reflecting_evolving_state` — starts
  a battery already at `soc = 1.0` and confirms every projected step still
  shows `max_import_kw = 0.0`. `capability()` is a step function of state
  (full vs. not-full), so this specifically checks that the trajectory
  *re-derives* capability from the stepped state at each point, rather than
  computing it once from the initial state and repeating it `n` times — a
  bug that would be invisible for an idling non-full battery (where the
  values happen to stay constant anyway) but wrong for any asset whose
  capability actually changes over the horizon.

## What was deliberately not tested

- `capability_trajectory`/`simulate_free` were only exercised against a
  battery in a state where idling doesn't drift SoC (no self-discharge
  model). A future asset type with time-based drift (e.g. a heater losing
  heat while idle) would need its own test of these same trait defaults —
  this report doesn't claim the *trait* is fully proven, only that its
  battery-backed default-implementation logic is.
- `create_from_body`'s interaction with `ShiftableLoad`-specific fields
  (`power_kw`, `duration_min`, `earliest_start`/`latest_end`) and the Plan D
  per-device overrides (`soft_deadline`, `target_temp_c`) isn't covered here
  — those fields pass straight through to `UserRequest` untouched by this
  function (no branching logic reads them), so there was no behavior left to
  pin down beyond what serde's own (derive-tested) deserialization already
  guarantees.
