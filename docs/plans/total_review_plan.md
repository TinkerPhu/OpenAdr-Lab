# Total Project Review Plan

Status: **DONE (2026-07-16).** All parts complete: review (A: code, B: docs),
consolidation (C1), doc fixes (C2), code fixes (C3), close-out (C4). Merged to
main at 1e7e807.
Scope was the whole repository — code (VEN, VTN, UIs, scripts, CI) and
documentation (docs/**, root-level docs, wiki/**).

Everything executed, including the full findings log, step notes, owner
decisions, and the B12 reduction proposal, is recorded in git history of this
file and in the "Total Project Review" entry of
`docs/history/project_journal.md`. Learnings: `docs/reference/KEY_LEARNINGS.md`
§Total Project Review.

## Where the remaining work lives

All findings that were not fixed during the review are parked in the standing
registers — nothing is tracked in this file:

- `docs/reference/TECHNICAL_DEBTS.md` — R-23 through R-38 (architecture
  placements, injectable-clock gaps, unwrap triage, lint hygiene, BFF error
  flattening/duplication, dead behave steps, tooling, repo hygiene).
- `docs/plans/refactoring_backlog.md` — R-03, R-08.
- `docs/BACKLOG.md` — BL-34, BL-35, GB-11 (and the refreshed
  Dependency Vulnerabilities section).

## Documentation content rule (standing policy from this review)

Every document except the exempt list below describes only (a) the **current
state** of code and features and (b) **future visions/plans**. No historical
narrative — no "it used to be X", "was changed on \<date\> to Y". Just "it is
Y." Permitted exception: a short mention of the rejected alternative X and
*why* it was not chosen, **only when the choice is not obvious**.

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
