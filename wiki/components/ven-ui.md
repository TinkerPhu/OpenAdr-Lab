---
title: VEN UI
type: component
created: 2026-07-04
updated: 2026-10-21
synced_commit: a3a306b
sources: [VEN/ui/src, lab-ui/src, docs/history/project_journal.md, docs/architecture/chart_diagrams.md, VEN/src/routes/timeline.rs, VEN/src/controller/timeline.rs, VEN/ui/src/pages/History.tsx, VEN/ui/src/pages/Planner.tsx, VEN/ui/src/components/sessions/SessionProgressBoard.tsx, VEN/ui/src/pages/Weather.tsx, VEN/ui/src/components/devices/ArbiterSettingsCard.tsx]
tags: [ui, react, timeline]
---

# VEN UI

React + TypeScript SPA (Vite build, nginx-served, port 8214) — the per-site dashboard for
[[openadr-lab]]'s VEN containers (docs/history/project_journal.md §6).

## Structure

- `src/api/client.ts` — `VenApi` fetch wrapper; `src/api/hooks.ts` — react-query hooks
  with `refetchInterval` polling; `src/api/types.ts` — pass-through DTO types
  ([[dto-pass-through]]).
- `VenContext` — multi-VEN selector switching all pages across the three instances.
- Pages (`App.tsx` routes): Dashboard, Planner, Controller, Devices, History, Plan History,
  Programs, Events, Reports, Notifications, and the Diagnostics group — Weather, Measurement,
  Capacity Forecast, Raw Diagnostics, Metrics, Tasks, Event Log. The planner timeline views are
  exercised by `tests/features/ven_ui_planner.feature` and `ven_timeline.feature`.
- **Fleet dropdown**: VENs beyond the default trio come from the VTN's VEN registry
  (`/api/vens-registry`); a VEN on another host carries a `DASHBOARD_URL` attribute the browser uses
  directly (VEN_ARCHITECTURE §4.11). Fleet-wide views are the [[fleet-monitoring]] pages of the VTN UI.
- **Dashboard/diagnostics surfaces**: the Dashboard names the degraded component, not just the overall
  status; **Tasks** shows supervised task health (healthy = `restart_count === 0`); **Event Log** is the
  VEN-operational trail (SSE stream), deliberately separate from the Notifications feed ([[notifications]]);
  **Capacity Forecast** shows the sustained-commitment power/duration/energy curves, distinct from the
  instantaneous **Site Headroom** chart (`GridHeadroomCell`), where one click anchors the dashed
  import/export commitment curves at a future time; **Plan History** shows persisted solve quality
  ([[history-store]]).
- **Weather page** (`pages/Weather.tsx`): raw MQTT feed (`WeatherRawPanel`) and the
  derived PV forecast (`WeatherDerivedPanel`) from [[weather-forecast]]'s `GET /weather`
  — the UI face of that plugin, per the `ui-transparency` rule.
- **Measurements page** (Diagnostics nav group, `pages/Measurement.tsx`): both real-
  measurement signals' (PV, baseline load) raw reading, freshness, and source-alive state
  from [[real-measurement-mqtt]]'s `GET /measurement` — same `ui-transparency` rationale.
- **ArbiterSettingsCard** (Devices page, `components/devices/ArbiterSettingsCard.tsx`):
  the toggle for [[deviation-arbiter]]'s `deviation_arbiter_enabled` runtime gate, via
  `GET/PUT /arbiter-settings`, plus a live readout (net site power, deviation, active lever)
  from `GET /arbiter-diagnostics` while enabled.
- Notifications, comfort curves, request modes: `NotificationsBell` in the app bar (badge + feed panel, 10 s
  polling — the UI face of [[notifications]]); a `ComfortCurveCard` on the Devices
  page (per-asset fill%/bid table plus a `CurveChart` visualization of the
  curve, POST installs an override, Reset restores the built-in default —
  WP4.2/BL-19); the request dialogs (created via the unified `POST /user-requests`
  flow, see [[hems-planning]]) carry a request-mode select (`ModeSelect`, native
  `<select>` for testability) and a budget field shown only for `MAX_COST`
  (WP4.1/BL-28). The dialogs use `/user-requests` only; there are
  no per-device session calls.
- WP4.6 observability polish: `GridSignalStrip` on the Controller page (chips for
  active alert / SIMPLE / dispatch / capacity, from the `GET /signals` aggregate;
  renders nothing when idle), hatched+dimmed estimated-rate slots in the plan
  matrix (WP4.4 `rate_estimated`), persona labels in the VEN selector (from the
  VEN `PERSONA` attribute via `/api/vens-registry`), a Mode column in the
  All-Requests table and mode chips on all device cards. Rate charts enforce a
  minimum axis span (`axisDomain.ts`) so near-flat cost/CO₂ lines don't render
  as noise. Build gate lessons: vitest/eslint don't typecheck — `npm run build`
  (tsc) is part of the local gates for UI-typed changes — and neither vitest
  (jsdom, unbundled) nor tsc sees *production-bundle* breakage: only the Node1
  browser E2E exercises the built bundle. The toolchain is vite ^7 /
  vitest ^4 / plugin-react ^5; vite 8's rolldown bundler mis-resolves a MUI
  default-import interop at bundle time (React #130 at runtime with all unit
  tests green), so the vite major is held at 7 until that interop is proven.

## Every asset under its own id

The UI never names a fixed asset id. `/sim` carries each asset's declared `asset_type` and its
`roster_index`, and `VEN/ui/src/api/assets.ts` is the one place asking "which assets of this type,
in roster order" (`assetsOfType`) and "in which order do power charts stack" (`stackOrder`). The
Devices page renders one EV and one heater card per asset, titled with its id; a card requests for
its own asset. The Controller builds one cell per asset from a per-type table, and the History,
comfort-curve and tariff paths cover every roster asset. A profile may hold two EVs or
two arrays (`VEN/profiles/multi_asset_test.yaml`), which [[asset-layer]] and [[deviation-arbiter]]
handle per asset as well.

An asset's kind arrives as the generated union `AssetKind` (`asset_type`), declared by the asset and
never read from its id; `components/controller/assetDisplay.ts` is the one place deciding colour,
label and planning role from it ("PV · pv-west" when a VEN has two arrays).

## EV card, sessions and comfort cost

The EV card shows the whole session queue with each window (not just the deadline), and a clashing
submission becomes a choice: the 409 body names the replaceable sessions and the dialog offers to
displace exactly those (`useSubmitRequest.ts`; [[hems-planning]] for the queue rule), a failure is shown
rather than swallowed. An EV with a usage schedule shows its mode (`simulated` or `forecast`) and next
trip; the trip estimate is one optional pair. The comfort-curve card shows what the curve can cost beside
what the plan spends, and `ComfortSpendRows` lets the curve be entered as the total to pay from the current
charge — an input and read-back of the same curve, never a second one ([[milp-planner]]). Static nameplate
specs live on the Devices tab (`AssetSpecsTable`); the Controller shows each asset's key features under its
name.

## Chart kit

Every chart in the app — Controller cells, History, the Devices comfort-curve preview, Raw
Diagnostics — is built from one primitive kit shared by **both** UIs (`lab-ui/`, imported as `@lab/ui`,
an npm workspace) plus three named compositions: `TimeSeriesChart` (in `lab-ui`) and the VEN-specific
`StackedTimeSeriesChart` and `CurveChart` under `VEN/ui/src/components/charts/`. The VTN UI's
Fleet and Tariff charts use the same kit. Full architecture (why one kit, the cursor-correctness invariant that motivated
it, per-composition behavior): `docs/architecture/chart_diagrams.md`. Two facts worth pulling
up here rather than just linking. (`lab-ui/` also holds the code both UIs share that is not a
chart: the Prometheus parser, the toolbar's `RefreshControls`, `debugLog`, the `JsonDialog`, the
"value or dash" formatters in `unitFormat.ts`, and the TypeScript/ESLint base configs each UI
extends; see the same document.)

- **Data-presence filtering is generic, not per-caller.** `TimeSeriesChart` itself hides any
  series with no non-null value anywhere in its data, for every current and future series a
  caller declares — no chart writes its own `hasCostData`/`hasCo2Data`-style boolean (the
  `generic-over-bespoke` rule in `.claude/CLAUDE.md`).
- **Interactive legends** (`useLegendToggle`/`ChartLegend`, opt-in per composition instance
  via `interactiveLegend`) let a user hide/show individual series by clicking a checkbox
  legend entry; enabled on `AssetTimelineChart`, `TariffEnvelopeChart`, `GridRatesChart`, and
  `GridAccumulatedCell`'s stacked chart, not on the Raw Diagnostics or `PlanPowerStack`
  instances (out of that capability's shipped scope).
- **`StackedTimeSeriesChart` doesn't get `TimeSeriesChart`'s automatic data-presence
  filtering** (its `StackedAreaPoint` fields are always plain `number`, never `null`, so
  there's no absence signal to filter on) — so its two callers each narrow the asset roster
  they pass in themselves, via a shared `assetIdsWithTimelineData()` helper
  (`GridAccumulatedCell.tsx`), before it reaches the chart. Without this, an asset with no
  timeline data yet still got a legend entry with nothing plotted next to it — a legend/graph
  drift bug found and fixed on the Controller "Accumulated Power" cell.

## Tariff and rates cells (direct vs derived)

The Controller shows grid price information as two cells, split so a VTN signal and a VEN computation
never share a diagram (`VEN/ui/src/components/controller/`):

- **Tariff & Envelope** (`GridTariffCell` -> `charts/TariffEnvelopeChart`): the *direct VTN signals*, shown
  as announced — import/export tariff (€/kWh, `/tariffs`) on the left axis, and on the right axis (kW) the
  Dynamic Operating Envelope from `/capacity/schedule`: import/export **limit** (dotted) and contracted
  **allowance** (dashed; subscription + reservation, either alone counts, as the planner reads it —
  `tariffBuilders.ts::buildCapacityLimitPoints`). Tariff and capacity are different quantities, so they
  never share an axis. Left column: current import/export price, CO₂eq, cost rate ([[tariffs-and-capacity]]).
- **Grid Rates** (`GridRatesCell` -> `charts/GridRatesChart`): the *derived signals* — cost rate (€/h) and
  CO₂ rate (g/h), which the VEN computes as tariff x grid power, not something the VTN sends. History and
  now-points carry only `power_kw`, so `fillCostRateFromTariffs` fills the rates from the tariff in force
  (last observation carried forward); import is positive, export negative (revenue/displaced emissions),
  and near-zero power is snapped to 0 so no needles appear. Future plan slots keep the backend's own rates.

Both pin and expand like any Controller cell and build on `TimeSeriesChart`, so series with no data stay
hidden (chart kit above).

## Planner tab

`VEN/ui/src/pages/Planner.tsx` is the [[milp-planner]]'s decision-process view (purpose
analysis: [[planner-tab-purpose]]). Composition, top to bottom: objective selector (the
one real control — min_cost/GHG/grid/autarky/revenue) + collapsible weight legend; SSE
`PlannerStatusBar` (live solve progress via `usePlannerEvents`); `PlanHeaderBar` (plan
metadata + warnings badge); `PlanPowerStack`; `PlanTriggerTimeline` (why replans fired);
`PlanDecisionMatrix` (one row per asset the plan scheduled, from `Plan.planned_asset_ids`, idle
ones included, with no asset id in its own code; per-slot decisions, hatched estimated-rate slots, the
[[milp-planner]] marginal-cost dual as a "Marginal €" column, and — only when
`Plan.penalty_rules_active` is non-empty — a "Peak demand" row, green/red per slot
against the tightest active `threshold_kw`, WP6.3/BL-09); `SessionProgressBoard`;
collapsed `TraceTable` accordion.

**SessionProgressBoard** (`components/sessions/SessionProgressBoard.tsx`, 9ba32e7)
renders one card per
[[hems-planning]] user request grouped Active/Done: EV fill gauge from the live sim
`soc` vs `target_soc`, heater current→target temperature (deliberately no % gauge),
deadline countdown/OVERDUE, an on-track/at-risk chip comparing the plan's
`planned_kw_by_asset` energy up to the session deadline against the plan envelope's
`energy_needed_kwh` (the first UI consumer of `Plan.session_flexibility`), and a budget bar from
`estimated_cost_eur` labeled "est." (per-session accumulated cost doesn't exist —
BL-39). A `variant="condensed"` chip strip plus a read-only objective chip sits on the
Dashboard (`dash-session-strip`, BL-36); the objective control stays on the Planner tab.

**`PlanPowerStack`** shares its `StackedAreaPoint`-building logic
(`buildStackedFromAllTimelines`) with the Controller's `GridAccumulatedCell` rather than
building its own from `usePlan()`'s raw `Plan` object; two independent implementations
could drift (one dropped `net_export_kw` and showed the grid line near zero under an autarky
objective while PV was heavily exporting), so both read the timeline's already-signed
`net_import_kw − net_export_kw` (`controller/timeline.rs`). PV stacks
first — closest to the X axis — in every `StackedTimeSeriesChart` instance, matching how a
reader visually reads "generation at the bottom, consumption piling up above it."

Controller page: the PV asset's `AssetTimelineChart` shades hardware-capped, planned-curtailment,
manual-override, and unplanned regions distinctly, reflecting `PvState.curtailment_source` from
[[asset-layer]]'s PV curtailment model (`generation_limit_kw`; sources `none`, `plan`, `capacity`, `arbiter`, `manual`, `comms_loss`).

**Per-asset chart declarations and state shading** (052-ev-plugged-band, 2026-10-04): what an
asset's `AssetTimelineChart` draws beyond its power line — its state line (SoC or tank
temperature) and its time-range shadings — is declared once per asset in
`components/controller/assetChartSpecs.ts` and looked up by both the Controller cells and the
History page; neither branches on the asset id. Shading is one shared primitive,
`lab-ui/src/StateShading.tsx` (classify each point, shade every contiguous run, weight
folded into the fill alpha), with two declarations: PV curtailment (overlay; every source but
the plan is "unplanned", comms-loss included) and **EV unplugged** (background, EV blue). The
EV band marks *absence*: no band means plugged in, like every always-present asset. It reads
the EV's own `plugged` value (1 = plugged) — measured on past points and the now-point, the
plan's predicted presence (the availability mask the solve ran under) on future points, drawn
lighter with a dashed outline as "Predicted away". A sibling primitive, `DayNightShading`, washes a chart from solar elevation (a function of time alone) and is passed as `backgroundAreas` so it paints under the curves. Details: `docs/architecture/chart_diagrams.md`.

**Nullable slider convention** (`components/controller/DynamicControl.tsx`): a plain slider
falling back to its `min` when no override is active would make "no override" look like
"curtailed to the minimum" — for `pv_generation_limit_kw`, whose `max` (`inverter_max_kw`) is
physically identical to "no limit", that would read as PV capped to zero while it exports
normally. A `nullable` flag on `ControlDescriptor` (schema-driven, set only on this one
control) pins the slider to `max` and shows "Off" when the value is `null`; dragging into the
top 5% of the range and releasing sends `null` instead of the numeric max — a snap-to-off zone,
no separate toggle control needed. `AssetRightSection`/`AssetTimelineChart` tests cover the
null/Off rendering path.

## Timeline specifics

The timeline renders the plan produced by the [[milp-planner]] including its variable-step
zones ([[three-tier-plan-grid]]): zone shading uses per-zone opacity (so
Zone A stays visible), and a **now-point** marker shows the live simulator value at the
exact request time — deliberately *not* snapped to the plan grid
(docs/architecture/ven_milp_planner.md §2.2, timeline now-point).

`GET /timeline/:asset_id` and `/timeline/all` (`VEN/src/routes/timeline.rs`,
`VEN/src/controller/timeline.rs`) serve the chart data. The **future/forecast segment**
returns one real point per real plan slot at its native per-zone step size (5/10/15 min,
`build_asset_timeline`) — deliberately not resampled onto a fixed-width grid:
fixed-bucket resampling with time-weighted averaging would blend real slot values into
synthetic buckets and desynchronise the displayed timestamp from any real planning
decision whenever the bucket width didn't line up with a zone's step size (routine in
the expanded 48 h view). The **history segment** is grid-resampled at a fixed
resolution, since it has no natural "slot" structure to preserve. recharts' tooltip snap
reads the real `ts` values from the data array, so it snaps to real plan-slot boundaries.

Testing: Vitest + React Testing Library component tests, `data-testid`/`aria` attributes
per `docs/guidelines/REACT_GUIDELINES.md`; part of suite 1 in [[testing-strategy]].

## History page

The `History` page (`VEN/ui/src/pages/History.tsx`) queries the
`GET /history/*` routes and reuses the existing `AssetTimelineChart`/`TariffEnvelopeChart`
components rather than introducing new chart code. It is a distinct concern from the
live/forecast timeline above: History shows the durably-persisted operational record
(ticks, grid samples, events, reports, forecast-accuracy samples), backed by the VEN-local
SQLite store described in [[history-store]], not the in-memory simulator ring buffers.

The page defaults to a rolling **last-24h** window on load (a "Last 24h" button returns to
it explicitly) rather than requiring a manual date-range pick every visit — the common case
("what just happened") needed zero interaction. Clicking either date control switches out of
rolling mode into that fixed date and force-refreshes the charts, since a manually-picked
date is a request for that exact window, not "keep following now." For the two assets
forecast-accuracy-tracking tracks (PV, base_load), `AssetTimelineChart`
overlays near-lead (fine dotted) and far-lead (coarse dashed) forecast lines from
`GET /history/forecast-accuracy` alongside the actual-power line — see [[history-store]]'s
"Forecast accuracy tracking" section for the backend mechanism and
`docs/architecture/chart_diagrams.md`'s "Forecast-accuracy overlay" note for how the overlay
folds into the same cursor-correctness-safe merged data array as every other series.
Since schema v12 the persisted rows also carry the EV's `plugged` fraction and the PV
`curtailment_source` as the live timeline's numeric code, so History draws the same EV
"Unplugged" band and the planned/unplanned PV curtailment shading the Controller does (not
hardware-capped: `inverter_max_kw` is not persisted). Rows persisted under an older schema show neither.
