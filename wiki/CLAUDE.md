# Wiki Rules — OpenADR Lab LLM Wiki

This directory is an LLM-maintained wiki (Karpathy "LLM wiki" pattern): Claude writes and
maintains every page here; the human curates strategy through `purpose.md`. The wiki
synthesizes the **whole repository** — code, specs, docs, BDD tests, git history — not just
`docs/`. Read `purpose.md` before any ingest or sync.

## Roles

- **Sources** (read-only during wiki work — never edit them from a wiki workflow):
  `VEN/src/`, `VEN/ui/`, `VTN/`, `tests/features/`, `specs/`, `openspec/`, `docs/`,
  and git history (`git log`).
- **wiki/** — the only directory wiki workflows write to.
- **purpose.md** — human-curated scope and emphasis. Never rewrite it without an explicit
  user request; propose changes as suggestions instead.

## Layout

| Path              | Role                                                                 |
|-------------------|----------------------------------------------------------------------|
| `CLAUDE.md`       | This file — schema and rules (auto-loaded when working in wiki/)     |
| `purpose.md`      | Why the wiki exists, what to emphasize (human-curated)               |
| `index.md`        | Catalog of all pages; updated on every ingest/sync                   |
| `log.md`          | Append-only operation log                                            |
| `review.md`       | Human-in-the-loop queue: contradictions, gaps, uncertain claims      |
| `overview/`       | Synthesis pages: evolving thesis, system-level summaries, vision     |
| `architecture/`   | Ring map, ports, dependency rules, deployment topology               |
| `components/`     | One page per significant module (milp_planner, dispatcher, simulator…)|
| `concepts/`       | OpenADR 3 terms, DR/HEMS ideas, techniques (MILP, 3-tier planning…)  |
| `use-cases/`      | Scenarios the system serves; BDD features are primary sources        |
| `decisions/`      | Why-choices (ADR-style), mined from specs/, journal, commit history  |
| `sources/`        | One summary page per ingested document                               |
| `queries/`        | Filed answers worth keeping                                          |

## Page schema

Every page is markdown with YAML frontmatter:

```yaml
---
title: MILP Planner
type: component        # overview | architecture | component | concept | use-case | decision | source | query
created: 2026-07-04
updated: 2026-07-04
synced_commit: 6cb8ca6 # repo HEAD (short sha) when content was last verified against sources
sources: [VEN/src/controller/milp_planner/, docs/architecture/VEN_ARCHITECTURE.md]
tags: [planner, milp]
---
```

- `sources:` lists repo-relative files **or directories** this page derives from. It drives
  staleness detection (`git diff <synced_commit>..HEAD -- <sources>`) and cascade updates.
  Inline array or YAML block list are both fine; no paths with spaces.
- `synced_commit:` is bumped every time the page content is re-verified against its sources.

## Conventions

1. Filenames are kebab-case slugs, unique across the whole wiki; the filename **is** the
   wikilink target: `components/milp-planner.md` ⇢ `[[milp-planner]]`.
2. Every content page links to **at least 2** other wiki pages with `[[wikilink]]` syntax
   (Obsidian-compatible). Links *from* `index.md` do not count as inbound links.
3. Every claim cites its source in place: a repo path (`VEN/src/controller/dispatcher.rs`),
   a doc section (`docs/REQUIREMENTS.md §2.3`), a spec citation, or a commit sha for
   historical claims.
4. **Synthesize, don't duplicate.** If `docs/` already explains something, link it and
   summarize in ≤3 sentences — then add what the docs don't say: how the code actually
   implements it, gaps, tensions, evolution.
5. Vocabulary follows the glossary in `docs/REQUIREMENTS.md` (single source of truth).
   Physical quantities keep their unit suffixes (`power_kw`, `tariff_eur_per_kwh`, `soc_pct`).
6. Callouts:
   - `> **CONTRADICTION** …` — two sources disagree
   - `> **OPEN QUESTION** …` — unresolved design/domain question
   - `> **DRIFT** …` — docs say X, code does Y (cite both)
7. OpenADR reference material: OpenADR 3 only (`docs/openadr_3_1_specs/`). Never read
   `docs/specs/pdf/`.

## Editorial rules

- **Two-step writing**: before creating or updating a page, read `index.md` and every page
  you intend to link. Identify connections first; write second. Never write a page in
  isolation.
- Every workflow run appends one line to `log.md` (date, operation, commit range, pages touched).
- When uncertain or when sources contradict, add an item to `review.md` instead of guessing.
- After any write batch, update `index.md`.
- Deleting a page requires user confirmation and a check for inbound links (cascade).

## Workflows

| Command        | Does                                                              |
|----------------|-------------------------------------------------------------------|
| `/wiki-sync`   | Update pages whose sources changed since their `synced_commit`; seed the wiki if empty |
| `/wiki-ingest` | Deep-ingest one source file/dir/topic into the wiki               |
| `/wiki-query`  | Answer a question from the wiki with citations                   |
| `/wiki-lint`   | Mechanical checks (`scripts/wiki_lint.sh`) + content-level review |

Mechanical invariants (broken links, orphans, staleness, frontmatter) are checked by
`bash scripts/wiki_lint.sh` — run it after any larger write batch.
