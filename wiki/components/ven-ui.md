---
title: VEN UI
type: component
created: 2026-07-04
updated: 2026-08-21
synced_commit: 6a5a678
sources: [VEN/ui/src, docs/history/project_journal.md, docs/architecture/chart_diagrams.md, VEN/src/routes/timeline.rs, VEN/src/controller/timeline.rs, VEN/ui/src/pages/History.tsx, VEN/ui/src/pages/Planner.tsx, VEN/ui/src/components/sessions/SessionProgressBoard.tsx, VEN/ui/src/pages/Weather.tsx, VEN/ui/src/components/devices/ArbiterSettingsCard.tsx]
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
- Pages: Dashboard, History (placed directly after Dashboard in the nav),
  Controller, Programs, Events, Sensors, Weather, Measurements (Diagnostics nav group);
  plus the planner timeline views exercised by `tests/features/ven_ui_planner.feature`
  and `ven_timeline.feature`.
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
- Phase 4 additions: `NotificationsBell` in the app bar (badge + feed panel, 10 s
  polling — the UI face of [[notifications]]); a `ComfortCurveCard` on the Devices
  page (per-asset fill%/bid table plus a `CurveChart` visualization of the
  curve, POST installs an override, Reset restores the built-in default —
  WP4.2/BL-19); the request dialogs (created via the unified `POST /user-requests`
  flow, see [[hems-planning]]) gained a request-mode select (`ModeSelect`, native
  `<select>` for testability) and a budget field shown only for `MAX_COST`
  (WP4.1/BL-28). BL-41 (2026-08-05) removed the per-device `/ev-session`
  POST/DELETE, `/heater-target`, `/shiftable-loads` client methods/hooks once the
  UI's migration to `/user-requests` was confirmed complete — the dialogs
  themselves didn't change, only the now-dead API surface underneath them.
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

## Chart kit

Every chart in the app — Controller cells, History, the Devices comfort-curve preview, Raw
Diagnostics — is built from one shared primitive kit plus three named compositions under
`VEN/ui/src/components/charts/` (`TimeSeriesChart`, `StackedTimeSeriesChart`, `CurveChart`;
the older `StackedAreaChart`/`ComfortCurveChart` names and locations are gone, renamed and
moved here). Full architecture (why one kit, the cursor-correctness invariant that motivated
it, per-composition behavior): `docs/architecture/chart_diagrams.md`. Two facts worth pulling
up here rather than just linking:

- **Data-presence filtering is generic, not per-caller.** `TimeSeriesChart` itself hides any
  series with no non-null value anywhere in its data, for every current and future series a
  caller declares — no chart writes its own `hasCostData`/`hasCo2Data`-style boolean (the
  `generic-over-bespoke` rule in `.claude/CLAUDE.md`).
- **Interactive legends** (`useLegendToggle`/`ChartLegend`, opt-in per composition instance
  via `interactiveLegend`) let a user hide/show individual series by clicking a checkbox
  legend entry; enabled on `AssetTimelineChart`, `TariffChart`, and
  `GridAccumulatedCell`'s stacked chart, not on the Raw Diagnostics or `PlanPowerStack`
  instances (out of that capability's shipped scope).
- **`StackedTimeSeriesChart` doesn't get `TimeSeriesChart`'s automatic data-presence
  filtering** (its `StackedAreaPoint` fields are always plain `number`, never `null`, so
  there's no absence signal to filter on) — so its two callers each narrow the asset roster
  they pass in themselves, via a shared `assetIdsWithTimelineData()` helper
  (`GridAccumulatedCell.tsx`), before it reaches the chart. Without this, an asset with no
  timeline data yet still got a legend entry with nothing plotted next to it — a legend/graph
  drift bug found and fixed on the Controller "Accumulated Power" cell.

## Planner tab

`VEN/ui/src/pages/Planner.tsx` is the [[milp-planner]]'s decision-process view (purpose
analysis: [[planner-tab-purpose]]). Composition, top to bottom: objective selector (the
one real control — min_cost/GHG/grid/autarky/revenue) + collapsible weight legend; SSE
`PlannerStatusBar` (live solve progress via `usePlannerEvents`); `PlanHeaderBar` (plan
metadata + warnings badge); `PlanPowerStack`; `PlanTriggerTimeline` (why replans fired);
`PlanDecisionMatrix` (per-slot decisions, hatched estimated-rate slots, the
[[milp-planner]] marginal-cost dual as a "Marginal €" column, and — only when
`Plan.penalty_rules_active` is non-empty — a "Peak demand" row, green/red per slot
against the tightest active `threshold_kw`, WP6.3/BL-09); `SessionProgressBoard`;
collapsed `TraceTable` accordion; a `CorrectionBanner` snackbar labeled "Plan F: Layer 1
reactive correction" that is permanently dead — it listens for SSE `correction_active`/
`correction_cleared` events that no backend code emits (predates and was never rewired to
[[deviation-arbiter]]; see that page's DRIFT callout).

**SessionProgressBoard** (`components/sessions/SessionProgressBoard.tsx`, 9ba32e7)
replaced the Phase-D-orphaned `PacketProgressBoard`, which had polled the deleted
`GET /packets` endpoint and permanently rendered empty. It renders one card per
[[hems-planning]] user request grouped Active/Done: EV fill gauge from the live sim
`soc` vs `target_soc`, heater current→target temperature (deliberately no % gauge),
deadline countdown/OVERDUE, an on-track/at-risk chip comparing the plan's
`planned_kw_by_asset` energy up to the session deadline against the plan envelope's
`energy_needed_kwh` (the first UI consumer of `Plan.envelopes`), and a budget bar from
`estimated_cost_eur` labeled "est." (per-session accumulated cost doesn't exist —
BL-39). A `variant="condensed"` chip strip plus a read-only objective chip sits on the
Dashboard (`dash-session-strip`, BL-36); the objective control stays on the Planner tab.

**`PlanPowerStack`** shares its `StackedAreaPoint`-building logic
(`buildStackedFromAllTimelines`) with the Controller's `GridAccumulatedCell` rather than
building its own from `usePlan()`'s raw `Plan` object — the two independent
implementations that existed before this consolidation is exactly what let one of them
silently drop `net_export_kw` and show the grid line near zero under an autarky objective
even while PV was heavily exporting (fixed: both now read the timeline's already-signed
`net_import_kw − net_export_kw`, `controller/timeline.rs`). `PlanPowerStack` also no longer
refetches the plan timeline on every render (a `useEffect` dependency bug), and PV stacks
first — closest to the X axis — in every `StackedTimeSeriesChart` instance, matching how a
reader visually reads "generation at the bottom, consumption piling up above it."

Controller page: the PV asset's `AssetTimelineChart` shades hardware-capped, planned-curtailment,
manual-override, and unplanned regions distinctly, reflecting `PvState.curtailment_source` from
[[asset-layer]]'s PV curtailment model (renamed `generation_limit_kw`, fourth `manual` source
added alongside the pre-existing `none`/`plan`/`capacity`).

**Nullable slider convention** (`components/controller/DynamicControl.tsx`): a plain slider
falling back to its `min` whenever no override is active made "no override" visually
indistinguishable from "curtailed to the minimum" — for `pv_generation_limit_kw` specifically,
whose `max` (`inverter_max_kw`) is physically identical to "no limit" since the inverter can
never exceed it anyway, that reads as PV being capped to zero when it's actually exporting
normally. A new `nullable` flag on `ControlDescriptor` (schema-driven, set only on this one
control) pins the slider to `max` and shows "Off" when the value is `null`; dragging into the
top 5% of the range and releasing sends `null` instead of the numeric max — a snap-to-off zone,
no separate toggle control needed. `AssetRightSection`/`AssetTimelineChart` tests extended to
cover the null/Off rendering path.

## Timeline specifics

The timeline renders the plan produced by the [[milp-planner]] including its variable-step
zones ([[three-tier-plan-grid]]): zone shading uses per-zone opacity (fixed at 693b9b4 so
Zone A is not invisible), and a **now-point** marker shows the live simulator value at the
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
resolution, since it has no natural "slot" structure to preserve. The frontend needed no
change: recharts' existing tooltip snap already reads real `ts` values from the data
array, so it now snaps to real plan-slot boundaries instead of fake grid buckets.

Testing: Vitest + React Testing Library component tests, `data-testid`/`aria` attributes
per `docs/guidelines/REACT_GUIDELINES.md`; part of suite 1 in [[testing-strategy]].

## History page

Phase 1 added a `History` page (`VEN/ui/src/pages/History.tsx`) that queries the
`GET /history/*` routes and reuses the existing `AssetTimelineChart`/`TariffChart`
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
