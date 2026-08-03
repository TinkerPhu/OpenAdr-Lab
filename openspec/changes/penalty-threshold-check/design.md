## Context

The planner (`VEN/src/controller/milp_planner/`) is a two-phase MILP (Phase 1:
cost minimization, Phase 2: friction minimization). It already has a soft
capacity-violation idiom: `p_imp[t] <= p_imp_max_cont_kw[t] + s_imp_viol[t]`,
with `s_imp_viol[t] >= 0` penalized in the objective at `pen_imp_eur_kwh`
(`solver_phase1.rs:219-242`, default 10,000 €/kWh). This is the template this
change reuses.

`entities/design_vocabulary.rs:118-340` already sketches a much larger
vocabulary for this space (`PenaltyCondition`, `PenaltyThreshold`,
`PenaltyRule`) describing a **stateful, persisted billing-period tracker**:
rolling averages, `breached_this_period`, `current_peak_value` surviving
restarts, and four `PenaltyCondition` variants beyond peak demand. That
vocabulary is unwired and has zero consumers today.

## Goals / Non-Goals

**Goals:**
- Planner keeps every measurement window's peak grid import at or below a
  configured `threshold_kw`, rescheduling flexible load when cheaper than
  paying the penalty rate.
- Config is opt-in per profile, off by default.
- The decision (avoided vs. accepted) is visible in the plan output and the
  VEN UI without needing to read logs or the trace table.

**Non-Goals:**
- No persisted, cross-restart billing-period state (`breached_this_period`,
  rolling averages spanning multiple plan cycles). Each solve re-evaluates
  its own horizon fresh.
- No `EnergyBudgetExceeded`, `EventNoncompliance`, or `ExportLimitExceeded`
  conditions — only `PeakDemandExceeded`. The other three stay unimplemented
  sketches in `design_vocabulary.rs`.
- No VTN-facing capacity-reservation workflow (that's the separate, parked
  BL-24) and no VTN UI changes.
- No binary "any breach = flat cost" semantics — cost is linear in kW over
  threshold (see Decisions).

## Decisions

**D1 — Soft-penalty MILP term, not post-hoc reallocation.**
Alternative considered: solve normally, then detect breaches and re-run
allocation with an added constraint. Rejected: two solves per plan cycle,
and a second unconstrained pass can converge to a different, incompatible
allocation rather than the minimal-disruption one. A single-solve soft
penalty (extra slack var + objective term) lets the same MILP that already
balances cost/comfort/wear also balance "is avoiding this penalty worth the
disruption" — consistent with how `s_imp_viol` already works. It also keeps
the model a pure LP (no new binaries), so solve time is unaffected.

**D2 — Fixed, horizon-aligned windows, not sliding.**
A sliding window (peak evaluated at every possible `measurement_window_s`
offset) would need one slack/constraint per slot-window pair instead of per
fixed-bucket, multiplying variable count and reintroducing overlap-handling
complexity for no benefit at planning-time granularity (a sliding window
matters for *billing* accuracy against a real meter, not for steering a
plan). Windows are computed as
`(slot.start - horizon.start) / measurement_window_s`, deterministic and
clock-free.

**D3 — Linear €/kW-over-threshold, not binary "any breach = flat cost".**
`design_vocabulary::PenaltyRule`'s comment describes binary barriers. Real
utility demand charges are near-universally linear on the metered peak, and
a linear slack keeps the model LP. A high `penalty_eur_per_kw` still
functions as an effective hard barrier in practice. If a genuinely binary
cost is ever needed, it requires a MIP indicator variable — deferred, no
current requirement demands it.

**D4 — New lightweight `PenaltyRuleParams` entity, not reuse of
`design_vocabulary::PenaltyRule`.**
`PenaltyRule` carries fields (`period_s`, `cost_unit`, `breached_this_period`,
`rolling_average_kw`, `current_peak_value`) that only make sense for a
persisted billing-period tracker — none of which this per-solve formulation
needs or should touch. Reusing it would either leave those fields
meaningless/unused (bad for a type meant to model real state) or force this
change to also build the tracker. A new, minimal struct
(`rule_id`, `threshold_kw`, `measurement_window_s`, `penalty_eur_per_kw`) is
implemented instead; `design_vocabulary::PenaltyRule` stays parked for a
future proposal if billing-period tracking is genuinely needed.

**D5 — Config lives in profile YAML, reusing the existing `PlanZone`
cross-ring pattern.**
`profile/schema.rs` already imports entity types directly for
Deserialize-driven config (`PlanZone`, line 5). `PenaltyRuleParams` follows
the same shape: defined once in `entities/planner_params.rs`, deserialized
directly by `profile/schema.rs` — no duplicate profile-layer struct, no new
mapping code, consistent with the project's "profile is an outer ring
depending on entities" rule (`entities/`, `controller/`, `routes/` must never
depend on `profile`, but the reverse is normal and already established).

**D6 — "Penalty accepted" visibility rides the existing `PlanWarning` →
`PlanHeaderBar` path; only "which slots got split" needs new UI.**
`entities/plan.rs::PlanWarning` already renders unconditionally in
`PlanHeaderBar.tsx:148-173` (severity chip, message, suggested action). No
new frontend plumbing is needed for the "rule still breached, penalty
accepted" case — just emit the warning. What *does* need new UI is showing
*why* two slots each show a smaller allocation instead of one large spike,
which is a `PlanDecisionMatrix` visualization concern, not a
warning/notification concern — hence the new "Peak demand" row there,
gated on a new `penalty_rules_active` field so profiles without the feature
see no UI change.

## Correction (found during implementation)

D5 assumed invalid penalty-rule config would surface via
`DomainError::ProfileInvalid`. On inspection, profile-load validation already
has its own established, tested mechanism — `Profile::validate(&self) ->
Result<(), Vec<String>>` (`profile/validate.rs`), which every other profile
invariant (asset bounds, `plan_zones` multiples, `phase2_epsilon_eur` sanity)
goes through, called once at startup in `main.rs` with all violations printed
before `process::exit(1)`. `DomainError::ProfileInvalid`'s own doc comment
marks it reserved for a not-yet-built *hot-reload* validation path, a
different feature. Penalty-rule validation was implemented through the
existing `validate()` mechanism instead, consistent with the codebase's own
established pattern rather than introducing a second, redundant one.

## Risks / Trade-offs

- **[Risk]** A very tight `threshold_kw` relative to fixed baseline load
  makes the plan permanently infeasible-to-satisfy for that window →
  **Mitigation**: this is by design treated like today's `s_imp_viol` —
  the slack absorbs it, cost is paid, a persistent `PlanWarning` says so.
  Not a solver failure, not a config error.
- **[Risk]** Adding a new field to `PlannerParams` breaks the ad-hoc
  `PlannerParams { ... }` test/service constructors that don't use struct
  update syntax (`services/planning.rs` ×4, `controller/milp_planner/tests/
  solver.rs`) → **Mitigation**: `cargo build`/`cargo test` will fail loudly
  at those exact sites; fix by adding the field (default `vec![]`) at each,
  verified as part of the standard test-first workflow, not a runtime risk.
- **[Risk]** File-size ceiling: `solver_phase1.rs` is already large →
  **Mitigation**: new constraint-building logic goes in a dedicated
  `controller/milp_planner/penalty.rs`, keeping both files under
  `scripts/audit_file_sizes.py`'s 500-line ceiling.
- **[Trade-off]** Linear penalty (D3) is a simplification vs. some real
  tariffs' true billing mechanics (e.g. ratcheted demand charges that persist
  across months) — acceptable because D-Non-Goals already excludes
  persisted billing-period tracking; a future change can layer that on if
  ever needed, without touching this formulation.

## Migration Plan

Purely additive: new profile field defaults to `vec![]` (feature disabled),
so existing profiles and existing plans are byte-for-byte unaffected until a
profile opts in. No data migration, no breaking API change. Rollback is
deleting the profile's `penalty_rules` entries (or reverting the change) —
no persisted state to unwind since D1/D2 keep everything per-solve.

## Open Questions

None outstanding — all decisions above were made explicitly rather than left
for implementation time, per the risk/effort concern that motivated this
proposal.
