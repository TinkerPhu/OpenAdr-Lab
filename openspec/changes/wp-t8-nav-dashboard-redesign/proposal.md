## Why

The current UI nav is a flat, alphabetical-ish list of 11 top-level tabs
(Dashboard, History, Controller, Planner, Devices, Programs, Events, Reports,
Metrics, RawDiagnostics, Notifications) plus the two newer Diagnostics pages
from WP-T3/WP-T4 (Tasks, Event Log) bolted on the end. It treats a tab checked
constantly (Dashboard) the same as one opened once a week (Programs), and the
Dashboard itself still shows only the old health chip — the three new status
signals shipped by WP-T1 (VTN connection), WP-T2 (plan solve status), and
WP-T3 (task supervision) are all available over the API but have no home on
the page a resident actually lands on. WP-T8 of
`docs/plans/ven-ui-transparency.md` §3.

## What Changes

- Regroup the top nav by usage frequency (§3.2 of the plan doc): a primary bar
  (Dashboard, Devices, Controller, History, Planner, Notifications) plus two
  grouped dropdown menus — "VTN Feed" (Reports, Programs, Events) and
  "Diagnostics" (Metrics, Raw Data, Tasks, Event Log) — the latter always
  visible, never gated behind a mode flag.
- Rebuild the Dashboard's top section into three traffic-light status rows —
  VTN Connection (WP-T1), Plan status (WP-T2), Active tasks (WP-T3) — each a
  single green line when healthy, expanding inline with detail only when
  degraded.
- No backend change: this WP only consumes existing endpoints
  (`/vtn/status`, `/plan`, `/tasks/status`) that WP-T1/T2/T3 already shipped.

## Capabilities

### New Capabilities
- `nav-dashboard-redesign`: grouped primary/VTN-Feed/Diagnostics navigation and
  a three-row traffic-light Dashboard status summary.

## Impact

- Affected specs: `nav-dashboard-redesign` (new)
- Affected code: `VEN/ui/src/App.tsx`, `VEN/ui/src/pages/Dashboard.tsx`, new
  `VEN/ui/src/components/dashboard/StatusRows.tsx`, `VEN/ui/src/api/hooks.ts`
  (new `useVtnStatus` hook — `client.ts` already has `vtnStatus()`, unused
  until now), plus their tests.
