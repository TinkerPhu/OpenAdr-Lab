docker: docker runs on ssh Node1 (primary — VTN, its DB, BFF, VTN UI, and the main VEN
docker-compose stack) or ssh Node2 (secondary — ven-4 + build/test offload, see
VEN/scale_out/node2/). Run tasks with docker on the intended host via ssh in
directory /srv/docker/openadr_lab (same path on both hosts).

node1-lock: Node1 is shared between multiple parallel sessions/worktrees. Before ANY
docker build or test run on Node1, acquire the lease lock and hold it for the
whole sequence:
  bash scripts/docker_host_lock.sh acquire -m "<branch>: <what you are doing>" [-l MIN]
  ... all ssh Node1 docker commands ...
  bash scripts/docker_host_lock.sh release
The acquirer declares its own lease end (-l minutes, default 60) which is stored in
the lock as UTC time; once that time passes the lock counts as dead and the next
acquirer steals it. Pick -l honestly for the expected runtime; `refresh [-l MIN]`
extends from now if a run overshoots. `status` shows holder, task, and lease end.
`acquire` blocks up to ~9 min then exits 2 — rerun it to keep waiting; report to the
user if the lock stays held unusually long instead of bypassing it. run_all_tests.sh
acquires/releases the lock automatically (-l 180) for remote docker suites. Never run
docker commands on Node1 while another owner holds an unexpired lock.

node2-lock: Node2 (192.168.1.104) is a second docker host, used to take build/test load
off Node1. It shares the same lock script and mechanism as Node1 (docker_host_lock.sh isn't
Node1-specific despite the name — the lock file is derived from the target host, so
each host gets its own independent lock). Set LOCK_HOST=Node2 to target it
directly, or run `DOCKER_HOST=Node2 bash run_all_tests.sh ...`, which sets this
automatically. Node1 and Node2 can be used concurrently by different sessions without
contention, since each holds its own lock. Same lease semantics and default
(60min) as Node1. Node2 also has `tests`/`VTN` in its sparse-checkout and openleadr-rs
submodule initialized, so all four suites (`--rust`, `--e2e`, `--resilience`, and the
full run_all_tests.sh) run there too, not just `--rust`.

test-host-preference: this project now has two build/test-capable docker hosts (Node1,
Node2) alongside local WSL. Prefer Node2 for build/test runs (`DOCKER_HOST=Node2 bash
run_all_tests.sh ...`) — Node1 also runs the always-on production VTN/BFF/VEN stack, so
heavy test builds there compete with and can slow down live services. Use Node1 only when
Node2 is unavailable/locked, or when a test specifically needs Node1 (e.g. verifying
something against the real production stack).

local-rust: WSL is installed on this Windows machine. Use `wsl cargo check` (or `wsl cargo test`) inside the VEN directory for local Rust compilation instead of native Windows cargo, which lacks cmake/HiGHS. For a full test run including HiGHS, use the Node1 docker stack.

memory-budget: this laptop has only 8 GB RAM — WSL cargo builds have crashed the host
(pagefile exhaustion, "Catastrophic failure Wsl/Service/E_UNEXPECTED"). Before starting
any large-memory task (cargo build/check/test/clippy in WSL, parallel npm builds), check
free memory first:
  Get-CimInstance Win32_OperatingSystem | % { "{0:N1} GB free" -f ($_.FreePhysicalMemory/1MB) }
Rules: always pass `-j 2` to cargo in WSL; if free physical memory is below ~1 GB, wait or
ask the user to close applications before starting.

wsl-lock: the WSL instance on this laptop is shared between multiple parallel
sessions/worktrees, same as Node1. Before ANY `wsl cargo build/check/test/clippy` (or
other large-memory WSL command), acquire the lease lock and hold it for the whole
sequence:
  bash scripts/wsl_lock.sh acquire -m "<branch>: <what you are doing>" [-l MIN]
  ... all wsl cargo commands ...
  bash scripts/wsl_lock.sh release
Same semantics as node1-lock (self-declared lease stored as UTC epoch, re-entrant per
owner, dead locks are stolen with a warning): `-l` sets the lease in minutes (default
20 — override for long test runs), `refresh [-l MIN]` extends from now if a run
overshoots, `status` shows holder/task/lease end. `acquire` blocks up to ~9 min then
exits 2 — rerun it to keep waiting; report to the user if the lock stays held unusually
long instead of bypassing it. Never run a WSL cargo build/test while another owner
holds an unexpired wsl_lock.

dto: avoid DTO normalization. pass through upstream field names (e.g. OpenADR spec names) across all layers — backend, BFF, UI. one vocabulary everywhere reduces boilerplate and debugging friction.

workflow: 1. always keep a project_journal.md in projects where you write for each large step what you did, why you did it and what issues/key-learnings you had. it shall explain, how the project was implemented. The journal lives at docs/history/project_journal.md.
2. write key learnings into KEY_LEARNINGS.md (at docs/reference/KEY_LEARNINGS.md) and consider them when making decissions.
3. no lingering old plans or plan items after implementation. The entire openspec/ folder is
construction/change-management scaffolding, not a documentation destination — this includes
openspec/specs/, not just openspec/changes/. Once a plan (docs/plans/) or an openspec change
(openspec/changes/, including its specs/) has been implemented and successfully tested, wave its
feature/functionality into the appropriate current-state documentation under docs/ (mechanism-level
facts into the relevant docs/architecture/*.md; user-observable behavior into the relevant
docs/use-cases/*.md; new doc files only when no existing one fits, not by default), then delete the
plan/change (do not archive — the openspec-archive-change skill is removed, replaced by this delete
workflow). Don't restate what tests already assert with full rigor — point to the test file by
name instead of hand-copying SHALL/Scenario prose into docs; that duplication rots as the code
evolves. Implementation-issue narrative is not carried forward once a thing is implemented; if an
issue holds a durable lesson for future work, put it in KEY_LEARNINGS.md, not in a surviving plan
file. If only part of a plan/change is done and tested, remove just that part and leave the rest in
place — do not delete a plan/change wholesale on partial completion.
Deletion here is git-recoverable (history isn't lost), so this is about keeping the working tree's
active plans/specs true to what's still outstanding, not about erasing the record.
4. when a feature enables or improves a user-facing use case, document it in the relevant
docs/use-cases/*.md (new file only if none fits) and add or extend a BDD scenario in
tests/features/ that exercises that use case specifically — not just implementation-level unit
tests. A unit test proves the mechanism works internally; a use-case scenario proves the thing a
user actually does (set a curve, create a session, get a different plan) works end-to-end. Do
this as part of the same piece of work, not a deferred follow-up.
5. no lingering checkouts/worktrees on Node1 or Node2 after a branch is merged to main. Once a
branch's work is confirmed merged into origin/main, switch that host's checkout back to main
(`git checkout main && git pull`) and delete the now-merged local branch; if the work was done in
a `git worktree` (e.g. under worktrees/), remove the worktree (`git worktree remove`, `sudo rm -rf`
first if build artifacts like VEN/target left root-owned files, then `git worktree prune`) rather
than leaving it checked out on an old commit. Do this cleanup as soon as a merge is confirmed, not
as a periodic sweep.

NEVER stop docker containers that are not involved in this project without asking. They are productive containers.

branching: feature branches use the pattern NNN-whatever-case-description where NNN is the
openspec feature ID (e.g. 030-my-feature). Refactor branches: refactor/<slug>.
Fix branches: fix/<slug>. All those branches target main. Never force-push to main. The goal is to rebase and fast forward merge.
DCO sign-off is enforced by CI — do not add co-author footers (see rule above).
Merge only after all CI checks pass: cargo fmt, cargo clippy --all-targets --all-features -- -D warnings, cargo audit,
file-size audit (scripts/audit_file_sizes.py — tasks/ ≤ 200, VEN/src/ ≤ 500 production
lines), and E2E tests green on Node1.

testing: full guide at docs/guidelines/TESTING.md. Four suites:
  1. UI unit (local)       — cd VEN/ui && npm test  |  cd VTN/ui && npm test
  2. Rust unit+integration — wsl cargo test -p ven-app  (local, no HiGHS needed for most)
  3. E2E BDD (Node1)         — bash run_all_tests.sh --e2e  (behave)
  4. Resilience (Node1)      — bash run_all_tests.sh --resilience
Run everything: bash run_all_tests.sh
VEN Rust pyramid (4 layers, all must stay green after any VEN change):
  Domain → Use-case → Adapter-contract → Integration
  Shared mock adapters live in VEN/src/services/test_support/ (test-only,
  #[cfg(test)]-gated via services/mod.rs).
  Test naming: <function>_<scenario> (no test_ prefix — redundant inside #[cfg(test)]).
All suites must pass before merging a PR to main.

test-first: write the test first. Selectively run it to confirm it fails. Implement until it is green.
This applies at unit level for every new function and at BDD level for every new behaviour.

session-start: at the start of every AI session read docs/reference/SESSION_START.md
and follow the checklist before touching any code.

linting: cargo fmt --check and cargo clippy --all-targets --all-features -- -D warnings
(same command CI runs) must pass before any commit.
For JS/TS (VEN/ui, VTN/ui): eslint must report zero errors. Suppress clippy lints with
#[allow(...)] only if justified with a comment on the same line. No enforced coverage floor —
keep domain and application layer tests meaningful.

build:
  local VEN Rust : wsl cargo build  (or wsl cargo check for fast syntax check)
  local UI       : cd VEN/ui && npm run build  |  cd VTN/ui && npm run build
  Node1 docker     : ssh Node1 "cd /srv/docker/openadr_lab && docker compose build"
  Node1 single svc : ssh Node1 "cd /srv/docker/openadr_lab && docker compose build ven"
  Node2 docker     : ssh Node2 "cd /srv/docker/openadr_lab && docker compose build"
  Always use wsl for Rust compilation — native Windows cargo lacks cmake/HiGHS.
  CI: .github/workflows/ holds three workflows — pre-pr-checks-splittasks.yml
  (fmt/clippy/audit/DCO on PR), file_size_audit-splittasks.yml (scripts/audit_file_sizes.py
  on push/PR), e2e-tests.yml (manual dispatch only). Still run linting + tests manually
  before merging — these workflows don't yet block merges.

determinism: any code path that depends on the current date/time must accept an injectable
clock (e.g. a Fn() -> DateTime<Utc> parameter or typed wrapper). Applied in the MILP
planner, simulator, and assets rings (R-24). All new modules that schedule, timestamp,
or expire must follow the same pattern. Makes tests reproducible without sleep or
wall-clock coupling.

dependencies: pin all new crates to a semver range in Cargo.toml; npm packages use caret
ranges in package.json — package-lock.json is the pinning mechanism (commit it; never
delete it to "fix" installs). Run cargo audit and npm audit before each release; add
findings to docs/BACKLOG.md with severity. Acceptable licences: MIT, Apache-2.0,
BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, CDLA-Permissive-2.0, and the
OpenSSL licence (aws-lc-sys). Review every new import — AI-generated code frequently
introduces undeclared dependencies.

ui-transparency: every backend capability, external feed/port, and piece of derived state must
have a visible surface in the corresponding UI (VEN UI / VTN UI) — no functionality should exist
only server-side with no way to inspect it. This applies to both the raw received state (e.g. a
fetched external forecast, a polled upstream value) and any state VEN/VTN constructs from it (e.g.
a derived forecast, a computed status). When adding a new capability, plan its UI surface (a
status row, a diagnostics-page entry, a panel) as part of the same piece of work, not a deferred
follow-up — a route or port with no UI-visible counterpart is an incomplete implementation, not a
finished one with polish pending. Precedent: the WP-T1..T8 work (docs/history/project_journal.md —
search "WP-T"; docs/architecture/VEN_ARCHITECTURE.md §4.10) that put VTN connection/plan/task
status directly on the VEN UI Dashboard and under its Diagnostics menu group.

no-half-built-features: don't build one side of an interface without the other in the same
piece of work — a backend endpoint with no client calling it, or a client/hook with no UI
surface using it, is an incomplete implementation, not a finished one with polish pending
(same principle as `ui-transparency` above, generalized to any producer/consumer pair, not
just backend→UI visibility). Before adding a new endpoint, identify what will call it in this
same change; before adding a client method/hook, identify what will render it. If a richer
mechanism is added to replace a simpler one (e.g. a unified request API superseding a
per-device CRUD API), retire the superseded side's client, routes, and tests in the same
piece of work — don't leave the old routes live-and-tested with a UI that quietly moved on,
even though nothing is technically broken. Precedent: BL-41 (`docs/BACKLOG.md`) — a
"Device Sessions API" (`/ev-session`, `/heater-target`, `/shiftable-loads`,
`/baseline-override`) stayed live, mounted, and BDD-tested for an unknown period after the UI
fully migrated to the richer unified `/user-requests` flow that constructs the same underlying
domain objects; found only by an unrelated dead-code audit, not by anyone noticing the drift.

refactoring: before adding a feature in an area listed in docs/reference/TECHNICAL_DEBTS.md,
check that file first. If the relevant debt is Small or Trivial effort, refactor it before
adding new behaviour. All tests must pass before and after any refactor. Record newly
discovered debt in TECHNICAL_DEBTS.md immediately — do not let debt accumulate silently.

When researching about OpenADR reference, only use OpenADR 3 resources. General Questions can be researched from any versions.

Do not add co-authoring footers to commit messages or PR descriptions. they might get rejected.

Only consider upstream PR and commits after the code is tested completely without failure and the commits are ready for the upstream CI acceptance tests.
After creating upstream PR, wait for the CI to actually run and report before drawing any conclusions about main branch being pre-broken. If anything fails, we investigate it properly rather than writing it off.

test failures: Do NOT distinguish between "pre-existing" and "new" failures — that distinction is an excuse to not fix things. When a test fails, read the error and focus on what can be done: fix the code, fix the test, or fix the fixture. If a fixture has stale data (e.g. CRLF line endings, outdated snapshot), update it. If a test exercises a real invariant, make the code satisfy it. Passing tests are the goal, not categorizing blame.

It is also legitimate to question whether a test's purpose or form is still correct — requirements change, and a test's expectations can become wrong. It is also valid to ask whether a test could be done more cleanly in a different setup. However, this is NOT a free pass to delete or weaken a failing test. Any change to test purpose, form, or expectations must be explained to the user with the reasoning and the alternatives considered. Never silently change test expectations to make failures go away.

docs/openadr_3_1_specs/pdf/: do not read, search, or reference any files under this directory. Use the markdown versions in docs/openadr_3_1_specs/ instead.

error-handling: all cross-layer failures use the domain-owned DomainError enum
(VEN/src/entities/error.rs); translate technical errors to a domain variant at the
boundary where they occur; carry structured context (typed fields, not pre-flattened
Strings); terse one-line Display; remediation hints only in the component's own
vocabulary — deployment/config advice belongs at the presentation boundary. Full
rules: docs/guidelines/ERROR_HANDLING.md.

naming: variables and function names for physical quantities must include the unit as suffix (e.g. `power_kw`, `energy_kwh`, `temperature_c`, `tariff_eur_per_kwh`, `soc_pct`). When adding new code, check nearby code or nearby source files for existing suffixes to stay consistent.

ven-architecture: VEN/src/ follows Hexagonal + Clean Architecture. Dependency rule: inner rings NEVER import outer rings.

  Ring map (outer → inner):
    Adapters   : routes/, tasks/
    Application: services/
    Domain     : entities/, controller/
    Infra      : assets/, simulator/, vtn.rs, controller/milp_planner/

  Port obligations — use traits, never bypass with concrete types:
    SimulatorPort    : domain/services → simulator (snapshot, inject)
    SolverPort       : services → controller/milp_planner (solve)
    VtnPort          : services → vtn.rs (fetch programs/events/obligations)
    AssetMilpContext : milp_planner accepts Vec<Box<dyn AssetMilpContext>> — NEVER import A_BAT/A_EV/A_HTR directly

  Profile rule: no `use crate::profile` in entities/, controller/, or routes/. Profile values are
  injected as typed parameter structs (e.g. BatteryParams) constructed in the application/infra layer.

  File size: no VEN/src/ file > 500 production lines; tasks/ files must stay < 200
  production lines. "Production lines" = non-blank lines excluding #[cfg(test)] blocks
  and whole test-only files/directories (e.g. controller/milp_planner/tests/) — this is
  exactly what scripts/audit_file_sizes.py measures; run it to check compliance.
  Allowlisted exceptions (cohesive dispatch/glue code, not a line-count problem) are
  listed inside that script — currently just assets/mod.rs, whose real fix is the
  enum→trait refactor tracked in docs/plans/refactoring_backlog.md.

  Verifiable invariants — run before any VEN PR:
    grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes  → must be empty
    grep -r "use crate::assets::" VEN/src/controller/milp_planner --include="*.rs" | grep -v "cfg(test)\|tests/"  → must be empty
    grep "serde_json::Value" VEN/src/vtn.rs                                           → must be empty or internal only
    grep -r "use crate::assets::" VEN/src/entities                                   → must be empty

  Reference: docs/architecture/VEN_ARCHITECTURE.md and
  docs/architecture/module_dependency_graph.md