# AI Session Start Checklist

Run through this list at the beginning of every Claude Code session before writing any code.

## 1. Load context
- [ ] Re-read `.claude/CLAUDE.md` (architecture rules, naming, ports, invariants)
- [ ] Read the last 15–20 entries of `docs/history/project_journal.md`
- [ ] Read `docs/reference/KEY_LEARNINGS.md` (skim headings; read relevant sections)
- [ ] For architecture/domain-heavy tasks: consult the wiki first (`wiki/index.md` is the
      catalog; `/wiki-query` answers with citations)

## 2. Check project state
- [ ] `git status` — any uncommitted changes?
- [ ] `git log --oneline -10` — what was done last?
- [ ] Confirm current branch name and its purpose (check `docs/plans/` or the openspec
      feature list if unclear)

## 3. Check open work
- [ ] Scan `docs/BACKLOG.md` for high-priority open items
- [ ] Scan `docs/reference/TECHNICAL_DEBTS.md` — any debt in the area you are about to touch?
- [ ] Check `specs/` for any active feature spec (tasks not yet marked done)

## 4. Quarterly controls *(do this ~every 3 months, not every session)*
- [ ] Run architecture invariant checks (from CLAUDE.md `ven-architecture:` section)
- [ ] Generate Mermaid module diagram and compare to
      `docs/architecture/module_dependency_graph.md`
- [ ] Compare `DOCUMENTATION.md` to actual code; update stale sections
- [ ] Run `cargo audit` and `npm audit`; add findings to BACKLOG.md

## 5. Definition of Done *(verify before closing a feature)*
- [ ] All test suites green (UI unit, Rust unit+integration, E2E BDD)
- [ ] `cargo clippy -- -D warnings` clean
- [ ] Architecture invariants verified (grep checks in CLAUDE.md)
- [ ] `docs/history/project_journal.md` updated
- [ ] `docs/BACKLOG.md` updated (close resolved items, add discovered items)
- [ ] `docs/reference/TECHNICAL_DEBTS.md` updated if new debt was found or resolved
- [ ] Feature spec archived to `specs/archive/` (if applicable)
- [ ] `/wiki-sync` run if the change touched architecture, domain behaviour, or specs
      (`bash scripts/wiki_lint.sh` reports stale pages)
