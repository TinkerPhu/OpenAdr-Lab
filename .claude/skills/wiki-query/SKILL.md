---
name: wiki-query
description: Answer a question from the OpenADR Lab wiki (wiki/) with citations to wiki pages and underlying repo sources; optionally file the answer to wiki/queries/. Use when the user asks a knowledge question about the project, domain, or architecture.
---

1. Read `wiki/index.md`; select the relevant pages; read them.
2. Synthesize an answer citing wiki pages (`[[slug]]`) **and** the underlying repo sources
   they point to. Prefer wiki content, but verify load-bearing claims against the cited
   source if the page's `synced_commit` is old.
3. If the wiki cannot answer: say so, answer from the repo directly, and append a
   coverage-gap item to `wiki/review.md` so the gap gets filled later.
4. File the answer to `wiki/queries/<slug>.md` (full frontmatter, per `wiki/CLAUDE.md`)
   only when the user asks for it; otherwise offer it once at the end.
