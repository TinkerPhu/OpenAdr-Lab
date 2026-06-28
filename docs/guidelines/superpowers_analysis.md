# Superpowers Framework — Fit Analysis

**Source:** [github.com/obra/superpowers](https://github.com/obra/superpowers)
**Date assessed:** 2026-06-25

## What is Superpowers?

An open-source agentic skills framework for Claude Code (and other AI CLIs) built around composable SKILL.md files. It enforces a structured development workflow: design first → isolated git worktree → micro-task plan with verification steps → subagent execution.

## What it would add to this project

- **Worktree isolation per feature** — each feature branch gets its own `git worktree`, eliminating branch switching friction and keeping `main` clean while a subagent works in parallel.
- **Subagent-per-task execution** — tasks are handed off to subagents for parallel execution, reducing sequential bottlenecks.
- **Enforced design gate** — the agent cannot write code until a design step is complete, enforced at the skill level rather than relying on discipline.

## What this project already covers (overlap)

- `speckit-*` and `opsx:*` skills already enforce design → spec → plan → tasks → implement.
- `SESSION_START.md`, `project_journal.md`, and `KEY_LEARNINGS.md` already handle context and institutional memory.
- `CLAUDE.md` architecture rules (ven-architecture, test-first, naming, determinism) already constrain agent behavior heavily.

## Verdict

**Moderate benefit, mostly overlap.** The existing speckit/opsx workflow covers ~80% of what Superpowers provides. The one concrete gain is the **worktree isolation pattern** — given the Pi4-Server dependency for full test runs and DCO/CI discipline, having a clean worktree per feature reduces accidental cross-contamination between concurrent features.

The subagent execution model is also worth watching as the project grows in complexity.

**Recommendation:** Borrow the worktree-per-feature pattern without adopting the full framework, since the project's existing skill stack is already well-structured.
