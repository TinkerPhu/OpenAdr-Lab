# Phase 5 — Forecast & Baseline

> **Goal:** the VEN learns from its own past (SG-4): heuristic profiles for
> uncontrollable loads, external weather feeds for PV/thermal, and baseline reports
> that make the VTN-side report-usefulness evaluation (SG-3) rigorous (M&V-grade).
> **Remaining item:** WP5.4 — BASELINE reports + capability forecast + quality
> metadata (UC:baseline §7.5, UC:§8.7, UC:quality metadata). WP5.1–WP5.3 are done
> (see below).
> **Exit demonstration:** one experiment re-run where BASELINE reports let `kpi.py`
> quantify a single event's impact in kWh.
> **Total effort remaining:** ~1–2 weeks (WP5.4 only).

## WP5.1/WP5.2 — done: SITE_RESIDUAL + AssetHeuristics (BL-08, BL-14)

Both shipped and resolved in `docs/BACKLOG.md`. Architecture, design rationale, and
Node1 verification: `wiki/components/heuristics-pipeline.md` — the `site-residual`
virtual asset, the EWMA-weighted weekday/weekend learner
(`services/heuristics.rs`), and the daily seeding job feeding both the planner
baseline and the Controller tab's forecast timeline.

## WP5.3 — BL-17: weather/irradiation done; CO₂ remains (L)

**Weather/irradiation half: done.** Shipped as an MQTT-pushed feed rather than
the originally-sketched `ExternalDataSource` poll loop — the production
supplier (SRF Meteo, via the sibling `data_acquisition` project) pushes an
hourly forecast over MQTT, VEN consumes it behind `WeatherForecastPort`, and
`entities::solar` transposes GHI onto the PV array's plane (clear-sky-index
method) to produce an `AssetForecast` tagged `ForecastSource::WeatherModel` —
precedence `WeatherModel` > `Heuristic` > `LastKnown`, offline-friendly (a
stale or absent feed falls back to the sin model, never blocks planning).
Full architecture, wire contract, and known accuracy gaps:
`docs/architecture/weather_forecast.md`; remaining minor debts tracked as
R-52 through R-56 in `docs/reference/TECHNICAL_DEBTS.md`.

**CO₂-intensity half: still open**, tracked as BL-17 in `docs/BACKLOG.md`
(narrowed to CO₂ only). Electricity Maps has no free tier — no provider
chosen yet; keep using event-delivered GHG values until one is.

## WP5.4 — Baselines + capability forecast + quality metadata (L)

1. **BASELINE (§7.5):** baseline = heuristic forecast (WP5.2) computed *as if no
   event were active* — the counterfactual. During/after an event window, submit
   `BASELINE` payload alongside `USAGE`; `kpi.py` gains
   `event_impact_kwh = Σ(baseline − actual)` per event. This upgrade turns SG-3
   from directional to M&V-grade.
2. **UC:§8.7 capability forecast:** parse `reportDescriptor.historical` (currently
   ignored — the VEN can't distinguish forecast requests from historical ones); for
   forecast requests, report `LOAD_SHED_DELTA_AVAILABLE` /
   `GENERATION_DELTA_AVAILABLE` from the `FlexibilityEnvelope` (import/export heads
   already computed since Phase 3 WP3.6).
3. **Historical report replay:** with `historical=true` and a past time range, build
   the report from Phase-1 history instead of live state (this is what the history
   store makes possible; cert row §6 "historical reports" → Full).
4. **UC:quality metadata:** attach accuracy/confidence to report payloads — for
   forecasts use the heuristic's confidence (sample count / variance); for
   measurements a static high confidence. Small, ride along with 1–3.
5. BDD: one scenario per report kind (baseline during event, capability forecast on
   request, historical replay); assert payloads on the recorder side (Phase 1 WP1.7).

## Remaining risk (WP5.4 only)

Simulated households may be *too* regular, making the heuristic-baseline
counterfactual look better than it would in reality — note this in the
experiment write-up, and consider adding stochastic base-load noise to the
simulator (small follow-up item, record in BACKLOG if adopted).

Bookkeeping (on WP5.4 completion): cert rows §6 (forecast/historical/quality)
updated; journal + `/wiki-sync` ([[milp-planner]], new forecasting/baseline
concept page).
