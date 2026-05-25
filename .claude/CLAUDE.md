docker: docker runs on ssh Pi4-Server. run all tasks with docker on Pi4-Server via ssh in directory /srv/docker/openadr_lab.

local-rust: WSL is installed on this Windows machine. Use `wsl cargo check` (or `wsl cargo test`) inside the VEN directory for local Rust compilation instead of native Windows cargo, which lacks cmake/HiGHS. For a full test run including HiGHS, use the Pi4-Server docker stack.

dto: avoid DTO normalization. pass through upstream field names (e.g. OpenADR spec names) across all layers — backend, BFF, UI. one vocabulary everywhere reduces boilerplate and debugging friction.

workflow: 1. always keep a project_journal.md in projects where you write for each large step what you did, why you did it and what issues/key-learnings you had. it shall explain, how the project was implemented. The journal lives at docs/history/project_journal.md.
2. write key learnings into KEY_LEARNINGS.md (at docs/reference/KEY_LEARNINGS.md) and consider them when making decissions.

NEVER stop docker containers that are not involved in this project without asking. They are productive containers.

branching: feature branches use the pattern NNN-whatever-case-description where NNN is the
openspec feature ID (e.g. 030-my-feature). Refactor branches: refactor/<slug>.
Fix branches: fix/<slug>. All those branches target main. Never force-push to main. The goal is to rebase and fast forward merge.
DCO sign-off is enforced by CI — do not add co-author footers (see rule above).
Merge only after all CI checks pass: cargo fmt, cargo clippy -D warnings, cargo audit,
file-size audit (tasks/ ≤ 200 lines), and E2E tests green on Pi4.

testing: full guide at docs/guidelines/TESTING.md. Four suites:
  1. UI unit (local)       — cd VEN/ui && npm test  |  cd VTN/ui && npm test
  2. Rust unit+integration — wsl cargo test -p ven  (local, no HiGHS needed for most)
  3. E2E BDD (Pi4)         — bash run_all_tests.sh --e2e  (behave, ~49 scenarios)
  4. Resilience (Pi4)      — bash run_all_tests.sh --resilience
Run everything: bash run_all_tests.sh
VEN Rust pyramid (4 layers, all must stay green after any VEN change):
  Domain → Use-case → Adapter-contract → Integration
  Shared mock adapters live in VEN/src/services/test_support/ (not cfg(test)).
  Test naming: test_<function>_<scenario>.
All suites must pass before merging a PR to main.

test-first: write the test first. Selectively run it to confirm it fails. Implement until it is green.
This applies at unit level for every new function and at BDD level for every new behaviour.

session-start: at the start of every AI session read docs/reference/SESSION_START.md
and follow the checklist before touching any code.

When researching about OpenADR reference, only use OpenADR 3 resources. General Questions can be researched from any versions.

Do not add co-authoring footers to commit messages or PR descriptions. they might get rejected.

Only consider upstream PR and commits after the code is tested completely without failure and the commits are ready for the upstream CI acceptance tests.
After creating upstream PR, wait for the CI to actually run and report before drawing any conclusions about main branch being pre-broken. If anything fails, we investigate it properly rather than writing it off.

test failures: Do NOT distinguish between "pre-existing" and "new" failures — that distinction is an excuse to not fix things. When a test fails, read the error and focus on what can be done: fix the code, fix the test, or fix the fixture. If a fixture has stale data (e.g. CRLF line endings, outdated snapshot), update it. If a test exercises a real invariant, make the code satisfy it. Passing tests are the goal, not categorizing blame.

It is also legitimate to question whether a test's purpose or form is still correct — requirements change, and a test's expectations can become wrong. It is also valid to ask whether a test could be done more cleanly in a different setup. However, this is NOT a free pass to delete or weaken a failing test. Any change to test purpose, form, or expectations must be explained to the user with the reasoning and the alternatives considered. Never silently change test expectations to make failures go away.

docs/specs/pdf/: do not read, search, or reference any files under this directory. Use the markdown versions in docs/specs/ instead.

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

  File size: no VEN/src/ file > 500 lines. tasks/ files must stay < 200 lines.

  Verifiable invariants — run before any VEN PR:
    grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes  → must be empty
    grep -r "use crate::assets::" VEN/src/controller/milp_planner --include="*.rs" | grep -v "cfg(test)\|tests/"  → must be empty
    grep "serde_json::Value" VEN/src/vtn.rs                                           → must be empty or internal only
    grep -r "use crate::assets::" VEN/src/entities                                   → must be empty

  Reference: docs/plans/ven_backend_architecture_refactoring.md