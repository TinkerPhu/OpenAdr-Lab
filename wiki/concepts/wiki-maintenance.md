---
title: Wiki Maintenance Workflow
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 5a9a304
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
| Overview of open flags | `wiki/callouts.md` — auto-generated index of all DRIFT/CONTRADICTION/OPEN QUESTION callouts (`bash scripts/wiki_callouts.sh` to refresh; sync/lint do it automatically) |
| Project direction shifts | edit `wiki/purpose.md` yourself |

## Enforcing a `purpose.md` change

`purpose.md` steers **future** writes automatically — every workflow re-reads it before
touching anything, so pages written after the edit follow the new emphasis without
further action. Existing pages are the catch: their `sources:` lists do **not** include
`purpose.md`, so the git-anchored staleness check will never flag them when the purpose
changes. Enforcement across existing content is therefore an explicit step:

1. Edit `wiki/purpose.md` and commit it.
2. In the same session (best — the session sees the diff), run `/wiki-sync` or
   `/wiki-lint` and say explicitly: **"purpose.md changed — re-audit all pages against
   it."** The content pass then checks every page against the new scope and emphasis,
   not just against its `sources:`.
3. Pages that no longer fit the purpose get updated, or filed to `wiki/review.md` if the
   right response needs an owner decision.

Rules of thumb: wording tweaks in purpose.md need no enforcement pass at all; a shifted
*emphasis* (new focus area, dropped goal, changed audience) warrants step 2 at the next
sync; a *scope* change (new source types, new page categories) warrants it immediately.

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
