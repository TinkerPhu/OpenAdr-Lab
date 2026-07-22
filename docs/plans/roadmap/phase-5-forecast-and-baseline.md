# Phase 5 — Forecast & Baseline

> **Goal:** the VEN learns from its own past (SG-4): heuristic profiles for
> uncontrollable loads, external weather feeds for PV/thermal, and baseline reports
> that make the VTN-side report-usefulness evaluation (SG-3) rigorous (M&V-grade).
> **Items:** BL-08 (SITE_RESIDUAL), BL-14 (AssetHeuristics), BL-17
> (ExternalDataSource), UC:baseline §7.5, UC:§8.7 (capability forecast),
> UC:quality metadata.
> **Prerequisites:** Phase 1 history store with **≥ 4 weeks of accumulated data**
> (calendar constraint — verify before starting BL-14); Phase 3's `AssetForecast`
> (BL-15) as the delivery shape; Phase 4's `HEURISTIC_FORECAST` stub to replace.
> **Exit demonstration:** on a held-out week, the heuristic forecast for
> SITE_RESIDUAL beats last-known extrapolation (lower MAE); one experiment re-run
> where BASELINE reports let `kpi.py` quantify a single event's impact in kWh.
> **Total effort:** ~4–5 weeks.

## WP5.1 — BL-08: SITE_RESIDUAL virtual asset (M)

The load the heuristics will forecast — build the consumer before the forecaster.

1. Monitor 1 s tick: `residual_kw = grid_meter_kw − Σ modelled_asset_kw`; expose as a
   read-only virtual asset (`AssetType::SiteResidual` exists, never instantiated).
2. Include in the planner baseline so background load is budgeted; include in
   Phase-1 `tick_samples` (asset_id `site-residual`) so history accumulates for it —
   **land this early; its history is what BL-14 trains on.**
3. Unit test per BL-08 verify: sim with known base_load + PV, meter shows extra
   500 W → residual reads 0.5 kW; planner baseline includes it.
4. UI: residual appears in the controller chart stack (it explains "unexplained"
   import that users currently can't see — a comfort/trust win too).

## WP5.2 — BL-14: AssetHeuristics learned from history (L)

1. Aggregation job (background task, daily + on-demand route for tests): for each
   heuristic-eligible asset (site-residual, base load, PV-without-weather), compute
   from `tick_samples`:
   - `daytime_profile_kw[24]` — mean power by hour-of-day,
   - `weekday_weights[7]` — day-type scaling,
   - `seasonal_factor` — rolling 30-day level vs. long-run level.
   Rolling window (e.g. 6 weeks), exponentially weighted so recent behaviour
   dominates. All from `HistoryPort` queries — no direct DB access from the job.
2. Produce `AssetForecast` entries tagged `ForecastSource::Heuristic` (shape from
   Phase 3 WP3.6); planner consumes them for baseline slots; Phase 4's
   `StaleRatePolicy::HEURISTIC_FORECAST` stub now becomes real.
3. Test-first with synthetic history (the BL-14 verify condition): inject a
   multi-week synthetic pattern (morning peak weekdays, flat weekends) via
   `MockHistoryPort` → learned profile converges to the injected pattern within
   tolerance. Plus: cold-start (< 1 week data) → job declines to produce a forecast
   (confidence gate) and the LAST_KNOWN fallback stays active.
4. **Validation harness** (`experiments/forecast_eval.py`): train on weeks 1–5,
   predict week 6, compare MAE vs. last-known extrapolation — this produces the
   phase-exit evidence. Run against *real* accumulated fleet history, not synthetic.

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

## Order & risks

```
WP5.1 (early — starts residual history accumulating)
  → WP5.2 (needs ≥4 weeks of WP5.1 data; build+test on synthetic meanwhile)
WP5.3 (independent, parallel)
WP5.4 after WP5.2 (baseline = heuristic counterfactual)
```

Risks: (a) the calendar dependency — if Phase 1 shipped late, WP5.2's real-data
validation slips; mitigate by landing WP5.1 + the synthetic-data pipeline first;
(b) simulated households may be *too* regular, making heuristics look better than
they'd be in reality — note this in the experiment write-up, and consider adding
stochastic base-load noise to the simulator (small follow-up item, record in
BACKLOG); (c) MQTT weather-feed coupling — the offline-degradation rule (stale
or absent feed falls back to the sin model) is non-negotiable and has
resilience-suite coverage.

Bookkeeping: mark BL-08/14 resolved and BL-17 narrowed to CO₂-only, cert rows §6 (forecast/historical/quality)
updated; `StaleRatePolicy::HEURISTIC_FORECAST` stub note removed; journal +
`/wiki-sync` ([[milp-planner]], [[tariffs-and-capacity]], new forecasting concept
page); add BACKLOG items for CO₂ feed and simulator noise if adopted.
