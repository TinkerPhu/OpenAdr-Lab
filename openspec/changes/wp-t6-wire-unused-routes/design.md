## Context

Confirmed by reading `VEN/src/routes/mod.rs`'s full route table and each
handler's implementation (not just the plan doc's list, which predates several
routes added since — `/vtn/status`, `/tasks/status`, `/events/log*`) that these
four routes are genuinely unwired and return simple, already-correct shapes:
`/capability/:asset_id` → `{max_import_kw, max_export_kw, is_fixed}`,
`/forecast` → `Vec<AssetForecast>`, `/history/plans?from=&to=` →
`Vec<PlanSnapshot>`, `/obligations` → `Vec<OadrReportObligation>`.

## Goals / Non-Goals

**Goals:** surface these four routes' data on the most natural existing page for
each, per the plan doc's "favor an existing page over a new tab" principle.

**Non-Goals:** no new backend routes or contract changes. No `/forecast/:asset_id`
or `/history/:asset_id` wiring (see proposal.md — overlaps with existing
Timeline functionality, needs new chart UI, out of this WP's contained scope).
No SSE consumption in the UI (consistent with WP-T4).

## Decisions

**D1 — A standalone `FlexibilityForecastPanel`, not new `AssetCell` props.**
`Controller.tsx`'s `AssetCell` is already a large, tightly-composed component
(timeline charts, pin/collapse state, override controls). Adding
capability/forecast data as new props there would mean touching its internals
and tests for a WP whose only goal is *surfacing existing data*, not redesigning
the cell. A separate panel, fetching independently via `useAssetCapabilities`
(parallel `useQueries`, since there's no bulk `/capability` endpoint) and
`useAssetForecasts`, keeps the change additive and isolated — `AssetCell` and its
existing tests are untouched.

**D2 — `useAssetCapabilities` uses `@tanstack/react-query`'s `useQueries`, not a
new bulk endpoint.** No server-side bulk-capability endpoint exists, and adding
one would be a backend change this WP explicitly scopes out. `useQueries` runs
one lightweight query per asset in parallel — for the handful of assets a single
VEN profile has, this is cheaper than adding new backend surface for a
UI-convenience concern.

**D3 — Plan snapshot detail via the existing `JsonDialog`, not a new viewer.**
`plan_json` is a raw JSON string; `Reports.tsx` already has exactly this pattern
(`JsonDialog` showing a selected report's full JSON). Reusing it for a selected
plan snapshot (after `JSON.parse`) needed no new component.

**D4 — Obligation "Overdue" status is computed client-side from `due_at` vs.
now, not read from a server field.** The backend's `OadrReportObligation::is_due`
helper isn't exposed as a computed API field — the raw `due_at`/`fulfilled` pair
is all the wire shape has. Recomputing "is this overdue" client-side from those
two fields is simple and avoids waiting on a backend change for a presentation
concern.

## Risks / Trade-offs

- **[Risk] `useAssetCapabilities`'s parallel per-asset queries add N requests
  every 10s (N = asset count).** → Mitigation: acceptable at the scale this VEN
  operates at (a handful of assets per profile); revisit if a fleet-wide view
  ever needs this data across many VENs simultaneously (out of scope here).

## Migration Plan

UI-only, additive; no migration.

## Open Questions

None.
