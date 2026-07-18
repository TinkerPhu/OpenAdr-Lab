## Why

Several backend routes already work but the VEN UI never calls them —
`/capability/:asset_id`, `/forecast`, `/history/plans`, and `/obligations` are all
data the resident/operator would want to see, sitting unused behind a working
endpoint. WP-T6 of `docs/plans/ven-ui-transparency.md`.

## What Changes

- Wire `GET /capability/:asset_id` and `GET /forecast` into a new standalone
  "Flexibility & Forecast" panel on the Controller page (per-asset feasible power
  range + the plan cycle's predicted power/confidence/source).
- Wire `GET /history/plans` into the History page as a "Plans" section (plan
  snapshots for the selected day, with a detail dialog for the raw plan JSON).
- Wire `GET /obligations` into the Reports page as a "Pending Obligations"
  section (due/overdue/fulfilled status per obligation).

## Capabilities

### New Capabilities
- `wired-diagnostic-routes`: the Controller, History, and Reports pages surface
  per-asset flexibility/forecast, plan snapshots, and pending report
  obligations — data the backend already computed but no UI page displayed.

### Modified Capabilities
(none)

## Impact

- **VEN UI only**: `pages/Controller.tsx` (new panel), `pages/History.tsx` (new
  section), `pages/Reports.tsx` (new section), new
  `components/controller/FlexibilityForecastPanel.tsx`, `api/types.ts`/
  `api/client.ts`/`api/hooks.ts` additions, new/updated tests.
- **No backend change** — every route wired here already exists and already
  works; confirmed by reading each handler's implementation before wiring.
- **Non-goals / scope decisions**:
  - `/forecast/:asset_id` and `/history/:asset_id` (the query-param-driven
    forward/backward sample-series routes in `routes/assets.rs`) are **excluded**.
    Both need a timespan control and a chart, and materially overlap with what
    the existing Timeline/RawDiagnostics pages already show via `/timeline/*` —
    wiring them would be new chart-building work, not a contained "surface this
    existing data" change, and risks duplicating an existing feature rather than
    filling a real gap.
  - `/notifications/events` SSE is **excluded** from UI wiring, consistent with
    WP-T4's precedent (the backend SSE route exists and works; consuming it from
    the UI is a follow-up, not dropped).
  - `/sim/inject/reset`, `/sim/config/battery`, `/plan/trigger`,
    `/debug/heuristics/preload` are **excluded** — confirmed by their own doc
    comments to be dev/test-only debug endpoints, not resident-facing surface.
  - The bare `/forecast` (per-asset `AssetForecast` from the latest plan cycle)
    and `/forecast/:asset_id` (a different, physics-model forward sample series)
    are two distinct concepts sharing a path prefix — only the former is wired
    here; conflating them in one UI panel would misrepresent what each actually
    means.
