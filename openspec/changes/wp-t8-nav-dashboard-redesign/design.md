## Context

This is the last WP of `docs/plans/ven-ui-transparency.md` — deliberately
sequenced last because it consumes the surfaces WP-T1/T2/T3/T4 shipped
(`/vtn/status`, `Plan.solve_status`, `/tasks/status`, plus the Tasks/Event Log
pages themselves) rather than adding a new one. No new backend endpoint, no
new port — this is presentation-layer only.

## Goals / Non-Goals

**Goals:** reduce the top-level nav decision from 11 flat tabs to 6 primary +
2 grouped menus (§3.2); give the Dashboard a real "is everything okay right
now" answer using the three status signals that already exist but have no
Dashboard home yet.

**Non-Goals:** no new component-level detail beyond what `/health`,
`/vtn/status`, `Plan`, and `/tasks/status` already expose. No SSE wiring for
Event Log (that's noted as a separate follow-up in WP-T4's design.md,
untouched here). No change to the existing Health chip in the AppBar or the
`NotificationsBell` — both already correct (WP-T1's fix, and pre-existing).

## Decisions

**D1 — Grouped menus via MUI `Menu`, not nested routes.** The "VTN Feed" and
"Diagnostics" groups are just a `Button` + `Menu` anchored dropdown; each
`MenuItem` is a `Link` to the existing route. No route paths change, so
deep-linking (`/reports`, `/tasks`, etc.) and every existing page-level test
keep working unmodified — only `App.tsx`'s nav markup and `App.test.tsx`'s nav
assertions change.

**D2 — Diagnostics group is always rendered, never conditionally hidden.**
Per design principle 2 (§2 of the plan doc) and the mockup in §3.2 — this is a
transparency-focused lab tool, not a resident-facing product where technical
detail should hide by default.

**D3 — Status rows collapse to one line when healthy, expand only when
degraded.** Mirrors the pattern `PlanHeaderBar.tsx` already established for
plan warnings (`Collapse` + expand `IconButton`) — reused directly rather than
inventing a second expand/collapse idiom on the same page.

**D4 — Task summary "healthy" definition matches `Tasks.tsx`'s existing rule:**
`restart_count === 0`, not `last_success` (a task's first still-running
attempt legitimately has `last_success === null` and is not degraded — see
`Tasks.tsx`'s own comment). Reusing the exact same rule avoids the Dashboard
and the Tasks page disagreeing about what "degraded" means for the same data.

**D5 — Plan status row uses the same missing-plan-is-not-degraded rule as
`/health`'s `planner` component** (`routes/system.rs::plan_is_ok`): no plan
yet is a neutral "waiting" state, not a red/degraded one — only
`solve_status === "INFEASIBLE"` is degraded. Keeps the Dashboard's own
top-level `/health`-summary color and this row's color from disagreeing about
the same condition.

**D6 — New `useVtnStatus` hook, not embedding `client.vtnStatus()` calls in
components directly.** `client.ts::vtnStatus()` already exists (added by
WP-T1 for a page that in the end used `/health` instead) but nothing calls it
yet — every other endpoint in this codebase is wrapped in a `useX` hook in
`hooks.ts`, so this fills that one gap rather than being the one call site
that bypasses the hook layer.

## Risks / Trade-offs

- Moving Reports/Programs/Events/Metrics/RawDiagnostics/Tasks/Event Log behind
  a dropdown costs one extra click versus a flat bar. Accepted per the plan
  doc's usage-frequency reasoning (§3.1) — those are exactly the tabs opened
  "once a week," not the ones checked constantly.
