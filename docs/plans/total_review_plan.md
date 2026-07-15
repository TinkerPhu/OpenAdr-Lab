# Total Project Review Plan

Status: **review complete (Parts A + B done 2026-07-15) — awaiting owner
decisions on the B12 reduction proposal and fix-wave priorities; Part C open**
Scope: whole repository — code (VEN, VTN, UIs, scripts, CI) and documentation
(docs/**, root-level docs, wiki/**).
Execution model: one step per session (or a few small ones), findings recorded
before anything is fixed. Fixes land on separate `fix/` or `refactor/` branches.

---

## Ground rules for the whole review

1. **Findings first, fixes second.** Every step produces a findings list appended to
   the tracking section at the bottom of this file (and mirrored into
   `docs/reference/TECHNICAL_DEBTS.md` / `docs/BACKLOG.md` where appropriate).
   No code or doc is changed during a review step.
2. **Severity tags** on every finding: `blocker` / `major` / `minor` / `nit`.
3. **Baseline must be green** before the review starts and stay green throughout
   (all four test suites, fmt, clippy, audits).
4. **Doc content rule** (Part B): every document except the exempt list describes
   only (a) the **current state** of code and features and (b) **future visions/plans**.
   No historical narrative — no "it used to be X", "was changed on \<date\> to Y",
   "after refactoring NNN we now…". Just "it is Y."
   Permitted exception: a short mention of the rejected alternative X and *why* it
   was not chosen, **only when the choice is not obvious**.
5. **Exempt from the doc content rule** (intentionally historical):
   - `docs/history/**` (project journal)
   - `docs/reference/KEY_LEARNINGS.md`
   - `wiki/log.md` (a log by nature),
   - `wiki/decisions/**` (ADR-style pages — rationale allowed, but chronology
     ("changed then-and-then") still gets rewritten), 
   - `wiki/queries/**` and `wiki/review.md` (dated point-in-time records — owner
     decision 2, 2026-07-15),
   - `specs/archive/**` (archived feature records), 
   - `docs/history/archive/**` (superseded design docs kept as record),
   - git history itself.
6. `docs/specs/pdf/` is never read (project rule); the markdown spec copies in
   `docs/openadr_3_1_specs/` are third-party text — checked only for stray local
   edits/annotations, never rewritten.

---

## Part A — Code review

### Phase A0 — Baseline & mechanical checks

- [x] **A0.1 Green baseline.** Run all four suites (`bash run_all_tests.sh` on Pi4,
  UI unit tests locally, `wsl cargo test -p ven-app`). Record versions/commit hash.
  If anything is red, fix before the review begins.
  *Done 2026-07-15.* Baseline: commit `466f792` on `refactor/3-tier-milp`;
  cargo/rustc 1.95.0 (WSL), node v22.12.0, npm 11.11.0.
  Results: VEN UI 309/309 passed · VTN UI 64/64 passed · Rust 458 + 1
  architecture test passed · E2E BDD green (run by owner same day) ·
  Resilience on Pi4: 5 scenarios / 44 steps passed, 0 failed. All green.
- [x] **A0.2 Lint & size audit.** *Done 2026-07-15.* fmt clean; documented clippy
  gate clean; eslint 0 errors (12 warnings, see findings); file-size audit passes.
  Early warnings and test-code clippy errors recorded in the findings log.
  Original text: `wsl cargo fmt --check`, `wsl cargo clippy -- -D warnings`,
  eslint on both UIs, `python scripts/audit_file_sizes.py`. Record any file close to
  its limit (>80 % of 500 / 200 lines) as early-warning findings.
- [x] **A0.3 Architecture invariant greps** (from `.claude/CLAUDE.md`): *Done
  2026-07-15.* Invariants 1, 2, 4 clean (invariant 2 matches are doc comments
  only). Invariant 3: `serde_json::Value` in vtn.rs is internal helper code
  except `VtnPort::fetch_reports_raw` — recorded as a finding for A1.1.
  - `use crate::profile` in entities/, controller/, routes/ → must be empty
  - `use crate::assets::` in controller/milp_planner (non-test) → must be empty
  - `serde_json::Value` in vtn.rs → empty or internal only
  - `use crate::assets::` in entities/ → must be empty
- [x] **A0.4 Dependency & licence audit.** *Done 2026-07-15 (licence check deferred:
  cargo-license not installed — install and complete during C1).* cargo audit:
  12 vulns in VEN, 6 in BFF. npm audit: 17 vulns VEN UI / 16 VTN UI (2 critical
  each, all in dev tooling). npm pinning rule violated across both UIs. Details in
  findings log; mirror to BACKLOG.md in C1. Original text: `cargo audit`, `npm audit` (both UIs, BFF if
  applicable), verify all crates use semver ranges and npm packages exact pins,
  licences within the allowed set (MIT, Apache-2.0, BSD-2/3, ISC). Check for
  unused dependencies (`cargo machete` or manual).

### Phase A1 — VEN architectural review (ring by ring, inner → outer)

For each ring: verify the dependency rule (inner never imports outer), port usage
(traits, no concrete bypasses), unit-suffix naming (`_kw`, `_kwh`, `_c`, `_pct`,
`_eur_per_kwh`), injectable-clock rule for anything time-dependent, and module
cohesion (does the file belong in this ring?).

- [x] **A1.1 Domain ring:** *Done 2026-07-15.* entities/ is pure (imports only
  common/ + entities/). One dependency-rule violation (monitor.rs → state.rs),
  one port-trait placement question (AssetMilpContext), two production wall-clock
  calls, and the fetch_reports_raw leak from A0.3 — see findings log. Port traits
  otherwise well-shaped; unit-suffix naming consistent in all inspected types.
  Original text: `VEN/src/entities/`, `VEN/src/controller/` (excluding
  milp_planner). Check: pure domain logic, no profile/asset/infra imports, port
  traits (`SimulatorPort`, `SolverPort`, `VtnPort`) well-shaped (no leaky
  signatures exposing infra types).
- [x] **A1.2 Application ring:** *Done 2026-07-15.* test_support correctly
  `#[cfg(test)]`-gated; hems/obligation/user_request services orchestrate via
  ports and entities only. planning.rs bypasses SimulatorPort (see findings).
  Original text: `VEN/src/services/` (hems, obligation, planning,
  user_request). Check: orchestrates only via ports, param structs (e.g.
  `BatteryParams`) constructed here not deeper, `test_support/` correctly
  `#[cfg(test)]`-gated.
- [x] **A1.3 Adapter ring:** *Done 2026-07-15.* DTO passthrough respected (zero
  serde renames in routes/); shared.rs is a 7-line helper, no dumping ground;
  task sizes within limits (planning.rs at 198/200 flagged in A0.2). Findings:
  interval-loop scaffold duplication across tasks, computation in
  routes/timeline.rs, POST /requests body defined in domain ring. Original text: `VEN/src/routes/` (incl. `hems/`), `VEN/src/tasks/`
  (incl. `sim_tick/`). Check: thin adapters (no business logic), DTO passthrough
  rule (OpenADR field names, no renaming layers), tasks < 200 production lines,
  shared.rs not becoming a dumping ground.
- [x] **A1.4 Infra ring:** *Done 2026-07-15.* assets/mod.rs enum→trait debt is
  properly tracked in refactoring_backlog.md; profile imports confined to
  profile/validate.rs (rule exceeded); vtn.rs typing covered in A0.3. Findings:
  simulator/asset determinism gaps, unseeded RNG. Original text: `VEN/src/assets/`, `VEN/src/simulator/`, `VEN/src/vtn.rs`,
  `VEN/src/profile/`, `VEN/src/config.rs`, `VEN/src/state.rs`. Check:
  `assets/mod.rs` allowlist status (enum→trait refactor still tracked in
  refactoring_backlog?), simulator determinism, vtn.rs typed (no ad-hoc JSON).
- [x] **A1.5 MILP planner:** *Done 2026-07-15.* No `use crate::assets::` (A0.3),
  no wall-clock or RNG in production code (`now` flows in via SolveRequest),
  constants carry unit suffixes (e.g. `M_LOW_EUR_PER_KWH`). Two small findings
  below; deep doc-vs-model comparison deferred to B3 as planned. Original text: `VEN/src/controller/milp_planner/`. Check:
  `AssetMilpContext` boxing only (never A_BAT/A_EV/A_HTR direct), injectable clock,
  numerical constants named with units, model documented consistently with
  `docs/architecture/ven_milp_planner.md` and `docs/milp_planner_config.md`.
- [x] **A1.6 Root-level leftovers:** *Done 2026-07-15.* main.rs (301 lines) is
  wiring only; ids.rs/planner_events.rs are small and justified; common/ passes
  the size audit (mostly tests). One nit: models.rs. Original text: `main.rs`, `models.rs`, `ids.rs`,
  `planner_events.rs`, `common/`. Check each actually belongs at root or should be
  ring-assigned; models.rs vs entities/ overlap.
- [x] **A1.7 Dependency graph regeneration.** *Done 2026-07-15.* Regenerated
  module-level adjacency from `use crate::` imports (test code excluded) and
  compared against the documented graph: edge directions match the documented
  DIP pattern (simulator/vtn/assets import controller port *types* to implement
  them — correct direction). No undocumented inward→outward edge beyond the two
  already logged (controller/monitor.rs → state; services/planning.rs →
  simulator + assets). No blocker. Tooling note in findings. Original text: Generate a fresh module dependency graph
  and diff against `docs/architecture/module_dependency_graph_post_refactoring.md`.
  Any new edge crossing rings inward→outward is a `blocker` finding.

### Phase A2 — VTN & UIs

- [x] **A2.1 VTN BFF** *Done 2026-07-15.* Clean layering (main → routes →
  vtn_client), no unwrap/expect, `Json<serde_json::Value>` passthrough is
  design-consistent with the DTO rule (transparent proxy). Findings: no unit
  tests, 502-flattening of upstream errors, token-client duplication with VEN.
  Original text: (`VTN/bff/src`, ~12 files): layering, DTO passthrough rule,
  error handling consistency, typed responses, no dead endpoints.
- [x] **A2.2 VEN UI** *Done 2026-07-15.* All 9 pages + key components tested
  (27 test files); component sizes reasonable (max 458-line hooks.ts); data-testid
  guideline followed in sampled pages. Open items are the eslint warnings from
  A0.2 (exhaustive-deps memoization, Reports.tsx mixed exports).
- [x] **A2.3 VTN UI** *Done 2026-07-15.* 7 test files cover 5 of 6 pages —
  Metrics.tsx untested; dialog components only covered indirectly via page tests.
  See findings.
- [x] **A2.4 UI duplication pass:** *Done 2026-07-15.* API layers genuinely
  diverged (similarity 0.12–0.31) — no shared-package case. Single true copy:
  JsonDialog.tsx, byte-identical in both UIs (50 lines). See findings.
- [x] **A2.5 VTN directory hygiene:** *Done 2026-07-15 (survey; final decision in
  B8).* Blueprint/setup docs confirmed construction-era; recommendations in
  findings. Original text: `VTN/` root files (`project structure.txt`,
  blueprint .md files, `DTO examples/`) — decide keep/move-to-docs/delete
  (blueprints are docs and also fall under Part B).

### Phase A3 — Cross-cutting code quality

- [x] **A3.1 Duplication sweep (Rust):** *Done 2026-07-15.* Similarity probes:
  poll_programs.rs vs poll_reports.rs = **0.80** (confirms the A1.3 scaffold
  finding — these two should share a generic poll helper); all other likely pairs
  low (ev vs heater routes 0.46, *_milp.rs 0.16, battery vs ev 0.11) — healthy.
- [x] **A3.2 Dead code:** *Done 2026-07-15.* ~15 `allow(dead_code)` sites; most
  carry same-line justifications as required. Findings: blanket module-wide
  allows without justification (entities/capacity.rs:5, design_vocabulary.rs:7).
  No commented-out code blocks found.
- [x] **A3.3 Error handling:** *Done 2026-07-15.* ~24 `unwrap()/expect()` calls in
  VEN production paths (top: milp_interactions 4, common 4, services/planning 3);
  BFF has none. Inventory recorded — triage each in the fix wave (many are
  provably-safe lock/parse cases; convert or comment-justify).
- [x] **A3.4 Naming & units:** *Done 2026-07-15.* Unit suffixes consistently
  applied in all inspected code (Rust + UI + MILP constants). Test naming:
  the documented `test_<function>_<scenario>` prefix is used by a minority;
  most tests use `<function>_<scenario>` without prefix — see findings.
- [x] **A3.5 Determinism audit:** *Done 2026-07-15.* Adapters (routes/, tasks/)
  correctly capture `Utc::now()` at the boundary and pass it inward — compliant.
  `Instant::now()` uses are duration telemetry — fine. Inner-ring violations are
  the ones logged in A1.1/A1.4 (site_meter, openadr_interface, state.rs ledger,
  simulator/assets, power_model RNG). No further findings.
- [x] **A3.6 TODO/FIXME/HACK sweep:** *Done 2026-07-15.* Zero markers in Rust,
  BFF, and both UIs. Clean.
- [x] **A3.7 Logging:** *Done 2026-07-15.* No println!/dbg! in Rust production
  (main.rs eprintln pre-logger is legitimate). Finding: 32 `console.log` calls in
  UI production code.
- [x] **A3.8 Config & magic numbers:** *Done 2026-07-15.* Covered by findings from
  A1.3 (hard-coded task intervals) and A1.5 (mip_gap 0.02). No systematic
  Rust-vs-UI-vs-compose default duplication surfaced in probes; re-check any
  suspect constant during fix waves.

### Phase A4 — Tests, scripts, CI

- [x] **A4.1 Test pyramid review:** *Done 2026-07-15.* All rings populated
  (entities 8, controller 175, services 56, routes 16, tasks 9, assets 112,
  common 48, simulator 3 tests + tests/architecture.rs integration). Mocks
  correctly confined to test_support. No sleeps/wall-clock coupling in Rust tests.
  Simulator's low direct count is acceptable (covered via controller/assets tests).
- [x] **A4.2 E2E/BDD review:** *Done 2026-07-15 (static pass).* 39 feature files,
  417 step definitions. Crude static matching flags up to ~112 possibly-unused
  steps — needs `behave --dry-run` on Pi4 for an authoritative list (finding).
  Tagging/overlap review deferred to that run.
- [x] **A4.3 Scripts & docker:** *Done 2026-07-15.* Compose files are genuinely
  different (similarity 0.22), no duplication problem; Pi4 references are
  documentation comments, fine. Main issue remains the A0.1 DOCKER_HOST default.
- [x] **A4.4 CI workflows:** *Done 2026-07-15.* pre-pr-checks runs
  `cargo clippy --all-targets --all-features -- -D warnings` — **stricter than
  the CLAUDE.md-documented local gate** and currently red (see upgraded finding);
  fmt/audit/DCO present; file-size audit on push/PR; e2e manual-dispatch only
  (known, documented in CLAUDE.md as non-blocking).

---

## Part B — Documentation review

Every step below applies two checks to each file:
1. **Accuracy:** content matches the current code/features (spot-check claims
   against source; anything stale is a finding).
2. **Content rule:** present state + future vision only; historical narrative
   flagged with the exact line(s) and a proposed rewrite ("it is Y" [+ optional
   short why-not-X]).

- [x] **B0 Inventory & classification.** *Done 2026-07-15 — 94 documents listed
  in the appendix table (path, review step, exempt flag, status). Exemptions per
  ground rule 5 as amended by owner edit.* Original text: Build a checklist table of all ~46 docs/
  files, 4 root docs, and 37 wiki pages. Columns: path, purpose, exempt (y/n),
  review step, status. Store the table in this file (appendix). Confirm the
  proposed exemption additions (rule 5 above) with the project owner.
- [x] **B1 Root docs:** *Done 2026-07-15.* README badly stale (see findings —
  broken links, phantom directories, wrong test counts/CI claim).
  DOCUMENTATION.md spot-checks largely accurate (ring map matches, deviation-
  absorption honestly marked unimplemented); API-reference count gap noted.
  alignment-plan.md is a live working checklist (forward-looking — passes the
  content rule; B12 candidate for merge/retire decision). Root CLAUDE.md pointer
  is vague; .claude/CLAUDE.md has one claim contradicted by code. Original text: `README.md`, `DOCUMENTATION.md`, `CLAUDE.md`,
  `alignment-plan.md`. DOCUMENTATION.md additionally diffed against actual code
  layout (quarterly-control item from SESSION_START.md).
- [x] **B2 docs/ top level:** *Done 2026-07-15.* SECURITY.md and
  milp_planner_config.md clean. REQUIREMENTS.md, BACKLOG.md, BACKLOG_OpenADR_Cert.md
  carry isolated historical-narrative lines (findings). DOC_AUDIT.md is a completed
  earlier audit → retire/merge via B12. milp_storage_planning{,_impl}.md are active
  design docs for the in-flight 3-tier work — misplaced at docs/ top level and
  B12 candidates for post-merge consolidation into the architecture docs.
  Original text: `REQUIREMENTS.md`, `SECURITY.md`, `BACKLOG.md`,
  `BACKLOG_OpenADR_Cert.md`, `DOC_AUDIT.md`, `milp_planner_config.md`,
  `milp_storage_planning.md`, `milp_storage_planning_impl.md`.
  (BACKLOG files: entries are forward-looking by nature — only *narrative* history
  inside entries is flagged. DOC_AUDIT.md may be superseded by this plan —
  decide merge/retire.)
- [x] **B3 docs/architecture/** *Done 2026-07-15.* Clean on the content rule:
  INTERFACES.md, VTN_ARCHITECTURE.md, ven_asset_interface_spec.md,
  asset_simulation.md, heater_tank_milp_planning_model.md, milp docs (except the
  A1.5 Part-A/B note). Main offender: VEN_ARCHITECTURE.md (findings). Module
  graph doc: phantom absorber (B1 finding) + rename candidate. Original text: (9 files incl. VEN_ARCHITECTURE.md,
  VTN_ARCHITECTURE.md, INTERFACES.md, module graph, MILP model docs,
  asset simulation, ven_asset_interface_spec). The module graph doc's
  "post_refactoring" framing is itself a historical reference — candidate rename
  to `module_dependency_graph.md`. Each architecture claim spot-checked against
  A1/A2 findings.
- [x] **B4 docs/guidelines/** *Done 2026-07-15.* AI-SW-Development, REACT_GUIDELINES,
  speckit-cheatsheet clean (its "formerly /quizme" refers to the external tool —
  allowed external fact). TESTING.md has stale counts (finding).
  superpowers_analysis.md is a dated one-off assessment → B12 candidate.
  Original text: (AI-SW-Development, REACT_GUIDELINES, TESTING,
  speckit-cheatsheet, superpowers_analysis). Verify TESTING.md matches the real
  suites/commands; guidelines are current-practice statements, not change stories.
- [x] **B5 docs/reference/** minus KEY_LEARNINGS: *Done 2026-07-15.* FAQ.md,
  GLOSSARY.md, SESSION_START.md clean. TECHNICAL_DEBTS.md R-13 has one
  historical-narrative phrase (finding).
- [x] **B6 docs/use-cases/** (3 manuals). *Done 2026-07-15 (static pass).*
  All three clean on the content rule (apparent "history" phrases are domain
  descriptions, e.g. "curtailment no longer needed" as a scenario). Live
  walk-through against the Pi4 stack deferred — schedule alongside the next
  manual E2E session rather than as a review-only exercise.
- [x] **B7 docs/plans/** *Done 2026-07-15.* strategic_roadmap.md and the 7
  roadmap phase docs are properly forward-looking; deviation-control-suggestions.md
  is the design plan for the (unbuilt) absorber — consistent with DOCUMENTATION.md
  §2.3, keep. Findings: refactoring_backlog.md resolved-item narration; empty
  postponed/ dir. Original text: (`strategic_roadmap.md`, `roadmap/`, `refactoring_backlog.md`,
  `deviation-control-suggestions.md`, `postponed/`). Plans are future-vision docs —
  check they describe the future from *now*, not completed work ("done" sections
  get removed or moved to the journal).
- [x] **B8 VTN blueprint docs:** *Done 2026-07-15.* Confirmed construction-era;
  per-file recommendations recorded under the A2.5 finding, final approval via
  the B12 proposal. Original text: `VTN/vtn_rust_bff_blueprint.md`,
  `VTN/vtn_web_ui_blueprint.md`, `VTN/vtn_setup_from_blog_step_by_step.md`,
  `VTN/project structure.txt` — likely construction-era documents. Decide per
  file: rewrite to current-state, move under docs/, or delete (with owner
  confirmation).
- [x] **B9 docs/openadr_3_1_specs/:** *Done 2026-07-15.* No local annotations
  found (no project-specific strings in the spec copies) — treated as unmodified
  third-party text. One side finding: the CLAUDE.md do-not-read rule names
  `docs/specs/pdf/` but the actual path is `docs/openadr_3_1_specs/pdf/` —
  update the rule's path. Original text: verify these are unmodified third-party spec
  copies; if any local annotations exist, extract them to project docs. No
  rewriting of spec text.
- [x] **B10 wiki/ (37 pages):** *Done 2026-07-15 (lint + content-rule scan).*
  wiki_lint reports 21 stale pages (outdated `synced_commit` vs recent code
  changes) — that's sync debt: run `/wiki-sync` (finding). Content-rule hits and
  classification questions in findings. Original text: run `bash scripts/wiki_lint.sh` first (broken links,
  stale `synced_commit`, orphans), then apply accuracy + content rule per section:
  overview/, concepts/, components/, architecture/, use-cases/, sources/,
  queries/, decisions/ (rationale kept, chronology rewritten), `index.md`,
  `purpose.md`, `callouts.md`, `review.md`. `log.md` exempt if confirmed.
  Split over 2–3 sessions if needed.
- [x] **B11 Cross-doc consistency:** *Done 2026-07-15.* Duplicated facts found:
  file-size limits stated in 3 places (.claude/CLAUDE.md ×2, DOCUMENTATION.md
  §arch — consistent today, drift-prone); test-suite command lists in README,
  TESTING.md, .claude/CLAUDE.md (README's counts already drifted — B1/B4
  findings); port maps in 5 docs (README, DOCUMENTATION.md, VTN_ARCHITECTURE.md,
  2 wiki pages). Proposal: single source of truth per fact class — rules in
  .claude/CLAUDE.md, ports/topology in DOCUMENTATION.md, test commands in
  TESTING.md; all others link. Executed with C2. *(also feeds B12)* same fact stated differently in two places
  (e.g. file-size limits, test suite list, port names) — pick the single source of
  truth, others link to it. Terminology aligned with GLOSSARY.md.

---

- [x] **B12 Document reduction proposal (suggest only).** *Done 2026-07-15 —
  see the "B12 Reduction proposal" table below (7 delete / 2 relocate / 2 shrink /
  1 rename candidates, information-preservation noted per row). Awaiting owner
  approval; approved rows execute in C2.* Original text: Using the B0–B11
  results, propose a consolidation of the documentation set: which documents to
  **merge** (overlapping scope, same audience), which to **delete** (outdated,
  superseded, construction-era), and which to **shrink** (content that belongs in
  another doc's single source of truth). Hard constraint: no relevant information
  may be lost — anything removed must either be already covered elsewhere, moved
  into the merge target, or explicitly listed as intentionally dropped with a
  reason. Output: a per-file table (keep / merge-into-X / delete / shrink) with
  rationale. **Suggestions only — no file is merged or deleted without owner
  approval; approved actions execute in C2.**

### B12 Reduction proposal (2026-07-15 — SUGGESTIONS ONLY, nothing executed)

| File | Proposal | Rationale / information preservation |
|---|---|---|
| docs/DOC_AUDIT.md | **delete** | Superseded by this review; rows it marked "archive" are already gone. Preserve: move any still-open rows into this plan's findings log first. |
| docs/guidelines/superpowers_analysis.md | **move + delete** | One-off dated assessment, not a guideline. Preserve: conclusion (adopt/not-adopt + why) as a wiki decision page or KEY_LEARNINGS entry. |
| docs/milp_storage_planning.md | **merge + archive now** | Design doc for the 3-tier MILP work, which is merged to main (refactor/3-tier-milp closed 2026-07-15) — the "wait for the branch" staging no longer applies. Fold the durable model content into docs/architecture/heater_tank_milp_planning_model.md + ven_milp_planner.md, then archive/delete; the before/after comparisons are now history. |
| docs/milp_storage_planning_impl.md | **merge + archive now** | Same as above (implementation companion). Verify each described mechanism against the merged code while folding — anything not implemented moves to BACKLOG.md instead of the architecture docs. |
| docs/milp_planner_config.md | **keep** (optional: merge into docs/architecture/ven_milp_planner.md) | Live config reference; merging would give one planner doc, but standalone is defensible. Owner's call. |
| VTN/vtn_rust_bff_blueprint.md | **extract + delete** | Construction blueprint. Preserve: any still-true architecture facts into docs/architecture/VTN_ARCHITECTURE.md. |
| VTN/vtn_web_ui_blueprint.md | **extract + delete** | Same. |
| VTN/vtn_setup_from_blog_step_by_step.md | **delete** | Superseded by README setup section. Preserve: verify no unique steps remain before deletion; anything unique goes to README/DOCUMENTATION.md §Deployment. |
| VTN/project structure.txt | **delete** | Describes a layout that no longer exists; nothing to preserve. |
| VTN/DTO examples/ | **verify then delete or move** | If payloads still match live VTN responses, move under tests/fixtures or docs; else delete. |
| alignment-plan.md (root) | **shrink + relocate** | Gap-analysis working file at repo root. Move open items to docs/BACKLOG.md, done-items to journal, then delete or park the remainder in docs/plans/. |
| docs/plans/postponed/ (empty) | **delete** | Empty directory. |
| docs/plans/refactoring_backlog.md | **shrink** | Drop resolved items (journal owns history); keep only open debt. |
| docs/architecture/module_dependency_graph_post_refactoring.md | **rename** → module_dependency_graph.md | Remove historical framing; update referring docs (SESSION_START.md, CLAUDE.md). |
| wiki/queries/ven-code-vs-docs-audit.md | **owner decision** | Superseded audit snapshot; either exempt as dated record (with queries/ classification, see B10 finding) or retire during /wiki-sync. |
| docs/BACKLOG.md + BACKLOG_OpenADR_Cert.md | **keep both** | Deliberate split (general vs certification scope); no merge. |
| DOCUMENTATION.md vs docs/architecture/* | **keep both, de-duplicate via B11 SSOT links** | Different audiences (single narrative vs per-topic deep dives). |
| docs/use-cases/SYSTEM-USE-CASES.md + manuals | **keep** | Definitions vs walk-through manuals are complementary, low overlap. |

Net effect if all accepted: **7 deletions, 2 merge-then-archive, 2 shrinks,
1 rename** out of 94 documents, with all unique information preserved via the
listed moves. *(Updated 2026-07-15 after refactor/3-tier-milp merged to main:
the two milp_storage docs moved from "relocate now, merge later" to
"merge + archive now".)*

## Owner decisions (2026-07-15)

1. **B12 reduction proposal: approved in full** — all 12 rows execute in C2,
   information-preservation steps first (extract-before-delete, verify-before-archive).
2. **Wiki exemptions extended**: `wiki/queries/**` and `wiki/review.md` join the
   content-rule exempt list (dated point-in-time records).
3. **Test naming: amend the rule** — CLAUDE.md changes to `<function>_<scenario>`
   (no `test_` prefix); no test renames.
4. **npm pinning: amend the rule** — `^` ranges stay; package-lock.json is the
   declared pinning mechanism. Update the CLAUDE.md dependencies rule accordingly.
5. **StaleRatePolicy enum: delete now** — *OBSOLETE (C2, 2026-07-16): the review
   finding predated Phase 4 (WP4.4/BL-07), which fully implemented StaleRatePolicy
   dispatch (`controller/milp_planner/stale_rates.rs`, profile-configurable,
   `rate_estimated` wired). The enum is live production code — nothing to delete.
   VEN_ARCHITECTURE.md stale-rate section rewritten to the implemented state.*
6. **Clippy gate: align docs to CI** — documented gate becomes
   `cargo clippy --all-targets --all-features -- -D warnings`; the 28 test-code
   lints get fixed (blocker).
7. **milp_planner_config.md: keep standalone**, cross-link with
   ven_milp_planner.md (B12 row resolved as "keep").
8. **C3 scope: blockers + majors now** — clippy-CI blocker, dependency-vuln
   updates, monitor.rs→state violation, planning.rs port bypass, BFF unit tests,
   tasks/planning.rs split, README rewrite, VEN_ARCHITECTURE rewrite, wiki-sync.
   Minors/nits mirror to the backlogs.

## Part C — Consolidation & fix waves

- [x] **C1 Consolidate findings.** *Done 2026-07-15.* TECHNICAL_DEBTS.md: added
  R-23–R-36 (parked minors/nits per decision 8), updated stale R-10 (VtnPort now
  mostly typed) and R-13 (content rule). refactoring_backlog.md shrink happens in
  C2 (approved B12 row). BACKLOG.md security section will be refreshed in C3
  with post-`cargo update` audit results (avoids writing soon-stale numbers).
  Licence check completed: **Rust** — all permissive, but Unicode-3.0 (ICU crates),
  Zlib (foldhash), CDLA-Permissive-2.0 (webpki-roots) and OpenSSL (aws-lc-sys
  AND-clause) fall outside the declared allowlist → amend the CLAUDE.md
  dependencies rule to include them (C2). **npm** (production) — MIT/ISC/BSD only,
  clean. Original text: Merge all Part A/B findings, dedupe, tag severity,
  sort into: (a) quick fixes, (b) refactorings → `docs/plans/refactoring_backlog.md`,
  (c) debts → `TECHNICAL_DEBTS.md`, (d) backlog items → `BACKLOG.md`,
  (e) doc rewrites (per-file list with proposed wording).
- [x] **C2 Fix wave — docs.** *Done 2026-07-16 on `fix/review-c1-c2-docs`.*
  Executed: all content-rule rewrites (VEN_ARCHITECTURE ×4 incl. stale-rate
  section corrected to implemented WP4.4 state, REQUIREMENTS, both BACKLOGs,
  TESTING counts→commands, TECHNICAL_DEBTS R-13, refactoring_backlog shrink);
  CLAUDE.md rule amendments (clippy gate→CI command, test naming, npm pinning→
  lockfile, licence allowlist extension, pdf path, determinism wording, root
  pointer); README fixed (links, structure, CI claim, 3.0→3);
  module graph renamed + phantom absorber node removed; B12 executed in full
  (DOC_AUDIT + superpowers_analysis + 4 VTN construction files + alignment-plan
  + empty postponed/ deleted; superpowers decision → wiki/decisions/;
  alignment Pass-3 remainder → BACKLOG GB-11; milp_storage docs →
  docs/history/archive/ after extracting c_terminal into ven_milp_planner.md §7);
  DOCUMENTATION.md API reference completed (+12 routes, /sim/override → tri-state
  /sim/inject in FR-SIM-09 + D-06); 2 wiki content-rule fixes. Full /wiki-sync
  deferred until after C3 (code changes would immediately re-stale it).
  Original text: Apply approved doc rewrites in small commits
  (one branch, e.g. `fix/doc-review-rewrites`); run `wiki-lint` after wiki edits.
- [ ] **C3 Fix wave — code quick fixes.** `blocker`/`major` code findings with
  Small/Trivial effort, test-first, one `fix/` branch per theme. Larger items stay
  in the refactoring backlog for their own openspec features.
- [ ] **C4 Close-out.** Re-run baseline (A0.1–A0.3), journal entry in
  `docs/history/project_journal.md`, new learnings into KEY_LEARNINGS.md,
  retire/merge `docs/DOC_AUDIT.md` if decided in B2, mark this plan done.

---

## Findings log

*(appended during execution — one bullet per finding: `[step] [severity] path:line — description`)*

- [A0.1] [minor] run_all_tests.sh:24 — `DOCKER_HOST=""` makes the script attempt
  **local** docker by default, contradicting the Pi4-only docker rule; `--resilience`
  and `--e2e` fail on this machine unless the variable is edited. Suggest defaulting
  to `Pi4-Server` or reading an env override (`DOCKER_HOST="${OPENADR_DOCKER_HOST:-Pi4-Server}"`).
- [A0.2] [major] VEN/src/tasks/planning.rs — 198/200 production lines (99 % of the
  tasks/ limit); the very next change to this file breaks the size gate. Split
  proactively.
- [A0.2→A4.4] [blocker] Rust test code fails `cargo clippy --all-targets -- -D warnings`
  with ~28 errors — **and CI's pre-pr-checks workflow runs exactly this strict
  variant** (`--all-targets --all-features`), so the next PR from this branch
  will fail CI. Fix before any PR. Also align the CLAUDE.md-documented local gate
  (`cargo clippy -- -D warnings`) with the CI command. Error inventory (unnecessary `.get(..).is_none()`, `Default` on unit structs,
  too-many-arguments, never-used test helpers `battery_config`/`base_load_kw`, …)
  across milp_planner/tests, dispatcher.rs, test_support, profile/validate.rs,
  reporter.rs, poll_events.rs, entities/timeline.rs, assets/battery_milp.rs.
  The documented gate (`cargo clippy -- -D warnings`, production only) passes —
  decide whether the gate should include `--all-targets` and clean these up.
- [A0.2] [minor] size early-warnings (>80 % of limit, next largest after
  tasks/planning.rs): services/planning.rs 473/500, simulator/mod.rs 470/500,
  controller/reporter.rs 433/500, milp_planner/results.rs 415/500, state.rs 412/500,
  tasks/poll_events.rs 162/200. assets/mod.rs 617/500 is the documented allowlist
  exception (enum→trait refactor pending).
- [A0.2] [nit] eslint reports 12 warnings (0 errors): react-hooks/exhaustive-deps
  around `tariffs`/`requests` memoization in VEN UI, react-refresh/only-export-components
  in VEN/ui/src/pages/Reports.tsx and VTN/ui/src/App.tsx. Also lints the generated
  VTN/ui/coverage/ directory — coverage/ should be in the eslint ignore list.
- [A0.3] [minor] VEN/src/controller/vtn_port.rs:25 — `VtnPort::fetch_reports_raw`
  returns `Vec<serde_json::Value>`, leaking untyped JSON through the port trait into
  the domain ring. All other Value usage in vtn.rs is private helper plumbing;
  `values: Vec<serde_json::Value>` fields are documented as intentional (OpenADR 3
  polymorphic values). Review consumers in A1.1 and type or relocate the method.
- [A0.4] [major] VEN `cargo audit`: 12 vulnerabilities — aws-lc-sys 0.37.0 (×5, incl.
  two 7.5-high PKCS7 verify bypasses), rustls-webpki 0.103.9 (×4, incl. 8.7-high
  wildcard name-constraint bypass), quinn-proto 0.11.13 (×2 DoS), crossbeam-epoch
  0.9.18 (×1); plus unsound-warnings for anyhow 1.0.101 and rand 0.8.5/0.9.2.
  All are transitive (mostly via reqwest/rustls) — fix wave: `cargo update` and
  re-audit. VTN BFF: 6 vulnerabilities (same families).
- [A0.4] [major] npm audit — VEN UI: 17 vulns (2 critical: vitest/@vitest/coverage-v8;
  high: form-data, lodash, rollup, vite, ws); VTN UI: 16 similar. All in
  devDependencies (build/test tooling), not shipped runtime code — still fix via
  `npm audit fix` (+ review breaking `--force` items) in the fix wave.
- [A0.4] [major] npm pinning rule violated: **all** 26 VEN UI and 25 VTN UI packages
  use `^` ranges; project rule requires exact versions in package.json. Decide:
  enforce the rule (strip carets) or amend the rule to rely on package-lock.json.
- [A0.4] [minor] VEN/Cargo.toml carries blueprint-era comments ("If you decide to
  use the Rust VEN library", commented-out `openleadr-client`, "Optional: if you
  want persistence") — stale scaffolding text in a production manifest; clean up.
- [A0.4] [minor] Licence verification not run (cargo-license not installed); npm
  side also unchecked. Complete during C1 with cargo-license / license-checker.
- [A1.1] [major] VEN/src/controller/monitor.rs:8 — domain ring imports infra:
  `use crate::state::AssetLedgerEntry`. `AssetLedgerEntry` (state.rs:22) is a pure
  domain value type (per-asset cumulative energy_kwh/cost_eur/co2_g); move it to
  entities/ and have state.rs re-import it. Only dependency-rule violation found
  in the domain ring.
- [A1.1] [minor] VEN/src/controller/solver_port.rs:7 — the domain-level port
  imports trait `AssetMilpContext` from `controller/milp_planner` (infra ring).
  Consider moving the trait definition into the domain ring (e.g. next to
  solver_port.rs), leaving milp_planner and assets/ to implement/consume it.
- [A1.1] [minor] injectable-clock rule violated in domain-ring production code:
  VEN/src/entities/site_meter.rs:49 (`ts: Utc::now()`) and
  VEN/src/controller/openadr_interface.rs:230 (`last_updated = Some(Utc::now())`).
  Also VEN/src/state.rs:35 (`AssetLedgerEntry::new` stamps `started_at`).
  Thread the caller's clock through instead. (Full sweep follows in A3.5.)
- [A1.1] [nit] entities/user_request.rs & controller/user_request.rs doc comments
  carry "Stage 5 —" implementation-phase labels — historical staging narrative in
  code docs; drop the stage prefix. controller/user_request.rs also defines the
  `POST /requests` body type in the domain ring — re-examine placement in A1.3.
- [A1.2] [major] VEN/src/services/planning.rs:19,41,61 — PlanningService functions
  (`apply_pending_pv_inject`, `build_asset_contexts`) take concrete
  `crate::simulator::SimState` instead of going through `SimulatorPort`, and
  planning.rs:46 reaches into concrete asset physics
  (`crate::assets::{AssetConfig, PvInverter}`, `PvInverter::natural_irradiance_at`)
  inside the application ring. Either extend SimulatorPort/SimSnapshot to carry
  what these functions need, or move the sim-mutating inject logic into the
  simulator behind the port.
- [A1.3] [minor] VEN/src/tasks/ — six task files (poll_programs, poll_events,
  poll_reports, obligation, state_persist, progress_ticker) repeat the same
  `tokio::time::interval` + `loop { tick().await; … }` scaffold. Extract a shared
  periodic-spawn helper (would also centralize supervision and shrink
  tasks/planning.rs away from its 198/200 limit).
- [A1.3] [minor] VEN/src/routes/timeline.rs — adapter file contains non-trivial
  computation (`snap_up_to_nice`, `resolve_resolution_s`, `build_grid_aligned_array`,
  `zones_from_plan`). Presentation-shaping, but consider extracting a
  timeline-presentation module so the route handler stays thin.
- [A1.3] [minor] VEN/src/tasks/state_persist.rs:8 (15 s) and progress_ticker.rs:15
  (1 s) hard-code their intervals while poll tasks take `secs` parameters —
  inconsistent; hoist to config/constants (cross-ref A3.8).
- [A1.3] [minor] `CreateUserRequestBody` (POST /requests body) lives in
  controller/user_request.rs (domain ring) and is imported by services and routes.
  As an HTTP DTO it belongs in routes/ (or an explicit api-types module); the
  domain fn should take domain params. Confirmed from A1.1.
- [A1.4] [minor] injectable-clock rule gaps in simulator/assets production code,
  despite CLAUDE.md stating determinism is "already applied in the simulator":
  simulator/mod.rs:156 (`last_tick: Utc::now()`), simulator/mod.rs:367,
  assets/base_load.rs:108, assets/battery.rs:142, assets/ev.rs:184,
  assets/grid.rs:86. Some may be legitimate live-loop entry points — classify
  each and thread the tick clock through the rest.
- [A1.4] [minor] VEN/src/simulator/power_model.rs:5 — `random_voltage()` uses
  unseeded `rand::thread_rng()`; simulator output is nondeterministic. Inject a
  seeded RNG (mirror the injectable-clock pattern) or make variance configurable
  to zero for tests.
- [A1.4] [nit] VEN/src/state.rs mixes app wiring (`AppState`) with domain-ish
  value types (`AssetLedgerEntry`, `EvSettings`, `HemsState`); when moving
  AssetLedgerEntry (A1.1 finding), review the neighbours for the same relocation.
- [A1.5] [minor] VEN/src/controller/milp_planner/solver_phase1.rs:151 —
  `with_mip_gap(0.02)` hard-codes the solver tolerance; name it (e.g.
  `MIP_GAP_REL`) or expose via PlannerParams like the other tuning knobs.
- [A1.5] [nit] docs/architecture/ven_milp_planner.md:127 — "current Part A …
  Part B will populate 3 entries" phased narrative; the 3-tier work is merged to
  main, so verify whether "Part B" is now the implemented state and rewrite the
  section present-tense (drop the Part A/B framing).
- [A1.6] [nit] VEN/src/models.rs — 34-line grab-bag holding `SensorSnapshot`/
  `SensorInput` (sensor DTOs used by simulator, sim_tick, routes/events). The
  generic "models" name predates the ring layout; fold the types into entities/
  (or a simulator-owned module) and delete models.rs.
- [A1.7] [minor] no script exists to regenerate the module dependency graph; the
  quarterly control in SESSION_START.md is manual. Add e.g.
  `scripts/gen_module_graph.py` emitting Mermaid from `use crate::` imports so
  the comparison with docs/architecture/module_dependency_graph_post_refactoring.md
  is mechanical.
- [A2.1] [major] VTN/bff/src — zero unit tests (no `#[cfg(test)]` anywhere in the
  crate). Contradicts the test-first rule; only E2E coverage exists. Add tests at
  least for TtlCache expiry, AppError mapping, and vtn_client status handling
  (with a mocked upstream).
- [A2.1] [minor] VTN/bff/src/error.rs — every error becomes `502 BAD_GATEWAY`
  with a stringified anyhow chain; upstream VTN 4xx (validation, conflict)
  surface to the UI as 502. Propagate the upstream status class where known.
- [A2.1] [minor] VTN/bff/src/vtn_client.rs duplicates VEN/src/vtn.rs's OAuth
  token + 401-retry + get/put-JSON plumbing (~300 lines each). Separate crates,
  so extraction needs a shared crate or workspace — record as debt, don't force it.
- [A2.3] [minor] VTN/ui/src/pages/Metrics.tsx has no test file (only untested
  page in either UI). Add a Metrics.test.tsx following the existing page-test
  pattern.
- [A2.4] [nit] VEN/ui/src/components/JsonDialog.tsx and VTN/ui/src/components/
  JsonDialog.tsx are byte-identical (50 lines). Too small to justify a shared
  package; either accept the copy (add a header comment noting the twin) or
  fold into a tiny shared UI package if one ever materializes.
- [A2.5] [minor] VTN/ root hygiene recommendations (decide in B8):
  `project structure.txt` — stale (describes `src/app/App.tsx` layout that doesn't
  exist) → delete; `vtn_setup_from_blog_step_by_step.md` — construction-era
  narrative → delete or rewrite as current-state setup doc under docs/;
  `vtn_rust_bff_blueprint.md` + `vtn_web_ui_blueprint.md` — build blueprints;
  extract still-true architecture content into docs/architecture/VTN_ARCHITECTURE.md,
  then delete; `DTO examples/` — verify against live payloads or delete;
  `VTN/data/db` — runtime artifact, should be gitignored (verify).
- [A3.2] [nit] VEN/src/entities/capacity.rs:5 and entities/design_vocabulary.rs:7 —
  module-wide `#![allow(dead_code)]` without the required same-line justification;
  scope down to the specific items or justify.
- [A3.3] [minor] ~24 production `unwrap()/expect()` calls in VEN
  (milp_interactions.rs ×4, common/mod.rs ×4, services/planning.rs ×3,
  user_request.rs ×2, routes/hems/sessions.rs ×2, openadr_interface.rs ×2,
  heater/ev/battery_milp.rs ×2 each, sim_tick/tick.rs, services/hems.rs,
  milp_planner/inputs.rs ×1 each). Triage: convert to Result or add
  safety-justifying comment.
- [A3.4] [minor] test naming: the documented `test_<function>_<scenario>`
  convention is followed by a minority of tests; most use `<function>_<scenario>`
  (e.g. `shiftable_runtime_is_running`, `new_event_emits_arrived`). Either rename
  tests or amend the documented convention to match dominant practice — decide
  with the owner, don't silently rename.
- [A3.7] [minor] 32 `console.log` calls in UI production code (e.g. the
  `[VEN-UI]` debug logging visible in test output). Strip or gate behind a debug
  flag / logger utility.
- [A4.2] [minor] tests/features — static analysis flags up to ~112 of 417 behave
  step definitions as unused (crude pattern matching, false positives likely).
  Run `behave --dry-run --no-summary` in the test container on Pi4 to get the
  authoritative unused-step list, then delete dead steps.
- [B1] [major] README.md — stale in at least five ways: (1) 4 of 12 documentation
  links broken (system_design.md, concept_vtn_ven_demand_response_simulation.md,
  USE-CASE-MANUAL.md, USE-CASES.md — actual files are named differently or gone);
  (2) Project Structure lists nonexistent dirs `docs/VEN_Controller/`,
  `docs/specs/` (actual: openadr_3_1_specs/), `docs/plans/active/`;
  (3) test counts "15 features, 49 scenarios" vs actual 39 feature files /
  211 scenarios; (4) "Tests also run automatically via GitHub Actions on push
  to main" — only file-size audit and pre-PR checks run; E2E is manual dispatch;
  (5) says "OpenADR 3.0" while the spec copies and project language are 3.1/
  OpenADR 3 — unify. Rewrite in fix wave C2.
- [B1] [major] docs/architecture/module_dependency_graph_post_refactoring.md
  documents a module that does not exist: node `C_ABSORBER` ("absorber.rs,
  AbsorberState, apply_deviation_absorption()") with edge T_SIMTICK→C_ABSORBER.
  No absorber.rs anywhere in VEN/src (DOCUMENTATION.md §2.3 correctly marks the
  feature "not yet implemented"). Remove the phantom node or mark it planned.
  (Full B3 review will re-verify the rest of the graph — regenerated adjacency
  from A1.7 is the reference.)
- [B1] [minor] VEN/src/controller/simulator_port.rs:3 — doc comment lists
  `apply_deviation_absorption` among "all 6 named controller functions"; the
  function does not exist. Fix the comment (code-side doc staleness).
- [B1] [minor] DOCUMENTATION.md HTTP API reference lists 31 routes; routes/mod.rs
  registers 37. Diff and document the missing ~6 during C2.
- [B1] [minor] root CLAUDE.md says only "read the current plan" without naming a
  file — ambiguous since docs/plans/ holds several plans. Point it at a concrete
  entry file (e.g. docs/plans/strategic_roadmap.md, or this review plan while
  active).
- [B1] [minor] .claude/CLAUDE.md determinism section claims the injectable-clock
  pattern is "Already applied in the MILP planner and simulator" — true for the
  MILP planner, contradicted for the simulator by A1.4 findings (Utc::now() in
  simulator/mod.rs, assets). Reword once fixed, or now, to "MILP planner (done);
  simulator (partial)".
- [B2] [minor] docs/REQUIREMENTS.md:273 — "**TariffSnapshot** (formerly
  `RateSnapshot` / `OadrEventSnapshot`)" — rename history; state the current name
  only (the old names add nothing a reader needs).
- [B2] [minor] docs/BACKLOG_OpenADR_Cert.md:106 — "the code path that once
  attempted this … was dead … and was removed" — rewrite present-state: "no VTN
  report is built from PlanCycle events" (+ short why-not if needed).
- [B2] [minor] docs/BACKLOG.md:207 — entry narrates a decision's history and cites
  a design doc that no longer exists (openspec/changes/warnings-cleanup/design.md).
  Rewrite as current state ("dead-beat P-controller exists unit-tested but
  unwired; kept deliberately") and drop the dead reference.
- [B2] [minor] docs/DOC_AUDIT.md — completed audit from an earlier pass, partially
  executed (files it marks "archive" are already gone). Superseded by this review;
  B12 should propose retiring it (moving any still-open rows into this plan's
  findings).
- [B2] [minor] docs/milp_storage_planning.md + milp_storage_planning_impl.md —
  design docs for the 3-tier MILP work, which is now merged to main. Fold the
  durable content into docs/architecture/heater_tank_milp_planning_model.md /
  ven_milp_planner.md and archive the rest (B12 proposal, "merge + archive now");
  the before/after comparisons are history under the content rule.
- [B3] [major] docs/architecture/VEN_ARCHITECTURE.md — multiple content-rule
  violations: §3.3 "Reactor (REMOVED)" is a pure history section ("removed in
  spec kit 001 (2026-03-15)", rationale about Phases 15/20–23) → delete section,
  move narrative to the journal if not already there; :209 "this section
  previously described a configurable StaleRatePolicy…" → rewrite as current
  state; :436 "Legacy: GET /trace has been replaced by GET /trace/events" →
  document only the current endpoints; :594 "the three-strategies fragmentation
  this section used to describe no longer exists" → state the current single
  abstraction plainly. Also :207 cites
  docs/plans/review_items_resolution_strategy.md, deleted in commit 466f792 —
  dead reference.
- [B3] [minor] VEN/src/entities/design_vocabulary.rs — `StaleRatePolicy` enum
  (4 variants) is defined but never referenced anywhere (confirmed by
  VEN_ARCHITECTURE.md gap note and the module's blanket `#![allow(dead_code)]`
  from A3.2). Decide: implement (BL-07 backlog item) or delete the enum and let
  the backlog entry carry the vision.
- [B3] [minor] rename docs/architecture/module_dependency_graph_post_refactoring.md
  → module_dependency_graph.md ("post_refactoring" is itself historical framing);
  update the SESSION_START.md and CLAUDE.md references.
- [B4] [minor] docs/guidelines/TESTING.md — stale counts: "VEN UI 8 test files"
  (actual 27), "16 feature files, ~49 scenarios, ~348 steps" (actual 39 files,
  211 scenarios, 417 step definitions). Replace hard counts with an order of
  magnitude or a generation command so this can't rot again.
- [B4] [minor] docs/guidelines/superpowers_analysis.md — one-off framework fit
  assessment dated 2026-06-25; not a guideline. B12: move the conclusion to
  wiki/decisions/ (or KEY_LEARNINGS) and delete from guidelines/.
- [B5] [minor] docs/reference/TECHNICAL_DEBTS.md R-13 — "VEN_ARCHITECTURE.md §2.1
  previously stated it triggers a 'Direct Dispatcher override' — never
  implemented" → rewrite present-state: "DISPATCH_SETPOINT has no handling path;
  only a dead field on the unreferenced OadrEventCache struct."
- [B7] [minor] docs/plans/refactoring_backlog.md:3,63 — "review conducted
  2026-04-28. Updated 2026-05-25 to reflect resolved items" and "the boiler alias
  issue (previously in routes/hems.rs) is resolved — it now appears…" — drop
  resolved items entirely (journal owns that history) and keep only open debt,
  stated present-tense.
- [B7] [nit] docs/plans/postponed/ is empty — delete the directory or add a
  README stating its purpose.
- [B10] [major] wiki is out of sync: 21 of 37 pages stale per
  `bash scripts/wiki_lint.sh` (heaviest: ven-hexagonal-architecture.md and
  ven-code-vs-docs-audit.md, stale against ~45 source files each). Run
  `/wiki-sync` before or as part of fix wave C2.
- [B10] [minor] content-rule hits in non-exempt wiki pages:
  wiki/components/openadr-interface.md:70 ("dead TELEMETRY_STATUS code path was
  removed, not fixed") and wiki/components/ven-ui.md:36 ("it is no longer
  resampled … that resampling used to blend") → rewrite present-state during
  /wiki-sync.
- [B10] [minor] classification to confirm with owner: `wiki/queries/**` (dated
  Q&A snapshots) and `wiki/review.md` (dated review log with RESOLVED entries)
  are point-in-time records like decisions/log — propose adding both to the
  exempt list rather than rewriting their history references.
- [A0.1] [nit] VEN/src/controller/milp_planner/tests — `solve_ven3_heater_three_tier_zones_feasible`
  runs > 60 s in a debug-build `cargo test`; dominates suite runtime (120 s total).
  Candidate for a smaller horizon or `#[ignore]`-with-CI-tag if it grows further.

## Appendix: document inventory

| # | Path | Step | Exempt | Status |
|---|------|------|--------|--------|
| 1 | README.md | B1 |  | open |
| 2 | DOCUMENTATION.md | B1 |  | open |
| 3 | CLAUDE.md | B1 |  | open |
| 4 | alignment-plan.md | B1 |  | open |
| 5 | .claude/CLAUDE.md | B1 |  | open |
| 6 | docs/architecture/asset_simulation.md | B3 |  | open |
| 7 | docs/architecture/heater_tank_milp_planning_model.md | B3 |  | open |
| 8 | docs/architecture/INTERFACES.md | B3 |  | open |
| 9 | docs/architecture/module_dependency_graph_post_refactoring.md | B3 |  | open |
| 10 | docs/architecture/VEN_ARCHITECTURE.md | B3 |  | open |
| 11 | docs/architecture/ven_asset_interface_spec.md | B3 |  | open |
| 12 | docs/architecture/ven_milp_planner.md | B3 |  | open |
| 13 | docs/architecture/VTN_ARCHITECTURE.md | B3 |  | open |
| 14 | docs/BACKLOG.md | B2 |  | open |
| 15 | docs/BACKLOG_OpenADR_Cert.md | B2 |  | open |
| 16 | docs/DOC_AUDIT.md | B2 |  | open |
| 17 | docs/guidelines/AI-SW-Development.md | B4 |  | open |
| 18 | docs/guidelines/REACT_GUIDELINES.md | B4 |  | open |
| 19 | docs/guidelines/speckit-cheatsheet.md | B4 |  | open |
| 20 | docs/guidelines/superpowers_analysis.md | B4 |  | open |
| 21 | docs/guidelines/TESTING.md | B4 |  | open |
| 22 | docs/history/project_journal.md | — | yes | open |
| 23 | docs/history/project_journal_condensed.md | — | yes | open |
| 24 | docs/milp_planner_config.md | B2 |  | open |
| 25 | docs/milp_storage_planning.md | B2 |  | open |
| 26 | docs/milp_storage_planning_impl.md | B2 |  | open |
| 27 | docs/openadr_3_1_specs/0_READ ME_OpenADR 3 Information and Certification_v3.1.0.md | B9 |  | open |
| 28 | docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md | B9 |  | open |
| 29 | docs/openadr_3_1_specs/3_OpenADR 3.1.0_User_Guide_20250801.md | B9 |  | open |
| 30 | docs/openadr_3_1_specs/4_Change Log 3.1.0.md | B9 |  | open |
| 31 | docs/plans/deviation-control-suggestions.md | B7 |  | open |
| 32 | docs/plans/refactoring_backlog.md | B7 |  | open |
| 33 | docs/plans/roadmap/phase-0-quick-wins.md | B7 |  | open |
| 34 | docs/plans/roadmap/phase-1-data-foundation.md | B7 |  | open |
| 35 | docs/plans/roadmap/phase-2-fleet-enablement.md | B7 |  | open |
| 36 | docs/plans/roadmap/phase-3-control-method-lab.md | B7 |  | open |
| 37 | docs/plans/roadmap/phase-4-comfort-and-personas.md | B7 |  | open |
| 38 | docs/plans/roadmap/phase-5-forecast-and-baseline.md | B7 |  | open |
| 39 | docs/plans/roadmap/phase-6-fidelity-and-cert.md | B7 |  | open |
| 40 | docs/plans/roadmap/README.md | B7 |  | open |
| 41 | docs/plans/strategic_roadmap.md | B7 |  | open |
| 42 | docs/plans/total_review_plan.md | B7 |  | open |
| 43 | docs/reference/FAQ.md | B5 |  | open |
| 44 | docs/reference/GLOSSARY.md | B5 |  | open |
| 45 | docs/reference/KEY_LEARNINGS.md | B5 | yes | open |
| 46 | docs/reference/SESSION_START.md | B5 |  | open |
| 47 | docs/reference/TECHNICAL_DEBTS.md | B5 |  | open |
| 48 | docs/REQUIREMENTS.md | B2 |  | open |
| 49 | docs/SECURITY.md | B2 |  | open |
| 50 | docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md | B6 |  | open |
| 51 | docs/use-cases/SYSTEM-USE-CASE-MANUAL.md | B6 |  | open |
| 52 | docs/use-cases/SYSTEM-USE-CASES.md | B6 |  | open |
| 53 | wiki/architecture/deployment-topology.md | B10 |  | open |
| 54 | wiki/architecture/testing-strategy.md | B10 |  | open |
| 55 | wiki/architecture/ven-hexagonal-architecture.md | B10 |  | open |
| 56 | wiki/architecture/vtn-stack.md | B10 |  | open |
| 57 | wiki/callouts.md | B10 |  | open |
| 58 | wiki/CLAUDE.md | B10 |  | open |
| 59 | wiki/components/asset-layer.md | B10 |  | open |
| 60 | wiki/components/dispatcher.md | B10 |  | open |
| 61 | wiki/components/milp-planner.md | B10 |  | open |
| 62 | wiki/components/openadr-interface.md | B10 |  | open |
| 63 | wiki/components/reliability-and-config.md | B10 |  | open |
| 64 | wiki/components/simulator.md | B10 |  | open |
| 65 | wiki/components/ven-ui.md | B10 |  | open |
| 66 | wiki/concepts/demand-response.md | B10 |  | open |
| 67 | wiki/concepts/hems-planning.md | B10 |  | open |
| 68 | wiki/concepts/openadr-3.md | B10 |  | open |
| 69 | wiki/concepts/openadr-programs.md | B10 |  | open |
| 70 | wiki/concepts/openadr-security.md | B10 |  | open |
| 71 | wiki/concepts/sign-convention.md | B10 |  | open |
| 72 | wiki/concepts/tariffs-and-capacity.md | B10 |  | open |
| 73 | wiki/concepts/three-tier-plan-grid.md | B10 |  | open |
| 74 | wiki/concepts/wiki-maintenance.md | B10 |  | open |
| 75 | wiki/decisions/dto-pass-through.md | B10 | yes | open |
| 76 | wiki/decisions/hexagonal-refactoring.md | B10 | yes | open |
| 77 | wiki/decisions/milp-over-greedy.md | B10 | yes | open |
| 78 | wiki/index.md | B10 |  | open |
| 79 | wiki/log.md | B10 | yes | open |
| 80 | wiki/overview/openadr-lab.md | B10 |  | open |
| 81 | wiki/overview/vision-and-roadmap.md | B10 |  | open |
| 82 | wiki/purpose.md | B10 |  | open |
| 83 | wiki/queries/device-session-common-interface.md | B10 |  | open |
| 84 | wiki/queries/distributor-business-case-tiers.md | B10 |  | open |
| 85 | wiki/queries/openadr-programs-explained.md | B10 |  | open |
| 86 | wiki/queries/ven-code-vs-docs-audit.md | B10 |  | open |
| 87 | wiki/review.md | B10 |  | open |
| 88 | wiki/use-cases/openadr-spec-use-cases.md | B10 |  | open |
| 89 | wiki/use-cases/system-use-cases.md | B10 |  | open |
| 90 | VTN/vtn_rust_bff_blueprint.md | B8 |  | open |
| 91 | VTN/vtn_setup_from_blog_step_by_step.md | B8 |  | open |
| 92 | VTN/vtn_web_ui_blueprint.md | B8 |  | open |
| 93 | VTN/project structure.txt | B8 |  | open |

