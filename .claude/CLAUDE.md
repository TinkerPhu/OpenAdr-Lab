<!-- SPECKIT START -->
For additional context about technologies to be used, project structure,
shell commands, and other important information, read the active plan under
docs/plans/ — entry point: docs/plans/strategic_roadmap.md (phase plans in
docs/plans/roadmap/). The documentation content rule (current state + future
visions only, with its exempt list) is recorded in
docs/reference/DOCUMENTATION_STYLE.md.
<!-- SPECKIT END -->

one-concept-one-function (the rule above all others for code; older code and docs cite it as
`generic-over-bespoke`): every concept this project computes has exactly ONE implementation, and
every caller calls it. The recurring root cause of this project's bugs is the same logic living in
several places, each copy a little different (GB-48: "when does interval i of an OpenADR event run"
was decided in seven parsers with four rules). The differences between copies ARE the bugs.
  - Search before you write: grep for the concept (domain words, spec field names, UI label, unit
    suffix, the shapes it takes), not for the name you would give your function. "I didn't find it"
    must mean "I searched for it".
  - Found once: call it; if it does not quite fit, change the shared function (with its tests).
    Never copy-paste-and-adapt.
  - Found twice or more: consolidate into one shared function in the same piece of work, delete the
    copies, pin the unified behaviour with tests. Never add a third copy "for now".
  - Generalise the primitive, not the call site: when several callers solve one shape with
    near-identical helpers, push the general pattern into the shared component.
  - Every plan and openspec design names the existing functions it reuses or consolidates. Copies you
    cannot consolidate in reach go into docs/reference/TECHNICAL_DEBTS.md immediately.

env: the repo needs no lab host (fresh clone = local docker). This lab sets `OPENADR_LAB_HOST=Node2`
(or Node1) and `CI_HOST=Node3`; `ci/ci-handoff.sh` and `ci/ci-next.sh` refuse to run without CI_HOST.
All CI/CD lives in `ci/` (ci/README.md). Docker runs on Node1 (primary: VTN, DB, BFF, VTN UI, ven-1..3)
or Node2 (secondary: ven-4..20 and build/test offload), in /srv/docker/openadr_lab on both. Node3 is
the CI host. Details, compose service names and build commands: docs/guidelines/TESTING.md.

session-start: at the start of every AI session read docs/reference/SESSION_START.md and follow it
before touching any code.

hosts-and-locks (full mechanics: docs/guidelines/TESTING.md "Host locks, checkouts and builds"):
  - Node1, Node2, Node3 and the local WSL are each shared by parallel sessions. Before ANY docker
    build/test on a host, or any large WSL cargo command, take that machine's lease lock and hold it
    for the whole sequence: `bash ci/locks/docker_host_lock.sh acquire -m "<branch>: <what>" [-l MIN]`
    (`LOCK_HOST=Node2` for another host), `bash ci/locks/wsl_lock.sh acquire ...` for WSL; `release`
    at the end, `refresh` if a run overshoots, `status` to read it. `acquire` blocks ~9 min then exits
    2: rerun it, and report to the user if a lock stays held unusually long. Never bypass a lock.
  - one-checkout-per-host: one working tree per host: `run_all_tests.sh` does NOT switch branches. Under the lock, put the
    host on your branch and verify it (`git fetch && git checkout <b> && git reset --hard origin/<b>
    && git log -1`). Check the lock is FREE or yours in a separate step before touching a host tree.
  - test-host-preference: prefer Node2 for build/test and E2E; use Node1 only when Node2 is unavailable or a test needs the
    production stack. NEVER stop containers that do not belong to this project without asking.
  - Ask the user before resets, deploys or other changes to a host's checkout that they did not ask for.

ci-handoff (skill `ci-handoff`, docs/guidelines/TESTING.md "CI on Node3"): Node3 runs the CI runner,
which tests branches and is the only thing that merges to main. When work is done and the quick local
checks are green: commit the journal and KEY_LEARNINGS entries WITH the code, `git push -f origin
HEAD:<branch>`, `bash ci/ci-handoff.sh`, then start the `bash ci/ci-wait.sh <ticket> --ack` it prints
as a BACKGROUND command (never poll or sleep) and end the turn. Do not run the full suite yourself and
do not merge to main. Merged: delete the branch and worktree, return any host checkout to main, report.
Red/conflict/stale: fix and hand off again. A `--kind check` hand-off of work in progress does not end
the task. Deploys only when the user asks: `ci-handoff.sh --deploy <host> <stack> "<services>"`.
If the runner is not running (`ssh Node3 systemctl is-active ci-runner.timer`), merge the old way.

no-needless-runs: every build and test run costs minutes to hours on shared hardware. Do not repeat a
check that passed on the same commit; do not start a run while one is in flight; look for warm caches;
do not run the Rust or E2E suites for a change they cannot see (docs, wiki, openspec: ci/suites.map
decides); run the narrowest test that proves the change and leave the full gate to the runner. When a
run is lost to the environment, find out why before starting it again.

local-rust and memory-budget: use WSL for local Rust (native Windows cargo lacks cmake/HiGHS), through a
login shell and a script file so the real exit code survives:
`MSYS_NO_PATHCONV=1 wsl bash -l /mnt/c/.../scratchpad/cargo.sh <log> <crate-dir> <cargo args>`. The
laptop has little RAM: check free memory first (`Get-CimInstance Win32_OperatingSystem`), always pass
`-j 2`, wait or ask below ~1 GB free, and hold the WSL lock for the whole sequence.

workflow:
  1. Keep docs/history/project_journal.md: for each large step, what you did, why, and the issues
     and key learnings.
  2. Write key learnings into docs/reference/KEY_LEARNINGS.md and consider them when deciding.
  3. No lingering plans. The whole openspec/ folder is change-management scaffolding, not
     documentation. Once a plan (docs/plans/) or openspec change is implemented and tested, wave its
     feature into current-state docs (mechanism -> docs/architecture/*.md, user-observable behaviour ->
     docs/use-cases/*.md, new files only when none fits) and delete the plan/change (no archiving).
     Point to test files instead of copying SHALL/Scenario prose. Narrative of implementation issues
     is not carried forward; durable lessons go to KEY_LEARNINGS.md. Partial completion removes only
     the done part.
  4. A user-facing use case gets documented in docs/use-cases/*.md AND a BDD scenario in
     tests/features/ in the same piece of work, not a follow-up.
  5. After a merge, switch Node1/Node2 checkouts back to main and delete the merged branch and any
     worktree (`git worktree remove`, `git worktree prune`) right away.

branching and commits: feature branches `NNN-description` (openspec feature id); `refactor/<slug>`;
`fix/<slug>`; all target main. Rebase and fast-forward merge only (no PRs for this repo, never force
push main). Commits are signed off (-s); add NO co-author footer to commits or PR descriptions.
Merge gate (the Node3 runner runs it): cargo fmt, `cargo clippy --all-targets --all-features -- -D
warnings`, cargo audit, ci/audits (file sizes, duplication ratchet, debt gate, VEN architecture, and
step definitions when tests/features changed) and E2E. Run `python ci/audits/audit_duplication.py`
before handing off: a branch must not add or grow a duplicated cluster or a Rust function over 100
lines; `--update` only after reducing. Run `python ci/audits/audit_step_definitions.py` before any
change under tests/features (an undefined or ambiguous step aborts the whole E2E run; use `{x:g}`
for numeric arguments).

testing (docs/guidelines/TESTING.md): UI unit (`npm test` in VEN/ui and VTN/ui), Rust unit and
integration, E2E BDD, resilience. Test-first: write the test, run it selectively to see it fail,
implement until green; at unit level for every new function and at BDD level for every new behaviour.
Test naming `<function>_<scenario>`, no `test_` prefix. VEN Rust pyramid: Domain, Use-case,
Adapter-contract, Integration; shared mocks in VEN/src/services/test_support/. Linting: fmt and clippy
as above before any commit; eslint with zero errors for the UIs; `#[allow]` only with a same-line reason.

test failures: do NOT split "pre-existing" from "new" failures; read the error and fix the code, the
test or the fixture. A test's purpose may be questioned, but never weaken or delete a failing test
silently: explain the change and the alternatives to the user.

determinism: any code path depending on the current time takes an injectable clock; new modules that
schedule, timestamp or expire follow the same pattern.

dependencies: pin new crates to a semver range, npm packages with caret ranges (commit
package-lock.json, never delete it); run cargo audit and npm audit before a release and file findings
in docs/BACKLOG.md. Allowed licences: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-3.0, Zlib,
CDLA-Permissive-2.0, OpenSSL (aws-lc-sys). Review every new import.

dto: no DTO normalisation; pass upstream field names (OpenADR spec names) through all layers.

wire-contracts (profile: docs/reference/WIRE_PROFILE.md): an optional field in an external protocol is
mandatory here. Emit strictly: never send a value whose quantity, unit, sign and reading type are not
declared on the wire (payloadDescriptors, reportDescriptors). Accept explicitly: read the declaration
that accompanies a value; if a conformant peer omits it, apply the documented profile default and
surface that you did; never reject a peer for omitting what the spec lets it omit. A request is a
guarantee: what a peer asks for is what we send, or we fail visibly. Express the contract in protocol
fields first; extensions are namespaced, versioned and additive. The contract lives on the wire plus
the profile document, not in a second copy in code. Make it structural: constructors that cannot build
an undeclared payload, one validator, a test that fails when an object goes out without its contract.

ui-transparency and no-half-built-features: every backend capability, external feed and piece of
derived state needs a visible surface in the VEN UI or VTN UI, planned in the same piece of work.
Never build one side of a producer/consumer pair without the other. When a richer mechanism replaces a
simpler one, retire the old routes, client and tests in the same change.

refactoring and issues: check docs/reference/TECHNICAL_DEBTS.md before adding a feature in a listed
area; Small or Trivial debt is refactored first. Every open issue (R-*, GB-*, BL-*) carries Severity
(S1-S4), Kind, Cost and Why-open (docs/reference/ISSUE_KINDS.md). Cost under ~1 hour: fix it in the
branch, do not file it. Cost Small or more: file it with all four fields (Why-open: needs-decision |
needs-evidence | too-big, naming the first step). Nothing is deleted for age. A branch's final report
states issues filed, fixed and deferred. `python ci/audits/audit_debt_gate.py` lists open Small/Trivial
rows whose `Affected files` the branch touched; fix them or add a commit line
`Debt-deferred: R-NN: <why not now>`.

error-handling (docs/guidelines/ERROR_HANDLING.md): cross-layer failures use the domain-owned
`DomainError` (VEN/src/entities/error.rs); translate technical errors at the boundary where they occur;
typed context, terse one-line Display, remediation hints only in the component's own vocabulary.

naming: physical quantities carry a unit suffix (`power_kw`, `energy_kwh`, `temperature_c`,
`tariff_eur_per_kwh`, `soc_pct`); power is kW everywhere. Use the same word for the same concept across
a module, and reuse the UI label's wording in routes, DTOs and function names so a UI string greps
straight to its code. Declare per-case behaviour as data where the case is declared; do not branch on
it in shared code (a single guard clause or one-off decision is still a plain `if`).

file-headers: when you edit a VEN module with no header comment, give it one (what it decides or owns,
what it reads, what it must never do). Only as files are touched.

asset-competence-assurance: interpretation of an asset's state or forecast has exactly one authority:
the asset itself. Infrastructure may acquire and share raw data, but it flows into the asset as an
injected parameter. No other module computes its own answer for a live or future value (immutable
history is the exception). When you find a violation, sweep every other asset kind and the surroundings
(controller, arbiter, dispatcher, planner inputs, routes, test fixtures; fixtures build assets from the
real asset, never a hand-built capability). Fix what is in reach, register the rest in TECHNICAL_DEBTS.md.
See docs/architecture/VEN_ARCHITECTURE.md section 3.0d.

ven-architecture (docs/architecture/VEN_ARCHITECTURE.md, module_dependency_graph.md): VEN/src follows
Hexagonal + Clean Architecture; inner rings never import outer rings. Adapters (routes/, tasks/),
Application (services/), Domain (entities/, controller/), Infra (assets/, simulator/, vtn.rs,
controller/milp_planner/). Reach infra through ports (SimRosterPort/SimReadPort/HeadroomPort via
SimHandle, SolverPort, VtnPort, AssetMilpContext), profile values are injected as typed parameter
structs, adapters read the clock once and pass `now` down. Size caps: no VEN/src file over 500
production lines, tasks/ files under 200 (audit_file_sizes.py, empty allowlist). The complete list of
checked invariants is `python ci/audits/audit_ven_architecture.py` (run it before any VEN change):
read its output and its source for the rules, do not rely on memory of them. Key ones: incoming event
numbers are read only through `lab_core::wire_contract::PayloadReader`; unit conversion only in
`entities/units.rs`; no asset kind in the planner core; every VTN request through
`ClientCredentials::send`.

Research and external rules:
  - OpenADR research uses OpenADR 3 sources only; other questions may use any source.
  - Do not read, search or reference docs/openadr_3_1_specs/pdf/; use the markdown in
    docs/openadr_3_1_specs/.
  - Upstream PRs (openleadr-rs) only after everything is tested green and the commits are ready for the
    upstream CI; after creating one, wait for its CI and investigate any failure properly before
    concluding anything about main.

NEVER stop docker containers that are not part of this project without asking.

interview: when the user asks to be interviewed ("interview me", "interview me about X", "interview me for
all open points") follow this protocol without being told how. The goal is that all information has flowed
in both directions before anything is decided.
  1. Prepare to the scope of the request. A named topic or the current discussion: stay on it, look up only
     what bears on it, ask only for confirmation of what you find. A broad request: sweep the open points in
     docs/plans/, docs/REQUIREMENTS.md, docs/BACKLOG.md and the journal. Unclear scope: the first question
     is "which topic?" with likely candidates as options. Look up facts before asking; put questions that
     unlock others first.
  2. Small batches: at most 4 questions at a time, each self-contained (the user may see only the question),
     with its context in one or two lines.
  3. 2-4 distinct options per question, each with pros and cons; the recommended one first, marked
     "(Recommended)" with a one-line reason. Where a sensible default exists, state it and proceed.
  4. Side questions are always welcome: answer or discuss them fully first, without steering back early;
     then say where the interview stands and re-ask the open question of the batch, restated if the
     discussion changed its premises.
  5. Re-evaluate: if an answer or side discussion changes later options or recommendations, revise them and
     say so.
  6. Record each answer at once in the file the topic belongs to, mark who decided it, and remove the open
     point once answered.
  7. A suggestion is never a decision. Only an explicit answer decides; "sounds good" to a side comment
     leaves the question open. "As recommended" / "go with your recommendations" is an explicit answer: it
     covers the current batch, or the remaining batches too when stated for the whole interview; mark the
     decider as "user, as recommended". If a recommendation changed during a side discussion, restate the
     current one before applying it.
  8. Close with a summary: what was decided, what is still open, what changed in which file. The user can
     overrule any decision.
  Do not ask what you can look up.

explain: when the user asks for an explanation ("explain ...", "what does ... do", "why is ...", "how does
... work"), including a side question inside an interview, follow this protocol without being told how.
  1. Assume the user does not know the code: start from what the thing is for, then how it works.
  2. Always give 1-2 examples from this project (a real file, value or situation from the current
     discussion), not a textbook example. If the context has no fitting case, say so and build the closest
     realistic one from it.
  3. Explain every code element you mention (function, module, class, variable, flag, config key, file,
     command) at first mention: its purpose, and its semantics (what it takes, returns or changes, what its
     values mean). Never leave a name unexplained because it looks self-explanatory.
  4. Stay on the question, at the depth asked; offer to go deeper instead of adding background.
  5. Say what is inferred and not verified in the code; never present a guess as fact.
