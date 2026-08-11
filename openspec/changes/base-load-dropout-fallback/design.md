## Context

`SimState::tick`'s `BaseLoad` arm (`VEN/src/simulator/mod.rs`) and its
read-only preview twin `peek_base_load_kw`
(`VEN/src/simulator/base_load_preview.rs`) currently resolve a tick's
base-load value with the same 2-tier chain:

```rust
let natural_base_kw = base_load_measured_kw
    .unwrap_or_else(|| bl.baseline_kw_profile + bl.appliance_noise_kw(now));
```

`base_load_measured_kw` comes from `resolve_tick_context` →
`arbiter_glue::resolve_measurements_now`, which is `None` whenever the real
MQTT feed has never been configured, is disabled, or its last reading is
older than `MEASUREMENT_STALENESS_THRESHOLD` (5 min). In all of those cases
today, the fallback is the synthetic `baseline_kw_profile +
appliance_noise_kw(now)` model — invented, not derived from this site — and
that value flows unmarked into `tick_samples`, which
`services::heuristics::learn_asset_heuristics` treats identically to a real
reading on its next run. A feed outage therefore actively re-injects
synthetic-shaped behavior into the site's learned EWMA profile for up to
`rolling_window_days` (42) afterward.

Separately, `AssetHeuristics` (`entities/design_vocabulary.rs`) already
exists and is already populated by a daily job
(`tasks/heuristics_job::spawn_heuristics_job` →
`services::heuristics::learn_asset_heuristics`) — it is not new plumbing,
just an unused fallback source. `AssetHeuristics::sample_kw(now)` already
gives exactly the per-hour, weekday/weekend-bucketed mean this change needs.
`AppState::asset_heuristics()` (`state/heuristics.rs`) already exposes it as
an async read.

Two call sites, one invariant to protect: `peek_base_load_kw` exists
specifically so the deviation arbiter (`build_tick_setpoints`, called before
`sim_guard.tick`) sees the *same* base-load value that `tick` is about to
commit — enforced today by
`peek_base_load_kw_matches_tick_output_for_same_now` and its
lingering-offset sibling in
`VEN/src/simulator/tests/peek_base_load_kw_tests.rs`. Adding a third tier to
only one of the two functions would silently break that parity the moment a
dropout occurs (the arbiter would see a stale synthetic guess while `tick`
records a heuristic-based one, or vice versa) — both functions must receive
the identical resolved heuristic value from the same context field.

R-60 (`services/heuristics.rs` has no error-feedback loop against measured
actuals) lives in the same file and was raised from the same "how is the
learned base-load heuristic actually validated" angle. Bundled here per the
`proposal.md` rationale, but scoped conservatively — see Non-Goals.

## Goals / Non-Goals

**Goals:**
- Replace the 2-tier (measured → synthetic) fallback with a 3-tier
  (measured → learned heuristic → synthetic) chain in both `SimState::tick`
  and `peek_base_load_kw`, keeping their outputs identical for the same
  `now` (BL-40).
- Source the heuristic tier from the existing `AssetHeuristics::sample_kw`,
  gated on a heuristic actually existing for `ids::ASSET_BASE_LOAD` (cold
  start still falls through to synthetic).
- Thread the new value through `resolve_tick_context`/`TickContext` with an
  injectable `now`, consistent with the determinism rule already followed
  by `resolve_weather_pv_kw_now`/`resolve_measurements_now`.
- (Stretch) Add a minimal, additive error-feedback signal to
  `AssetHeuristics` so a future consumer (not necessarily this change) can
  weight the heuristic by its own recent accuracy.

**Non-Goals:**
- Do not add a provenance/origin tag to `tick_samples` rows. That's the
  separate, already-documented gap in
  `docs/architecture/real_measurement_mqtt.md` ("no measured/synthetic
  provenance tag") — orthogonal schema work, out of scope here. A
  heuristic-tier fallback tick is *still* recorded and re-learned exactly
  like a measured one; this change only makes that re-learned value derived
  from real history instead of an invented curve, per the proposal's stated
  limitation.
- Do not change `learn_asset_heuristics`'s core EWMA/bucketing algorithm.
- Do not build a full closed-loop online-learning system for R-60 (e.g.
  gradient correction, per-hour confidence weighting consumed by the
  planner). If investigation during implementation shows the minimal
  additive signal described below needs that machinery to be useful, split
  R-60 into its own follow-up change instead of scope-creeping this one —
  see `tasks.md` for how the split is gated.
- Do not touch the synthetic spike model itself (`appliance_noise_kw`) — it
  remains the true last-resort tier, unchanged.

## Decisions

### D1: New `TickContext` field, not a parameter recomputed per call site

Add `base_load_heuristic_kw_now: Option<f64>` to `TickContext`, resolved
once in `resolve_tick_context` via:

```rust
let base_load_heuristic_kw_now = state
    .asset_heuristics()
    .await
    .get(crate::ids::ASSET_BASE_LOAD)
    .map(|h| h.sample_kw(now));
```

placed alongside the existing `weather_pv_kw_now`/`pv_measured_kw_now`/
`base_load_measured_kw_now` resolution block (before the sync lock, per the
existing pre-lock/in-lock split `context.rs` documents at its top). Both
`tick.rs` call sites (`peek_base_load_kw` before the lock, `sim_guard.tick`
inside it) read `ctx.base_load_heuristic_kw_now` — one value, computed once,
consumed twice. This is what guarantees the parity invariant instead of
each function independently querying `state.asset_heuristics()` (which
would also violate the "no `.await` inside the sim-lock block" rule `tick.rs`
already enforces for `sim_guard.tick`, since that call happens inside the
lock).

Alternative considered: recompute inside `SimState::tick` by giving it a
`&AssetHeuristics` reference or an injectable heuristics port. Rejected —
`simulator/` is the Infra ring per `ven-architecture`, and reaching into
`state.asset_heuristics()` from inside the lock would need an `.await`,
which `tick.rs`'s own comments say the lock block must avoid; resolving it
pre-lock alongside the sibling measurement/weather reads is the established
pattern here, not a new one.

### D2: 3-tier chain shape mirrors PV's precedence exactly

```rust
let natural_base_kw = base_load_measured_kw
    .or(base_load_heuristic_kw)
    .unwrap_or_else(|| bl.baseline_kw_profile + bl.appliance_noise_kw(now));
```

Same `.or(...).unwrap_or_else(...)` shape as PV's `measured_power_kw.or(weather_power_kw)`
tier (see `real_measurement_mqtt.md`'s PV section), so a reader already
familiar with the PV precedence recognizes the pattern immediately. Applied
identically in `SimState::tick`'s `BaseLoad` arm and
`peek_base_load_kw`.

### D3: Both `SimState::tick` and `peek_base_load_kw` gain one new `Option<f64>` parameter

Both functions already take a long, explicit parameter list (no builder/
options-struct refactor in this change — out of scope, and `tick`'s
parameter list is a pre-existing, separately-tracked concern, not something
to fix opportunistically here per `refactoring: ... check
TECHNICAL_DEBTS.md first`; a scan of `TECHNICAL_DEBTS.md` turned up no
entry for `SimState::tick`'s arg count, so this change doesn't take on that
refactor). Add `base_load_heuristic_kw: Option<f64>` as a new trailing
parameter to both, named to match the existing `base_load_measured_kw`
naming convention. All existing call sites (including every test in
`simulator/tests.rs`, `simulator/tests/peek_base_load_kw_tests.rs`, and
`tasks/sim_tick/tick_tests.rs`) need one more trailing `None`/value argument
— mechanical, enumerated in tasks.md.

### D4: Cold-start / "heuristic never learned" gate reuses `AssetHeuristics` presence, not a separate flag

`state.asset_heuristics()` returns a `HashMap<String, AssetHeuristics>`
populated only after `learn_asset_heuristics` has cleared its own
`min_samples_for_confidence` cold-start gate (`Ok(None)` below that
threshold, per `heuristics.rs`) and `set_asset_heuristics` has stored a
result. So "no entry for `ids::ASSET_BASE_LOAD`" already means exactly
"never successfully learned yet" — no new cold-start bookkeeping needed in
this change; the `.get(...)` returning `None` naturally falls through to
the synthetic tier via D2's `.or(...)`.

### D5 (R-60, stretch): additive `recent_error_kw` field, populated by the existing daily job, consumed nowhere yet in this change

Scope R-60 to the smallest testable unit that's still useful standalone:
add `pub recent_mean_abs_error_kw: Option<f64>` to `AssetHeuristics`
(serde-compatible default `None` for existing persisted state — the struct
already derives `Default`/`Deserialize`, and profile/schema rules mean no
migration is needed since this is app-internal state, not the YAML
profile). Compute it inside `learn_asset_heuristics`'s existing single pass
over `ticks`: for each tick, compare `t.power_kw` against
`sample_kw`-equivalent using the *previous* run's `daytime_profile_kw` (the
heuristic that was actually in effect when that tick's fallback — if any —
would have been used), EWMA-weighted the same way the profile itself is.
Store the result; do not yet wire it into `sample_kw` or the planner. This
keeps R-60 additive-only (no behavior change to any existing consumer of
`AssetHeuristics`, so no existing test's expectations shift) while giving a
real, tested number that a follow-up change can act on (e.g. widening the
planner's uncertainty band, or gating BL-40's own heuristic tier on
accuracy). If, during implementation, computing this against the "previous
run's profile" proves to need extra persistence (i.e. `learn_asset_heuristics`
would need last run's `AssetHeuristics` as an input it doesn't currently
take), treat that as the signal that R-60 is materially larger than
scoped — stop, and split it into its own change per `tasks.md`'s gate
rather than forcing it through here.

## Risks / Trade-offs

- [Heuristic tier is smoother/quantized to an hourly mean, not
  minute-scale trapezoid noise, during a dropout] → Accepted as a stated
  stylistic trade-off (per `BL-40`'s own text) — still strictly better than
  an invented curve; not mitigated further in this change.
- [No provenance tag means a heuristic-tier fallback tick is still
  silently re-learned as if measured] → Explicitly out of scope (Non-Goals);
  documented as a persisting caveat in `real_measurement_mqtt.md`'s update.
- [`SimState::tick`'s parameter list grows by one more `Option<f64>`,
  compounding an already-long argument list] → Accepted; `#[allow(clippy::too_many_arguments)]`
  is already present on both functions, and TECHNICAL_DEBTS.md carries no
  entry calling this out as debt to fix opportunistically.
- [`simulator/mod.rs` is close to the 500-production-line cap
  (documented in `TECHNICAL_DEBTS.md` R-40's watch-list at 470/500 as of
  2026-07-16; re-measured during this proposal at 381 total lines via `wc
  -l`, comfortably under)] → Re-run `scripts/audit_file_sizes.py` after
  implementing D2/D3's edit to the `BaseLoad` arm (a few added lines); if it
  crosses 500 production lines, split the arm's fallback-resolution logic
  into a small free function in `simulator/` (e.g.
  alongside `base_load_preview.rs`) rather than inlining further — flagged
  here per this change's own instructions, decided at implementation time
  since the current measured margin is large.
- [R-60's `recent_mean_abs_error_kw` needs "previous run's profile" as an
  input `learn_asset_heuristics` doesn't currently take] → Mitigated by
  D5's explicit split-out trigger: if this turns out non-trivial, R-60 is
  deferred to its own change rather than blocking BL-40's tasks from
  completing.

## Migration Plan

No data migration. `AssetHeuristics` gains one new `Option` field
(R-60, if not split out) — existing persisted state deserializes with
`None` via `serde`'s default-on-missing-field behavior (already relied on
elsewhere in this struct's `Default` derive). No profile/YAML schema
changes, no route/API changes, no feature flag needed — the new fallback
tier only ever activates in the already-existing "measured feed is stale or
absent" branch, so behavior for any VEN instance that has never enabled
real measurements (the whole fleet except ven-1 today) is unchanged unless
that instance's heuristics job has already produced a base-load heuristic
(most simulated VENs will, once the daily job runs on synthetic-backfilled
history) — in which case they now also benefit from a smoother
in-dropout... except simulated VENs never go stale (they always report a
value), so in practice today's change is only observable on ven-1, matching
BL-40's own framing. Rollback is a plain revert (no persisted-state
incompatibility introduced).

## Open Questions

- Should the heuristic tier in `peek_base_load_kw` (which feeds the
  deviation arbiter's dispatch decision) be allowed to diverge from `tick`'s
  in some future change (e.g. arbiter wants a more conservative estimate
  during a dropout)? Not for this change — parity is a hard requirement per
  the existing test; noted here only so a future author doesn't assume the
  two were independently tunable.
- Final call on whether R-60 ships in this change or is split to a follow-up
  is deferred to implementation time per D5/tasks.md's gate, not decided in
  this design.
