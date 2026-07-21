# Deviation Scenarios — Analysis

> Status: analysis only, no implementation decisions made.
> Supersedes `docs/plans/deviation-control-suggestions.md` (deleted — see §1: its status table
> claimed a Tier 2 gate "Exists" and described Tier 1 as a fresh design; both were false/stale.
> Applicable, proven-useful pieces of that doc's design are salvaged into §4 below.)

## 1. What actually happened — a real-time deviation-correction layer was built, and removed, twice

This is not a hypothetical gap. A real-time control layer existed, was iterated on for weeks, and
was deliberately deleted because **it fought with the MILP plan and the opportunistic EV-surplus
overlay and produced sustained oscillation**. Any redesign has to explain how it avoids repeating
that failure, not just re-propose the same mechanism.

Chronology (from git history):

1. **Plan F** (`96ad946`, 2026-04-19) — first attempt: a "two-layer control loop with deviation
   transparency" bolted directly into the dispatcher/tick loop.
2. **Plan G / Plan G v2** (`57a3d8c`, `49cd132`) — fixes for a battery correction-hold oscillation
   bug found almost immediately.
3. **`56a8c3b`** — `prev_correction_kw` update made conditional "to prevent 3-tick hold
   oscillation." Oscillation was already a recurring, named failure mode before the feature was
   even generalized.
4. **`docs/plans/deviation-control-suggestions.md`** written (`8cf38b2`, 2026-04-30) as a pre-spec
   design doc — this is the document just deleted.
5. **Feature 017** (`a6ab3d5` onward, 2026-05-03) — full implementation: `absorber.rs` (~1300–1400
   lines), dead-band + settling/ramp state machine, priority order (battery → EV → heater), relay
   wear linger, EV departure guard, SSE `CorrectionActive`/`CorrectionCleared` events, `DeviceDeviation`
   `PlanTrigger` variant, profile config in `test.yaml`/`ven-{1,2,3}.yaml`. This is materially the
   same design `deviation-control-suggestions.md` proposed as "Tier 1/Tier 2."
6. During 017's life: BDD scenarios repeatedly tagged `@wip` for **"physics mismatch with MILP
   plan"**, PV-irradiance-based deviation injection found to be inverted/near-zero because the MILP
   plan doesn't forecast PV (`plan_net_kw` excludes PV, so `actual - plan` is dominated by PV sign,
   not the intended fault), and the Tier 2 trigger had to be changed from raw deviation to
   accumulated *residual* (post-absorption) specifically because raw deviation caused spurious
   replans.
7. **`refactor(018)`** (`09623dc`, 2026-05-09) — first removal: strips `AbsorberState` from the
   tick loop, the `DeviceDeviation` `PlanTrigger`, the SSE events, and profile config. The file
   `absorber.rs` itself was *not* deleted at this point (confirmed via `git log --follow`) — later
   commits (`019`/SimSnapshot refactor, `021`, `028`, `029`) continued to touch it while dormant.
8. **`feat: remove deviation absorption — wrong architecture, not yet working`** (`7aa84a3`,
   2026-05-22) — final removal: deletes `absorber.rs` outright, all BDD features/steps, and updates
   `DOCUMENTATION.md` §2.3 to mark the feature "planned — not yet implemented."

**Root causes, as recorded in `docs/reference/KEY_LEARNINGS.md`** (Deviation Absorber section) and
corroborated by the BDD `@wip` tags:

- **One-tick-lag interaction with the opportunistic EV-surplus overlay.** The overlay read a
  pre-physics snapshot (last tick's state) for a variable (PV) that is not itself controlled —
  it's physics-driven. Reading last tick's value for a physics-driven quantity means the overlay
  is always one tick behind reality. Layer that behind an absorber that is *also* reacting to
  actual-vs-plan on the same tick, and the two corrections can point in opposite directions on
  alternating ticks — this is a textbook oscillation source, and is exactly what "did not get along
  with planning and opportunistic charging" describes.
- **No PV forecast in the plan itself.** `plan_signed_net_kw` was computed from
  battery/EV/base-load allocation only; PV was excluded from the planning forecast entirely. So
  `deviation_kw = actual_net_kw - plan_net_kw` was structurally dominated by whatever PV was doing,
  not by genuine asset mistracking — the absorber was reacting to a modelling gap, not a real fault.
- **Raw deviation vs. residual deviation for the Tier 2 trigger.** Feeding raw (pre-absorption)
  deviation into the replan-trigger counter caused MILP replans for transients the absorber was
  already correctly handling — fixed by switching to residual, but this fix came *after* the
  oscillation pattern was already established, suggesting the underlying multi-loop coordination
  problem was bigger than any single metric choice.
- **Three control writers, no single arbitration order.** MILP dispatch (slot-based, 5–20 min
  cadence), the opportunistic EV-surplus overlay (continuous, persistent-for-the-slot), and the
  absorber (continuous, transient-per-tick) all wrote to the same actuators (battery, EV) without a
  single, well-specified order of "who wins when two layers want to move the same asset in the same
  tick." The commit message's own characterization — **"wrong architecture"** — points at this:
  the fix required wasn't a better dead-band or a better metric, it was a different structure for
  how independent control layers compose.

**Conclusion for any future attempt:** the individual mechanisms (dead-band, settling/ramp,
priority order, relay-wear linger, EV-departure guard, residual-based escalation) all worked
mechanically and are worth reusing (§4). What failed was composing them *underneath* an
already-existing continuous overlay without one arbitration rule, and doing so without the plan
itself accounting for the PV signal the absorber was meant to react to. Any redesign needs to fix
those two things first, or it will reproduce the same oscillation regardless of how well-tuned the
absorber's own state machine is.

## 2. Leftover references — correctness note

The removal was thorough in `VEN/src/` (`grep -r "DeviceDeviation\|apply_deviation_correction\|absorber" VEN/src`
→ zero matches — confirmed directly, not assumed). Three leftovers remain elsewhere and should be
cleaned up as their own small follow-up (not done here — out of scope for an analysis-only pass):

- `DOCUMENTATION.md` §2.3 correctly says "planned — not yet implemented" at the section heading,
  but a config example further down (`deviation_trigger_ticks: 120`, `min_state_linger_s: 0`,
  `ev_departure_guard_s: 1800`, plus two architecture-diagram lines) still presents these as live
  fields. Self-contradictory within the same document.
- `VEN/ui/src/pages/Planner.tsx:153` still special-cases `status.trigger === "DeviceDeviation"` for
  a warning color. Harmless (the value can never occur since the backend's `PlanTrigger` enum no
  longer has that variant) but implies to a reader that deviation-triggered replans are a current
  UI-visible behavior.
- `tests/features/steps/dispatcher_steps.py:159` has a stale section-comment ("Layer 2 —
  DeviceDeviation replan") over a generic trigger-polling step definition that still works fine
  with any trigger string — cosmetic only.

## 3. Asset physical limits vs. current simulation/control limits — two corrections

Two claims in the first draft of this analysis were wrong and are corrected here.

### PV curtailment

**Wrong (first draft):** treated PV as architecturally non-curtailable, a permanent forecast-only
input.
**Correct:** PV curtailment is physically real — real inverters support export limiting and active
curtailment. `PvInverter::step_inner` ignoring `setpoint_kw` is a **simulation modelling gap**
("Phase A" — the codebase's own term), not a physical or architectural ceiling. Once the simulator
models a controllable inverter, PV moves from "forecast input" to "lever" in §3/§5's tables below.
This matters directly for deviation handling: several `OpenADR` `LOAD_DISPATCH`-style signals
assume curtailment is available, and the deviation catalog's "positive deviation" scenarios (too
much import) have a genuinely different, cheaper answer once PV curtailment exists as an option
(reduce PV export directly) rather than only compensating via battery/EV.

### Heater temperature bounds are not symmetric

**Wrong (first draft):** described the heater's `[temp_min_c, temp_max_c]` band as a single
physical envelope with a safety floor and ceiling, and framed the `temp_min_c` emergency override
as a hard safety exception on par with a physical limit.
**Correct, per clarification:** the heater has **no physical lower limit**. It is entirely
physically fine to not heat at all — the tank or room simply drifts toward ambient temperature; the
only thing that "breaks" is the *service function* (hot water availability, room comfort), not
physics. The upper bound (`temp_max_c`) is the one genuine physical/safety ceiling (scalding risk,
tank pressure/relief valve limits, material limits) — that one should stay a hard, non-negotiable
constraint regardless of objective.

This means the current code (`VEN/src/assets/heater.rs`) conflates two different kinds of
constraint into one symmetric-looking band:

| Bound | What it really is today | What it should be |
|---|---|---|
| `temp_max_c` | Genuine physical/safety ceiling (e.g. 80 °C for a hot-water-tank fixture) | Stays a hard constraint no objective or absorber should ever cross |
| `temp_min_c` | Coded as an "emergency" override that forces heating on immediately, bypassing everything (`heater.rs` `emergency_active`) | Actually a **soft, service-level floor**, not a safety floor — there is no physical reason it must be defended immediately; it only affects comfort/service quality |

The practical implication: the current `emergency_active` behavior at `temp_min_c` is not "the
heater's unwritten hard-safety exception" — it's a *deliberate service-quality guarantee* the
system chooses to enforce immediately, which is a legitimate design choice, but it is not forced by
physics the way `temp_max_c` is. That distinction matters for deviation handling and for the
optimization-objective question in §5: under some objectives (e.g. an active grid emergency /
capacity-limit event with real financial or safety stakes on the grid side), it may be entirely
correct to let comfort drop further below `temp_min_c` than "immediately," because unlike
`temp_max_c` there is no physical consequence to doing so — only a service-quality one, which is
exactly the kind of trade a DR-obligation-priority objective (§5) is supposed to be allowed to make.

If the heater model is extended, the natural fix is to separate **safety bounds** (physical,
inviolable, asymmetric — real ceiling, no real floor) from **comfort bounds** (the target operating
band, currently what `temp_min_c`/`temp_max_c` actually represent — e.g. "40–80 °C comfort band" per
the existing tank fixture comment in `heater.rs`). That gives any future deviation-handling or
emergency-event logic real headroom to intentionally sacrifice comfort (drift below today's
`temp_min_c`, all the way toward ambient if the objective calls for it) without ever touching a
physical limit, instead of the current single band where "min" reads as a hard floor it isn't.

## 4. Deviation scenario catalog (unchanged from the first draft, still valid)

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

Three distinct meanings of "deviation" worth keeping separate throughout (unchanged from first
draft): **forecast error** (future-slot mismatch against a forecast/model), **plan tracking error**
(current-slot mismatch against the live MILP plan — what the removed feature 017 targeted), and
**baseline/commitment deviation** (mismatch against an externally promised value — a VTN
obligation, WP-T5 report-submission-status territory — the one with contractual consequences).

## 5. Asset response taxonomy (revised)

| Asset | Can it react within one control tick today? | Physical capability | Constraint that actually bounds it |
|---|---|---|---|
| **Battery** | Yes — fully electronic | Fast, no physical objection | SoC, `max_charge_kw`/`max_discharge_kw`, round-trip efficiency |
| **EV charger** | Yes — same electronic profile | Fast, no physical objection | SoC target, min_soc, session deadline/urgency |
| **PV inverter (curtailment)** | Not currently modelled in the simulator (`step_inner` ignores `setpoint_kw`) | **Physically curtailable** — this is a simulation gap, not a hardware limit | Rated kW, export limit; once modelled, curtailment speed is fast (inverter-electronic) |
| **Heater / boiler** | Slow, thermal-inertia bound; asymmetric bounds (see §3) | Ceiling is real physics; floor is service-quality only, not physics | `temp_max_c` (real), current `temp_min_c` (service-level, not physical), relay-wear concerns (`min_state_linger_s`, proven useful in feature 017, not currently implemented) |
| **Base load / uncontrollable appliance (e.g., washing machine)** | No — not a controllable asset at all, and no per-appliance model exists | Genuinely zero control authority — this is the one case in the table that stays "none" | None — it's a forecast input, not a lever, regardless of any future modelling work |

The washing-machine case remains the clean example of a load with **zero** control authority: it
cannot be told to change power draw, and — unlike PV — that's not a simulation gap to be closed
later, it's inherent to what the device is. Any deviation it causes must always be absorbed by a
different asset or accepted into the residual; it can never absorb its own deviation.

## 6. How the optimization objective changes the *ideal* response (unchanged reasoning, PV/heater notes folded in)

| Objective in force | Ideal absorption priority for a positive deviation (importing more than planned) | Why |
|---|---|---|
| **Cost minimization** | Lowest marginal-cost lever available right now — battery, EV, and (once modelled) PV curtailment only if export price is negative/near-zero | Absorbing via the "wrong" lever can be technically fine but financially suboptimal |
| **Self-consumption maximization** | Battery first, EV second, PV curtailment last resort (curtailing PV throws away free energy — opposite of the goal) | Grid exchange is what's being minimized; curtailing generation to fix an import deviation is self-defeating for this objective specifically |
| **Peak-shaving / capacity limit compliance** | Whatever protects the ceiling fastest — once modelled, PV curtailment is actually attractive here because it removes power at the source rather than compensating downstream | Consequence of missing the cap usually outweighs marginal cost differences |
| **Active DR event / VTN obligation (baseline deviation)** | Fastest lever that keeps the site inside the committed envelope, prioritized over cost/comfort — and, per §3, this is exactly the case where it's legitimate to push the heater below today's `temp_min_c`, since that floor is a comfort choice, not a physical one | Contractual/compliance risk outweighs a comfort-quality trade that costs nothing physically |
| **Comfort-priority mode (if ever added)** | Battery/EV/PV-curtailment absorb; heater shielded down to its *comfort* floor, but — per §3 — could still legitimately go lower before hitting any real physical constraint if the mode's own logic chooses to | The distinction from §3 is what makes this mode's "floor" a policy knob rather than a hard-coded emergency trigger |

## 7. Ideal vs. realistic vs. "where it fits in the VEN" (revised)

| Scenario | Ideal response | Realistic response given current architecture | Where it fits |
|---|---|---|---|
| A. PV cloud transient | Fast lever absorption per active objective, potentially including PV curtailment once modelled | Only battery/EV can react today; the historically-tried absorber for this was removed for oscillating with the opportunistic EV overlay and the plan's own PV blind spot (§1) — any rebuild must fix those two things first, not just re-tune dead-bands | A rebuilt real-time layer, but only after (a) the plan itself accounts for PV, and (b) a single arbitration order exists across MILP dispatch / opportunistic overlay / any new absorber |
| B. PV forecast bias (systematic) | Better forecast input, not a control reaction | `PvInverter::forecast()` and `step_inner` still share the same sin-model for planning, so forecast error remains architecturally near-zero for anything the MILP/absorber would react to. **Update (2026-07-21): a live external weather forecast now exists** — MQTT `openadr-lab/weather/<site_id>/forecast`, ~10 min past every hour (`VEN/src/weather.rs`, `controller/weather_port.rs`, design in `docs/plans/weather-forecast-plugin.md`). It is currently a **parallel, additive** `ForecastSource::Weather` feeding a `/weather` visibility endpoint — it does not yet feed the MILP plan or any deviation/absorption logic. This is the first real forecast-vs-ground-truth channel in the system and is the natural data source once a genuine forecast-error signal is wanted, but closing that loop (feeding it into planning or a future absorber) is still open work | Forecast subsystem (`services/forecast.rs`) — the external feed exists now; wiring it into planning/deviation logic is the remaining step |
| C. Inverter clipping | Planned inside the export limit; if not, hard ceiling, no lever needed (today) — once curtailable, an active lever | Export limit enforced as a clamp in `dispatcher.rs` | MILP input validation today; active curtailment lever once modelled |
| D. Base load step (washing machine) | Absorbed by whichever lever the objective prefers; the load itself never participates | Same historical caveat as scenario A | Rebuilt real-time layer, same prerequisites |
| E. Base load slow drift | Adaptive forecasting, not real-time control | `Heuristic` forecast source exists but has no error-feedback loop | Forecast subsystem — close the loop from measured actuals |
| F. EV session change | Immediate replan — hard input change, not noise | Fits an existing hard-trigger category already | MILP replan trigger, already exists in `PlanTrigger` |
| G. Asset fault/capability loss | Immediate hard replan with reduced flexibility envelope | Unconfirmed whether `CapacityChange`/`Alert` triggers are wired to asset-level (not just tariff/VTN) faults today | Needs its own verification pass |
| H. VTN event boundary mismatch | Fast, obligation-aware absorption allowed to override normal priority/comfort bounds (including heater's comfort floor, §3/§6) for the event's duration | No obligation-aware override path exists | New: an obligation-priority mode, tied to WP-T5 report-submission-status tracking |
| I. Measurement noise | Dead-band ignore | Dead-band concept proven useful in the removed feature 017; not currently implemented | Rebuilt real-time layer's dead-band, if/when rebuilt |
| J. Heater comfort-floor recovery | Should be a policy choice, not an unconditional immediate override (§3) | Currently unconditional (`emergency_active` bypasses everything) | Split safety ceiling from comfort floor in the heater model first — this is the concrete model extension identified in §3 |
| K. Comms loss | Documented fail-safe default per asset | No explicit fail-safe-on-comms-loss behavior found; assets appear to hold last commanded setpoint by default | Separate fault-handling/watchdog design, out of scope for deviation-absorption specifically |

## 8. Open questions for any future redesign

1. **Fix the plan's PV blind spot before rebuilding any real-time layer.** The historical oscillation
   was partly caused by the MILP plan not forecasting PV at all, making "deviation" mostly a PV
   signal in disguise. This should be resolved independently of, and before, any absorber redesign.
   The live external weather feed (below) is the natural input for this once wired in — but the
   sin-model-vs-plan blind spot is a separate, more urgent bug than the presence of a real feed
   would fix by itself: even with real weather data, the plan must actually consume it.
2. **Define one arbitration order across all control writers up front.** MILP dispatch, the
   opportunistic EV overlay, and any real-time correction layer need a specified, single answer to
   "who wins this tick" — not three independently-reasoning layers converging on the same actuator.
3. **Split heater safety bounds from comfort bounds in the model** (§3) — this unlocks legitimate,
   objective-driven comfort trade-offs (§6) without ever touching the real physical ceiling, and
   removes the current false symmetry between `temp_min_c` and `temp_max_c`.
4. **Model PV curtailment in the simulator** — currently the single biggest gap preventing PV from
   being evaluated as a lever at all, for both routine deviation absorption and OpenADR
   `LOAD_DISPATCH`-style signals that assume it exists.
5. **Objective-conditional priority/override rules** (§6): should priority order and comfort-floor
   overridability change per active objective/event, or should DR-obligation compliance be a
   separate, higher-priority mode that pre-empts normal-day logic entirely during an event window?
   The latter is simpler to reason about.
6. **Clean up the three confirmed leftover references** (§2) — small, low-risk, but worth doing so
   the docs/UI stop implying the removed feature is live.
7. **Decide whether/how to wire the new live weather feed into planning and deviation detection**
   (§7 scenario B) — it exists today only as a visibility source (`ForecastSource::Weather`); using
   it to replace or supplement the sin-model forecast in the MILP, and/or as the reference for a
   real forecast-error signal, is unbuilt work this analysis did not previously anticipate having
   real data available for.
