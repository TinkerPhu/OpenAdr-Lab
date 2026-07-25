# Deviation Scenarios — Analysis

> Status: analysis only, no implementation decisions made.
> Supersedes `docs/plans/deviation-control-suggestions.md` (deleted — see §1: its status table
> claimed a Tier 2 gate "Exists" and described Tier 1 as a fresh design; both were false/stale.
> Applicable, proven-useful pieces of that doc's design are salvaged into §3 below.)

## 1. What actually happened — a real-time deviation-correction layer was built, and removed, twice

A real-time control layer existed, was iterated on for several weeks, and was deliberately deleted
twice because **it fought with the MILP plan and the opportunistic EV-surplus overlay and produced
sustained oscillation**. Any redesign has to explain how it avoids repeating that failure, not just
re-propose the same mechanism.

It shipped as feature 017 (`a6ab3d5`, 2026-05-03): `absorber.rs` (~1300–1400 lines), a dead-band +
settling/ramp state machine, priority order (battery → EV → heater), relay-wear linger, an
EV-departure guard, SSE `CorrectionActive`/`CorrectionCleared` events, and a `DeviceDeviation`
`PlanTrigger` variant — materially the same design the (now superseded) pre-spec doc
`deviation-control-suggestions.md` had proposed. A battery correction-hold oscillation bug had
already surfaced during earlier iteration on the same mechanism, before the feature was even
generalized — a recurring failure mode, not a one-off bug. During 017's life, BDD scenarios were
repeatedly tagged `@wip` for "physics mismatch with MILP plan": PV-irradiance-based deviation
injection came out inverted/near-zero because the MILP plan didn't forecast PV at the time
(`plan_net_kw` excluded PV, so `actual - plan` was dominated by PV sign, not the intended fault —
since fixed, see §6 scenario B), and the replan trigger had to switch from raw deviation to
accumulated *residual* because raw deviation caused spurious replans.

It was removed in two steps: `refactor(018)` (`09623dc`, 2026-05-09) stripped it from the tick loop
first; the final removal, `7aa84a3` ("remove deviation absorption — wrong architecture, not yet
working", 2026-05-22), deleted `absorber.rs` outright along with all BDD features/steps.

**Root causes**, from `docs/reference/KEY_LEARNINGS.md` (Deviation Absorber section), corroborated
by the BDD `@wip` tags:

- **One-tick-lag interaction with the opportunistic EV-surplus overlay.** The overlay read a
  pre-physics snapshot (last tick's state) for PV — a physics-driven variable it doesn't control —
  so it was always one tick behind. Layered behind an absorber reacting to the same tick's
  actual-vs-plan, the two corrections could point in opposite directions on alternating ticks: a
  textbook oscillation source. **Still open** — this is an arbitration/timing problem, not a
  forecasting one.
- **No PV forecast in the plan itself, at the time.** `deviation_kw` was structurally dominated by
  whatever PV was doing, not genuine asset mistracking. **Resolved since** — the plan now resolves
  PV through the weather forecast (§6 scenario B) — but a new correction layer still needs the
  *current*-tick PV value, not a lagged one; that's the still-open bullet above.
- **Raw deviation vs. residual for the replan trigger.** Feeding raw deviation into the replan
  counter caused spurious MILP replans for transients the absorber was already correctly handling;
  switching to residual came only after the oscillation was already established, suggesting the
  underlying multi-loop coordination problem was bigger than any single metric choice.
- **Three control writers, no single arbitration order.** MILP dispatch, the opportunistic overlay,
  and the absorber all wrote to the same actuators without one specified order for "who wins this
  tick." The commit message's own characterization — **"wrong architecture"** — points here: the
  fix needed wasn't a better dead-band or metric, it was a different structure for how independent
  control layers compose.

**Conclusion for any future attempt:** the individual mechanisms (dead-band, settling/ramp,
priority order, relay-wear linger, EV-departure guard, residual-based escalation) all worked
mechanically and are worth reusing (§3). What failed was composing them *underneath* an
already-existing continuous overlay without one arbitration rule, while reacting to a PV signal the
plan itself didn't yet model. The PV gap is closed now (§6 scenario B); the arbitration gap is not.
§5 proposes a concrete design for the arbitration problem: a single arbiter that subsumes the
opportunistic overlay (fixing the stale-snapshot lag) and reads a marginal-cost signal taken
directly from the planner's own solve.

## 2. Asset physical limits vs. current simulation/control limits

### PV curtailment

PV curtailment is physically real and already modelled in the simulator. `export_limit_kw` clamps
export every tick (`PvInverter::step_inner`, `VEN/src/assets/pv.rs`), and is driven today by VTN
`EXPORT_CAPACITY_LIMIT` events and by sim-injected test scenarios. (`step_inner`'s `setpoint_kw`
argument is unused dead code — it was never the curtailment mechanism; `export_limit_kw` is.)

**What needs implementing:** the MILP has no PV-export decision variable. PV is only ever a forecast
input to the power-balance equation, so the plan never evaluates "would curtailing X kW of PV
improve the objective this slot," and nothing feeds a planner-derived value into `export_limit_kw` —
today that field only ever comes from an external VTN signal or a test injection, never from
optimization. Once the planner gains that decision variable, PV moves from "forecast input" to
"lever" in §4's table below and in §5's marginal-cost design. This matters directly for deviation
handling: several `OpenADR` `LOAD_DISPATCH`-style signals assume curtailment is available as an
*optimized* choice, and the deviation catalog's "positive deviation" scenarios (too much import) get
a genuinely different, cheaper answer once the planner can choose to reduce PV export directly
rather than only compensating via battery/EV.

### Heater temperature bounds are not symmetric

Today's `temp_min_c`/`temp_max_c` (`VEN/src/assets/heater.rs`) are a **comfort/service band**, not
the asset's true physical limits — they're what a user configured as the acceptable tank-temperature
range for normal day-to-day operation, and what the planner is expected to stay inside under ordinary
objectives (cost, self-consumption, etc.).

Outside that band, on both sides, there is a separate, wider **safety envelope** the comfort band sits
well inside of:

| Direction | Comfort-band edge (today's config) | True safety limit | What's between them |
|---|---|---|---|
| Low side | `temp_min_c` (e.g. 40 °C) | Ambient temperature — no physical harm ever, tank just drifts | Pure service-quality degradation, no safety concern at any point |
| High side | `temp_max_c` (e.g. 80 °C) | A real hard ceiling above it (e.g. 90 °C — scalding risk, tank pressure/relief-valve limits) | Still physically safe, but outside what the user configured as comfortable |

The comfort band is what normal planning should respect. The wider safety envelope should only be
reachable under an **active VTN emergency directive**, not during routine optimization: an emergency
curtailment request lets the tank drift below `temp_min_c` all the way toward ambient (no physical
cost to doing so, only comfort); an emergency energy-absorption request lets it heat above
`temp_max_c` up to the real safety ceiling (still physically safe, just outside normal comfort). The
current code doesn't model this distinction — `temp_min_c`/`temp_max_c` are the only bounds that
exist, `emergency_active` already treats `temp_min_c` as if it were a hard limit rather than a
comfort edge, and there is no separate field for the true safety ceiling above `temp_max_c` at all.

The natural model extension is a second, wider pair of bounds — a true safety floor (ambient) and
true safety ceiling (e.g. 90 °C) — that only an active VTN emergency signal is allowed to use; normal
objectives stay confined to the existing comfort band regardless of how aggressively they'd otherwise
want to trade comfort for cost or grid compliance.

This also means the per-site profile format needs extending, not just the code: today's profiles
(`VEN/profiles/*.yaml`) configure only `temp_min_c`/`temp_max_c`, and the values vary a lot by
installation — `ven-2.yaml`'s 40–80 °C tank vs. `no_pv_test.yaml`'s 18–23 °C room-heating band — so
the true safety floor/ceiling would need to be new, separately configurable fields per profile (e.g.
`temp_safety_min_c`/`temp_safety_max_c`), not a hardcoded constant, since a room heater's real safety
ceiling has nothing in common with a hot-water tank's.

## 3. Deviation scenario catalog

| # | Scenario | Type | Typical magnitude/speed | Duration |
|---|---|---|---|---|
| A | PV cloud transient (fast-moving cloud shadow) | Plan tracking | -80% of rated PV in 5–30 s | Seconds to a few minutes |
| B | PV forecast systematically low/high for the day (seasonal/weather-model bias) | Forecast error | ±10–30% of daily forecast | Hours (whole day) |
| C | Inverter clipping / export-limit hit unexpectedly early | Plan tracking | Hard ceiling, not gradual | Slot-persistent |
| D | Base load step change (appliance turns on/off outside model — e.g., washing machine cycle, oven, unmodelled resistive load) | Plan tracking | Step of 0.5–3 kW, instant | Minutes (uncontrollable duration) |
| E | Base load slow drift (occupancy pattern shift day to day) | Forecast error | Small, cumulative | Days |
| F | EV session parameters change (user plugs in late, unplugs early, changes target SoC via app) | Plan tracking (self-inflicted, not physical) | Up to full charger rating | Until next replan |
| G | Battery/EV/heater capability degrades or a fault occurs (thermal derate, BMS fault, breaker trip) | Plan tracking, structural | Partial to full loss of an asset's flexibility | Until fixed / next replan |
| H | VTN event boundary mismatch (event starts/ends but site was mid-transition) | Baseline deviation | Depends on event type | Minutes at boundary |
| I | Grid-side measurement noise / meter jitter | None (not real) | ±0.05–0.2 kW | Continuous, high-frequency |
| J | Heater comfort-floor recovery triggers (`temp_min_c` breached) | Plan tracking, intentional | Full heater power, immediate under current code | Until temp recovers |
| K | Communication loss to VTN or to an asset controller | Structural | Unknown state | Variable |

Three distinct meanings of "deviation" worth keeping separate throughout: **forecast error** (future-slot mismatch against a forecast/model), **plan tracking error**
(current-slot mismatch against the live MILP plan — what the removed feature 017 targeted), and
**baseline/commitment deviation** (mismatch against an externally promised value — a VTN
obligation, WP-T5 report-submission-status territory — the one with contractual consequences).

## 4. Asset response taxonomy

| Asset | Can it react within one control tick today? | Physical capability | Constraint that actually bounds it |
|---|---|---|---|
| **Battery** | Yes — fully electronic | Fast, no physical objection | SoC, `max_charge_kw`/`max_discharge_kw`, round-trip efficiency |
| **EV charger** | Yes — same electronic profile | Fast, no physical objection | SoC target, min_soc, session deadline/urgency |
| **PV inverter (curtailment)** | Physically curtailable and already modelled in the simulator (`export_limit_kw` clamps every tick) — but not yet a planner decision variable, only ever set externally (VTN signal or test injection) | **Physically curtailable**, fast (inverter-electronic) | Rated kW, export limit; the gap is the MILP not choosing a value, not the simulator lacking the mechanism |
| **Heater / boiler** | Slow, thermal-inertia bound; two-tier bounds (see §2) | Both `temp_min_c`/`temp_max_c` are comfort-level, not physical; the true safety envelope (ambient floor, real ceiling above `temp_max_c`) is wider and currently unmodelled | `temp_max_c`/`temp_min_c` (comfort band, today's only bounds), no field yet for the true safety ceiling; relay-wear concerns (`min_state_linger_s`, proven useful in feature 017, not currently implemented) |
| **Base load / uncontrollable appliance (e.g., washing machine)** | No — not a controllable asset at all, and no per-appliance model exists | Genuinely zero control authority — this is the one case in the table that stays "none" | None — it's a forecast input, not a lever, regardless of any future modelling work |

The washing-machine case remains the clean example of a load with **zero** control authority: it
cannot be told to change power draw, and — unlike PV — that's not a simulation gap to be closed
later, it's inherent to what the device is. Any deviation it causes must always be absorbed by a
different asset or accepted into the residual; it can never absorb its own deviation.

## 5. Marginal-cost single-arbiter design (proposed) — replaces the per-objective priority table

> Status: proposed, not implemented. Supersedes an earlier version of this section that gave each
> objective its own hand-written lever-priority table (cost minimization, self-consumption,
> peak-shaving, DR obligation, comfort-priority — five separate rows of manually-reasoned priority
> order). That table is deleted below in favor of one mechanism that produces the same behavior as
> a *consequence* of the plan's own numbers, rather than as five independently-maintained rules that
> can drift out of sync with each other and with the planner.

### 5.1 The signal is per-slot marginal cost, not "which slots are minimizing vs. maximizing import"

An earlier framing of this idea used the plan's min-vs-max-import phases as the priority signal.
That's too coarse: the informative number is the **shadow price on the slot's power-balance
constraint** — how much the objective would change if the site were forced to import one more kWh
right now — not the direction of flow. Two slots can both be "charging the battery" and have very
different marginal costs depending on whether the battery is at its power limit.

Worked example, day-ahead tariff with a cheap overnight block, 10 kWh / 3 kW battery:

| Slot | Tariff (€/kWh) | Plan action | Shadow price on import (€/kWh) | Why |
|---|---|---|---|---|
| 02:00–02:15 | 0.12 (cheap) | Battery charging, mid-range, no bound active | ≈ 0.12 | Nothing else is binding; marginal cost is just the tariff |
| 07:30–07:45 | 0.34 (peak) | Battery discharging at its 3 kW power limit | ≈ 0.6–0.9 | The battery is *at a bound* — one more kWh of import can't be offset by discharging harder, so the marginal cost is worse than the tariff alone |
| 12:30–12:45 | 0.18 (mid) | Battery full, EV done charging | ≈ 0.18 | No asset is binding; marginal cost collapses back to the tariff |

The 07:30 row is the one a "min vs. max import" framing would miss — the plan doesn't look
qualitatively different there, but the shadow price does, because it's driven by which asset is
pinned at a limit, not by the sign of the flow.

### 5.2 Getting the number out of the solver

Battery/EV/heater mode selection almost certainly introduces a binary (e.g. forbidding simultaneous
charge/discharge, or heater on/off). Raw MILP duals aren't meaningful once integers are involved, so
the shadow price needs a second, cheap solve per planning cycle:

1. Solve the MILP as today → read off the optimal binary assignment per slot (e.g.
   `battery_mode = discharge` at 07:30).
2. Re-solve the same slot as a pure LP with that binary **fixed** (not re-optimized) → HiGHS now
   returns a real dual value on the power-balance constraint.
3. Add that value as a new field on whatever `SolverPort::solve` returns today — one
   `marginal_cost_eur_per_kwh` per slot. This is a small, additive change to the solver output, not
   a new solver.

**The dual is directional, not a single number, whenever the objective is kinked at zero net
exchange.** A dual value is the sensitivity to perturbing a constraint in *one* direction. Under cost
minimization the import-side and export-side marginal costs are usually close because the tariff
curve is close to linear through zero, so treating it as one number is a harmless simplification.
Under an objective that penalizes import and export differently around zero (self-consumption
maximization, or autarky if ever added as a soft objective — §5 stays valid there, see the
conversation that produced this note), the objective has a genuine kink at zero net exchange:
importing one more kWh and exporting one more kWh are *not* symmetric perturbations of the same
constraint, and can have materially different shadow prices. Concretely, step 2 needs to fix not
just the asset-mode binaries but also the import/export-mode binary (there is almost certainly
already one, to forbid simultaneous import and export), and read the dual that matches the actual
deviation being corrected: a positive deviation (over-importing) needs the import-side dual; a
negative deviation (over-exporting, e.g. a PV surplus) needs the export-side dual. `SolverPort`
should expose both — `marginal_cost_import_eur_per_kwh` and `marginal_cost_export_eur_per_kwh` per
slot — rather than a single `marginal_cost_eur_per_kwh`, so the arbiter picks the one that matches
the sign of the deviation it's correcting.

This does **not** double the solver cost. Branch-and-bound over the integers is the expensive part
of a MILP solve; fixing the binaries and reading duals off the remaining LP is a single extra LP
solve, essentially free by comparison — standard practice, not a novel expensive step. It also
doesn't require a second solve per direction per slot: for a non-degenerate LP, the dual from one
solve is already valid for small perturbations in *either* direction, up to the point where a
different constraint would become binding (standard LP sensitivity/ranging, which HiGHS reports
alongside the dual at negligible extra cost). The two-solve-per-slot case only matters if a
deviation is large enough to actually flip which mode a slot is in (e.g. a slot that was importing
gets pushed into net export) — that's a much bigger, rarer event than the transient corrections this
mechanism targets, and should be treated the same as scenario F/G's hard replan triggers rather than
precomputed for every slot up front. So the opposite-direction dual only needs to be computed
lazily, for the one slot in question, on the rare occasion it's actually needed.

### 5.3 One arbiter, one owner of every actuator write per tick

§1's postmortem identified "three control writers, no single arbitration order" as a root cause,
independent of any priority metric. The marginal-cost signal only pays off if it's read by a single
arbiter that owns every actuator write for the tick — which means the opportunistic EV-surplus
overlay is folded into this arbiter rather than left running as a separate loop, and the arbiter
reads **this tick's** actual PV/base-load, not the prior tick's snapshot (the specific lag that
caused 017's oscillation, §1). Priority order is then just "rank available levers by current
marginal cost, cheapest first" — with the important caveat that a lever's *remaining capacity* must
be checked, not just its existence: an EV already at its target SoC, or a battery already at its
SoC limit, is zero-capacity right now, not merely "lower priority."

### 5.4 Worked examples across the deviation catalog (§3)

**Scenario A — PV cloud transient.** PV drops from 4.2 kW to 0.9 kW for 90 s at 12:40 → 3.3 kW more
import than planned. EV is mid-session under the (now-folded-in) opportunistic overlay; pulling its
opportunistic draw back costs nothing against the session's own target (≈ 0.10 €/kWh-equivalent).
Battery discharge right now costs ≈ 0.18 €/kWh (mid-tariff, nothing binding). Arbiter picks the EV
first — reduces its opportunistic draw from 5 kW to 1.7 kW for the transient — no battery movement,
no second loop to fight it.

**Scenario D — base load step (washing machine, +2 kW instantly).** EV is already at target SoC
(zero remaining capacity, correctly excluded rather than just deprioritized); battery at 40% SoC,
marginal cost ≈ 0.18 €/kWh, 3 kW headroom. Arbiter covers the 2 kW from the battery. If the heater
happens to be mid-cycle heating toward its `temp_max_c` ceiling at that moment, pausing it is a
zero-cost lever (§2: no physical floor) and gets used opportunistically — not because a static rule
ranked the heater third, but because its marginal cost is genuinely zero whenever it's available.

**Scenario H — VTN obligation boundary.** Under a `LOAD_DISPATCH` capacity limit with a contractual
penalty for breach (say €50 for the event), the shadow price on the import constraint during the
obligation window isn't the routine 0.15–0.30 €/kWh, it's whatever the breach penalty amortizes to —
effectively far higher than any comfort-preservation value. The same greedy-cheapest-lever arbiter
therefore prefers whatever avoids the breach over whatever preserves comfort, including pushing the
heater below today's `temp_min_c` (§2) toward ambient — not because a special "DR event" row
overrides normal priority, but because the obligation's penalty is baked into that window's shadow
price and the arbiter is just following the numbers. This is what collapses the old table's five
separate rows into one mechanism: the "Active DR event" and "comfort-priority" rows were really just
different shadow-price magnitudes on the same constraint, not different logic.

### 5.5 Where pure greedy breaks — the SoC-coupling trap

Marginal cost computed *now* only reflects the plan as it stood at the last solve. SoC is a resource
shared across all future slots, so a sequence of individually-correct greedy choices can still be
wrong in aggregate. Example: battery at 50% SoC (5 kWh) at 09:00; four separate 2 kW deviations
during the morning each get greedily absorbed by the battery at its then-current marginal cost
(each looks fine in isolation, costing ~0.3 kWh apiece); by 14:00 a genuine 0.40 €/kWh price spike
hits that the *original* plan was counting on the battery to shave — but 1.2 kWh of that capacity is
already gone to morning noise the plan never anticipated needing to protect against. Every individual
greedy decision was locally optimal; the aggregate undermined a later, more valuable use of the same
resource.

This is why greedy-by-marginal-cost is a **tie-breaker within a tick**, not a substitute for
replanning: the residual-based escalation feature 017 evolved toward (§1) — trigger a real MILP
replan once accumulated residual crosses a threshold, rather than continuing to absorb — is what
catches this case, because a replan recomputes the battery's future obligations against where SoC
actually is instead of trusting hours-stale marginal-cost numbers.

### 5.6 Preconditions before this can be built

- Requires the `SolverPort` output extension in §5.2 (one extra cheap LP solve per cycle).
- Requires collapsing the opportunistic EV-surplus overlay and any real-time corrector into the one
  arbiter described in §5.3 — building this as a second independent loop reproduces 017's failure
  regardless of how the priority number is computed.
- The plan-must-forecast-PV precondition once listed here is already satisfied (§6 scenario B) — but
  §6 B's own open point still applies: no experiment has confirmed how much that forecast actually
  reduces PV-driven deviation, which is worth checking before relying on the resulting shadow prices.

## 6. Ideal vs. realistic vs. "where it fits in the VEN"

| Scenario | Ideal response | Realistic response given current architecture | Where it fits |
|---|---|---|---|
| A. PV cloud transient | Fast lever absorption per active objective, potentially including PV curtailment once the planner can choose it (§2/§7 task 1) | Only battery/EV can react today; the historically-tried absorber for this was removed for oscillating with the opportunistic EV overlay (§1) — a rebuild must fix that arbitration gap first, not just re-tune dead-bands | The §5 marginal-cost arbiter, once (a) it subsumes the opportunistic overlay and reads current-tick PV, and (b) the `SolverPort` duals (§5.2) exist |
| B. PV forecast bias (systematic) | Better forecast input, not a control reaction | A live external weather forecast now exists and feeds the planner — MQTT `openadr-lab/weather/<site_id>/forecast`, ~10 min past every hour (`VEN/src/weather.rs`, `controller/weather_port.rs`, architecture in `docs/architecture/weather_forecast.md`). `PvInverter::build_milp_context`'s PV input now resolves through `entities::solar::resolve_weather_pv_kw` (weather-sourced forecast when fresh, sin-model fallback otherwise), and the same resolution feeds the `/weather` visibility endpoint (`ForecastSource::WeatherModel`) — so this is a real forecast-vs-ground-truth channel wired into planning, not just a parallel visibility feed. Whether it measurably reduces PV-driven deviation/absorption events is still open — no experiment has quantified it yet | Forecast subsystem (`services/forecast.rs`, `entities/solar.rs`) — wired; quantifying its effect on deviation is the remaining step |
| C. Inverter clipping | Planned inside the export limit; if not, hard ceiling, no lever needed (today) — an active lever once the planner can choose `export_limit_kw` itself (§7 task 1) | Export limit enforced as a clamp in `dispatcher.rs`; the clamp mechanism already works (§2), it's just never planner-driven | MILP input validation today; active curtailment lever once the planner gains that decision variable |
| D. Base load step (washing machine) | Absorbed by whichever lever the objective prefers; the load itself never participates | Same arbitration-gap caveat as scenario A | The §5 marginal-cost arbiter, same prerequisites as scenario A |
| E. Base load slow drift | Adaptive forecasting, not real-time control | `Heuristic` forecast source exists but has no error-feedback loop | Forecast subsystem — close the loop from measured actuals |
| F. EV session change | Immediate replan — hard input change, not noise | Fits an existing hard-trigger category already | MILP replan trigger, already exists in `PlanTrigger` |
| G. Asset fault/capability loss | Immediate hard replan with reduced flexibility envelope | Unconfirmed whether `CapacityChange`/`Alert` triggers are wired to asset-level (not just tariff/VTN) faults today | Needs its own verification pass |
| H. VTN event boundary mismatch | Fast, obligation-aware absorption allowed to override normal priority/comfort bounds (including heater's comfort floor, §2/§5.4) for the event's duration | No obligation-aware override path exists | New: the §5 marginal-cost arbiter, with the obligation's breach penalty baked into that window's shadow price (§5.4) — not a separate mode, tied to WP-T5 report-submission-status tracking |
| I. Measurement noise | Dead-band ignore | Dead-band concept proven useful in the removed feature 017; not currently implemented | Rebuilt real-time layer's dead-band, if/when rebuilt |
| J. Heater comfort-floor recovery | Should be a policy choice bounded by the comfort band, not an unconditional immediate override into the (currently unmodelled) safety envelope (§2) | Currently unconditional (`emergency_active` bypasses everything) | Add the true safety floor/ceiling as a second, wider bound pair — reachable only under an active VTN emergency directive — plus the corresponding profile fields (§2) |
| K. Comms loss | Documented fail-safe default per asset | No explicit fail-safe-on-comms-loss behavior found; assets appear to hold last commanded setpoint by default | Separate fault-handling/watchdog design, out of scope for deviation-absorption specifically |

## 7. Remaining implementation work

The items below started as open questions in earlier drafts of this analysis; they've since been
decided (see the sections referenced). What's left is building them, in the order below — not
deciding them.

**Decided, not tracked as separate tasks:**

- Arbitration: a single arbiter owns every actuator write per tick, subsuming the opportunistic
  overlay (§5.3) — folded into task 3 below, not a standalone design question.
- Objective-conditional priority/override rules: fall out of the marginal-cost/shadow-price signal
  (§5) rather than separate per-objective priority tables — folded into tasks 2–3 below.
- Whether the now-wired weather forecast measurably reduces PV-driven deviation: not treated as a
  prerequisite for anything above. Deviations stay inherently unpredictable regardless of forecast
  quality — that unpredictability is the reason a correction mechanism is needed at all — and
  isolating the forecast's specific contribution isn't practical to measure cleanly. Not a task.

**Backlog, in build order:**

1. **Give the planner a PV-export decision variable** (§2) — the simulator already curtails
   correctly (`export_limit_kw`); the MILP just needs to choose a value for it instead of treating
   PV as forecast-only. Standalone value (better peak-shaving/capacity-limit/autarky plans) with no
   dependency on the arbiter rebuild, and none of the risk that's bitten twice before.
2. **`SolverPort` marginal-cost/shadow-price extension** (§5.2) — one extra cheap LP solve per
   planning cycle, exposing directional duals per slot. Only useful once building the arbiter, so it
   comes right before it.
3. **Build the single arbiter** (§5.3) — the highest-risk piece, the one that's failed twice
   already (§1). Do it last, so it inherits the richer lever set from task 1 and whatever's learned
   building task 2; give it its own focused design pass rather than rushing it alongside everything
   else.
