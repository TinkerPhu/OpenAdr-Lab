---
name: wiki-lint
description: Check the OpenADR Lab wiki (wiki/) for mechanical and content problems — broken wikilinks, orphan pages, stale pages, missing frontmatter, contradictions, and duplication of docs/. Use periodically or after large ingest/sync batches.
---

Read `wiki/CLAUDE.md` first.

1. **Mechanical pass**: run `bash scripts/wiki_lint.sh`. It reports broken wikilinks,
   orphan pages (no inbound links from content pages), missing/incomplete frontmatter,
   missing source paths, and stale pages (`sources:` changed since `synced_commit`).
2. **Content pass** (LLM): read the pages and look for
   - contradictions between pages, and claims superseded by current code
   - pages that duplicate `docs/` instead of synthesizing (violates convention 4)
   - heavily-referenced concepts that have no page of their own (missing hubs)
   - `CONTRADICTION` / `OPEN QUESTION` / `DRIFT` callouts that are now resolvable
3. Report all findings with suggested fixes. Apply straightforward fixes (stale content,
   broken links, frontmatter) directly; ask before deleting or merging pages.
4. Bump `updated`/`synced_commit` on touched pages, update `wiki/index.md` if pages changed,
   append one `wiki/log.md` line, and move resolved `wiki/review.md` items to done.
