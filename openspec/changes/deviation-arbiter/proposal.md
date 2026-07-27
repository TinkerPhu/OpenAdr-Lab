## Why

`docs/plans/deviation-scenarios-analysis.md` §5.3–§5.5 identifies the missing piece left after
backlog item 1 (`solver-marginal-cost`, merged — `PlanTimeSlot` now carries
`marginal_cost_import_eur_per_kwh`/`marginal_cost_export_eur_per_kwh` per slot): a single arbiter
that reads those shadow prices and decides, every tick, how the site's flexible levers absorb the
gap between the plan's expectation and what's actually happening right now. This is backlog item 2
— explicitly flagged in §7 as "the highest-risk piece, the one that's failed twice already."

A real-time deviation-correction layer (feature 017, `absorber.rs`) was built and removed **twice**
because it fought with the MILP plan and an opportunistic EV-surplus overlay, producing sustained
oscillation. Root causes (§1):

1. **One-tick-lag interaction**: the EV overlay read a stale (pre-physics) PV snapshot while the
   absorber reacted to the same tick's live actual-vs-plan — two loops correcting in opposite
   directions on alternating ticks.
2. **Three uncoordinated writers**: MILP dispatch, the opportunistic overlay, and the absorber all
   wrote to the same actuators with no single arbitration order. The removal commit's own words:
   "wrong architecture, not yet working."
3. **Raw deviation, not residual, drove the replan trigger** — causing spurious MILP replans for
   transients the absorber was already correctly absorbing; switching to residual came only after
   the oscillation was already established.

This change is scoped to designing and building a replacement that structurally rules out all three
root causes — not re-proposing the same mechanism with better tuning. A critical review of an
earlier draft of this design (recorded in full below) additionally surfaced two further risks that a
naive rebuild would silently reintroduce: a previously-real single-lever oscillation bug in the
battery correction mechanism this design reuses, and an unthrottled replan-escalation trigger. Both
are addressed explicitly in "What Changes" below, not left as residual risk.

## What Changes

- **One new module, `VEN/src/controller/arbiter.rs`**, becomes the single owner of every reactive
  (non-plan, non-VTN-override) actuator adjustment per tick. It absorbs — moves, does not
  duplicate — `dispatcher::apply_surplus_ev_overlay` (becomes the EV lever) and
  `dispatcher::apply_battery_correction_overlay` (currently `#[allow(dead_code)]`, becomes the
  battery lever), and adds two new levers: heater (pause-within-comfort-band, plus
  `HeaterEmergencyMode::Curtail`/`Absorb` for obligation-penalty-driven cases) and PV curtailment
  (export-excess backstop). `dispatcher::build_setpoints` shrinks back to plan-allocation only.
- **Tick sequence** (`tasks/sim_tick/tick.rs` PHASE 2): `dispatcher::build_setpoints` (plan base
  allocation) → **`arbiter::reconcile` (new)** → `apply_dispatch_override` (VTN alert/dispatch-
  setpoint — unchanged, still wins last, since those are external contractual commands, not routine
  deviation noise).
- **Deviation signal, without the one-tick PV lag that already caused the original bug**: adds
  `SimState::peek_base_load_kw` (new `simulator/base_load_preview.rs`, mirroring the existing
  `peek_pv_kw`'s structure/precedence exactly) so base-load-driven deviations (e.g. scenario D, a
  washing machine) are no longer one-tick-stale — closing the specific gap §1 leaves open (PV's lag
  was fixed for the EV overlay; base load's never was). The arbiter's `projected_net_kw` reuses the
  EV overlay's existing `net_other_kw` filter/fallback logic, generalized to prefer both
  `live_pv_kw` and the new `live_base_load_kw` over the necessarily-stale `SimSnapshot`.
- **Greedy, marginal-cost-ranked lever selection** (§5.3): battery and heater-pause priced off
  `PlanTimeSlot.marginal_cost_import/export_eur_per_kwh`; EV and heater-pause treated as flat
  zero-cost while available (§5.4's own framing — "not because a static rule ranked it third, but
  because its marginal cost is genuinely zero whenever available"); `HeaterEmergencyMode`
  transitions gated behind a new `heater_comfort_override_eur_per_kwh` threshold so routine tariff
  swings never invade the safety envelope, only obligation-penalty-inflated marginal costs do
  (§5.4 scenario H); PV curtailment priced at the forgone export tariff, ranking it last (backstop
  only). Zero-or-below-capacity levers are excluded from ranking outright, not merely deprioritized
  (§5.3's explicit requirement).
- **Residual escalation, in scope for v1** (§5.5): a new per-asset (battery + EV) absorbed-kWh
  accumulator, reset at every plan adoption, that fires a new `PlanTrigger::ResidualThreshold` when
  an asset's absorbed-kWh since the last adoption exceeds a configurable fraction of its available
  charge/discharge capacity at that adoption. This is deliberately accumulator-based from day one —
  never a raw-per-tick-deviation trigger — closing root cause 3 above by construction rather than by
  discovering it the way feature 017 did.
- **Rollout gate**: a new `deviation_arbiter_enabled` flag (profile-configurable, same pattern as
  `EvSettings.opportunistic_charging_enabled`), default `false`. When disabled, the tick loop takes
  today's exact code path — a fully reversible rollout, since this replaces currently-working
  production dispatch logic (the EV overlay) with something new.
- **No BREAKING changes to existing behavior when the flag is off.** When on, EV-overlay behavior
  is preserved functionally (its existing test cases move with it and must still pass) but now
  coexists with three additional levers under one ranking instead of running as the sole reactive
  layer.

## Why this doesn't reproduce feature 017's oscillation — and two further risks a naive rebuild would miss

**Root cause 1 (stale PV) and root cause 2 (three writers) are ruled out structurally**: there is
exactly one function (`arbiter::reconcile`), called exactly once per tick, with one internal
ranked-execution loop — nothing left for a second or third writer to fight — and both
physics-driven inputs (PV, and now base load) are previewed for the current tick, never read stale.

**Root cause 3 (raw-deviation replan trigger) is ruled out by design, not by later correction**:
`PlanTrigger::ResidualThreshold` is accumulator/hysteresis-based from the first version, unlike
feature 017 which started on raw deviation and only switched after oscillation was already
established.

A critical review of this design (before it was finalized) surfaced two further, previously-
unaddressed risks that a naive "one arbiter" rebuild does not automatically avoid, both closed by
explicit tasks below rather than left as an assumption:

- **The battery dead-beat corrector has its own, separate, previously-real oscillation history,
  independent of the multi-writer problem.** `apply_battery_correction_overlay`'s own doc comment
  names a `prev_correction_kw`/`loops.rs` "holding" contract that **no longer exists anywhere in the
  codebase** (confirmed by grep) — and §1 itself records that "a battery correction-hold oscillation
  bug had already surfaced during earlier iteration on the same mechanism, before the feature was
  even generalized." This function must be re-verified with a multi-tick convergence test before
  reuse, not assumed safe because it already has (single-call) unit tests.
- **`PlanTrigger::ResidualThreshold`, as a hard trigger, would bypass all replan-rate gating.**
  Confirmed in `services/planning.rs::evaluate_acceptance_gate`: every non-`Periodic` trigger
  adopts unconditionally, with zero rate-limiting anywhere in the codebase today. Without a
  cooldown, a persistent (not transient) load change could re-cross the residual threshold within a
  few ticks of a replan, causing back-to-back MILP replans — "replan thrashing," the same
  self-retriggering shape as a setpoint oscillation, just at a different timescale.

A third, related risk — chattering at new binary/near-boundary decisions (heater-mode threshold
crossings, near-tied lever costs) that this design introduces and that feature 017's own proven-good
dead-band/settling/linger mechanisms (§3) would have prevented — is likewise closed by an explicit
hysteresis task rather than left unaddressed.

## Capabilities

### New Capabilities
- `deviation-arbiter`: the single-arbiter tick-time lever-selection mechanism (battery, EV, heater,
  PV curtailment), the lag-free deviation computation, the `PlanTrigger::ResidualThreshold`
  residual-escalation mechanism, and the rollout gate.

### Modified Capabilities
- `pv-export-curtailment`: `resolve_pv_export_limit_kw`'s two-source (VTN/capacity, plan)
  tighter-wins resolution gains a third source (arbiter); `PvCurtailmentSource` gains an `Arbiter`
  variant.
- Opportunistic EV overlay behavior is preserved functionally (existing test cases move, unchanged)
  but relocates from `dispatcher.rs` into `arbiter.rs` and now runs under ranked lever selection
  rather than as an unconditional last step.

## Impact

- **Affected files**: `controller/arbiter.rs` (new), `controller/dispatcher.rs` (shrinks — overlay/
  correction functions removed, moved not duplicated), `tasks/sim_tick/tick.rs` + `helpers.rs` (new
  arbiter call site, `live_base_load_kw` threading), `simulator/base_load_preview.rs` (new, mirrors
  `pv_preview.rs`), `entities/asset.rs` (`PlanTrigger::ResidualThreshold`), a new per-asset residual-
  state type (see spec — `entities/site_meter.rs`'s existing `DispatchState` is confirmed dead and
  not reused, wrong shape), `state/mod.rs` (residual-accumulator field + accessors,
  `deviation_arbiter_enabled` setting), `services/planning.rs` (residual reset at the plan-adoption
  call site), `entities/asset_params.rs` (`PvCurtailmentSource::Arbiter`).
- **Preconditions**: `solver-marginal-cost` (merged) — satisfied.
- **No VTN/BFF changes.**
- **This proposal is a design-only pass** (per explicit scope decision): implementation is a
  separate, later decision, not bundled into approval of this document.
