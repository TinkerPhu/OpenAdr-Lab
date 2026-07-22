# Documentation Content Rule

Every document in this repository, except the exempt list below, describes
only (a) the **current state** of code and features and (b) **future
visions/plans**. No historical narrative — no "it used to be X", "was
changed on \<date\> to Y". Just "it is Y." Permitted exception: a short
mention of a rejected alternative X and *why* it was not chosen, **only
when the choice is not obvious**.

Exempt (intentionally historical):

- `docs/history/**` (project journal)
- `docs/reference/KEY_LEARNINGS.md`
- `wiki/log.md` (a log by nature)
- `wiki/decisions/**` (ADR-style pages — rationale allowed, chronology still
  gets rewritten)
- `wiki/queries/**` and `wiki/review.md` (dated point-in-time records)
- `specs/archive/**` (archived feature records)
- `docs/history/archive/**` (superseded design docs kept as record)
- git history itself

`docs/openadr_3_1_specs/pdf/` is never read (project rule); the markdown spec
copies in `docs/openadr_3_1_specs/` are third-party text — never rewritten.

## Practical corollary

A plan document (`docs/plans/**`) whose work has been fully implemented is
not "current state" or "future vision" — it is done. Once a plan is fully
implemented:

- Remove the plan document.
- Fold anything still worth keeping (architecture rationale, wire contracts,
  design decisions a future maintainer would need) into a permanent
  current-state document under `docs/architecture/` or `docs/reference/`.
- Anything left undone gets recorded as an open item in the standing
  registers (`docs/reference/TECHNICAL_DEBTS.md`, `docs/BACKLOG.md`,
  `docs/plans/refactoring_backlog.md`) rather than left inside a stale plan.
- The historical record of what was built, why, and what was learned lives
  in `docs/history/project_journal.md` and `docs/reference/KEY_LEARNINGS.md`
  (both exempt from this rule), plus git history — not in the plan itself.
