## Context

`ComfortRate` (`entities/asset.rs:61-66`) is a `{fill: f64, max_marginal_price: €/kWh, max_marginal_co2}`
point on a bid curve: the marginal price the user is willing to pay for the *next* unit of
energy at a given fill level. Both existing `default_comfort_rates()` implementations
(`heater.rs:418-431`, `ev.rs:249-262`) are 2-point curves shaped the same way — high price at
`fill=0.0` (urgency), low price at `fill=1.0` (top-off is worth less) — e.g. heater
`(0.0, €0.30/kWh) → (1.0, €0.10/kWh)`, EV `(0.0, €0.35/kWh) → (1.0, €0.05/kWh)`.

The curve is resolved per-session today (`services/comfort.rs::effective_comfort_rates`,
override-wins-over-default) into `AssetRequestSlice.comfort_rates`, but is dropped at
`controller/user_request.rs::create_from_body` (bound to `_comfort_rates`, never read —
`UserRequest` has no field for it) and never reaches `EvMilpContext`/`HeaterMilpContext`,
which are built independently in `simulator/plan_context.rs::build_asset_contexts` from
`EvSession`/`HeaterTarget` (which also have no field for it).

Separately, both MILP contexts already have an economic-reward mechanism that is
*structurally* exactly what a fill-vs-price curve needs to drive, but both are currently fed
from fixed, session-agnostic `PlannerParams` scalars instead of per-session data:

- **EV** (`ev_milp.rs::from_state`, `asset_port.rs:82-123`): `v_core_eur` (one-time reward for
  reaching the core target, MayRun only — documented as `e_core_kwh × v_ev_core_eur_kwh`) and
  `v_extra_eur_kwh` (per-kWh reward for opportunistic energy above core). Both are passed into
  `from_state` as `v_ev_core_eur_kwh` / `v_ev_extra_eur_kwh` parameters, sourced today from
  global planner config, not the session.
- **Heater** (`heater_milp.rs::objective`, `asset_port.rs:385-410`): `w_tier_penalty_eur` biases
  `z_heat_full` vs `z_heat_mid`. Unlike the EV fields, this parameter is *not* session data at
  all — Phase 2 reuses the asset-agnostic ramp-cost constant `c_ramp_eur_kw` for it (same value
  passed to every `AssetKind`), so it can't simply be repointed at a per-session value without
  conflating it with an unrelated ramp cost.

## Goals / Non-Goals

**Goals:**
- Carry the resolved `Vec<ComfortRate>` from the HTTP route through to `EvMilpContext` /
  `HeaterMilpContext` without dropping it.
- Make the EV's existing `v_core_eur` / `v_extra_eur_kwh` reward mechanism session-driven:
  source both from the session's resolved curve instead of global `PlannerParams` defaults.
- Add a new, separate session-driven reward term to the heater's objective (not reusing
  `w_tier_penalty_eur`, since that parameter is shared with an unrelated ramp cost) so the
  curve's `fill=1.0` price shapes the heater's own tier choice.
- Two identical sessions (same asset, same physical state, same tariffs) with different
  curves must produce different allocations — this is the item's own stated verify condition.
- No-curve sessions keep today's behavior exactly (`default_comfort_rates()` fallback already
  exists in `effective_comfort_rates()`; this change doesn't touch that fallback).

**Non-Goals:**
- No new MILP variables, binaries, or hard constraints — the EV side reuses `z_ev_core`/
  `e_ev_extra` exactly as they exist; the heater side adds one new linear reward term next to
  the existing `z_heat_full` tier penalty, not a new binary.
- No true piecewise-linear interpolation machinery for curves with >2 points beyond simple
  linear interpolation between the two bracketing breakpoints — curves in this codebase are
  2-point today (validated by `services/comfort.rs::validate_curve`); a general N-point
  piecewise objective is out of scope until a real need for >2 points exists.
- Battery/PV/base-load: unchanged (see proposal's Non-goals — no session-intent path exists
  for them).

## Decisions

### D1: Add a `value_at_fill` interpolation helper next to `ComfortRate`

Add `pub fn value_at_fill(rates: &[ComfortRate], fill: f64) -> f64` in `entities/asset.rs`,
next to `ComfortRate`'s definition (pure domain logic, no `crate::profile` dependency — keeps
the `entities/` ring rule intact). Behavior: sort-independent (curve is validated
non-decreasing in `fill` by `services/comfort.rs::validate_curve` already), clamp `fill` to
`[rates[0].fill, rates[last].fill]`, linearly interpolate `max_marginal_price` between the two
bracketing points. With today's 2-point curves this reduces to a single lerp between the two
stored points.

**Alternative considered**: a full piecewise-linear MILP formulation with per-segment
auxiliary variables (true "bid stack" solved by the optimizer itself, letting the solver pick
*where* on the curve to land). Rejected for this change — it's real added MILP complexity
(new continuous vars + SOS/big-M segment-selection constraints) for a benefit (blending
between breakpoints "just so") that neither asset's existing structure needs: the EV already
has exactly two economically distinct states (core vs extra) and the heater exactly two tiers
(mid vs full), so evaluating the curve at the two structurally-meaningful fill levels (0.0 and
1.0) is sufficient and keeps the change scoped to "reward source", not "reward mechanism".

### D2: Thread `comfort_rates` through the existing intent chain, don't invent a parallel path

Add `pub comfort_rates: Vec<ComfortRate>` to `UserRequest` (`entities/user_request.rs`),
`EvSession`, and `HeaterTarget` (`entities/device_session.rs`). Populate it in
`create_from_body` from the value currently bound to `_comfort_rates` (rename, stop
discarding). Thread it through whatever already constructs `EvSession`/`HeaterTarget` from a
`UserRequest` (same call sites that set `target_soc`/`departure_time` /
`target_temp_c`/deadline today).

**Alternative considered**: a separate side-channel map (asset ID → curve) fetched directly by
`plan_context.rs` from `AppState.comfort_overrides`, bypassing `UserRequest`/`EvSession`/
`HeaterTarget` entirely. Rejected: it would silently pick up whatever the *current* override
is at solve time, decoupled from the session that was actually created against a specific
curve — sessions must plan against the curve resolved at creation time, matching how
`target_soc`/deadline already work (session-pinned, not live-refetched).

### D3: EV — repoint existing reward sourcing, no new fields on `EvMilpContext`

In `ev_milp.rs::from_state`, when `ev_session` is `Some`, compute:
```
let v_core_eur_kwh = ComfortRate::value_at_fill(&session.comfort_rates, 0.0);
let v_extra_eur_kwh = ComfortRate::value_at_fill(&session.comfort_rates, 1.0);
```
and use these **only in the `UserRequestMode::ByDeadline | UserRequestMode::Asap` match arm**
(`ev_milp.rs:336-357`) — confirmed by re-reading the full `match session.mode` block that this
is the *only* arm where `v_core_eur`/`v_extra_eur_kwh` retain their original "reward for
completing core / reward for topping off beyond core" meaning. Every other arm
(`Opportunistic | AsapFree`, `MaxCost`, `ByDeadlineFree`) already overrides `v_extra_eur_kwh`
with an unrelated economic signal — `v_ev_free_charge_eur_kwh` (free-PV-energy incentive) or
`BUDGET_CHARGE_REWARD_EUR_KWH` (budget-driven completion reward) — so applying the comfort
curve there would conflate two independent concepts. `v_core_eur = core_kwh × v_core_eur_kwh`
as today's doc comment already specifies, unchanged only when `session.soft_deadline` is true
(hard `MustRun` still gets `v_core_eur = 0.0`, per D5). The plugged-no-session early return
and all other match arms keep using the passed-in global defaults untouched.

**Implementation-time finding**: the `v_extra_eur_kwh`/`e_ev_extra` half of this reward is a
documented pre-existing no-op for driving allocation — `e_ev_extra` is only bounded *above* by
`e_extra_max_kwh * z_ev_core`, so the solver "banks" the reward without any real charging
change (tracked as **R-18** in `docs/reference/TECHNICAL_DEBTS.md`, independently
re-discovered while writing this change's tests). Only the `v_core_eur`/`z_ev_core` half is
correctly wired and was used for the verify test. The source-of-truth fix here (curve instead
of global constant) is still correct for `v_extra_eur_kwh` — it just has no observable effect
until R-18 is separately fixed. Not fixed as part of this change (out of scope, see proposal
Non-goals).

### D4: Heater — new additive reward term, not a repoint of `w_tier_penalty_eur`

Add `pub comfort_full_reward_eur_kwh: f64` to `HeaterMilpContext`, computed in
`heater_milp.rs::from_state` as `ComfortRate::value_at_fill(&target.comfort_rates, 1.0)` (0.0
when there's no session/curve, preserving today's behavior exactly). In the inherent
`objective()` (`heater_milp.rs:199-224`), add a new parameter (not a `self` read, so it can be
phase-gated by the caller the same way `w_tier_penalty_eur` already is):
```
pub fn objective(
    &self, v: &HeaterMilpVars,
    w_tier_penalty_eur: f64, m_low_eur_kwh: f64, lambda_sw_eur: f64,
    comfort_full_reward_eur_kwh: f64,   // new
    n: usize, dt_h: &[f64],
) -> Expression {
    ...
    obj += w_tier_penalty_eur * v.z_heat_full[t];
    obj -= comfort_full_reward_eur_kwh * dt * v.z_heat_full[t];   // new
    ...
}
```
**Gated to Phase 2 only**, mirroring exactly how `w_tier_penalty_eur` is already phase-gated in
the `AssetMilpContext::objective()` trait impl (`asset_port.rs:385-410`): the Phase 1 branch
(`c_startup_eur == 0.0`) passes `0.0` for the new parameter; the Phase 2 branch passes
`self.comfort_full_reward_eur_kwh`. Rationale: `w_tier_penalty_eur` is `0.0` in Phase 1 (tier
choice has no cost there — Phase 1's "value of stored energy" lever is the separate
`c_terminal_eur_kwh` terminal reward, not the tier penalty), so an *unconditional* comfort
reward on `z_heat_full` would be a free, uncounterbalanced bias toward full-tier in Phase 1's
coarse allocation. Gating to Phase 2 keeps the new term exactly analogous to the tier penalty
it competes against — both are Phase-2-only friction/smoothing-pass terms — and needs no
change to Phase 1's terminal-reward mechanism or `c_terminal_eur_kwh`'s formula.

**Alternative considered**: fold the curve's `fill=1.0` price into `c_terminal_eur_kwh` instead
(Phase 1's existing "value of stored heat at horizon end" lever), leaving `z_heat_full`/tier
penalty untouched entirely. Semantically also defensible — arguably cleaner reuse of an
existing mechanism — but rejected for this change: it would require re-deriving
`c_terminal_eur_kwh`'s auto-computed formula (today `mean(c_imp) + c_ctrl_imp_malus`) and
verifying that change doesn't alter Phase 1 behavior for sessions that already relied on the
current terminal-reward calibration. The Phase-2-gated tier reward is a strictly additive,
lower-blast-radius change: Phase 1's objective is untouched byte-for-byte when
`comfort_full_reward_eur_kwh` is passed as `0.0`.

### D5: Scope to MayRun/soft paths; MustRun ignores the curve for feasibility

Where a session is `MustRun` (hard deadline, energy requirement not optional), the curve does
not gate feasibility — `e_core_kwh`/`e_target_kwh` hard constraints are unchanged either way.
The curve only shapes objective *rewards*, so a `MustRun` session with an unaffordable curve
still solves; it just won't get extra/full-tier energy beyond what the hard constraint
already forces. This matches the item's own framing (soft reward, not a new hard constraint)
and avoids infeasibility regressions.

## Risks / Trade-offs

- **[Resolved] Reward-term interaction with existing Phase-1/Phase-2 friction weights**: settled
  by D4 — `comfort_full_reward_eur_kwh` is threaded as an explicit `objective()` parameter and
  gated to Phase 2 only (`0.0` in the Phase 1 call), mirroring `w_tier_penalty_eur`'s existing
  phase-gating. → verify with a Phase 1 test asserting the objective is byte-for-byte unchanged
  when the parameter is `0.0`, and a Phase 2 test asserting curve changes shift `z_heat_full`
  allocation.
- **[Resolved] EV mode-arm scope**: settled by D3 — the curve applies only to
  `UserRequestMode::ByDeadline | UserRequestMode::Asap` (the only arm where `v_core_eur`/
  `v_extra_eur_kwh` retain their original core/extra-comfort meaning); every other arm already
  redirects `v_extra_eur_kwh` to an unrelated signal (free-energy incentive, budget reward) and
  is left untouched.
- **[Risk] `value_at_fill` clamping semantics for fill values outside `[0.0, 1.0]`** are
  untested territory (today's callers always query `0.0`/`1.0` exactly, so clamping is inert
  in practice, but the helper must still define clamp behavior for future callers). →
  **Mitigation**: unit-test `value_at_fill` directly with in-range, boundary, and
  out-of-range `fill` inputs.

## Migration Plan

No data migration — `comfort_rates` is a new struct field populated at request time, not
persisted state that needs backfilling (existing `SettingsPort`-persisted overrides are read
unchanged via `effective_comfort_rates()`, which already exists). Land in one PR: entity
field additions → `create_from_body` fix → `plan_context.rs` threading → `EvMilpContext`/
`HeaterMilpContext` changes → objective changes → tests. No feature flag needed — this fixes a
no-op path, so there is no prior behavior to preserve behind a flag; the fallback
(`default_comfort_rates()`) is the existing safe default for any session that predates this
change (none can, since sessions aren't persisted across deploys) or omits an override.

## Open Questions

- **Resolved**: two independent routes construct `EvSession`/`HeaterTarget` in production —
  `POST /user-requests` (→ `create_from_body`, the VEN UI's only session-creation path,
  confirmed via `usePostRequest`'s single caller in `Devices.tsx`) and the legacy
  `POST /ev-session`/`POST /heater-target` direct routes (unused by any UI page). This change
  only touches the `/user-requests` construction path in `services/user_request.rs`; the
  direct routes stay as they are (see proposal Non-goals).
- Whether `HeaterTarget`'s `MayRun` autonomous (no active session) path should synthesize a
  curve from `default_comfort_rates()` explicitly, or simply leave
  `comfort_full_reward_eur_kwh` at `0.0` (today's behavior) — leaning toward `0.0`/unchanged
  since autonomous mode has no user-facing session to attach a curve to.
