---
title: Wiki Maintenance Workflow
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [wiki/CLAUDE.md, scripts/wiki_lint.sh, .claude/skills/, docs/reference/SESSION_START.md]
tags: [wiki, workflow, meta]
---

# Wiki Maintenance Workflow

How this wiki stays current. Rules live in `wiki/CLAUDE.md`; scope and emphasis in
`wiki/purpose.md` (human-curated — workflows never rewrite it).

## The core mechanism: git-anchored freshness

Every content page records `sources:` (repo paths it derives from) and `synced_commit:`
(HEAD when last verified). Staleness is therefore *computable*:
`git diff <synced_commit>..HEAD -- <sources>` — no guessing about what changed.

## The one habit

**After finishing a feature, run `/wiki-sync`.** It finds the last synced commit, diffs
the repo, updates only affected pages, creates pages (or review items) for uncovered new
modules, and logs the operation. It is wired into the Definition of Done in
`docs/reference/SESSION_START.md`, so it rides the existing merge checklist.

## On demand

| Trigger | Action |
|---|---|
| Big new doc or module deserves deep coverage | `/wiki-ingest <path or topic>` |
| Any knowledge question | `/wiki-query` — answers cite pages + sources; unanswerable questions auto-file a gap to `wiki/review.md` |
| Quick staleness check (2 s, CI-able) | `bash scripts/wiki_lint.sh` — broken links, orphans, under-linked pages, frontmatter, stale pages; exit 1 on findings |
| Project direction shifts | edit `wiki/purpose.md` yourself |

## Periodic (fits the quarterly-controls slot in SESSION_START.md)

Run `/wiki-lint` (mechanical script + LLM content pass: contradictions, doc drift,
duplication of `docs/`, missing hub pages) and work through the checkboxes in
`wiki/review.md` — the human-in-the-loop queue where sync/ingest park contradictions and
coverage gaps instead of guessing.

## Never

- Hand-edit content pages to "fix" facts — ask a session, so frontmatter
  (`updated`, `synced_commit`) stays truthful.
- Let a wiki workflow modify source documents — `wiki/` is its only write target.

Context: the wiki's role in the project is described in [[vision-and-roadmap]];
what it covers starts at [[openadr-lab]].
