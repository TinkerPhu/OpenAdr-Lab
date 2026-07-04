---
name: wiki-ingest
description: Deep-ingest one source (a file, directory, or topic) into the OpenADR Lab wiki (wiki/), creating or updating source, component, concept, use-case, and decision pages with full traceability. Use when a specific document or code area deserves thorough wiki coverage.
---

Argument: a repo path or a topic. Read `wiki/CLAUDE.md` and `wiki/purpose.md` first;
all rules there apply.

1. Read the source material fully. For a topic instead of a path, locate the relevant
   files with Grep/Glob and list them as the page's `sources:`.
2. If the user gave framing or emphasis notes, honor them; otherwise follow `purpose.md`.
3. **Two-step rule**: read `wiki/index.md` and every existing page you intend to link.
   Identify connections and contradictions before writing anything.
4. Write or update:
   - `wiki/sources/<slug>.md` — summary page (for document sources; skip for code dirs)
   - affected `components/`, `concepts/`, `use-cases/`, `decisions/` pages
   - `wiki/overview/` synthesis pages if the ingest changes the big picture
5. Every touched page gets full frontmatter; `synced_commit` = current HEAD short sha.
6. Update `wiki/index.md`; append one `wiki/log.md` line; add `wiki/review.md` items for
   contradictions you cannot resolve from the sources.
7. Report created/updated pages and open review items.
