---
name: wiki-sync
description: Update the OpenADR Lab wiki (wiki/) to reflect repository changes since the last sync, using git diff against each page's synced_commit. Also performs the initial seed ingest when the wiki has no content pages yet. Use after merging work or whenever wiki pages may be stale.
---

Read `wiki/CLAUDE.md` (schema, conventions, editorial rules) and `wiki/purpose.md`
(scope, emphasis) first. All rules there apply to every step below.

## Bootstrap (wiki has no content pages yet)

1. Propose a seed page list (~15–25 pages) grounded in: `docs/architecture/*`,
   `docs/REQUIREMENTS.md`, `docs/use-cases/`, the `VEN/src/` module tree, `VTN/` layout,
   `tests/features/`, `openspec/specs/`, and `docs/history/project_journal.md`.
   Get user confirmation before mass-creating pages.
2. Write the pages per the schema; `synced_commit` = current HEAD short sha on every page.
3. Rebuild `wiki/index.md`, append a `log.md` entry, run `bash scripts/wiki_lint.sh`.

## Incremental sync

1. Baseline = the newest commit recorded in `wiki/log.md` sync entries (fallback: the newest
   `synced_commit` across page frontmatter).
2. `git diff --name-only <baseline>..HEAD -- . ':!wiki'` to list changed source files
   (run via the Bash tool — the `':!wiki'` exclude pathspec is bash-quoting syntax).
3. Map changed files to pages: grep `sources:` frontmatter across `wiki/`. A page whose
   source is a directory owns every file under it.
4. For each affected page: re-read its sources, update the content (respect the two-step
   rule — read linked pages first), bump `updated` and `synced_commit` to HEAD.
5. Changed files not covered by any page: judge significance (new module, spec, feature).
   Create a page, or append a coverage-gap item to `wiki/review.md`.
6. Read `git log --oneline <baseline>..HEAD` for decisions or direction changes worth
   capturing in `decisions/` or `overview/`.
7. Update `wiki/index.md`; append one `wiki/log.md` line: date, `sync`, `<baseline>..<HEAD>`,
   pages touched.
8. Run `bash scripts/wiki_lint.sh` and fix mechanical findings; regenerate the callout
   index with `bash scripts/wiki_callouts.sh`.
9. Report: pages updated/created, review items added, anything skipped and why.
