# Review Queue

Human-in-the-loop items found during sync/ingest/lint: contradictions, uncertain claims,
coverage gaps. Claude appends items instead of guessing; the human resolves or delegates.
Open items only — resolved items are deleted (resolution records live in `log.md` and git
history), per `wiki/CLAUDE.md`.

Format: `- [ ] YYYY-MM-DD — <description> (found during <workflow>; pages: page-slug, other-slug)`

- [ ] 2026-07-31 — Stale-page remainder, batch 2 of the triage started 2026-07-31: 22 pages still carry a `synced_commit` older than changes to their sources (down from 35 after batch 1 — see the 2026-07-31 log entry for which 13 were cleared and how). These are the larger/heavier-diffed ones: `ven-hexagonal-architecture` (~140-file source list, largest), the `openadr_3_1_specs/`-citing pages (`openadr-3`, `openadr-security`, `openadr-programs`, `dto-pass-through`, `openadr-programs-explained`), `openadr-lab`/`vision-and-roadmap` (BACKLOG.md + journal churn), `ven-code-vs-docs-audit` (near-whole-codebase snapshot, likely needs a fresh full read rather than incremental diffing), `distributor-business-case-tiers` (large tests/features/ source list), and the MILP/asset-adjacent pages (`deviation-arbiter`, `dispatcher`, `heuristics-pipeline`, `history-store`, `openadr-interface`, `tariffs-and-capacity`, `three-tier-plan-grid`, `milp-over-greedy`, `device-session-common-interface`, `history-store-persistence-format`, `openadr-spec-use-cases`, `system-use-cases`). Same triage method as batch 1: diff `synced_commit..HEAD` against each page's sources, fix real drift, bump `synced_commit` where the diff is cosmetic/unrelated. (found during /wiki-sync; pages: see lint output)
