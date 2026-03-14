<!--
SYNC IMPACT REPORT
==================
Version change: [unversioned template] → 1.0.0 (initial adoption)
Modified principles: N/A — first-time fill of blank template
Added sections: Core Principles (×5), Technology Constraints, Development Workflow, Governance
Removed sections: None
Templates requiring updates:
  ✅ plan-template.md — "Constitution Check" gate already present; principles align
  ✅ spec-template.md — no constitution-specific references; no changes needed
  ✅ tasks-template.md — task categorization compatible with all five principles
  ✅ agent-file-template.md — generic template; no updates required
Deferred TODOs: None — all fields resolved from project context
-->

# OpenADR Lab Constitution

## Core Principles

### I. OpenADR Spec Fidelity (NON-NEGOTIABLE)

Field names MUST match the OpenADR 3 specification verbatim across every layer —
backend, BFF, and UI. DTO normalization (renaming, aliasing, or reshaping OpenADR
fields at any layer boundary) is PROHIBITED. One vocabulary everywhere eliminates
translation errors and reduces debugging friction.

Concrete rules:
- Use `programName`, `programID`, `eventName`, `venName`, `createdDateTime`, etc.
  exactly as the spec defines them — never rename to camelCase, snake_case, or
  "cleaner" equivalents.
- When the spec adds or renames a field, update all layers atomically.
- OpenADR reference MUST come exclusively from OpenADR 3 resources (not v2.x).
  Spec markdown lives at `docs/specs/openadr_3_1_specs/`.

**Rationale**: Mismatched field names between layers caused repeated debugging
sessions; a single vocabulary made issues immediately visible.

### II. BDD-First Testing

New behavior MUST be described in behave scenarios (`.feature` files) before
implementation begins. Tests MUST fail before any implementation code is written.
All acceptance scenarios from a spec MUST have corresponding BDD coverage.

Concrete rules:
- Feature files live in `tests/features/`; step definitions in `tests/steps/`.
- Integration tests run in Docker via `docker compose -f tests/docker-compose.test.yml
  run --build --rm test-runner`.
- Always pass `--build` when any source file copied into the test-runner image
  has changed; omitting it silently runs stale tests.
- Unit tests (vitest) and cargo tests supplement BDD but do not replace it for
  behavior specifications.
- Upstream PR branches MUST have zero test failures before a PR is opened.

**Rationale**: BDD provides living documentation and prevents behavior regressions
across 27+ features and 800+ steps. Skipping the red phase caused phantom-passing
tests on multiple occasions.

### III. Upstream Compatibility

All changes to `openleadr-rs` (the git submodule at `openleadr-rs/`) MUST be
suitable for upstream contribution. Code quality, commit hygiene, and DCO signoff
are non-negotiable on every PR branch.

Concrete rules:
- Every commit on a PR branch MUST carry a `Signed-off-by:` line (DCO).
- Run `cargo fmt`, `cargo clippy`, and `cargo audit` locally before pushing.
  rustfmt wraps differently from hand-written style — always format before push.
- Squash to a single clean commit before opening a PR (one logical change =
  one commit).
- Do NOT open an upstream PR until all local and CI tests pass completely.
  After opening, wait for CI to actually report before drawing conclusions.
  Investigate every CI failure as potentially caused by our changes — never write
  off failures as pre-existing without evidence.
- The submodule on Pi4 resets to the recorded commit after `git pull`; always
  re-run `cd openleadr-rs && git checkout <branch>` before testing a PR branch.

**Rationale**: Sloppy commits, missing DCO, and premature PRs caused rework and
confusion. The upstream project deserves the same quality bar as production code.

### IV. Lean Architecture

Every design decision MUST start from the simplest solution that meets the current
requirement. Abstractions, helpers, and indirection are justified only when a
concrete need exists today — not for hypothetical future flexibility.

Concrete rules:
- No DTO normalization layers (see Principle I).
- Do not add error handling, fallbacks, or validation for scenarios that cannot
  happen. Trust Rust's type system and framework guarantees at internal boundaries;
  validate only at system entry points (HTTP handlers, external API responses).
- Three similar code paths are preferable to a premature abstraction.
- Do not add docstrings, comments, or logging to code that was not changed in the
  current task unless the logic is non-obvious.
- Complexity violations MUST be justified in the plan's Complexity Tracking table
  before being introduced.

**Rationale**: Unnecessary abstraction added maintenance overhead without benefit
in multiple phases; leaning on Rust's type system catches most errors that defensive
code would guard against.

### V. Infrastructure Parity

All Docker operations MUST run on Pi4-Server via SSH in `/srv/docker/openadr_lab`.
Dev, test, and production environments MUST use the same Docker Compose definitions.
No manual server state outside of Compose files and committed configuration.

Concrete rules:
- Deploy flow: commit locally → `git push` → `ssh Pi4-Server "cd /srv/docker/
  openadr_lab && git pull"` → `docker compose up -d --build`.
- NEVER stop containers not involved in this project without explicit user
  confirmation — other containers are productive services.
- Named cargo volumes survive power cycles; incremental builds resume from cache.
  After source changes, always rebuild the image explicitly (`docker compose build
  <service>`) — `--build` on `run` may not rebuild the target service.
- ARM64 (Pi4) resource constraints (`cpus: 1.5`, `memory: 1500M`,
  `CARGO_BUILD_JOBS=4`) MUST stay in committed files; document removal instructions
  for non-Pi hosts in the README.

**Rationale**: Inconsistent environments caused builds that passed locally but
failed on Pi4. All test and deployment commands are defined once and run through
SSH to eliminate drift.

## Technology Constraints

**VTN backend**: Rust (openleadr-rs, axum) — git submodule at `openleadr-rs/`.
**BFF**: Rust (axum) — dual OAuth credentials, at `VTN/bff/`.
**VEN backend**: Rust (axum + tokio) — physics-based simulator + HEMS controller, at `VEN/src/`.
**Frontends**: React + MUI + Vite + TypeScript (VTN UI at `VTN/ui/`, VEN UI at `VEN/ui/`).
All React code MUST follow `docs/guidelines/REACT_GUIDELINES.md`:
- Components as named `function` declarations (not `const FC`); props destructured on line 1.
- `data-testid` on every interactive and data-displaying element; `aria-*` attributes where applicable.
- Vitest + @testing-library/react for unit tests; TanStack React Query for all API access.
**Database**: PostgreSQL 16 — managed by SQLx migrations inside openleadr-rs.
**Integration tests**: Python behave (BDD) — at `tests/features/`.
**E2E tests**: Playwright — browser-driven scenarios against running stack.
**Unit tests**: vitest (UI), cargo test (Rust).
**Platform**: Linux ARM64 (Raspberry Pi 4), Docker Compose v2.

SQLx offline cache MUST be regenerated on Pi4-Server whenever SQL in Rust source
changes (see `docs/reference/KEY_LEARNINGS.md` — "SQLx Offline Cache" section).
A wrong cache wastes ~25 min per rebuild cycle.

## Development Workflow

1. **Journal**: Every significant implementation step MUST be recorded in
   `docs/history/project_journal.md` — what was done, why, issues encountered,
   and key learnings.

2. **Key Learnings**: Hard-won lessons MUST be written to
   `docs/reference/KEY_LEARNINGS.md` and consulted before making similar decisions
   in future phases.

3. **Specs**: Feature planning MUST use the speckit workflow:
   `/speckit.specify` → `/speckit.plan` → `/speckit.tasks` → `/speckit.implement`.
   OpenADR-specific design documents live in `docs/VEN_Controller/` and
   `docs/specs/`.

4. **Commit hygiene**: Do not add co-authoring footers to commit messages or PR
   descriptions — they may be rejected by upstream. Commits on upstream PR branches
   MUST be DCO-signed.

5. **Upstream PRs**: Only consider creating an upstream PR after all tests pass
   completely without failure and commits are ready for CI acceptance. After
   creating a PR, wait for CI to actually run and report — do not draw conclusions
   from a pending state.

## Governance

This constitution supersedes all other project practices. Any conflict between
this document and an individual task or plan MUST be resolved in favour of the
constitution.

**Amendment procedure**:
1. Identify the principle or section to amend.
2. Propose the change with rationale; update `LAST_AMENDED_DATE` and bump version.
3. Update any dependent templates listed in the Sync Impact Report header.
4. Commit with message: `docs: amend constitution to vX.Y.Z (<summary>)`.

**Versioning policy** (semantic):
- MAJOR: principle removed or fundamentally redefined.
- MINOR: new principle or section added, or existing principle materially expanded.
- PATCH: clarifications, wording, non-semantic refinements.

**Compliance review**: Every implementation plan's "Constitution Check" gate MUST
be cleared before Phase 0 research begins, and re-checked after Phase 1 design.
Violations require an entry in the plan's Complexity Tracking table.

Runtime development guidance is available in `CLAUDE.md` (project root) and
`docs/reference/KEY_LEARNINGS.md`.

**Version**: 1.0.0 | **Ratified**: 2026-03-13 | **Last Amended**: 2026-03-13
