# Review Queue

Human-in-the-loop items found during sync/ingest/lint: contradictions, uncertain claims,
coverage gaps. Claude appends items instead of guessing; the human resolves or delegates.
Open items only — resolved items are deleted (resolution records live in `log.md` and git
history), per `wiki/CLAUDE.md`.

Format: `- [ ] YYYY-MM-DD — <description> (found during <workflow>; pages: page-slug, other-slug)`

- [ ] 2026-07-31 — Stale-page remainder, batch 2 of the triage started 2026-07-31, narrowed
  during the 2026-08-09 sync (49389c1..4c4f149): that sync's own commit range touched and
  re-verified `ven-hexagonal-architecture`, `dispatcher`, `heuristics-pipeline`,
  `history-store`, `openadr-interface`, `device-session-common-interface`,
  `history-store-persistence-format`, and `ven-code-vs-docs-audit` already, so they're
  cleared from this list. Still outstanding: the `openadr_3_1_specs/`-citing pages
  (`openadr-3`, `openadr-security`, `openadr-programs`, `dto-pass-through`,
  `openadr-programs-explained`), `openadr-lab`/`vision-and-roadmap` (BACKLOG.md + journal
  churn), `distributor-business-case-tiers` (large tests/features/ source list),
  `deviation-arbiter`, `tariffs-and-capacity`, `three-tier-plan-grid`, `milp-over-greedy`,
  `openadr-spec-use-cases`, `system-use-cases` — none of these had sources touched by
  49389c1..4c4f149 specifically, so this sync didn't re-verify them either. Same triage
  method as batch 1: diff `synced_commit..HEAD` against each page's sources, fix real drift,
  bump `synced_commit` where the diff is cosmetic/unrelated. (found during /wiki-sync;
  pages: see lint output)
- [ ] 2026-10-21 — Coverage gap after the partial 4c4f149..f8f6775 sync: not re-verified against the 992-commit range — reliability-and-config, heuristics-pipeline (per-weekday buckets), real-measurement-mqtt, experiment-harness, fleet-tooling (N=20, usage simulation), distributor-business-case-tiers, system-use-cases, openadr-spec-use-cases, ven-code-vs-docs-audit. Also unmapped: CO2 comfort bid (BL-17), site-headroom/capacity envelope engine, comms-loss curtailment (R-59), BASELINE reports. (found during /wiki-sync; pages: see lint output)
