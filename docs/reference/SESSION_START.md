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

## 4. Recurring controls *(tracked in `jobs.json`, not by memory)*

"~every 3 months" is not a schedule anyone can follow: nothing here knew when the last
time was. `jobs.json` records each check's interval and when it last ran, and
`scripts/jobs.py` answers "what is due".

- [ ] `python scripts/jobs.py due` — nothing due prints nothing, so no news is good news
      (the `SessionStart` hook in `.claude/settings.json` already runs this for you)
- [ ] `python scripts/jobs.py run` — runs every due check that has a command
- [ ] `python scripts/jobs.py done <id>` — for the ones only a human can do, once handled

Registered today: the openspec version check (weekly, also covered by CI), the
`cargo audit` / `npm audit` sweep, the `ven-architecture` invariant greps plus the module
diagram, and documentation drift. Add a job rather than a checklist line whenever the
trigger is "time passed" rather than "this commit".

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
