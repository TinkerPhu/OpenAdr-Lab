# Review Queue

Human-in-the-loop items found during sync/ingest/lint: contradictions, uncertain claims,
coverage gaps. Claude appends items instead of guessing; the human resolves or delegates.
Open items only — resolved items are deleted (resolution records live in `log.md` and git
history), per `wiki/CLAUDE.md`.

Format: `- [ ] YYYY-MM-DD — <description> (found during <workflow>; pages: page-slug, other-slug)`

- [ ] 2026-07-16 — Stale-page remainder from the scoped 2026-07-16 sync: ~30 pages carry `synced_commit` older than changes to their sources (mostly docs edited during the total-review C2 rewrites, spec-file source lists, and import-path-only code renames — see the 2026-07-16 log entry for the scoping rationale). Work through `bash scripts/wiki_lint.sh`'s STALE list: re-verify each page against its sources and bump `synced_commit`, or correct content where the C2 doc rewrites changed a cited claim. (found during /wiki-sync; pages: see lint output)
