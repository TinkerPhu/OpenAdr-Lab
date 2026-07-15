---
title: "Decision: Superpowers Framework Not Adopted (Borrow Worktree Pattern)"
type: decision
created: 2026-06-25
updated: 2026-07-16
synced_commit: 2c79d53
sources: [docs/guidelines/AI-SW-Development.md, docs/reference/SESSION_START.md]
tags: [decision, tooling, workflow, agentic]
---

# Decision: Superpowers Framework Not Adopted (Borrow Worktree Pattern)

**Decision (assessed 2026-06-25):** do not adopt the
[Superpowers](https://github.com/obra/superpowers) agentic skills framework;
borrow only its worktree-per-feature pattern.

## Context

Superpowers is an open-source agentic skills framework for Claude Code built
around composable SKILL.md files. It enforces a structured workflow:
design first → isolated git worktree → micro-task plan with verification
steps → subagent execution.

## Rationale

The existing skill stack already covers ~80 % of what Superpowers provides:

- `speckit-*` / `opsx:*` skills enforce design → spec → plan → tasks → implement
  (the same spec-driven workflow that produced the [[hexagonal-refactoring]]).
- `SESSION_START.md`, `project_journal.md`, and `KEY_LEARNINGS.md` handle
  context and institutional memory.
- `CLAUDE.md` architecture rules (ven-architecture, test-first, naming,
  determinism) already constrain agent behaviour heavily.

The one concrete gain is **worktree isolation per feature** — given the
Pi4-Server dependency for full test runs (see [[testing-strategy]]) and
DCO/CI discipline, a clean
worktree per feature reduces accidental cross-contamination between
concurrent features. This pattern is used (see `worktrees/`), without the
rest of the framework.

The subagent execution model is worth re-evaluating if project complexity
grows substantially.
