# VEN UI Transparency Plan

Status: **DONE (2026-07-18).** All 8 work packages shipped and merged to
`main`: WP-T1 (VTN connection + multi-component `/health`), WP-T2 (MILP solve
status badge), WP-T3 (background task status), WP-T4 (Event Log), WP-T5 (VTN
report submission status), WP-T6 (wire unused routes), WP-T7 (Metrics page
labeling), WP-T8 (nav re-architecture + Dashboard redesign).

Each WP's own OpenSpec change under `openspec/changes/wp-t{1..8}-*/`
(proposal/design/specs/tasks) is the durable record of what was decided and
why. The full narrative — what was built, issues hit, and key learnings per
WP — is in `docs/history/project_journal.md` (search for "WP-T").

## Where the remaining work lives

Everything not fixed in-scope during these WPs is parked in the standing
registers — nothing open is tracked in this file:

- `docs/reference/TECHNICAL_DEBTS.md` — R-43 (dormant `/history/reports`
  route) and R-44 through R-49 (deferred lower-priority findings from the
  combined-branch code review before WP-T8).
- `docs/BACKLOG.md` — GB-12 (infeasible-solve BDD scenario), GB-13 (Event Log
  SSE wiring, still polling-based).
