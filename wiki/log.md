# Wiki Operation Log (append-only)

Format: `- YYYY-MM-DD — <operation> — <commit or range> — <pages touched / notes>`

- 2026-07-04 — scaffold — 6cb8ca6 — Created CLAUDE.md, purpose.md (draft), index.md, log.md, review.md, directory skeleton, skills (/wiki-sync, /wiki-ingest, /wiki-query, /wiki-lint), scripts/wiki_lint.sh. Replaces llm_wiki_instructions.md. No content pages yet.
- 2026-07-04 — review — 6cb8ca6 — Extended wiki_lint.sh (under-linked check, created/updated/type validation, path-form link tolerance); wired wiki into docs/reference/SESSION_START.md (load-context + Definition of Done). All checks re-tested.
- 2026-07-04 — seed sync — 6cb8ca6 — Seeded 23 content pages: overview (2), architecture (4), components (6), concepts (7 incl. wiki-maintenance), use-cases (1), decisions (3). Index rebuilt. 3 review items filed (stale CLAUDE.md plan reference, greedy-planner glossary drift, spec-implied use-case analysis pending).
- 2026-07-04 — review fixes — 6cb8ca6..4695762 — All 3 open review items resolved: (1) .claude/CLAUDE.md architecture reference fixed, (2) REQUIREMENTS.md Planner glossary updated to MILP, (3) new page openadr-spec-use-cases (spec-implied use cases, gap-checked). New DRIFT found and filed: Energy Packet glossary vs device-session code. Pages updated: ven-hexagonal-architecture, milp-over-greedy, hems-planning, system-use-cases, vision-and-roadmap, index. All synced_commit bumped to 4695762 (sources fully committed as of that sha).
